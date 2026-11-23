// SPDX-License-Identifier: Apache-2.0

use fsl_testgen_core::render::{Language, literal};
use serde_json::Value;

/// Append the rejected step of a forbidden scenario and its assertion.
pub(super) fn append_forbidden(
    lines: &mut Vec<String>,
    scenario: &Value,
    language: Language,
    indent: &str,
) {
    if scenario["kind"].as_str() != Some("forbidden") || !scenario["forbidden_step"].is_object() {
        return;
    }
    let step = &scenario["forbidden_step"];
    let rejected = scenario
        .get("rejected_by")
        .filter(|value| !value.is_null())
        .map_or_else(|| "null".to_owned(), |value| literal(value, language));
    let action = literal(&step["action"], language);
    let params = literal(&step["params"], language);
    match language {
        Language::Swift => {
            lines.push(format!("{indent}let result = a.step({action}, {params})"));
            lines.push(format!("{indent}assertRejected(result, {rejected})"));
        }
        Language::Kotlin => {
            lines.push(format!("{indent}val result = a.step({action}, {params})"));
            lines.push(format!("{indent}assertRejected(result, {rejected})"));
        }
        Language::Dart => {
            lines.push(format!(
                "{indent}final result = a.step({action}, {params});"
            ));
            lines.push(format!("{indent}assertRejected(result, {rejected});"));
        }
        Language::Php => {
            lines.push(format!("{indent}$result = $a->step({action}, {params});"));
            lines.push(format!(
                "{indent}$this->assertRejected($result, {rejected});"
            ));
        }
    }
}
