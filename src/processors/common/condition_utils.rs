use super::field_utils::FieldUtils;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A test on one field of a message: `field_path`, `operation`, `value`. Used by `rule`'s rules.
///
/// A missing field makes every operation but `always` false (`not_equals` included).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Condition {
    #[serde(default)]
    pub field_path: String,
    pub operation: String,
    #[serde(default)]
    pub value: Value,
}

impl Condition {
    /// Checks the operation and, unless it is `always`/`never`, the field path. The error reads
    /// after "has", e.g. "unsupported operation: 'is'".
    pub fn validate(&self) -> Result<()> {
        if self.operation.is_empty() {
            return Err(anyhow!("empty operation"));
        }
        let operation = ConditionOperation::from_str(&self.operation)
            .ok_or_else(|| anyhow!("unsupported operation: '{}'", self.operation))?;
        if !operation.is_unconditional() {
            FieldUtils::validate_field_path(&self.field_path)
                .map_err(|error| anyhow!("invalid field_path: {}", error))?;
        }
        Ok(())
    }

    /// Whether the condition holds for `payload`.
    pub fn matches(&self, payload: &Value) -> bool {
        let Some(operation) = ConditionOperation::from_str(&self.operation) else {
            tracing::warn!("Unknown condition operation: {}", self.operation);
            return false;
        };
        if operation.is_unconditional() {
            return ConditionEvaluator::evaluate_condition(&Value::Null, &operation, &Value::Null);
        }
        match FieldUtils::extract_field_value(payload, &self.field_path) {
            Some(field_value) => {
                ConditionEvaluator::evaluate_condition(field_value, &operation, &self.value)
            }
            None => {
                tracing::debug!("Field '{}' not found in payload", self.field_path);
                false
            }
        }
    }
}

/// Condition operations supported by processors
#[derive(Debug, Clone, PartialEq)]
pub enum ConditionOperation {
    Always,
    Never,
    Equals,
    NotEquals,
    StartsWith,
    EndsWith,
    Contains,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl ConditionOperation {
    /// Parse a condition operation from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            "equals" | "==" => Some(Self::Equals),
            "not_equals" | "!=" => Some(Self::NotEquals),
            "startswith" => Some(Self::StartsWith),
            "endswith" => Some(Self::EndsWith),
            "contains" => Some(Self::Contains),
            ">" => Some(Self::GreaterThan),
            ">=" => Some(Self::GreaterThanOrEqual),
            "<" => Some(Self::LessThan),
            "<=" => Some(Self::LessThanOrEqual),
            _ => None,
        }
    }

    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Never => "never",
            Self::Equals => "equals",
            Self::NotEquals => "not_equals",
            Self::StartsWith => "startswith",
            Self::EndsWith => "endswith",
            Self::Contains => "contains",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
        }
    }

    pub fn is_unconditional(&self) -> bool {
        matches!(self, Self::Always | Self::Never)
    }
}

/// Utility functions for evaluating conditions against JSON values
pub struct ConditionEvaluator;

impl ConditionEvaluator {
    /// Evaluate a condition against a field value
    ///
    /// # Arguments
    /// * `field_value` - The value to test
    /// * `operation` - The condition operation
    /// * `expected_value` - The value to compare against
    pub fn evaluate_condition(
        field_value: &Value,
        operation: &ConditionOperation,
        expected_value: &Value,
    ) -> bool {
        match operation {
            ConditionOperation::Always => true,
            ConditionOperation::Never => false,
            ConditionOperation::Equals => field_value == expected_value,
            ConditionOperation::NotEquals => field_value != expected_value,

            ConditionOperation::StartsWith => {
                if let (Value::String(field_str), Value::String(expected_str)) =
                    (field_value, expected_value)
                {
                    field_str.starts_with(expected_str)
                } else {
                    false
                }
            }

            ConditionOperation::EndsWith => {
                if let (Value::String(field_str), Value::String(expected_str)) =
                    (field_value, expected_value)
                {
                    field_str.ends_with(expected_str)
                } else {
                    false
                }
            }

            ConditionOperation::Contains => {
                if let (Value::String(field_str), Value::String(expected_str)) =
                    (field_value, expected_value)
                {
                    field_str.contains(expected_str)
                } else {
                    false
                }
            }

            ConditionOperation::GreaterThan => {
                Self::compare_numbers(field_value, expected_value, |a, b| a > b)
            }

            ConditionOperation::GreaterThanOrEqual => {
                Self::compare_numbers(field_value, expected_value, |a, b| a >= b)
            }

            ConditionOperation::LessThan => {
                Self::compare_numbers(field_value, expected_value, |a, b| a < b)
            }

            ConditionOperation::LessThanOrEqual => {
                Self::compare_numbers(field_value, expected_value, |a, b| a <= b)
            }
        }
    }

    /// Parse and evaluate a filter-style condition string (like "startswith 'value'")
    /// This is for backward compatibility with FilterProcessor
    pub fn evaluate_filter_condition(field_value: &Value, condition_str: &str) -> bool {
        // String operations
        if let Some(value_str) = field_value.as_str() {
            // startswith condition
            if condition_str.starts_with("startswith '") && condition_str.ends_with("'") {
                let prefix = &condition_str[12..condition_str.len() - 1];
                return value_str.starts_with(prefix);
            }

            // endswith condition
            if condition_str.starts_with("endswith '") && condition_str.ends_with("'") {
                let suffix = &condition_str[10..condition_str.len() - 1];
                return value_str.ends_with(suffix);
            }

            // contains condition
            if condition_str.starts_with("contains '") && condition_str.ends_with("'") {
                let substring = &condition_str[10..condition_str.len() - 1];
                return value_str.contains(substring);
            }

            // equals condition for strings
            if condition_str.starts_with("== '") && condition_str.ends_with("'") {
                let expected = &condition_str[4..condition_str.len() - 1];
                return value_str == expected;
            }

            // not equals condition for strings
            if condition_str.starts_with("!= '") && condition_str.ends_with("'") {
                let expected = &condition_str[4..condition_str.len() - 1];
                return value_str != expected;
            }
        }

        // Numeric operations
        if let Some(value_num) = field_value.as_f64() {
            // Greater than
            if condition_str.starts_with("> ") {
                if let Ok(threshold) = condition_str[2..].trim().parse::<f64>() {
                    return value_num > threshold;
                }
            }

            // Greater than or equal
            if condition_str.starts_with(">= ") {
                if let Ok(threshold) = condition_str[3..].trim().parse::<f64>() {
                    return value_num >= threshold;
                }
            }

            // Less than
            if condition_str.starts_with("< ") {
                if let Ok(threshold) = condition_str[2..].trim().parse::<f64>() {
                    return value_num < threshold;
                }
            }

            // Less than or equal
            if condition_str.starts_with("<= ") {
                if let Ok(threshold) = condition_str[3..].trim().parse::<f64>() {
                    return value_num <= threshold;
                }
            }

            // Equals for numbers
            if condition_str.starts_with("== ") {
                if let Ok(expected) = condition_str[3..].trim().parse::<f64>() {
                    return (value_num - expected).abs() < f64::EPSILON;
                }
            }

            // Not equals for numbers
            if condition_str.starts_with("!= ") {
                if let Ok(expected) = condition_str[3..].trim().parse::<f64>() {
                    return (value_num - expected).abs() >= f64::EPSILON;
                }
            }
        }

        // Boolean operations
        if let Some(value_bool) = field_value.as_bool() {
            if condition_str == "== true" {
                return value_bool;
            }
            if condition_str == "== false" {
                return !value_bool;
            }
        }

        // If no condition matched, return false
        false
    }

    /// Helper function to compare numeric values
    fn compare_numbers<F>(field_value: &Value, expected_value: &Value, comparator: F) -> bool
    where
        F: Fn(f64, f64) -> bool,
    {
        if let (Value::Number(field_num), Value::Number(expected_num)) =
            (field_value, expected_value)
        {
            let field_val = field_num.as_f64().unwrap_or(0.0);
            let expected_val = expected_num.as_f64().unwrap_or(0.0);
            comparator(field_val, expected_val)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn condition(field_path: &str, operation: &str, value: Value) -> Condition {
        Condition {
            field_path: field_path.to_string(),
            operation: operation.to_string(),
            value,
        }
    }

    #[test]
    fn matches_a_field_against_a_value() {
        let payload = json!({ "kind": "position", "speed": 0.8, "items": [{ "id": "a" }] });
        assert!(condition("kind", "equals", json!("position")).matches(&payload));
        assert!(condition("speed", ">", json!(0.5)).matches(&payload));
        assert!(condition("items[0].id", "startswith", json!("a")).matches(&payload));
        assert!(!condition("kind", "not_equals", json!("position")).matches(&payload));
        assert!(condition("", "always", Value::Null).matches(&payload));
        assert!(!condition("", "never", Value::Null).matches(&payload));
    }

    #[test]
    fn a_missing_field_never_matches() {
        let payload = json!({ "kind": "sensor" });
        assert!(!condition("action", "equals", json!("pick")).matches(&payload));
        assert!(!condition("action", "not_equals", json!("pick")).matches(&payload));
    }

    #[test]
    fn validates_operation_and_field_path() {
        assert!(condition("kind", "equals", json!("x")).validate().is_ok());
        assert!(condition("", "always", Value::Null).validate().is_ok());
        let error = |c: Condition| c.validate().unwrap_err().to_string();
        assert_eq!(error(condition("kind", "", json!(1))), "empty operation");
        assert!(error(condition("kind", "is", json!(1))).contains("unsupported operation: 'is'"));
        assert!(error(condition("a..b", "equals", json!(1))).contains("invalid field_path"));
    }
}
