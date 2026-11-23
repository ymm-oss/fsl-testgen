// SPDX-License-Identifier: Apache-2.0

//! Reading `testgen-input.v1`: the one document `fslc testgen-input` emits.
//!
//! [`TestgenInput::from_document`] is the inverse of the writer in
//! ymm-oss/fsl (`fsl-tools`, `testgen/input_document.rs`) and runs the same
//! cross-checks, in the same order and with the same messages. A
//! [`TestgenInput`] exists only for a document that passed all of them, so a
//! target never sees one that did not.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::contract::{
    KERNEL_SCHEMA_VERSION, TESTGEN_INPUT_V1_SCHEMA_ID, TESTGEN_INPUT_V1_SCHEMA_VERSION,
    TESTGEN_TRACE_V1_SCHEMA_ID, TESTGEN_TRACE_V1_SCHEMA_VERSION, public_kernel_v1_root,
    required_array, required_object, required_str, unmapped_public_spelling,
};
use crate::render::escaped_source_name;
use crate::test_plan::validate_test_plan_v1;

const RESULT: &str = "testgen_input";
const COMPOSE_DIALECT: &str = "compose";
const ROOT: &str = "testgen-input";

/// One public action and its parameter names, in declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Action {
    pub name: String,
    pub params: Vec<String>,
}

/// A `testgen-input.v1` document that passed every cross-check.
///
/// State and action names are public, as the scenarios and the walk print
/// them. Scenarios, state objects and parameter objects are in the order the
/// document gives them; targets print them in that order.
#[derive(Clone, Debug, PartialEq)]
pub struct TestgenInput {
    spec_name: String,
    source_name: String,
    state_order: Vec<String>,
    actions: Vec<Action>,
    scenarios: Vec<Value>,
    walk: Value,
    document: Value,
}

impl TestgenInput {
    /// Name of the checked specification.
    #[must_use]
    pub fn spec_name(&self) -> &str {
        &self.spec_name
    }

    /// `source.name`, escaped so that it is safe in every built-in target's
    /// header: docstrings, line and block comments, and PHP source alike.
    #[must_use]
    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    /// Public state field names, in the order scenario states are keyed.
    #[must_use]
    pub fn state_order(&self) -> &[String] {
        &self.state_order
    }

    /// Public actions, in declaration order.
    #[must_use]
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// Parameter names of `action`, or none for a name `actions` lacks.
    #[must_use]
    pub fn params_of(&self, action: &str) -> &[String] {
        self.actions
            .iter()
            .find(|item| item.name == action)
            .map_or(&[][..], |item| item.params.as_slice())
    }

    /// The normalized scenarios.
    #[must_use]
    pub fn scenarios(&self) -> &[Value] {
        &self.scenarios
    }

    /// The `testgen-trace.v1` walk, as given.
    #[must_use]
    pub fn walk(&self) -> &Value {
        &self.walk
    }

    /// The whole document, as given.
    #[must_use]
    pub fn document(&self) -> &Value {
        &self.document
    }
}

/// Parse the text of a document, keeping the order of object members.
///
/// # Errors
///
/// Rejects text that is not one JSON value.
pub fn parse_document(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| format!("{ROOT} is not JSON: {error}"))
}

fn source_order(value: &Value, index: usize) -> (u64, u64, usize) {
    let span = value.get("span").and_then(Value::as_object);
    (
        span.and_then(|span| span.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX),
        span.and_then(|span| span.get("column"))
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX),
        index,
    )
}

fn validate_step(step: &Value, context: &str) -> Result<(), String> {
    let step = step
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    if !step.get("action").is_some_and(Value::is_string) {
        return Err(format!("{context}.action must be a string"));
    }
    if !step.get("params").is_some_and(Value::is_object) {
        return Err(format!("{context}.params must be an object"));
    }
    Ok(())
}

fn validate_closed_object(
    object: &Map<String, Value>,
    context: &str,
    allowed: &[&str],
) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{context} has unknown field '{key}'"));
    }
    Ok(())
}

fn validate_state_fields(
    state: &Value,
    context: &str,
    state_names: &BTreeSet<&str>,
    require_all: bool,
) -> Result<(), String> {
    let state = state
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    let actual = state.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if !actual.is_subset(state_names) {
        let unknown = actual.difference(state_names).copied().collect::<Vec<_>>();
        return Err(format!(
            "{context} has unknown state fields: {}",
            unknown.join(", ")
        ));
    }
    if require_all && actual != *state_names {
        return Err(format!(
            "{context} must contain every public Kernel state field"
        ));
    }
    Ok(())
}

fn validate_contract_step(
    step: &Value,
    context: &str,
    actions: &BTreeMap<&str, BTreeSet<&str>>,
) -> Result<(), String> {
    validate_step(step, context)?;
    let step = step.as_object().expect("validate_step checked the object");
    let action = step["action"]
        .as_str()
        .expect("validate_step checked the action");
    let expected_params = actions
        .get(action)
        .ok_or_else(|| format!("{context}.action names unknown action '{action}'"))?;
    let actual_params = step["params"]
        .as_object()
        .expect("validate_step checked params")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual_params != *expected_params {
        return Err(format!(
            "{context}.params must match public Kernel parameters for '{action}'"
        ));
    }
    Ok(())
}

fn validate_scenarios(
    scenarios: &Value,
    spec_name: &str,
    state_names: &BTreeSet<&str>,
    actions: &BTreeMap<&str, BTreeSet<&str>>,
) -> Result<Vec<Value>, String> {
    let root = scenarios
        .as_object()
        .ok_or_else(|| "testgen scenarios root must be an object".to_owned())?;
    let scenario_spec = root
        .get("spec")
        .and_then(Value::as_str)
        .ok_or_else(|| "testgen scenarios.spec must be a string".to_owned())?;
    if scenario_spec != spec_name {
        return Err(format!(
            "testgen scenarios spec '{scenario_spec}' does not match public Kernel spec '{spec_name}'"
        ));
    }
    let scenarios = root
        .get("scenarios")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "testgen scenarios.scenarios must be an array".to_owned())?;
    for (index, scenario) in scenarios.iter().enumerate() {
        let context = format!("testgen scenarios.scenarios[{index}]");
        let scenario = scenario
            .as_object()
            .ok_or_else(|| format!("{context} must be an object"))?;
        if !scenario.get("name").is_some_and(Value::is_string) {
            return Err(format!("{context}.name must be a string"));
        }
        validate_state_fields(
            scenario
                .get("initial_state")
                .ok_or_else(|| format!("{context}.initial_state is required"))?,
            &format!("{context}.initial_state"),
            state_names,
            true,
        )?;
        let steps = scenario
            .get("steps")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{context}.steps must be an array"))?;
        let expected = scenario
            .get("expected_states")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{context}.expected_states must be an array"))?;
        if steps.len() != expected.len() {
            return Err(format!(
                "{context}.steps and {context}.expected_states must have equal lengths"
            ));
        }
        for (step_index, step) in steps.iter().enumerate() {
            validate_contract_step(step, &format!("{context}.steps[{step_index}]"), actions)?;
        }
        for (state_index, state) in expected.iter().enumerate() {
            validate_state_fields(
                state,
                &format!("{context}.expected_states[{state_index}]"),
                state_names,
                false,
            )?;
        }
        if let Some(step) = scenario.get("forbidden_step") {
            validate_contract_step(step, &format!("{context}.forbidden_step"), actions)?;
        }
    }
    Ok(scenarios)
}

fn validate_walk(
    walk: &Value,
    spec_name: &str,
    state_names: &BTreeSet<&str>,
    actions: &BTreeMap<&str, BTreeSet<&str>>,
) -> Result<(), String> {
    let root = walk
        .as_object()
        .ok_or_else(|| "testgen concrete vector must be an object".to_owned())?;
    validate_closed_object(
        root,
        "testgen concrete vector",
        &[
            "$schema",
            "schema_version",
            "kernel_schema_version",
            "result",
            "spec",
            "initial",
            "steps",
        ],
    )?;
    for (key, expected) in [
        ("$schema", TESTGEN_TRACE_V1_SCHEMA_ID),
        ("schema_version", TESTGEN_TRACE_V1_SCHEMA_VERSION),
        ("kernel_schema_version", KERNEL_SCHEMA_VERSION),
        ("result", "testgen_trace"),
        ("spec", spec_name),
    ] {
        let actual = root
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("testgen concrete vector.{key} must be a string"))?;
        if actual != expected {
            return Err(format!(
                "testgen concrete vector.{key} '{actual}' does not match '{expected}'"
            ));
        }
    }
    validate_state_fields(
        root.get("initial")
            .ok_or_else(|| "testgen concrete vector.initial is required".to_owned())?,
        "testgen concrete vector.initial",
        state_names,
        true,
    )?;
    let steps = root
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| "testgen concrete vector.steps must be an array".to_owned())?;
    if steps.len() > 100 {
        return Err("testgen concrete vector.steps must contain at most 100 items".to_owned());
    }
    for (index, step) in steps.iter().enumerate() {
        validate_contract_step(
            step,
            &format!("testgen concrete vector.steps[{index}]"),
            actions,
        )?;
        validate_closed_object(
            step.as_object()
                .expect("validate_contract_step checked the object"),
            &format!("testgen concrete vector.steps[{index}]"),
            &["action", "params", "expected"],
        )?;
        if !step.get("expected").is_some_and(Value::is_object) {
            return Err(format!(
                "testgen concrete vector.steps[{index}].expected must be an object"
            ));
        }
        validate_state_fields(
            &step["expected"],
            &format!("testgen concrete vector.steps[{index}].expected"),
            state_names,
            true,
        )?;
    }
    Ok(())
}

fn ordered_object(value: &Value, order: &[String]) -> Value {
    let Some(values) = value.as_object() else {
        return value.clone();
    };
    let mut result = Map::new();
    for key in order {
        if let Some(value) = values.get(key) {
            result.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in values {
        if !result.contains_key(key) {
            result.insert(key.clone(), value.clone());
        }
    }
    Value::Object(result)
}

fn normalize_step(step: &mut Value, actions: &[Action]) {
    let Some(step) = step.as_object_mut() else {
        return;
    };
    let action = step
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let order = actions
        .iter()
        .find(|candidate| candidate.name == action)
        .map_or(&[][..], |candidate| candidate.params.as_slice());
    if let Some(params) = step.get_mut("params") {
        *params = ordered_object(params, order);
    }
}

fn normalize_scenarios(
    mut scenarios: Vec<Value>,
    state_order: &[String],
    actions: &[Action],
) -> Vec<Value> {
    let action_order = actions
        .iter()
        .enumerate()
        .map(|(index, action)| (action.name.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    scenarios.sort_by_key(|scenario| {
        scenario
            .get("action")
            .and_then(Value::as_str)
            .and_then(|name| action_order.get(name))
            .copied()
            .unwrap_or(usize::MAX)
    });
    for scenario in &mut scenarios {
        let Some(scenario) = scenario.as_object_mut() else {
            continue;
        };
        if let Some(Value::Array(states)) = scenario.get_mut("expected_states") {
            for state in states {
                *state = ordered_object(state, state_order);
            }
        }
        if let Some(initial) = scenario.get_mut("initial_state") {
            *initial = ordered_object(initial, state_order);
        }
        if let Some(Value::Array(steps)) = scenario.get_mut("steps") {
            for step in steps {
                normalize_step(step, actions);
            }
        }
        if let Some(step) = scenario.get_mut("forbidden_step") {
            normalize_step(step, actions);
        }
    }
    scenarios
}

/// The names, scenarios and walk of one checked model, after the
/// cross-checks every producer of the generators' input runs.
struct Checked {
    spec_name: String,
    state_order: Vec<String>,
    actions: Vec<Action>,
    scenarios: Vec<Value>,
}

fn build_input(
    spec_name: String,
    state_order: Vec<String>,
    actions: Vec<Action>,
    scenarios: &Value,
    walk: &Value,
) -> Result<Checked, String> {
    let state_names = state_order
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let action_params = actions
        .iter()
        .map(|action| {
            (
                action.name.as_str(),
                action
                    .params
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    validate_walk(walk, &spec_name, &state_names, &action_params)?;
    let scenarios = normalize_scenarios(
        validate_scenarios(scenarios, &spec_name, &state_names, &action_params)?,
        &state_order,
        &actions,
    );
    Ok(Checked {
        spec_name,
        state_order,
        actions,
        scenarios,
    })
}

/// Adapt Public Kernel v1 plus scenarios and a concrete walk vector: the
/// state order and actions the Kernel itself gives.
fn public_kernel_input(kernel: &Value, scenarios: &Value, walk: &Value) -> Result<Checked, String> {
    let root = public_kernel_v1_root(kernel)?;
    let spec = required_object(
        root.get("spec")
            .ok_or_else(|| "public Kernel root.spec is required".to_owned())?,
        "root.spec",
    )?;
    let spec_name = required_str(spec, "name", "root.spec")?.to_owned();

    let mut state = required_array(root, "state", "root")?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let object = required_object(value, &format!("root.state[{index}]"))?;
            Ok((
                source_order(value, index),
                required_str(object, "name", &format!("root.state[{index}]"))?.to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    state.sort_by_key(|(order, _)| *order);
    let mut state_order = state
        .into_iter()
        .map(|(_, name)| unmapped_public_spelling(&name))
        .collect::<Vec<_>>();
    if let Some(initial) = walk.get("initial").and_then(Value::as_object)
        && !initial.is_empty()
    {
        state_order = initial.keys().cloned().collect();
    }

    let mut actions = required_array(root, "actions", "root")?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let context = format!("root.actions[{index}]");
            let object = required_object(value, &context)?;
            let params = required_array(object, "parameters", &context)?
                .iter()
                .enumerate()
                .map(|(param_index, param)| {
                    let context = format!("{context}.parameters[{param_index}]");
                    required_str(required_object(param, &context)?, "name", &context)
                        .map(str::to_owned)
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((
                source_order(value, index),
                Action {
                    name: unmapped_public_spelling(required_str(object, "name", &context)?),
                    params,
                },
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    actions.sort_by_key(|(order, _)| *order);
    let actions = actions.into_iter().map(|(_, action)| action).collect();
    build_input(spec_name, state_order, actions, scenarios, walk)
}

fn string_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{context}.{key} must be a string"))
}

fn object_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a Map<String, Value>, String> {
    object
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{context}.{key} must be an object"))
}

fn array_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a [Value], String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{context}.{key} must be an array"))
}

/// A list of distinct names, in the order the document gives them.
fn name_list(values: &[Value], context: &str) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let name = value
                .as_str()
                .ok_or_else(|| format!("{context}[{index}] must be a string"))?;
            if !seen.insert(name) {
                return Err(format!("{context} names '{name}' more than once"));
            }
            Ok(name.to_owned())
        })
        .collect()
}

fn validate_header(root: &Map<String, Value>) -> Result<(), String> {
    validate_closed_object(
        root,
        ROOT,
        &[
            "$schema",
            "schema_version",
            "kernel_schema_version",
            "fsl",
            "result",
            "fslc_version",
            "spec",
            "source",
            "dialect",
            "gate",
            "state",
            "actions",
            "scenarios",
            "walk",
            "kernel",
            "test_plan",
        ],
    )?;
    let schema = string_field(root, "$schema", ROOT)?;
    if schema != TESTGEN_INPUT_V1_SCHEMA_ID {
        return Err(format!(
            "unsupported {ROOT} $schema '{schema}'; expected '{TESTGEN_INPUT_V1_SCHEMA_ID}'"
        ));
    }
    // Consumers accept the supported major and reject any other.
    let version = string_field(root, "schema_version", ROOT)?;
    let supported_major = TESTGEN_INPUT_V1_SCHEMA_VERSION
        .split('.')
        .next()
        .unwrap_or_default();
    let mut parts = version.split('.');
    let well_formed = parts.clone().count() == 3
        && parts
            .clone()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !well_formed || parts.next() != Some(supported_major) {
        return Err(format!(
            "unsupported {ROOT} schema_version '{version}'; expected {supported_major}.x"
        ));
    }
    for (key, expected) in [
        ("kernel_schema_version", KERNEL_SCHEMA_VERSION),
        ("result", RESULT),
    ] {
        let actual = string_field(root, key, ROOT)?;
        if actual != expected {
            return Err(format!(
                "{ROOT}.{key} '{actual}' does not match '{expected}'"
            ));
        }
    }
    if let Some(fsl) = root.get("fsl")
        && fsl != "1.0"
    {
        return Err(format!("{ROOT}.fsl must be \"1.0\""));
    }
    string_field(root, "fslc_version", ROOT)?;
    let gate = object_field(root, "gate", ROOT)?;
    let gate_context = format!("{ROOT}.gate");
    validate_closed_object(gate, &gate_context, &["depth", "deadlock", "strict"])?;
    if !gate.get("depth").is_some_and(Value::is_u64) {
        return Err(format!(
            "{gate_context}.depth must be a non-negative integer"
        ));
    }
    let deadlock = string_field(gate, "deadlock", &gate_context)?;
    if !matches!(deadlock, "warn" | "error" | "ignore") {
        return Err(format!(
            "{gate_context}.deadlock must be warn, error, or ignore"
        ));
    }
    if !gate.get("strict").is_some_and(Value::is_boolean) {
        return Err(format!("{gate_context}.strict must be a boolean"));
    }
    Ok(())
}

/// The escaped header spelling of `source.name`, after checking `source`.
fn source_name(root: &Map<String, Value>) -> Result<String, String> {
    let source = object_field(root, "source", ROOT)?;
    let context = format!("{ROOT}.source");
    validate_closed_object(source, &context, &["name", "digest"])?;
    let name = string_field(source, "name", &context)?;
    if name.is_empty() {
        return Err(format!("{context}.name must not be empty"));
    }
    let digest = string_field(source, "digest", &context)?;
    let well_formed = digest.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if !well_formed {
        return Err(format!(
            "{context}.digest must be sha256: followed by 64 lowercase hex digits"
        ));
    }
    Ok(escaped_source_name(name))
}

fn actions(root: &Map<String, Value>) -> Result<Vec<Action>, String> {
    let mut seen = BTreeSet::new();
    array_field(root, "actions", ROOT)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let context = format!("{ROOT}.actions[{index}]");
            let action = value
                .as_object()
                .ok_or_else(|| format!("{context} must be an object"))?;
            validate_closed_object(action, &context, &["name", "params"])?;
            let name = string_field(action, "name", &context)?;
            if !seen.insert(name) {
                return Err(format!("{ROOT}.actions names '{name}' more than once"));
            }
            Ok(Action {
                name: name.to_owned(),
                params: name_list(
                    array_field(action, "params", &context)?,
                    &format!("{context}.params"),
                )?,
            })
        })
        .collect()
}

impl TestgenInput {
    /// Read a `testgen-input.v1` document.
    ///
    /// # Errors
    ///
    /// Rejects an unsupported or malformed document; a spec name that differs
    /// between `spec`, `walk` and `kernel`; a scenario or walk that names a state
    /// field, action or parameter outside `state`/`actions`; `state` or `actions`
    /// that disagree with a non-null `kernel`; a null `kernel` for a dialect other
    /// than compose; and scenarios that are not in normalized order.
    pub fn from_document(document: &Value) -> Result<Self, String> {
        let root = document
            .as_object()
            .ok_or_else(|| format!("{ROOT} root must be an object"))?;
        validate_header(root)?;
        let source_name = source_name(root)?;
        let spec_name = string_field(root, "spec", ROOT)?;
        if spec_name.is_empty() {
            return Err(format!("{ROOT}.spec must not be empty"));
        }
        let dialect = string_field(root, "dialect", ROOT)?;
        let state_order = name_list(array_field(root, "state", ROOT)?, &format!("{ROOT}.state"))?;
        let actions = actions(root)?;
        let given_scenarios = array_field(root, "scenarios", ROOT)?;
        let scenarios = json!({"spec": spec_name, "scenarios": given_scenarios});
        let walk = root
            .get("walk")
            .ok_or_else(|| format!("{ROOT}.walk is required"))?;
        let kernel = root
            .get("kernel")
            .ok_or_else(|| format!("{ROOT}.kernel is required (an object, or null for compose)"))?;

        // The same cross-checks the in-process producers run.
        let input = build_input(spec_name.to_owned(), state_order, actions, &scenarios, walk)?;
        if kernel.is_null() {
            if dialect != COMPOSE_DIALECT {
                return Err(format!(
                    "{ROOT}.kernel is null for dialect '{dialect}'; only compose has no public Kernel"
                ));
            }
        } else {
            let adapted = public_kernel_input(kernel, &scenarios, walk)?;
            // Both adaptations checked `walk.spec`, so the three spec names agree.
            debug_assert_eq!(adapted.spec_name, input.spec_name);
            if adapted.state_order != input.state_order {
                return Err(format!(
                    "{ROOT}.state does not match the public Kernel state order"
                ));
            }
            if adapted.actions != input.actions {
                return Err(format!(
                    "{ROOT}.actions does not match the public Kernel actions"
                ));
            }
        }
        // Targets render object keys and scenario order as given, so a document
        // that is not already normalized would not reproduce `fslc testgen`.
        if serde_json::to_string(&input.scenarios).ok()
            != serde_json::to_string(given_scenarios).ok()
        {
            return Err(format!(
                "{ROOT}.scenarios is not normalized to the order of state and actions"
            ));
        }
        if let Some(test_plan) = root.get("test_plan") {
            validate_test_plan_v1(test_plan)?;
            if test_plan.get("spec").and_then(Value::as_str) != Some(spec_name) {
                return Err(format!(
                    "{ROOT}.test_plan.spec does not match spec '{spec_name}'"
                ));
            }
        }
        Ok(Self {
            spec_name: input.spec_name,
            source_name,
            state_order: input.state_order,
            actions: input.actions,
            scenarios: input.scenarios,
            walk: walk.clone(),
            document: document.clone(),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::contract::KERNEL_SCHEMA_ID;

    /// A normalized document: `zeta` before `alpha`, `begin` before `finish`,
    /// `id` before `amount`, while the Kernel lists them the other way round
    /// and orders them by span.
    pub(crate) fn document() -> Value {
        json!({
            "$schema": TESTGEN_INPUT_V1_SCHEMA_ID,
            "schema_version": TESTGEN_INPUT_V1_SCHEMA_VERSION,
            "kernel_schema_version": KERNEL_SCHEMA_VERSION,
            "result": "testgen_input",
            "fslc_version": "0.0.0",
            "spec": "Demo",
            "source": {
                "name": "demo.fsl",
                "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            },
            "dialect": "kernel",
            "gate": {"depth": 8, "deadlock": "warn", "strict": false},
            "state": ["zeta", "alpha"],
            "actions": [
                {"name": "begin", "params": ["id", "amount"]},
                {"name": "finish", "params": []}
            ],
            "scenarios": [
                {
                    "name": "forbidden_begin",
                    "kind": "forbidden",
                    "action": "begin",
                    "initial_state": {"zeta": 0, "alpha": 0},
                    "steps": [{"action": "begin", "params": {"id": 1, "amount": 2}}],
                    "expected_states": [{"zeta": 1, "alpha": 0}],
                    "forbidden_step": {"action": "begin", "params": {"id": 1, "amount": 9}},
                    "rejected_by": "requires_failed"
                },
                {
                    "name": "cover_finish",
                    "kind": "action_coverage",
                    "action": "finish",
                    "initial_state": {"zeta": 0, "alpha": 0},
                    "steps": [{"action": "finish", "params": {}}],
                    "expected_states": [{"zeta": 0, "alpha": 1}]
                }
            ],
            "walk": {
                "$schema": TESTGEN_TRACE_V1_SCHEMA_ID,
                "schema_version": TESTGEN_TRACE_V1_SCHEMA_VERSION,
                "kernel_schema_version": KERNEL_SCHEMA_VERSION,
                "result": "testgen_trace",
                "spec": "Demo",
                "initial": {"zeta": 0, "alpha": 0},
                "steps": [{
                    "action": "begin",
                    "params": {"id": 1, "amount": 2},
                    "expected": {"zeta": 1, "alpha": 0}
                }]
            },
            "kernel": {
                "$schema": KERNEL_SCHEMA_ID,
                "schema_version": KERNEL_SCHEMA_VERSION,
                "spec": {"name": "Demo"},
                "state": [
                    {"name": "alpha", "span": {"line": 2, "column": 1}},
                    {"name": "zeta", "span": {"line": 1, "column": 1}}
                ],
                "actions": [
                    {"name": "finish", "span": {"line": 4, "column": 1}, "parameters": []},
                    {
                        "name": "begin",
                        "span": {"line": 3, "column": 1},
                        "parameters": [{"name": "id"}, {"name": "amount"}]
                    }
                ]
            }
        })
    }

    fn rejection(mutate: impl FnOnce(&mut Value)) -> String {
        let mut document = document();
        mutate(&mut document);
        TestgenInput::from_document(&document).expect_err("the mutated document must be rejected")
    }

    #[test]
    fn a_normalized_document_is_read_as_given() {
        let document = document();
        let input = TestgenInput::from_document(&document).expect("read the document");
        assert_eq!(input.spec_name(), "Demo");
        assert_eq!(input.source_name(), "demo.fsl");
        assert_eq!(input.state_order(), ["zeta", "alpha"]);
        assert_eq!(
            input.actions(),
            [
                Action {
                    name: "begin".to_owned(),
                    params: vec!["id".to_owned(), "amount".to_owned()]
                },
                Action {
                    name: "finish".to_owned(),
                    params: vec![]
                }
            ]
        );
        assert_eq!(input.params_of("begin"), ["id", "amount"]);
        assert!(input.params_of("ghost").is_empty());
        assert_eq!(
            serde_json::to_string(input.scenarios()).expect("serialize"),
            serde_json::to_string(&document["scenarios"]).expect("serialize")
        );
        assert_eq!(input.walk(), &document["walk"]);
        assert_eq!(input.document(), &document);
        // Through text, as a generator receives it.
        let text = serde_json::to_string_pretty(&document).expect("serialize");
        let parsed = parse_document(&text).expect("parse");
        assert_eq!(
            TestgenInput::from_document(&parsed).expect("read parsed"),
            input
        );
    }

    /// The order of object members is part of the contract. `serde_json`
    /// keeps it only with its `preserve_order` feature; without the feature
    /// this test fails, and so does every byte comparison of a compose
    /// document, whose `walk.initial` order differs from `state`.
    #[test]
    fn parsing_keeps_the_order_of_object_members() {
        let parsed = parse_document(r#"{"zeta": 1, "alpha": 2, "mid": 3}"#).expect("parse");
        assert_eq!(
            parsed
                .as_object()
                .expect("object")
                .keys()
                .collect::<Vec<_>>(),
            ["zeta", "alpha", "mid"]
        );
        assert_eq!(
            parse_document("{").expect_err("not JSON"),
            "testgen-input is not JSON: EOF while parsing an object at line 1 column 1"
        );
    }

    #[test]
    fn compose_document_carries_a_null_kernel() {
        let mut compose = document();
        compose["dialect"] = json!("compose");
        compose["kernel"] = Value::Null;
        // A compose walk keeps the order the walk produced, which is not `state`.
        compose["walk"]["initial"] = json!({"alpha": 0, "zeta": 0});
        let input = TestgenInput::from_document(&compose).expect("read compose");
        assert_eq!(input.state_order(), ["zeta", "alpha"]);
        assert_eq!(
            input.walk()["initial"]
                .as_object()
                .expect("object")
                .keys()
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
    }

    #[test]
    fn source_name_comes_from_the_document_and_is_escaped() {
        let mut document = document();
        document["source"]["name"] = json!("o*her.fsl");
        let input = TestgenInput::from_document(&document).expect("read");
        assert_eq!(input.source_name(), "o\\x2aher.fsl");
    }

    #[test]
    fn schema_identity_and_major_are_checked() {
        assert!(
            rejection(|d| d["$schema"] = json!("https://example.invalid/x")).contains("$schema")
        );
        assert_eq!(
            rejection(|d| d["schema_version"] = json!("2.0.0")),
            "unsupported testgen-input schema_version '2.0.0'; expected 1.x"
        );
        assert_eq!(
            rejection(|d| d["schema_version"] = json!("0.9.0")),
            "unsupported testgen-input schema_version '0.9.0'; expected 1.x"
        );
        assert!(rejection(|d| d["schema_version"] = json!("1")).contains("schema_version"));
        let mut minor = document();
        minor["schema_version"] = json!("1.3.0");
        TestgenInput::from_document(&minor).expect("a later 1.x is accepted");
        assert!(
            rejection(|d| d["kernel_schema_version"] = json!("2.0.0"))
                .contains("kernel_schema_version")
        );
        assert!(rejection(|d| d["result"] = json!("kernel")).contains("result"));
        assert_eq!(
            TestgenInput::from_document(&json!([])).expect_err("not an object"),
            "testgen-input root must be an object"
        );
    }

    #[test]
    fn closed_shape_is_checked() {
        assert_eq!(
            rejection(|d| d["extra"] = json!(1)),
            "testgen-input has unknown field 'extra'"
        );
        assert_eq!(
            rejection(|d| {
                d.as_object_mut().expect("object").remove("walk");
            }),
            "testgen-input.walk is required"
        );
        assert_eq!(
            rejection(|d| {
                d.as_object_mut().expect("object").remove("kernel");
            }),
            "testgen-input.kernel is required (an object, or null for compose)"
        );
        assert!(
            rejection(|d| d["walk"]["steps"][0]["note"] = json!("x"))
                .contains("unknown field 'note'")
        );
        assert!(
            rejection(|d| d["gate"]["engine"] = json!("bmc")).contains("unknown field 'engine'")
        );
        assert!(rejection(|d| d["gate"]["deadlock"] = json!("maybe")).contains("deadlock"));
        assert!(rejection(|d| d["source"]["digest"] = json!("md5:00")).contains("digest"));
        assert!(
            rejection(|d| d["actions"][0]["span"] = json!({})).contains("unknown field 'span'")
        );
        assert!(rejection(|d| d["state"] = json!(["zeta", "zeta"])).contains("more than once"));
        assert!(rejection(|d| d["walk"]["extra"] = json!(true)).contains("unknown field 'extra'"));
        assert!(
            rejection(|d| d["walk"]["schema_version"] = json!("2.0.0"))
                .contains("concrete vector.schema_version")
        );
        assert!(
            rejection(|d| d["walk"]["steps"] = json!([{"action": "begin"}]))
                .contains("steps[0].params must be an object")
        );
        assert!(
            rejection(|d| d["scenarios"][0]["expected_states"] = json!([]))
                .contains("must have equal lengths")
        );
        assert!(
            rejection(|d| d["kernel"]["$schema"] = json!("https://example.com/kernel.schema.json"))
                .contains("unsupported public Kernel $schema")
        );
        assert!(
            rejection(|d| d["kernel"]["schema_version"] = json!("2.0.0"))
                .contains("unsupported public Kernel schema_version")
        );
    }

    #[test]
    fn spec_names_must_agree() {
        assert_eq!(
            rejection(|d| d["spec"] = json!("Other")),
            "testgen concrete vector.spec 'Demo' does not match 'Other'"
        );
        assert_eq!(
            rejection(|d| d["walk"]["spec"] = json!("Other")),
            "testgen concrete vector.spec 'Other' does not match 'Demo'"
        );
        assert_eq!(
            rejection(|d| d["kernel"]["spec"]["name"] = json!("Other")),
            "testgen concrete vector.spec 'Demo' does not match 'Other'"
        );
    }

    #[test]
    fn names_outside_state_and_actions_are_rejected() {
        assert!(
            rejection(|d| d["scenarios"][0]["expected_states"][0]["ghost"] = json!(1))
                .contains("has unknown state fields: ghost")
        );
        assert!(
            rejection(|d| d["scenarios"][0]["steps"][0]["action"] = json!("ghost"))
                .contains("names unknown action 'ghost'")
        );
        assert!(
            rejection(|d| d["scenarios"][0]["steps"][0]["params"]["ghost"] = json!(1))
                .contains("params must match public Kernel parameters for 'begin'")
        );
        assert!(
            rejection(|d| d["scenarios"][0]["forbidden_step"]["action"] = json!("ghost"))
                .contains("names unknown action 'ghost'")
        );
        assert!(
            rejection(|d| d["walk"]["steps"][0]["action"] = json!("ghost"))
                .contains("names unknown action 'ghost'")
        );
        assert!(
            rejection(|d| d["walk"]["steps"][0]["params"]["ghost"] = json!(1))
                .contains("params must match public Kernel parameters for 'begin'")
        );
        assert!(
            rejection(|d| d["walk"]["steps"][0]["expected"]["ghost"] = json!(1))
                .contains("has unknown state fields: ghost")
        );
        assert!(
            rejection(|d| d["walk"]["initial"]["ghost"] = json!(1))
                .contains("has unknown state fields: ghost")
        );
        // Dropping a name from `state`/`actions` makes its uses unknown.
        assert!(
            rejection(|d| d["state"] = json!(["zeta"])).contains("has unknown state fields: alpha")
        );
        assert!(
            rejection(|d| d["actions"] = json!([{"name": "begin", "params": ["id", "amount"]}]))
                .contains("names unknown action 'finish'")
        );
    }

    #[test]
    fn state_and_actions_must_match_a_present_kernel() {
        assert_eq!(
            rejection(|d| d["state"] = json!(["alpha", "zeta"])),
            "testgen-input.state does not match the public Kernel state order"
        );
        assert_eq!(
            rejection(|d| {
                d["actions"] = json!([
                    {"name": "finish", "params": []},
                    {"name": "begin", "params": ["id", "amount"]}
                ]);
            }),
            "testgen-input.actions does not match the public Kernel actions"
        );
        assert_eq!(
            rejection(|d| d["kernel"] = Value::Null),
            "testgen-input.kernel is null for dialect 'kernel'; only compose has no public Kernel"
        );
    }

    #[test]
    fn scenarios_must_already_be_normalized() {
        let expected =
            "testgen-input.scenarios is not normalized to the order of state and actions";
        // Scenario order.
        assert_eq!(
            rejection(|d| d["scenarios"].as_array_mut().expect("array").reverse()),
            expected
        );
        // State key order.
        assert_eq!(
            rejection(|d| d["scenarios"][0]["initial_state"] = json!({"alpha": 0, "zeta": 0})),
            expected
        );
        // Parameter key order.
        assert_eq!(
            rejection(|d| {
                d["scenarios"][0]["steps"][0]["params"] = json!({"amount": 2, "id": 1});
            }),
            expected
        );
    }

    #[test]
    fn an_embedded_test_plan_is_checked() {
        assert_eq!(
            rejection(|d| d["test_plan"] = json!({"surprise": 1})),
            "unknown test-plan field 'surprise'"
        );
        assert!(
            rejection(|d| d["test_plan"] = json!({}))
                .contains("test-plan.$schema must be a string")
        );
    }
}
