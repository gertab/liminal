//! `group_by` for stages that keep state per key (`window`, `reduce`): which key a message belongs
//! to, and the key's fields in an output.

use super::field_utils::FieldUtils;
use super::warn_once::WarnOnce;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

/// Past this many keys, a stage warns: `group_by` is probably on a field with too many values.
const KEY_COUNT_WARNING: usize = 10_000;

/// The `group_by` field paths of a stage.
#[derive(Debug, Clone, Default)]
pub struct GroupBy {
    paths: Vec<String>,
}

/// A message's key: its `group_by` values, and an id built from them.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupKey {
    pub id: String,
    pub values: Vec<Value>,
}

impl GroupBy {
    pub fn new(paths: Vec<String>) -> Result<Self> {
        for path in &paths {
            FieldUtils::validate_field_path(path)
                .with_context(|| format!("invalid group_by field path '{}'", path))?;
        }
        Ok(Self { paths })
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// The message's key, or `None` if one of the `group_by` fields is missing or null. With no
    /// `group_by` paths, every message has the same key.
    pub fn key_of(&self, payload: &Value) -> Option<GroupKey> {
        let mut values = Vec::with_capacity(self.paths.len());
        for path in &self.paths {
            match FieldUtils::extract_field_value(payload, path) {
                None | Some(Value::Null) => return None,
                Some(value) => values.push(value.clone()),
            }
        }
        let id = serde_json::to_string(&values).unwrap_or_default();
        Some(GroupKey { id, values })
    }

    /// `key_of`, warning once about messages without a key, which stages skip.
    pub fn key_or_warn(&self, payload: &Value, warnings: &mut WarnOnce) -> Option<GroupKey> {
        let key = self.key_of(payload);
        if key.is_none() {
            warnings.warn("missing-key", || {
                format!(
                    "messages without the group_by fields ({}) are skipped",
                    self.paths.join(", ")
                )
            });
        }
        key
    }

    /// For a stage about to add a key to the `key_count` it holds: warns once past
    /// `KEY_COUNT_WARNING`.
    pub fn check_key_count(&self, key_count: usize, warnings: &mut WarnOnce) {
        if key_count >= KEY_COUNT_WARNING {
            warnings.warn("key-count", || {
                format!("more than {KEY_COUNT_WARNING} keys: is group_by on the right field?")
            });
        }
    }

    /// Writes the key's values into `payload`, each at its own path.
    pub fn write_key(&self, key: &GroupKey, payload: &mut Value) -> Result<()> {
        for (path, value) in self.paths.iter().zip(&key.values) {
            FieldUtils::set_field_value(payload, path, value.clone())?;
        }
        Ok(())
    }

    /// A new output payload holding the key's values, each at its own path. Paths that can't all be
    /// written (`a` and `a.b`) are warned about once.
    pub fn key_payload(&self, key: &GroupKey, warnings: &mut WarnOnce) -> Value {
        let mut payload = Value::Object(Map::new());
        if let Err(error) = self.write_key(key, &mut payload) {
            warnings.warn("write-key", || {
                format!("could not write the group_by fields: {}", error)
            });
        }
        payload
    }

    /// The top-level output field each path writes to (`device.id` writes `device`).
    pub fn top_level_fields(&self) -> impl Iterator<Item = &str> {
        self.paths
            .iter()
            .map(|path| path.split(['.', '[']).next().unwrap_or(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_by_the_group_by_values() {
        let group_by = GroupBy::new(vec!["sensor".into(), "device.id".into()]).unwrap();
        let a = group_by.key_of(&json!({ "sensor": "t", "device": { "id": 1 }, "value": 2.0 }));
        let b = group_by.key_of(&json!({ "sensor": "t", "device": { "id": 1 }, "value": 3.0 }));
        let c = group_by.key_of(&json!({ "sensor": "t", "device": { "id": 2 } }));

        assert_eq!(a, b);
        assert_ne!(a.unwrap().id, c.unwrap().id);
    }

    #[test]
    fn a_missing_or_null_field_has_no_key() {
        let group_by = GroupBy::new(vec!["sensor".into()]).unwrap();
        assert!(group_by.key_of(&json!({ "value": 1.0 })).is_none());
        assert!(group_by.key_of(&json!({ "sensor": null })).is_none());

        let mut warnings = WarnOnce::new("stage");
        assert!(
            group_by
                .key_or_warn(&json!({ "value": 1.0 }), &mut warnings)
                .is_none()
        );
        assert!(warnings.has_warned("missing-key"));
    }

    #[test]
    fn without_paths_every_message_shares_one_key() {
        let group_by = GroupBy::new(vec![]).unwrap();
        assert_eq!(
            group_by.key_of(&json!({ "a": 1 })),
            group_by.key_of(&json!(5))
        );
    }

    #[test]
    fn writes_the_key_back_at_its_paths() {
        let group_by = GroupBy::new(vec!["sensor".into(), "device.id".into()]).unwrap();
        let key = group_by
            .key_of(&json!({ "sensor": "t", "device": { "id": 1 } }))
            .unwrap();
        let mut warnings = WarnOnce::new("stage");

        assert_eq!(
            group_by.key_payload(&key, &mut warnings),
            json!({ "sensor": "t", "device": { "id": 1 } })
        );
        assert!(!warnings.has_warned("write-key"));
        assert_eq!(
            group_by.top_level_fields().collect::<Vec<_>>(),
            vec!["sensor", "device"]
        );
    }

    #[test]
    fn warns_once_past_the_key_count() {
        let group_by = GroupBy::new(vec![]).unwrap();
        let mut warnings = WarnOnce::new("stage");
        group_by.check_key_count(KEY_COUNT_WARNING - 1, &mut warnings);
        assert!(!warnings.has_warned("key-count"));
        group_by.check_key_count(KEY_COUNT_WARNING, &mut warnings);
        assert!(warnings.has_warned("key-count"));
    }

    #[test]
    fn rejects_an_invalid_path() {
        assert!(GroupBy::new(vec!["a..b".into()]).is_err());
    }
}
