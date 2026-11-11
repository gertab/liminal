//! What a window outputs: the aggregates over its messages, with the key and the window's bounds.

use super::config::{AggregateFunction, AggregateSpec, WINDOW_END, WINDOW_START};
use crate::processors::common::field_utils::FieldUtils;
use crate::processors::common::group_key::{GroupBy, GroupKey};
use crate::processors::common::warn_once::WarnOnce;

use serde_json::{Map, Number, Value};
use std::collections::HashSet;

/// A message held in a window: its event time and the values of the aggregated fields only.
pub(super) struct Item {
    pub(super) at_ms: u64,
    values: Vec<Option<Value>>,
}

/// Computes the configured aggregates. Each distinct aggregated field is read once per message
/// into a slot of `Item::values`.
pub(super) struct Aggregator {
    aggregates: Vec<Aggregate>,
    fields: Vec<String>,
}

struct Aggregate {
    name: String,
    function: AggregateFunction,
    slot: Option<usize>,
}

impl Aggregator {
    pub(super) fn new(specs: &[AggregateSpec]) -> Self {
        let mut fields: Vec<String> = Vec::new();
        let aggregates = specs
            .iter()
            .map(|spec| Aggregate {
                name: spec.output_name(),
                function: spec.function,
                slot: spec.field.as_ref().map(|field| {
                    fields
                        .iter()
                        .position(|known| known == field)
                        .unwrap_or_else(|| {
                            fields.push(field.clone());
                            fields.len() - 1
                        })
                }),
            })
            .collect();
        Self { aggregates, fields }
    }

    /// What the window keeps of a message.
    pub(super) fn item(&self, at_ms: u64, payload: &Value) -> Item {
        Item {
            at_ms,
            values: self
                .fields
                .iter()
                .map(|field| FieldUtils::extract_field_value(payload, field).cloned())
                .collect(),
        }
    }

    /// One window's result: the key's fields, `window_start`, `window_end`, then each aggregate.
    pub(super) fn result(
        &self,
        group_by: &GroupBy,
        key: &GroupKey,
        window: &[&Item],
        start_ms: u64,
        end_ms: u64,
        warnings: &mut WarnOnce,
    ) -> Value {
        let mut payload = Value::Object(Map::new());
        if let Err(error) = group_by.write_key(key, &mut payload) {
            warnings.warn("write-key", || {
                format!("could not write the group_by fields: {}", error)
            });
        }
        if let Value::Object(map) = &mut payload {
            map.insert(WINDOW_START.to_string(), Value::from(start_ms));
            map.insert(WINDOW_END.to_string(), Value::from(end_ms));
            for aggregate in &self.aggregates {
                map.insert(
                    aggregate.name.clone(),
                    self.value(aggregate, window, warnings),
                );
            }
        }
        payload
    }

    fn value(&self, aggregate: &Aggregate, window: &[&Item], warnings: &mut WarnOnce) -> Value {
        let Some(slot) = aggregate.slot else {
            // `count`, the only aggregate without a field: the window's messages.
            return Value::from(window.len());
        };
        match aggregate.function {
            // Distinct JSON values, as written: `1` and `1.0` differ, as in Liminal's `equals`.
            AggregateFunction::DistinctCount => Value::from(
                window
                    .iter()
                    .filter_map(|item| item.values[slot].as_ref())
                    .filter(|value| !value.is_null())
                    .map(|value| value.to_string())
                    .collect::<HashSet<_>>()
                    .len(),
            ),
            function => numeric(
                function,
                &numbers(window, slot, &self.fields[slot], warnings),
            ),
        }
    }
}

/// The numeric values of a field in the window. Missing and non-numeric values are skipped.
fn numbers(window: &[&Item], slot: usize, field: &str, warnings: &mut WarnOnce) -> Vec<f64> {
    let mut numbers = Vec::with_capacity(window.len());
    for item in window {
        match &item.values[slot] {
            Some(Value::Number(number)) => numbers.extend(number.as_f64()),
            None | Some(Value::Null) => warnings.warn(&format!("missing:{field}"), || {
                format!("some messages have no '{}'; they are skipped", field)
            }),
            Some(_) => warnings.warn(&format!("non-numeric:{field}"), || {
                format!("'{}' has non-numeric values; they are skipped", field)
            }),
        }
    }
    numbers
}

/// A numeric aggregate, or `null` when there are no numbers (or the result isn't finite).
fn numeric(function: AggregateFunction, numbers: &[f64]) -> Value {
    if numbers.is_empty() {
        return Value::Null;
    }
    let n = numbers.len() as f64;
    let sum: f64 = numbers.iter().sum();
    let min = numbers.iter().copied().fold(f64::INFINITY, f64::min);
    let max = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let result = match function {
        AggregateFunction::Sum => sum,
        AggregateFunction::Avg => sum / n,
        AggregateFunction::Min => min,
        AggregateFunction::Max => max,
        AggregateFunction::Spread => max - min,
        // Population standard deviation, in two passes for accuracy.
        AggregateFunction::Std => {
            let mean = sum / n;
            (numbers.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt()
        }
        AggregateFunction::Count | AggregateFunction::DistinctCount => return Value::Null,
    };
    Number::from_f64(result).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn computes_the_numeric_aggregates() {
        let numbers = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let value = |function| numeric(function, &numbers).as_f64().unwrap();
        assert_eq!(value(AggregateFunction::Sum), 40.0);
        assert_eq!(value(AggregateFunction::Avg), 5.0);
        assert_eq!(value(AggregateFunction::Min), 2.0);
        assert_eq!(value(AggregateFunction::Max), 9.0);
        assert_eq!(value(AggregateFunction::Spread), 7.0);
        assert_eq!(value(AggregateFunction::Std), 2.0);
        assert_eq!(numeric(AggregateFunction::Std, &[3.5]), json!(0.0));
        assert_eq!(numeric(AggregateFunction::Avg, &[]), Value::Null);
    }

    #[test]
    fn reads_each_field_once_and_lays_out_the_result() {
        let specs: Vec<AggregateSpec> = serde_json::from_value(json!([
            { "function": "count" },
            { "function": "avg", "field": "value" },
            { "function": "max", "field": "value", "as": "peak" },
            { "function": "distinct_count", "field": "state" },
        ]))
        .unwrap();
        let aggregator = Aggregator::new(&specs);
        assert_eq!(aggregator.fields, vec!["value", "state"]);

        let items = [
            aggregator.item(1, &json!({ "value": 1.0, "state": "on" })),
            aggregator.item(2, &json!({ "value": 3.0, "state": "on" })),
        ];
        let window: Vec<&Item> = items.iter().collect();
        let group_by = GroupBy::new(vec!["sensor".into()]).unwrap();
        let key = group_by.key_of(&json!({ "sensor": "s1" })).unwrap();
        let result = aggregator.result(&group_by, &key, &window, 0, 10, &mut WarnOnce::new("t"));

        assert_eq!(
            result,
            json!({
                "sensor": "s1", "window_start": 0, "window_end": 10,
                "count": 2, "value_avg": 2.0, "peak": 3.0, "state_distinct_count": 1,
            })
        );
    }
}
