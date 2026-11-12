//! The reduce stage's parameters, their validation, and the updates they describe.

use crate::config::{ProcessorConfig, StageConfig, TimingConfig};
use crate::processors::common::condition_utils::Condition;
use crate::processors::common::expression::Expression;
use crate::processors::common::group_key::GroupBy;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value};

/// One entry of `update`, as written in the config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSpec {
    pub field: String,
    pub expression: String,
    #[serde(default)]
    pub when: Option<Condition>,
}

/// The `reduce` stage's parameters, as written in the config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReduceConfig {
    pub initial: Map<String, Value>,
    pub update: Vec<UpdateSpec>,
    #[serde(default)]
    pub reset_when: Option<Condition>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub include_message: bool,
    /// The stage's `[timing]` block (not a parameter).
    #[serde(skip)]
    pub timing: Option<TimingConfig>,
}

/// An update a valid config describes: its expression parsed.
#[derive(Debug, Clone)]
pub struct Update {
    pub field: String,
    pub expression: Expression,
    pub when: Option<Condition>,
}

impl ReduceConfig {
    /// The updates this config describes, or why one of them is invalid.
    pub fn updates(&self) -> Result<Vec<Update>> {
        if self.update.is_empty() {
            bail!("`update` needs at least one update");
        }
        self.update
            .iter()
            .map(|spec| {
                if !self.initial.contains_key(&spec.field) {
                    bail!(
                        "update of '{}': '{}' is not in `initial`",
                        spec.field,
                        spec.field
                    );
                }
                let expression = Expression::parse(&spec.expression)
                    .map_err(|error| anyhow!("update of '{}' has {}", spec.field, error))?;
                if let Some(when) = &spec.when {
                    when.validate().map_err(|error| {
                        anyhow!("update of '{}': `when` has {}", spec.field, error)
                    })?;
                }
                Ok(Update {
                    field: spec.field.clone(),
                    expression,
                    when: spec.when.clone(),
                })
            })
            .collect()
    }
}

impl ProcessorConfig for ReduceConfig {
    fn from_stage_config(config: &StageConfig) -> Result<Self> {
        let parameters: Map<String, Value> = config
            .parameters
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut reduce: ReduceConfig = serde_json::from_value(Value::Object(parameters))
            .map_err(|error| anyhow!("invalid reduce parameters: {}", error))?;
        reduce.timing = config.timing.clone();
        reduce.validate()?;
        Ok(reduce)
    }

    fn validate(&self) -> Result<()> {
        if self.initial.is_empty() {
            bail!("`initial` needs at least one state value");
        }
        for (name, value) in &self.initial {
            if !is_plain_name(name) {
                bail!(
                    "state name '{}' must be a plain name (letters, digits and _, not starting with a digit), so expressions can read it as state.<name>",
                    name
                );
            }
            if !matches!(value, Value::Number(_) | Value::String(_) | Value::Bool(_)) {
                bail!(
                    "the initial value of '{}' must be a number, string or boolean",
                    name
                );
            }
        }
        self.updates()?;
        if let Some(reset_when) = &self.reset_when {
            reset_when
                .validate()
                .map_err(|error| anyhow!("`reset_when` has {}", error))?;
        }
        // The output holds the group_by fields and the state side by side.
        let group_by = GroupBy::new(self.group_by.clone())?;
        for field in group_by.top_level_fields() {
            if self.initial.contains_key(field) {
                bail!(
                    "group_by field '{}' clashes with the state value of that name",
                    field
                );
            }
        }
        Ok(())
    }
}

/// An identifier expressions can read after `state.`: letters, digits and `_`, not starting with a
/// digit.
fn is_plain_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(parameters: Value) -> Result<ReduceConfig> {
        let Value::Object(parameters) = parameters else {
            panic!("parameters must be an object");
        };
        ReduceConfig::from_stage_config(&StageConfig {
            r#type: "reduce".to_string(),
            inputs: Some(vec!["in".to_string()]),
            output: Some("out".to_string()),
            concurrency: None,
            channel: None,
            timing: None,
            parameters: Some(parameters.into_iter().collect()),
        })
    }

    #[test]
    fn accepts_a_full_config() {
        let reduce = config(json!({
            "initial": { "distance_m": 0.0, "moves": 0, "label": "", "moving": false },
            "update": [
                { "field": "distance_m", "expression": "state.distance_m + math::hypot(x - prev.x, y - prev.y)",
                  "when": { "field_path": "kind", "operation": "==", "value": "position" } },
                { "field": "moves", "expression": "state.moves + 1" },
            ],
            "reset_when": { "field_path": "action", "operation": "==", "value": "place" },
            "group_by": ["entity"],
            "include_message": true,
        }))
        .unwrap();

        let updates = reduce.updates().unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].field, "distance_m");
        assert!(updates[0].when.is_some() && updates[1].when.is_none());
    }

    #[test]
    fn rejects_invalid_configs() {
        let update = json!([{ "field": "n", "expression": "state.n + 1" }]);
        let cases = [
            (json!({ "update": update }), "missing field `initial`"),
            (json!({ "initial": { "n": 0 } }), "missing field `update`"),
            (
                json!({ "initial": {}, "update": update }),
                "at least one state value",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": [] }),
                "at least one update",
            ),
            (
                json!({ "initial": { "n": [1, 2] }, "update": update }),
                "number, string or boolean",
            ),
            (
                json!({ "initial": { "n": null }, "update": update }),
                "number, string or boolean",
            ),
            (
                json!({ "initial": { "n": 0, "a.b": 0 }, "update": update }),
                "plain name",
            ),
            (
                json!({ "initial": { "n": 0, "2x": 0 }, "update": update }),
                "plain name",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": [{ "field": "m", "expression": "1" }] }),
                "'m' is not in `initial`",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": [{ "field": "n", "expression": "(state.n + 1" }] }),
                "invalid expression",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": [{ "field": "n", "expression": "1", "when": { "field_path": "x", "operation": "is" } }] }),
                "unsupported operation",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": update, "reset_when": { "operation": ">" } }),
                "`reset_when` has invalid field_path",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": update, "group_by": ["n"] }),
                "clashes",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": update, "group_by": ["a..b"] }),
                "invalid group_by",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": update, "reset": {} }),
                "unknown field `reset`",
            ),
            (
                json!({ "initial": { "n": 0 }, "update": [{ "field": "n", "expr": "1" }] }),
                "unknown field `expr`",
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
}
