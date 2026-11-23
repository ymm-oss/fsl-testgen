// SPDX-License-Identifier: Apache-2.0

//! The closed-shape check of an embedded `test-plan.v1` document. Copied from
//! ymm-oss/fsl (`fsl-tools`). Generators do not read the plan; a document
//! that carries a malformed one is still rejected.

use serde_json::Value;

use crate::contract::{
    KERNEL_SCHEMA_VERSION, TEST_PLAN_V1_SCHEMA_ID, TEST_PLAN_V1_SCHEMA_VERSION, required_array,
    required_object, required_str,
};

const ALLOWED_OUTCOME_KINDS: &[&str] = &[
    "ok",
    "requires_failed",
    "partial_op",
    "type_bound",
    "invariant",
    "trans",
    "ensures",
];

/// Fail-closed semantic validation for a closed `test-plan.v1` document.
///
/// # Errors
///
/// Returns an error when schema identity, required fields, or semantic invariants
/// are violated.
pub fn validate_test_plan_v1(plan: &Value) -> Result<(), String> {
    let root = required_object(plan, "test-plan root")?;
    for key in root.keys() {
        if !matches!(
            key.as_str(),
            "$schema"
                | "schema_version"
                | "kernel_schema_version"
                | "fsl"
                | "result"
                | "spec"
                | "depth"
                | "initial"
                | "formal_result"
                | "assurance_effect"
                | "oracle"
                | "selection_coverage"
                | "do_not_assume"
                | "layer_selection"
                | "cases"
        ) {
            return Err(format!("unknown test-plan field '{key}'"));
        }
    }
    let schema = required_str(root, "$schema", "test-plan")?;
    if schema != TEST_PLAN_V1_SCHEMA_ID {
        return Err(format!(
            "unsupported test-plan $schema '{schema}'; expected '{TEST_PLAN_V1_SCHEMA_ID}'"
        ));
    }
    let version = required_str(root, "schema_version", "test-plan")?;
    if version != TEST_PLAN_V1_SCHEMA_VERSION {
        return Err(format!(
            "unsupported test-plan schema_version '{version}'; expected '{TEST_PLAN_V1_SCHEMA_VERSION}'"
        ));
    }
    let kernel_version = required_str(root, "kernel_schema_version", "test-plan")?;
    if kernel_version != KERNEL_SCHEMA_VERSION {
        return Err(format!(
            "unsupported test-plan kernel_schema_version '{kernel_version}'; expected '{KERNEL_SCHEMA_VERSION}'"
        ));
    }
    if root.get("result").and_then(Value::as_str) != Some("testplan") {
        return Err("test-plan result must be 'testplan'".to_owned());
    }
    if root.get("formal_result").and_then(Value::as_str) != Some("not_run") {
        return Err("test-plan formal_result must be 'not_run'".to_owned());
    }
    if root.get("assurance_effect").and_then(Value::as_str) != Some("none") {
        return Err("test-plan assurance_effect must be 'none'".to_owned());
    }
    let oracle = required_object(
        root.get("oracle").ok_or("test-plan oracle is required")?,
        "oracle",
    )?;
    if oracle.get("evaluator_reimplemented") != Some(&Value::Bool(false)) {
        return Err("test-plan oracle.evaluator_reimplemented must be false".to_owned());
    }
    let coverage = required_object(
        root.get("selection_coverage")
            .ok_or("test-plan selection_coverage is required")?,
        "selection_coverage",
    )?;
    let available = coverage
        .get("vectors_available")
        .and_then(Value::as_u64)
        .ok_or("selection_coverage.vectors_available must be a non-negative integer")?;
    let selected = coverage
        .get("vectors_selected")
        .and_then(Value::as_u64)
        .ok_or("selection_coverage.vectors_selected must be a non-negative integer")?;
    if selected > available {
        return Err("selection_coverage.vectors_selected exceeds vectors_available".to_owned());
    }
    let cases = required_array(root, "cases", "test-plan")?;
    if cases.is_empty() {
        return Err("test-plan cases must be non-empty".to_owned());
    }
    for (index, case) in cases.iter().enumerate() {
        validate_case(case, index)?;
    }
    Ok(())
}

fn validate_case(case: &Value, index: usize) -> Result<(), String> {
    let object = required_object(case, &format!("cases[{index}]"))?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "name" | "setup" | "target" | "expected" | "source_vector"
        ) {
            return Err(format!("unknown test-plan case field '{key}'"));
        }
    }
    let expected = required_object(
        object
            .get("expected")
            .ok_or_else(|| format!("cases[{index}].expected is required"))?,
        &format!("cases[{index}].expected"),
    )?;
    let kind = required_str(
        expected,
        "outcome_kind",
        &format!("cases[{index}].expected"),
    )?;
    if !ALLOWED_OUTCOME_KINDS.contains(&kind) {
        return Err(format!("unsupported outcome kind '{kind}'"));
    }
    Ok(())
}
