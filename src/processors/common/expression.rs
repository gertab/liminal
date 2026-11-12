//! Expressions over message fields, evaluated with evalexpr. Used by `rule`'s `compute_field` and by
//! `reduce`.
//!
//! - Variables are a payload's fields flattened into names: `temperature`, `device.id`,
//!   `items[0].value`. Numbers are floats; nulls are left out.
//! - A few math functions can be called by their bare name and are rewritten to evalexpr's `math::`
//!   functions: `sqrt`, `sin`, `cos`, `tan`, `ln`, `abs`, `exp`, and `log` (base 10). evalexpr's own
//!   names (`math::pow`, `math::hypot`, `math::atan2`, …) and its bare `floor`, `round`, `ceil`, `min`,
//!   `max`, `if` work as they are.
//! - Expressions are parsed once, when the config loads. Structural errors (unbalanced brackets, two
//!   values in a row) show then; others, such as a missing operand (`x +`), when the expression runs.

use anyhow::{Result, anyhow};
use evalexpr::{ContextWithMutableVariables, DefaultNumericTypes, HashMapContext, Node};
use serde_json::{Number, Value};

pub use evalexpr::Value as ExpressionValue;

/// Bare function names and the evalexpr function each one calls.
const MATH_ALIASES: &[(&str, &str)] = &[
    ("sqrt", "math::sqrt"),
    ("sin", "math::sin"),
    ("cos", "math::cos"),
    ("tan", "math::tan"),
    ("log", "math::log10"),
    ("ln", "math::ln"),
    ("abs", "math::abs"),
    ("exp", "math::exp"),
];

#[derive(Debug, Clone)]
pub struct Expression {
    source: String,
    tree: Node<DefaultNumericTypes>,
}

impl Expression {
    pub fn parse(source: &str) -> Result<Self> {
        let tree =
            evalexpr::build_operator_tree::<DefaultNumericTypes>(&rewrite_math_aliases(source))
                .map_err(|error| anyhow!("invalid expression '{}': {}", source, error))?;
        Ok(Self {
            source: source.to_string(),
            tree,
        })
    }

    /// The expression as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn eval(&self, variables: &Variables) -> Result<ExpressionValue> {
        self.tree
            .eval_with_context(&variables.context)
            .map_err(|error| anyhow!("expression '{}' failed: {}", self.source, error))
    }

    /// The result as a float; anything else (an integer, a boolean, …) is an error.
    pub fn eval_number(&self, variables: &Variables) -> Result<f64> {
        self.tree
            .eval_float_with_context(&variables.context)
            .map_err(|error| anyhow!("expression '{}' failed: {}", self.source, error))
    }
}

/// The variables an expression can read.
#[derive(Debug, Default)]
pub struct Variables {
    context: HashMapContext<DefaultNumericTypes>,
}

impl Variables {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a payload's fields, flattened under `prefix` (`prefix.a.b`, `prefix[0]`), or as they are
    /// (`a.b`) with an empty prefix. Numbers are floats, so `value / 2` never rounds.
    pub fn add_payload(&mut self, prefix: &str, payload: &Value) {
        self.add(prefix, payload, false);
    }

    /// Like `add_payload`, but whole numbers stay integers: for values expressions wrote (a `reduce`
    /// state), so that an integer counter stays one.
    pub fn add_typed(&mut self, prefix: &str, values: &Value) {
        self.add(prefix, values, true);
    }

    fn add(&mut self, prefix: &str, value: &Value, keep_integers: bool) {
        match value {
            Value::Number(number) => match number.as_i64() {
                Some(integer) if keep_integers => self.set(prefix, ExpressionValue::Int(integer)),
                _ => {
                    if let Some(float) = number.as_f64() {
                        self.set(prefix, ExpressionValue::Float(float));
                    }
                }
            },
            Value::Bool(boolean) => self.set(prefix, ExpressionValue::Boolean(*boolean)),
            Value::String(string) => self.set(prefix, ExpressionValue::String(string.clone())),
            Value::Object(map) => {
                for (key, value) in map {
                    let name = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };
                    self.add(&name, value, keep_integers);
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    self.add(&format!("{}[{}]", prefix, index), value, keep_integers);
                }
            }
            Value::Null => {}
        }
    }

    pub fn set(&mut self, name: &str, value: ExpressionValue) {
        // Only fails for a type change of an existing variable, which a rebuilt context can't have.
        let _ = self.context.set_value(name.to_string(), value);
    }
}

/// An expression result as JSON: a number, boolean or string.
pub fn to_json(value: ExpressionValue) -> Result<Value> {
    match value {
        ExpressionValue::Float(float) => Number::from_f64(float)
            .map(Value::Number)
            .ok_or_else(|| anyhow!("result {} is not a finite number", float)),
        ExpressionValue::Int(integer) => Ok(Value::from(integer)),
        ExpressionValue::Boolean(boolean) => Ok(Value::Bool(boolean)),
        ExpressionValue::String(string) => Ok(Value::String(string)),
        other => Err(anyhow!(
            "result {} is not a number, boolean or string",
            other
        )),
    }
}

/// Rewrites bare calls of the `MATH_ALIASES` names (`sqrt(x)`) to their evalexpr functions. A name
/// that is part of a path (`math::sqrt`, `device.log`), not followed by `(`, or inside a string is
/// left alone.
fn rewrite_math_aliases(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut rewritten = String::with_capacity(source.len());
    let mut index = 0;
    let mut in_string = false;

    while index < chars.len() {
        let c = chars[index];
        if in_string {
            rewritten.push(c);
            if c == '\\' && index + 1 < chars.len() {
                rewritten.push(chars[index + 1]);
                index += 1;
            } else if c == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            rewritten.push(c);
            index += 1;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            let in_path = start > 0 && matches!(chars[start - 1], ':' | '.');
            let called = chars[index..]
                .iter()
                .find(|next| !next.is_whitespace())
                .is_some_and(|next| *next == '(');
            match MATH_ALIASES.iter().find(|(alias, _)| *alias == word) {
                Some((_, function)) if called && !in_path => rewritten.push_str(function),
                _ => rewritten.push_str(&word),
            }
            continue;
        }
        rewritten.push(c);
        index += 1;
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn variables(payload: Value) -> Variables {
        let mut variables = Variables::new();
        variables.add_payload("", &payload);
        variables
    }

    fn number(source: &str, payload: Value) -> f64 {
        Expression::parse(source)
            .unwrap()
            .eval_number(&variables(payload))
            .unwrap_or_else(|error| panic!("{source}: {error}"))
    }

    #[test]
    fn rewrites_only_bare_calls_of_the_aliases() {
        let cases = [
            ("sqrt(x)", "math::sqrt(x)"),
            ("log (x)", "math::log10 (x)"),
            ("math::sqrt(x)", "math::sqrt(x)"),
            ("math::log(x, 2.0)", "math::log(x, 2.0)"),
            ("log * 2.0", "log * 2.0"),
            ("device.exp + 1.0", "device.exp + 1.0"),
            ("floor(x) + ceil(x)", "floor(x) + ceil(x)"),
            (
                "if(name == \"sqrt(x)\", 1.0, 0.0)",
                "if(name == \"sqrt(x)\", 1.0, 0.0)",
            ),
            ("sqrt_ratio(x)", "sqrt_ratio(x)"),
        ];
        for (source, expected) in cases {
            assert_eq!(rewrite_math_aliases(source), expected, "{source}");
        }
    }

    #[test]
    fn evaluates_math_functions_bare_and_namespaced() {
        let payload = json!({ "x": 16.0 });
        assert_eq!(number("sqrt(x)", payload.clone()), 4.0);
        assert_eq!(number("math::sqrt(x)", payload.clone()), 4.0);
        assert_eq!(number("log(100.0)", payload.clone()), 2.0);
        assert_eq!(number("math::log(8.0, 2.0)", payload.clone()), 3.0);
        assert_eq!(number("math::pow(2.0, 3.0)", payload.clone()), 8.0);
        assert_eq!(
            number("floor(2.7) + ceil(2.2) + round(2.5)", payload.clone()),
            8.0
        );
        assert_eq!(number("abs(-x) + exp(0.0)", payload), 17.0);
    }

    #[test]
    fn reads_flattened_fields_including_ones_named_like_functions() {
        let payload = json!({ "log": 2.0, "device": { "exp": 3.0 }, "items": [{ "value": 4.0 }] });
        assert_eq!(number("log * device.exp + items[0].value", payload), 10.0);

        let mut prefixed = Variables::new();
        prefixed.add_payload("prev", &json!({ "x": 1.5 }));
        assert_eq!(
            Expression::parse("prev.x * 2.0")
                .unwrap()
                .eval_number(&prefixed)
                .unwrap(),
            3.0
        );
    }

    #[test]
    fn typed_values_keep_their_integers() {
        let mut variables = Variables::new();
        variables.add_payload("payload", &json!({ "n": 2 }));
        variables.add_typed("state", &json!({ "n": 2, "total": 2.0 }));
        let eval = |source: &str| {
            to_json(Expression::parse(source).unwrap().eval(&variables).unwrap()).unwrap()
        };

        assert_eq!(eval("payload.n / 4"), json!(0.5));
        assert_eq!(eval("state.n + 1"), json!(3));
        assert_eq!(eval("state.total + 1"), json!(3.0));
    }

    #[test]
    fn returns_typed_results() {
        let eval = |source: &str| {
            let mut variables = variables(json!({ "value": 3.0, "name": "MM1" }));
            variables.set("count", ExpressionValue::Int(2));
            to_json(Expression::parse(source).unwrap().eval(&variables).unwrap())
        };
        assert_eq!(eval("value * 2.0").unwrap(), json!(6.0));
        assert_eq!(eval("count + 1").unwrap(), json!(3));
        assert_eq!(eval("value > 2.0").unwrap(), json!(true));
        assert_eq!(eval("str::to_lowercase(name)").unwrap(), json!("mm1"));
        assert!(eval("1.0 / 0.0").is_err(), "infinity is not a JSON number");
    }

    #[test]
    fn reports_syntax_errors_at_parse_and_missing_variables_at_eval() {
        assert!(Expression::parse("(value + 1.0").is_err());
        assert!(Expression::parse("1.0 2.0").is_err());
        let error = Expression::parse("missing + 1.0")
            .unwrap()
            .eval(&variables(json!({})))
            .unwrap_err();
        assert!(error.to_string().contains("missing"), "{error}");
    }

    #[test]
    fn a_number_result_must_be_a_float() {
        let expression = Expression::parse("2 + 3").unwrap();
        assert!(expression.eval_number(&Variables::new()).is_err());
        assert_eq!(
            Expression::parse("2.0 + 3")
                .unwrap()
                .eval_number(&Variables::new())
                .unwrap(),
            5.0
        );
    }
}
