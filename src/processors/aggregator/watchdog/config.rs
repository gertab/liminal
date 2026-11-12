//! The watchdog stage's parameters and their validation.

use crate::config::{ProcessorConfig, StageConfig, TimingConfig};
use crate::processors::common::group_key::GroupBy;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The fields every alert has, besides the key's.
pub(super) const STATUS: &str = "status";
pub(super) const TIMEOUT_MS: &str = "timeout_ms";
pub(super) const SILENT_MS: &str = "silent_ms";
pub(super) const LAST_SEEN: &str = "last_seen";
/// The key's field without `group_by`: the input channel.
pub(super) const INPUT: &str = "input";

/// The `watchdog` stage's parameters, as written in the config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchdogConfig {
    pub timeout_ms: u64,
    #[serde(default)]
    pub timeouts: BTreeMap<String, u64>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub expected_keys: Vec<String>,
    #[serde(default)]
    pub emit_recovered: bool,
    /// The stage's `[timing]` block (not a parameter).
    #[serde(skip)]
    pub timing: Option<TimingConfig>,
    /// The stage's input channels (not a parameter): the keys without `group_by`.
    #[serde(skip)]
    pub inputs: Vec<String>,
}

impl WatchdogConfig {
    /// A key's timeout: its own in `timeouts`, or `timeout_ms`.
    pub fn timeout_for(&self, key_name: &str) -> u64 {
        self.timeouts
            .get(key_name)
            .copied()
            .unwrap_or(self.timeout_ms)
    }

    /// The keys watched from the start: `expected_keys`, or every input without `group_by`.
    pub fn watched_from_start(&self) -> &[String] {
        if self.group_by.is_empty() {
            &self.inputs
        } else {
            &self.expected_keys
        }
    }
}

impl ProcessorConfig for WatchdogConfig {
    fn from_stage_config(config: &StageConfig) -> Result<Self> {
        let parameters: Map<String, Value> = config
            .parameters
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut watchdog: WatchdogConfig = serde_json::from_value(Value::Object(parameters))
            .map_err(|error| anyhow!("invalid watchdog parameters: {}", error))?;
        watchdog.timing = config.timing.clone();
        watchdog.inputs = config.inputs.clone().unwrap_or_default();
        watchdog.validate()?;
        Ok(watchdog)
    }

    fn validate(&self) -> Result<()> {
        if self.timeout_ms == 0 {
            bail!("`timeout_ms` must be more than 0");
        }
        if let Some((key, _)) = self.timeouts.iter().find(|(_, timeout)| **timeout == 0) {
            bail!("the timeout of '{}' must be more than 0", key);
        }

        let group_by = GroupBy::new(self.group_by.clone())?;
        for field in group_by.top_level_fields() {
            if [STATUS, TIMEOUT_MS, SILENT_MS, LAST_SEEN].contains(&field) {
                bail!(
                    "group_by field '{}' clashes with the alert field of that name",
                    field
                );
            }
        }
        match self.group_by.len() {
            0 => {
                if !self.expected_keys.is_empty() {
                    bail!(
                        "`expected_keys` needs group_by; without it, every input of the stage is watched from the start"
                    );
                }
                if let Some(key) = self.timeouts.keys().find(|key| !self.inputs.contains(key)) {
                    bail!(
                        "`timeouts` names '{}', which is not an input of the stage (without group_by, its keys are inputs: {})",
                        key,
                        self.inputs.join(", ")
                    );
                }
            }
            1 => {
                if self.expected_keys.iter().any(String::is_empty) {
                    bail!("`expected_keys` can't have an empty key");
                }
            }
            _ => {
                if !self.timeouts.is_empty() || !self.expected_keys.is_empty() {
                    bail!(
                        "`timeouts` and `expected_keys` name a key by one value, so they need group_by with a single field"
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(parameters: Value) -> Result<WatchdogConfig> {
        let Value::Object(parameters) = parameters else {
            panic!("parameters must be an object");
        };
        WatchdogConfig::from_stage_config(&StageConfig {
            r#type: "watchdog".to_string(),
            inputs: Some(vec!["readings".to_string(), "status".to_string()]),
            output: Some("alerts".to_string()),
            concurrency: None,
            channel: None,
            timing: None,
            parameters: Some(parameters.into_iter().collect()),
        })
    }

    #[test]
    fn picks_each_keys_timeout_and_the_keys_watched_from_the_start() {
        let by_sensor = config(json!({
            "timeout_ms": 1500,
            "timeouts": { "machine2.temperature": 3000 },
            "group_by": ["sensor"],
            "expected_keys": ["mm1.vibration", "machine2.temperature"],
        }))
        .unwrap();
        assert_eq!(by_sensor.timeout_for("machine2.temperature"), 3000);
        assert_eq!(by_sensor.timeout_for("mm1.vibration"), 1500);
        assert_eq!(
            by_sensor.watched_from_start(),
            ["mm1.vibration", "machine2.temperature"]
        );

        let by_input =
            config(json!({ "timeout_ms": 1000, "timeouts": { "status": 60000 } })).unwrap();
        assert_eq!(by_input.timeout_for("status"), 60000);
        assert_eq!(by_input.watched_from_start(), ["readings", "status"]);
    }

    #[test]
    fn rejects_invalid_configs() {
        let cases = [
            (json!({}), "missing field `timeout_ms`"),
            (
                json!({ "timeout_ms": 0 }),
                "`timeout_ms` must be more than 0",
            ),
            (json!({ "timeout_ms": -5 }), "invalid value"),
            (
                json!({ "timeout_ms": 1000, "group_by": ["sensor"], "timeouts": { "a": 0 } }),
                "the timeout of 'a' must be more than 0",
            ),
            (
                json!({ "timeout_ms": 1000, "group_by": ["machine", "sensor"], "timeouts": { "a": 10 } }),
                "single field",
            ),
            (
                json!({ "timeout_ms": 1000, "group_by": ["machine", "sensor"], "expected_keys": ["a"] }),
                "single field",
            ),
            (
                json!({ "timeout_ms": 1000, "expected_keys": ["a"] }),
                "`expected_keys` needs group_by",
            ),
            (
                json!({ "timeout_ms": 1000, "timeouts": { "readingz": 10 } }),
                "'readingz', which is not an input",
            ),
            (
                json!({ "timeout_ms": 1000, "group_by": ["sensor"], "expected_keys": [""] }),
                "empty key",
            ),
            (
                json!({ "timeout_ms": 1000, "group_by": ["status"] }),
                "clashes with the alert field",
            ),
            (
                json!({ "timeout_ms": 1000, "group_by": ["a..b"] }),
                "invalid group_by",
            ),
            (
                json!({ "timeout_ms": 1000, "emit_recoverd": true }),
                "unknown field `emit_recoverd`",
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
