//! Argument validation for tool calls: a small JSON Schema subset.
//!
//! The router model's arguments are checked before any tool runs, and a call
//! that fails is sent back to the model once with the problems listed. The
//! schemas are our own (one per tool, written by hand), so a full JSON Schema
//! implementation would be all cost: the `jsonschema` crate pulls in HTTP and
//! file resolvers we would have to switch off, for keywords no tool uses.
//!
//! Supported: `type` (object, string, integer, number, boolean, array),
//! `properties`, `required`, `additionalProperties: false`, `enum` (strings),
//! `minimum` / `maximum`, `minLength` / `maxLength`, `items`, `minItems` /
//! `maxItems`, plus `description` and `format` as annotations.
//! [`check_schema`] refuses anything else, and a registry test runs it over
//! every tool, so a schema can never quietly use a keyword that is not checked.

use serde_json::{Map, Value};

const KNOWN_KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "enum",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "items",
    "minItems",
    "maxItems",
    "description",
    "format",
];

/// Check `value` against `schema`. On failure, every problem found, each naming
/// its path (`subject`, `attendees[1]`) so the model can fix it in one go.
pub fn validate(schema: &Value, value: &Value) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    check(schema, value, "", &mut problems);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Refuse a schema that uses a keyword this validator would ignore.
pub fn check_schema(schema: &Value) -> Result<(), String> {
    let Some(object) = schema.as_object() else {
        return Err("schema is not an object".into());
    };
    for key in object.keys() {
        if !KNOWN_KEYWORDS.contains(&key.as_str()) {
            return Err(format!("unsupported keyword `{key}`"));
        }
    }
    if let Some(additional) = object.get("additionalProperties") {
        if additional != &Value::Bool(false) {
            return Err("only `additionalProperties: false` is supported".into());
        }
    }
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        for (name, sub) in properties {
            check_schema(sub).map_err(|e| format!("{name}: {e}"))?;
        }
    }
    if let Some(items) = object.get("items") {
        check_schema(items).map_err(|e| format!("items: {e}"))?;
    }
    Ok(())
}

fn label(path: &str) -> String {
    if path.is_empty() {
        "arguments".to_string()
    } else {
        format!("`{path}`")
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn matches_type(expected: &str, value: &Value) -> bool {
    match expected {
        "object" => value.is_object(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        // A whole-number float ("3.0") is an integer for our purposes: models
        // emit both, and refusing one would cost a repair round for nothing.
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || value.as_f64().is_some_and(|f| f.fract() == 0.0 && f.is_finite())
        }
        "number" => value.is_number(),
        _ => false,
    }
}

fn check(schema: &Value, value: &Value, path: &str, problems: &mut Vec<String>) {
    let Some(schema) = schema.as_object() else {
        return;
    };

    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        if !matches_type(expected, value) {
            problems.push(format!(
                "{} must be {} {expected}, got {}",
                label(path),
                if expected.starts_with(['a', 'e', 'i', 'o', 'u']) {
                    "an"
                } else {
                    "a"
                },
                type_name(value)
            ));
            // Nothing below makes sense for a value of the wrong type.
            return;
        }
    }

    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            let options: Vec<String> = allowed
                .iter()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), |s| format!("\"{s}\"")))
                .collect();
            problems.push(format!(
                "{} must be one of {}",
                label(path),
                options.join(", ")
            ));
        }
    }

    match value {
        Value::Object(object) => check_object(schema, object, path, problems),
        Value::String(text) => {
            let length = text.chars().count() as u64;
            if let Some(min) = schema.get("minLength").and_then(Value::as_u64) {
                if length < min {
                    problems.push(format!("{} must be at least {min} characters", label(path)));
                }
            }
            if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
                if length > max {
                    problems.push(format!("{} must be at most {max} characters", label(path)));
                }
            }
        }
        Value::Number(number) => {
            let n = number.as_f64().unwrap_or(f64::NAN);
            if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
                if n < min {
                    problems.push(format!("{} must be at least {min}", label(path)));
                }
            }
            if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
                if n > max {
                    problems.push(format!("{} must be at most {max}", label(path)));
                }
            }
        }
        Value::Array(items) => {
            let count = items.len() as u64;
            if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
                if count < min {
                    problems.push(format!("{} needs at least {min} items", label(path)));
                }
            }
            if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
                if count > max {
                    problems.push(format!("{} allows at most {max} items", label(path)));
                }
            }
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    check(item_schema, item, &format!("{path}[{index}]"), problems);
                }
            }
        }
        _ => {}
    }
}

fn check_object(
    schema: &Map<String, Value>,
    object: &Map<String, Value>,
    path: &str,
    problems: &mut Vec<String>,
) {
    let properties = schema.get("properties").and_then(Value::as_object);
    let child = |key: &str| {
        if path.is_empty() {
            key.to_string()
        } else {
            format!("{path}.{key}")
        }
    };

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            // A null is how several models say "I have nothing for this"; for a
            // required field that is the same as leaving it out.
            if object.get(key).map_or(true, Value::is_null) {
                problems.push(format!("`{}` is required", child(key)));
            }
        }
    }

    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        for key in object.keys() {
            if !properties.is_some_and(|p| p.contains_key(key)) {
                problems.push(format!("`{}` is not a known argument", child(key)));
            }
        }
    }

    if let Some(properties) = properties {
        for (key, sub_schema) in properties {
            match object.get(key) {
                // An optional field sent as null means "not given".
                None | Some(Value::Null) => {}
                Some(value) => check(sub_schema, value, &child(key), problems),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn email_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {"type": "string", "minLength": 1},
                "subject": {"type": "string", "maxLength": 10},
                "priority": {"type": "string", "enum": ["low", "high"]},
                "count": {"type": "integer", "minimum": 1, "maximum": 5},
                "cc": {"type": "array", "items": {"type": "string"}, "maxItems": 2}
            },
            "required": ["to"],
            "additionalProperties": false
        })
    }

    #[test]
    fn a_valid_call_passes() {
        assert_eq!(
            validate(&email_schema(), &json!({"to": "Ron", "count": 2, "cc": ["a"]})),
            Ok(())
        );
    }

    /// The repair prompt hands every problem back at once, so the model fixes
    /// the call in one round rather than one problem per round.
    #[test]
    fn every_problem_is_reported_with_its_path() {
        let problems = validate(
            &email_schema(),
            &json!({"subject": "far too long a subject", "priority": "urgent",
                    "count": 9, "cc": ["a", 2, "c"], "bcc": "x"}),
        )
        .unwrap_err();
        let joined = problems.join(" | ");
        assert!(joined.contains("`to` is required"), "{joined}");
        assert!(joined.contains("`subject` must be at most 10"), "{joined}");
        assert!(joined.contains("`priority` must be one of"), "{joined}");
        assert!(joined.contains("`count` must be at most 5"), "{joined}");
        assert!(joined.contains("`cc[1]` must be a string"), "{joined}");
        assert!(joined.contains("`cc` allows at most 2"), "{joined}");
        assert!(joined.contains("`bcc` is not a known argument"), "{joined}");
    }

    #[test]
    fn a_null_required_field_counts_as_missing_and_a_null_optional_one_as_absent() {
        let problems = validate(&email_schema(), &json!({"to": null})).unwrap_err();
        assert_eq!(problems, vec!["`to` is required".to_string()]);
        assert_eq!(
            validate(&email_schema(), &json!({"to": "Ron", "subject": null})),
            Ok(())
        );
    }

    #[test]
    fn a_whole_float_is_accepted_as_an_integer() {
        assert_eq!(
            validate(&email_schema(), &json!({"to": "Ron", "count": 3.0})),
            Ok(())
        );
        assert!(validate(&email_schema(), &json!({"to": "Ron", "count": 2.5})).is_err());
    }

    #[test]
    fn the_wrong_top_level_type_is_one_clear_problem() {
        assert_eq!(
            validate(&email_schema(), &json!("send it")).unwrap_err(),
            vec!["arguments must be an object, got string".to_string()]
        );
    }

    #[test]
    fn character_counts_are_unicode_aware() {
        let schema = json!({"type": "string", "maxLength": 4});
        assert_eq!(validate(&schema, &json!("ईशान")), Ok(()));
    }

    #[test]
    fn check_schema_refuses_keywords_it_would_ignore() {
        assert!(check_schema(&email_schema()).is_ok());
        assert!(check_schema(&json!({"type": "string", "pattern": "^a"})).is_err());
        assert!(check_schema(&json!({"type": "object", "properties": {"a": {"oneOf": []}}})).is_err());
        assert!(check_schema(&json!({"type": "object", "additionalProperties": true})).is_err());
    }
}
