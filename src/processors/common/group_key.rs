//! `group_by` for stages that keep state per key: which window a message belongs to.

use super::field_utils::FieldUtils;

use anyhow::{Context, Result};
use serde_json::Value;

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

    /// Writes the key's values into `payload`, each at its own path.
    pub fn write_key(&self, key: &GroupKey, payload: &mut Value) -> Result<()> {
        for (path, value) in self.paths.iter().zip(&key.values) {
            FieldUtils::set_field_value(payload, path, value.clone())?;
        }
        Ok(())
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
        let mut payload = json!({});
        group_by.write_key(&key, &mut payload).unwrap();

        assert_eq!(payload, json!({ "sensor": "t", "device": { "id": 1 } }));
        assert_eq!(
            group_by.top_level_fields().collect::<Vec<_>>(),
            vec!["sensor", "device"]
        );
    }

    #[test]
    fn rejects_an_invalid_path() {
        assert!(GroupBy::new(vec!["a..b".into()]).is_err());
    }
}
