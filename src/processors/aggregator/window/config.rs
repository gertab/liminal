//! The window stage's parameters, their validation, and the windows they describe.

use crate::config::{ProcessorConfig, StageConfig, TimingConfig};
use crate::processors::common::field_utils::FieldUtils;
use crate::processors::common::group_key::GroupBy;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// The output fields every result has, besides the `group_by` fields and the aggregates.
pub(super) const WINDOW_START: &str = "window_start";
pub(super) const WINDOW_END: &str = "window_end";

fn default_max_items() -> usize {
    100_000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    Tumbling,
    Sliding,
    LastN,
}

impl WindowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WindowKind::Tumbling => "tumbling",
            WindowKind::Sliding => "sliding",
            WindowKind::LastN => "last_n",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emit {
    #[default]
    OnClose,
    EveryMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateFunction {
    Count,
    DistinctCount,
    Sum,
    Avg,
    Min,
    Max,
    Spread,
    Std,
    First,
    Last,
}

impl AggregateFunction {
    pub fn as_str(self) -> &'static str {
        match self {
            AggregateFunction::Count => "count",
            AggregateFunction::DistinctCount => "distinct_count",
            AggregateFunction::Sum => "sum",
            AggregateFunction::Avg => "avg",
            AggregateFunction::Min => "min",
            AggregateFunction::Max => "max",
            AggregateFunction::Spread => "spread",
            AggregateFunction::Std => "std",
            AggregateFunction::First => "first",
            AggregateFunction::Last => "last",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateSpec {
    pub function: AggregateFunction,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default, rename = "as")]
    pub output: Option<String>,
}

impl AggregateSpec {
    /// `as`, or `<field>_<function>` with the field path's parts joined by `_` (`accel.x` and `std`
    /// give `accel_x_std`), or `count` for a count without a field.
    pub fn output_name(&self) -> String {
        if let Some(name) = &self.output {
            return name.clone();
        }
        match &self.field {
            None => self.function.as_str().to_string(),
            Some(field) => {
                let parts: Vec<&str> = field
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .filter(|part| !part.is_empty())
                    .collect();
                format!("{}_{}", parts.join("_"), self.function.as_str())
            }
        }
    }
}

/// The `window` stage's parameters, as written in the config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowConfig {
    pub kind: WindowKind,
    #[serde(default)]
    pub size_ms: Option<u64>,
    #[serde(default)]
    pub slide_ms: Option<u64>,
    #[serde(default)]
    pub size_n: Option<usize>,
    pub aggregates: Vec<AggregateSpec>,
    #[serde(default)]
    pub emit: Emit,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub emit_empty: bool,
    #[serde(default = "default_max_items")]
    pub max_items: usize,
    /// The stage's `[timing]` block (not a parameter).
    #[serde(skip)]
    pub timing: Option<TimingConfig>,
}

/// The windows a valid config describes: `kind`, `emit` and the sizes, checked and combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Windows {
    /// Time windows `[end - size_ms, end)`, each output when it closes. One ends every `step_ms`:
    /// `size_ms` for tumbling, `slide_ms` for sliding.
    Closing { size_ms: u64, step_ms: u64 },
    /// `tumbling` with `every_message`: each message's window so far.
    TumblingRunning { size_ms: u64 },
    /// `sliding` with `every_message`: the last `size_ms` up to and including each message.
    SlidingRunning { size_ms: u64 },
    /// The last `size_n` messages, output with every message, or once per batch of `size_n`.
    LastN { size_n: usize, batches: bool },
}

impl WindowConfig {
    /// The windows this config describes, or why it doesn't describe any.
    pub fn windows(&self) -> Result<Windows> {
        if self.kind == WindowKind::LastN {
            if self.size_ms.is_some() {
                bail!("`size_ms` is only for time windows; kind = \"last_n\" uses `size_n`");
            }
            self.forbid_slide()?;
            let Some(size_n) = self.size_n.filter(|n| *n > 0) else {
                bail!("kind = \"last_n\" needs `size_n` > 0");
            };
            if size_n > self.max_items {
                bail!(
                    "`size_n` can't be more than `max_items` ({})",
                    self.max_items
                );
            }
            return Ok(Windows::LastN {
                size_n,
                batches: self.emit == Emit::OnClose,
            });
        }

        if self.size_n.is_some() {
            bail!("`size_n` is only for kind = \"last_n\"");
        }
        let Some(size_ms) = self.size_ms.filter(|size| *size > 0) else {
            bail!("kind = \"{}\" needs `size_ms` > 0", self.kind.as_str());
        };
        match (self.kind, self.emit) {
            (WindowKind::Sliding, Emit::OnClose) => match self.slide_ms {
                Some(slide) if slide > 0 && slide <= size_ms => Ok(Windows::Closing {
                    size_ms,
                    step_ms: slide,
                }),
                Some(_) => bail!("`slide_ms` must be more than 0 and at most `size_ms`"),
                None => bail!(
                    "kind = \"sliding\" with emit = \"on_close\" needs `slide_ms` (how often to output)"
                ),
            },
            (kind, emit) => {
                self.forbid_slide()?;
                Ok(match (kind, emit) {
                    (WindowKind::Tumbling, Emit::OnClose) => Windows::Closing {
                        size_ms,
                        step_ms: size_ms,
                    },
                    (WindowKind::Tumbling, Emit::EveryMessage) => {
                        Windows::TumblingRunning { size_ms }
                    }
                    _ => Windows::SlidingRunning { size_ms },
                })
            }
        }
    }

    fn forbid_slide(&self) -> Result<()> {
        if self.slide_ms.is_some() {
            bail!("`slide_ms` is only for kind = \"sliding\" with emit = \"on_close\"");
        }
        Ok(())
    }
}

impl ProcessorConfig for WindowConfig {
    fn from_stage_config(config: &StageConfig) -> Result<Self> {
        let parameters: Map<String, Value> = config
            .parameters
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut window: WindowConfig = serde_json::from_value(Value::Object(parameters))
            .map_err(|error| anyhow!("invalid window parameters: {}", error))?;
        window.timing = config.timing.clone();
        window.validate()?;
        Ok(window)
    }

    fn validate(&self) -> Result<()> {
        if self.max_items == 0 {
            bail!("`max_items` must be more than 0");
        }
        let windows = self.windows()?;
        if self.emit_empty && !matches!(windows, Windows::Closing { .. }) {
            bail!(
                "`emit_empty` is only for time windows (tumbling, sliding) with emit = \"on_close\""
            );
        }
        if self.aggregates.is_empty() {
            bail!("`aggregates` needs at least one aggregate");
        }

        // Output names: the window bounds, the group_by fields, then each aggregate.
        let group_by = GroupBy::new(self.group_by.clone())?;
        let mut taken = HashSet::from([WINDOW_START.to_string(), WINDOW_END.to_string()]);
        for field in group_by.top_level_fields() {
            if field == WINDOW_START || field == WINDOW_END {
                bail!("group_by field '{}' clashes with the window bounds", field);
            }
            taken.insert(field.to_string());
        }
        for aggregate in &self.aggregates {
            match (&aggregate.field, aggregate.function) {
                (Some(_), AggregateFunction::Count) => {
                    bail!("aggregate `count` counts the window's messages; it takes no `field`")
                }
                (None, AggregateFunction::Count) => {}
                (None, function) => bail!("aggregate `{}` needs a `field`", function.as_str()),
                (Some(field), _) => FieldUtils::validate_field_path(field)
                    .map_err(|error| anyhow!("invalid aggregate field '{}': {}", field, error))?,
            }
            let name = aggregate.output_name();
            if name.is_empty() || name.contains(['.', '[', ']']) {
                bail!(
                    "aggregate name '{}' must be a plain field name (no dots or brackets)",
                    name
                );
            }
            if !taken.insert(name.clone()) {
                bail!(
                    "aggregate name '{}' is used twice, or clashes with window_start, window_end or a group_by field",
                    name
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(parameters: Value) -> Result<WindowConfig> {
        let Value::Object(parameters) = parameters else {
            panic!("parameters must be an object");
        };
        WindowConfig::from_stage_config(&StageConfig {
            r#type: "window".to_string(),
            inputs: Some(vec!["in".to_string()]),
            output: Some("out".to_string()),
            concurrency: None,
            channel: None,
            timing: None,
            parameters: Some(parameters.into_iter().collect()),
        })
    }

    #[test]
    fn describes_the_windows_of_each_kind_and_emit() {
        let count = json!([{ "function": "count" }]);
        let cases = [
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": count }),
                Windows::Closing {
                    size_ms: 1000,
                    step_ms: 1000,
                },
            ),
            (
                json!({ "kind": "sliding", "size_ms": 1000, "slide_ms": 250, "aggregates": count }),
                Windows::Closing {
                    size_ms: 1000,
                    step_ms: 250,
                },
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "emit": "every_message", "aggregates": count }),
                Windows::TumblingRunning { size_ms: 1000 },
            ),
            (
                json!({ "kind": "sliding", "size_ms": 1000, "emit": "every_message", "aggregates": count }),
                Windows::SlidingRunning { size_ms: 1000 },
            ),
            (
                json!({ "kind": "last_n", "size_n": 4, "aggregates": count }),
                Windows::LastN {
                    size_n: 4,
                    batches: true,
                },
            ),
            (
                json!({ "kind": "last_n", "size_n": 4, "emit": "every_message", "aggregates": count }),
                Windows::LastN {
                    size_n: 4,
                    batches: false,
                },
            ),
        ];
        for (parameters, expected) in cases {
            let windows = config(parameters.clone()).unwrap().windows().unwrap();
            assert_eq!(windows, expected, "{parameters}");
        }
    }

    #[test]
    fn rejects_invalid_configs() {
        let count = json!([{ "function": "count" }]);
        let cases = [
            (
                json!({ "size_ms": 1000, "aggregates": count }),
                "missing field `kind`",
            ),
            (
                json!({ "kind": "tumbling", "aggregates": count }),
                "needs `size_ms`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 0, "aggregates": count }),
                "needs `size_ms`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "size_n": 5, "aggregates": count }),
                "`size_n` is only",
            ),
            (
                json!({ "kind": "last_n", "aggregates": count }),
                "needs `size_n`",
            ),
            (
                json!({ "kind": "last_n", "size_n": 5, "size_ms": 1000, "aggregates": count }),
                "`size_ms` is only",
            ),
            (
                json!({ "kind": "last_n", "size_n": 5, "slide_ms": 100, "aggregates": count }),
                "`slide_ms` is only",
            ),
            (
                json!({ "kind": "sliding", "size_ms": 1000, "aggregates": count }),
                "needs `slide_ms`",
            ),
            (
                json!({ "kind": "sliding", "size_ms": 1000, "slide_ms": 2000, "aggregates": count }),
                "at most `size_ms`",
            ),
            (
                json!({ "kind": "sliding", "size_ms": 1000, "slide_ms": 100, "emit": "every_message", "aggregates": count }),
                "`slide_ms` is only",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "slide_ms": 100, "aggregates": count }),
                "`slide_ms` is only",
            ),
            (
                json!({ "kind": "last_n", "size_n": 5, "emit_empty": true, "aggregates": count }),
                "`emit_empty` is only",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "emit": "every_message", "emit_empty": true, "aggregates": count }),
                "`emit_empty` is only",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [] }),
                "at least one aggregate",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [{ "function": "sum" }] }),
                "needs a `field`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [{ "function": "count", "field": "v" }] }),
                "takes no `field`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [{ "function": "median", "field": "v" }] }),
                "unknown variant `median`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": count, "sise_ms": 5 }),
                "unknown field `sise_ms`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [{ "function": "count", "feild": "v" }] }),
                "unknown field `feild`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [{ "function": "avg", "field": "v", "as": "x" }, { "function": "max", "field": "v", "as": "x" }] }),
                "used twice",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "group_by": ["sensor"], "aggregates": [{ "function": "count", "as": "sensor" }] }),
                "clashes",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [{ "function": "count", "as": "window_end" }] }),
                "clashes",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "group_by": ["window_start"], "aggregates": count }),
                "clashes with the window bounds",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": [{ "function": "count", "as": "a.b" }] }),
                "plain field name",
            ),
            (
                json!({ "kind": "last_n", "size_n": 10, "max_items": 5, "aggregates": count }),
                "more than `max_items`",
            ),
            (
                json!({ "kind": "tumbling", "size_ms": 1000, "max_items": 0, "aggregates": count }),
                "`max_items` must be",
            ),
        ];
        for (parameters, expected) in cases {
            let error = config(parameters.clone())
                .expect_err("an invalid config")
                .to_string();
            assert!(
                error.contains(expected),
                "{parameters}: expected '{expected}', got '{error}'"
            );
        }
    }

    #[test]
    fn names_aggregates_after_field_and_function() {
        let spec = |field: Option<&str>, function| AggregateSpec {
            function,
            field: field.map(str::to_string),
            output: None,
        };
        assert_eq!(spec(None, AggregateFunction::Count).output_name(), "count");
        assert_eq!(
            spec(Some("value"), AggregateFunction::Avg).output_name(),
            "value_avg"
        );
        assert_eq!(
            spec(Some("accel.x"), AggregateFunction::Std).output_name(),
            "accel_x_std"
        );
        assert_eq!(
            spec(Some("items[0].v"), AggregateFunction::Max).output_name(),
            "items_0_v_max"
        );
    }
}
