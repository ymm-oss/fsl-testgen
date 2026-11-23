// SPDX-License-Identifier: Apache-2.0

//! Literal, identifier and template helpers shared by targets. Copied from
//! the emitters of ymm-oss/fsl (`fsl-tools`, `testgen.rs`); their output is
//! byte-for-byte what `fslc testgen` prints.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

/// One visible spelling of a source file name, safe in every built-in
/// target's header context: docstrings, line/block comments, and PHP source
/// alike.
#[must_use]
pub fn escaped_source_name(name: &str) -> String {
    let mut escaped = String::new();
    for character in name.chars() {
        if character.is_ascii_control()
            || matches!(character, '\\' | '\'' | '"' | '*' | '/' | '?' | '>')
        {
            let _ = write!(escaped, "\\x{:02x}", u32::from(character));
        } else {
            escaped.push(character);
        }
    }
    escaped
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Insert the escaped source name into a checked-in template.
#[must_use]
pub fn source_template(template: &str, source_name: &str) -> String {
    normalize_newlines(template).replace("__SOURCE__", source_name)
}

/// Fill trusted named slots before inserting the escaped source name as data.
#[must_use]
pub fn fill_template(template: &str, source_name: &str, slots: &[(&str, String)]) -> String {
    let mut text = template.to_owned();
    for (key, value) in slots {
        debug_assert!(text.contains(key), "template slot {key} missing");
        text = text.replace(key, value);
    }
    source_template(&text, source_name)
}

/// A single-quoted Python string literal.
#[must_use]
pub fn python_string(value: &str) -> String {
    let mut result = String::from("'");
    for character in value.chars() {
        match character {
            '\\' => result.push_str("\\\\"),
            '\'' => result.push_str("\\'"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            value if value.is_control() => {
                let _ = write!(result, "\\x{:02x}", u32::from(value));
            }
            value => result.push(value),
        }
    }
    result.push('\'');
    result
}

/// A Python literal of a JSON value. The members of a top-level object are
/// printed in `key_order` first, then in the order given.
#[must_use]
pub fn python_literal(value: &Value, key_order: &[String]) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => python_string(value),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| python_literal(value, &[]))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(values) => {
            let ordered = key_order
                .iter()
                .filter(|key| values.contains_key(*key))
                .chain(values.keys().filter(|key| !key_order.contains(key)))
                .collect::<Vec<_>>();
            format!(
                "{{{}}}",
                ordered
                    .into_iter()
                    .map(|key| format!(
                        "{}: {}",
                        python_string(key),
                        python_literal(&values[key], &[])
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

/// A JSON value on one line, as a JavaScript/TypeScript literal.
#[must_use]
pub fn json_inline(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(json_inline)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_owned()),
                    json_inline(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()),
    }
}

/// A JavaScript/TypeScript array literal with one element per line.
#[must_use]
pub fn json_rows(values: &[Value]) -> String {
    if values.is_empty() {
        return "[]".to_owned();
    }
    format!(
        "[\n{},\n]",
        values
            .iter()
            .map(|value| format!("  {}", json_inline(value)))
            .collect::<Vec<_>>()
            .join(",\n")
    )
}

/// The languages [`quoted`] and [`literal`] print.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    Swift,
    Kotlin,
    Dart,
    Php,
}

/// A string literal of `language`.
#[must_use]
pub fn quoted(value: &str, language: Language) -> String {
    let quote = if matches!(language, Language::Dart | Language::Php) {
        '\''
    } else {
        '"'
    };
    let mut result = String::from(quote);
    for character in value.chars() {
        match (language, character) {
            (Language::Php, '\\') => result.push_str("\\\\"),
            (Language::Php | Language::Dart, '\'') => result.push_str("\\'"),
            (Language::Php, _) => result.push(character),
            (_, '"') if quote == '"' => result.push_str("\\\""),
            (_, '\\') => result.push_str("\\\\"),
            (Language::Kotlin | Language::Dart, '$') => result.push_str("\\$"),
            (_, '\n') => result.push_str("\\n"),
            (_, '\t') => result.push_str("\\t"),
            (_, '\r') => result.push_str("\\r"),
            (Language::Swift, '\0') => result.push_str("\\0"),
            (Language::Swift | Language::Dart, value) if value < ' ' => {
                let _ = write!(result, "\\u{{{:x}}}", u32::from(value));
            }
            (Language::Kotlin, value) if value < ' ' => {
                let _ = write!(result, "\\u{:04x}", u32::from(value));
            }
            (_, value) => result.push(value),
        }
    }
    result.push(quote);
    result
}

/// A literal of a JSON value in `language`, members in the order given.
#[must_use]
pub fn literal(value: &Value, language: Language) -> String {
    match value {
        Value::Null => match language {
            Language::Swift => "FSLNull.instance".to_owned(),
            _ => "null".to_owned(),
        },
        Value::Bool(value) => value.to_string(),
        Value::Number(value) if matches!(language, Language::Kotlin) => match value.as_i64() {
            Some(i64::MIN) => "Long.MIN_VALUE".to_owned(),
            Some(integer) if i32::try_from(integer).is_err() => format!("{integer}L"),
            _ => value.to_string(),
        },
        Value::Number(value)
            if matches!(language, Language::Php) && value.as_i64() == Some(i64::MIN) =>
        {
            "PHP_INT_MIN".to_owned()
        }
        Value::Number(value) => value.to_string(),
        Value::String(value) => quoted(value, language),
        Value::Array(values) if values.is_empty() => match language {
            Language::Swift | Language::Php => "[]".to_owned(),
            Language::Kotlin => "listOf<Any?>()".to_owned(),
            Language::Dart => "<dynamic>[]".to_owned(),
        },
        Value::Array(values) => {
            let items = values
                .iter()
                .map(|value| literal(value, language))
                .collect::<Vec<_>>()
                .join(", ");
            match language {
                Language::Kotlin => format!("listOf({items})"),
                _ => format!("[{items}]"),
            }
        }
        Value::Object(values) if values.is_empty() => match language {
            Language::Swift => "[String: Any]()".to_owned(),
            Language::Kotlin => "mapOf<String, Any?>()".to_owned(),
            Language::Dart => "<String, dynamic>{}".to_owned(),
            Language::Php => "[]".to_owned(),
        },
        Value::Object(values) => {
            let items = values
                .iter()
                .map(|(key, value)| match language {
                    Language::Swift | Language::Dart => {
                        format!("{}: {}", quoted(key, language), literal(value, language))
                    }
                    Language::Kotlin => {
                        format!("{} to {}", quoted(key, language), literal(value, language))
                    }
                    Language::Php => {
                        format!("{} => {}", quoted(key, language), literal(value, language))
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            match language {
                Language::Kotlin => format!("mapOf({items})"),
                Language::Dart => format!("{{{items}}}"),
                _ => format!("[{items}]"),
            }
        }
    }
}

/// `value` reduced to ASCII letters, digits and single underscores.
#[must_use]
pub fn safe_ident(value: &str) -> String {
    let mut result = String::new();
    let mut underscore = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            result.push(character);
            underscore = false;
        } else if !underscore {
            result.push('_');
            underscore = true;
        }
    }
    let result = result.trim_matches('_');
    if result.is_empty() {
        "scenario".to_owned()
    } else {
        result.to_owned()
    }
}

/// `prefix` + [`safe_ident`], numbered from the second use of the same name.
pub fn unique_ident(value: &str, seen: &mut BTreeMap<String, usize>, prefix: &str) -> String {
    let safe = safe_ident(value);
    let count = seen.entry(safe.clone()).or_default();
    *count += 1;
    if *count == 1 {
        format!("{prefix}{safe}")
    } else {
        format!("{prefix}{safe}_{count}")
    }
}

/// The name, steps and expected states of one scenario.
#[must_use]
pub fn scenario_parts(scenario: &Value) -> (&str, Vec<Value>, Vec<Value>) {
    (
        scenario["name"].as_str().unwrap_or("scenario"),
        scenario["steps"].as_array().cloned().unwrap_or_default(),
        scenario["expected_states"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn checked_in_templates_emit_the_same_text_after_windows_checkout() {
        let unix = "Source: __SOURCE__\nbody\n";
        let windows = "Source: __SOURCE__\r\nbody\r\n";
        assert_eq!(
            source_template(unix, "demo.fsl"),
            source_template(windows, "demo.fsl")
        );
    }

    #[test]
    fn source_names_are_escaped_for_every_header() {
        assert_eq!(
            escaped_source_name("we'ird*na>me?.fsl"),
            "we\\x27ird\\x2ana\\x3eme\\x3f.fsl"
        );
        assert_eq!(escaped_source_name("a/b\\c\"\n"), "a\\x2fb\\x5cc\\x22\\x0a");
    }

    #[test]
    fn literals_are_spelled_per_language() {
        let value = json!({"k": [1, null, true, "a'b\"$\n"], "empty": {}, "none": []});
        assert_eq!(
            python_literal(&value, &["none".to_owned()]),
            "{'none': [], 'k': [1, None, True, 'a\\'b\"$\\n'], 'empty': {}}"
        );
        assert_eq!(
            json_inline(&value),
            "{\"k\": [1, null, true, \"a'b\\\"$\\n\"], \"empty\": {}, \"none\": []}"
        );
        assert_eq!(
            literal(&value, Language::Swift),
            "[\"k\": [1, FSLNull.instance, true, \"a'b\\\"$\\n\"], \"empty\": [String: Any](), \"none\": []]"
        );
        assert_eq!(
            literal(&value, Language::Kotlin),
            "mapOf(\"k\" to listOf(1, null, true, \"a'b\\\"\\$\\n\"), \"empty\" to mapOf<String, Any?>(), \"none\" to listOf<Any?>())"
        );
        assert_eq!(
            literal(&value, Language::Dart),
            "{'k': [1, null, true, 'a\\'b\"\\$\\n'], 'empty': <String, dynamic>{}, 'none': <dynamic>[]}"
        );
        assert_eq!(
            literal(&value, Language::Php),
            "['k' => [1, null, true, 'a\\'b\"$\n'], 'empty' => [], 'none' => []]"
        );
        assert_eq!(
            literal(&json!(i64::MIN), Language::Kotlin),
            "Long.MIN_VALUE"
        );
        assert_eq!(
            literal(&json!(4_294_967_296_i64), Language::Kotlin),
            "4294967296L"
        );
        assert_eq!(literal(&json!(i64::MIN), Language::Php), "PHP_INT_MIN");
        assert_eq!(json_rows(&[]), "[]");
        assert_eq!(json_rows(&[json!(1), json!([2])]), "[\n  1,\n  [2],\n]");
    }

    #[test]
    fn identifiers_are_sanitized_and_numbered() {
        let mut seen = BTreeMap::new();
        assert_eq!(
            unique_ident("reach_bank.Settled", &mut seen, "test_scenario_"),
            "test_scenario_reach_bank_Settled"
        );
        assert_eq!(
            unique_ident("reach_bank-Settled", &mut seen, "test_scenario_"),
            "test_scenario_reach_bank_Settled_2"
        );
        assert_eq!(safe_ident("..."), "scenario");
        assert_eq!(safe_ident("a  b__c"), "a_b__c");
    }
}
