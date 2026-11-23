// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_testgen_core::render::{
    Language, fill_template, literal, quoted, scenario_parts, unique_ident,
};
use fsl_testgen_core::{GeneratedFile, Options, Target, TestgenInput, UnwiredAdapter};
use serde_json::Value;

use super::forbidden::append_forbidden;
use super::{NOT_WIRED_MESSAGE, fail_header, one_file};

/// Standalone Swift Testing conformance scaffold.
pub struct Swift;

impl Target for Swift {
    fn name(&self) -> &str {
        "swift"
    }

    fn generate(
        &self,
        input: &TestgenInput,
        options: &Options,
    ) -> Result<Vec<GeneratedFile>, String> {
        Ok(one_file(
            "ConformanceTests.swift".to_owned(),
            emit(input, options.unwired),
        ))
    }
}

fn emit(input: &TestgenInput, unwired: UnwiredAdapter) -> String {
    let walk = input.walk();
    let (slots, test_attribute) = match unwired {
        UnwiredAdapter::Fail => (
            [
                ("__UNWIRED_HEADER__", fail_header("// ")),
                (
                    "__NOT_WIRED_ERROR__",
                    format!(
                        "enum FSLNotWired: Error, CustomStringConvertible {{\n    case notWired\n    var description: String {{ {} }}\n}}",
                        quoted(NOT_WIRED_MESSAGE, Language::Swift)
                    ),
                ),
                (
                    "__MAKE_ADAPTER_NOTE__",
                    "// Wire your implementation here. Throwing fails every test (\"adapter not wired\")."
                        .to_owned(),
                ),
            ],
            "@Test",
        ),
        UnwiredAdapter::Skip => (
            [
                (
                    "__UNWIRED_HEADER__",
                    "// Wire `makeAdapter()` to your implementation. Until it is wired every test is\n\
                     // skipped (the `.enabled(if:)` trait checks the adapter and disables the test)."
                        .to_owned(),
                ),
                (
                    "__NOT_WIRED_ERROR__",
                    "enum FSLNotWired: Error { case notWired }".to_owned(),
                ),
                (
                    "__MAKE_ADAPTER_NOTE__",
                    "// Wire your implementation here. Throwing leaves the suite skipped (not failed)."
                        .to_owned(),
                ),
            ],
            "@Test(.enabled(if: isAdapterWired()))",
        ),
    };
    let mut parts = vec![fill_template(
        include_str!("swift.txt"),
        input.source_name(),
        &slots,
    )];
    let mut seen = BTreeMap::new();
    for scenario in input.scenarios() {
        let (name, steps, states) = scenario_parts(scenario);
        let function = unique_ident(name, &mut seen, "scenario_");
        let mut lines = vec![
            format!("{test_attribute} func {function}() throws {{"),
            "    let a = try makeAdapter()".to_owned(),
            "    a.reset()".to_owned(),
        ];
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "    _ = a.step({}, {})",
                literal(&step["action"], Language::Swift),
                literal(&step["params"], Language::Swift)
            ));
            lines.push(format!(
                "    assertPartial(a.observe(), {})",
                literal(states.get(index).unwrap_or(&Value::Null), Language::Swift)
            ));
        }
        append_forbidden(&mut lines, scenario, Language::Swift, "    ");
        lines.push("}".to_owned());
        parts.push(lines.join("\n"));
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "        (action: {}, params: {}, expected: {})",
                literal(&step["action"], Language::Swift),
                literal(&step["params"], Language::Swift),
                literal(&step["expected"], Language::Swift)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "[]".to_owned()
    } else {
        format!("[\n{},\n    ]", rows.join(",\n"))
    };
    parts.push(
        [
            format!("{test_attribute} func randomWalkConformance() throws {{"),
            "    // random-walk conformance (baked oracle trace)".to_owned(),
            "    let a = try makeAdapter()".to_owned(),
            "    a.reset()".to_owned(),
            format!(
                "    let initial: [String: Any] = {}",
                literal(&walk["initial"], Language::Swift)
            ),
            "    assertPartial(a.observe(), initial)".to_owned(),
            format!(
                "    let walk: [(action: String, params: [String: Any], expected: [String: Any])] = {walk_literal}"
            ),
            "    for step in walk {".to_owned(),
            "        _ = a.step(step.action, step.params)".to_owned(),
            "        assertPartial(a.observe(), step.expected)".to_owned(),
            "    }".to_owned(),
            "}".to_owned(),
        ]
        .join("\n"),
    );
    parts.join("\n\n") + "\n"
}
