// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_testgen_core::render::{
    Language, fill_template, literal, quoted, safe_ident, scenario_parts, unique_ident,
};
use fsl_testgen_core::{GeneratedFile, Options, Target, TestgenInput, UnwiredAdapter};
use serde_json::Value;

use super::forbidden::append_forbidden;
use super::one_file;

/// The Kotlin variant: `--allow-unwired` is rejected for Kotlin.
pub(super) const NOT_WIRED_MESSAGE: &str =
    "adapter not wired: implement makeAdapter() to connect your implementation";

/// Standalone kotlin.test conformance scaffold.
pub struct Kotlin;

impl Target for Kotlin {
    fn name(&self) -> &str {
        "kotlin"
    }

    /// Rejects [`UnwiredAdapter::Skip`]: kotlin.test has no portable runtime
    /// skip, so an unwired Kotlin test could only return early and pass, which
    /// is the silent green this option must not produce.
    fn generate(
        &self,
        input: &TestgenInput,
        options: &Options,
    ) -> Result<Vec<GeneratedFile>, String> {
        if options.unwired == UnwiredAdapter::Skip {
            return Err(
                "--allow-unwired is not available for --target kotlin: kotlin.test has no portable runtime skip, so an unwired test would pass without calling the implementation"
                    .to_owned(),
            );
        }
        Ok(one_file(
            format!("{}ConformanceTest.kt", safe_ident(input.spec_name())),
            emit(input),
        ))
    }
}

fn emit(input: &TestgenInput) -> String {
    let walk = input.walk();
    let spec_name = input.spec_name();
    let preamble = fill_template(
        include_str!("kotlin.txt"),
        input.source_name(),
        &[(
            "__NOT_WIRED_MESSAGE__",
            quoted(NOT_WIRED_MESSAGE, Language::Kotlin),
        )],
    );
    let mut lines = vec![format!("class {spec_name}ConformanceTest {{")];
    let mut seen = BTreeMap::new();
    for scenario in input.scenarios() {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "    @Test fun {}() {{",
            unique_ident(name, &mut seen, "scenario_")
        ));
        lines.push("        val a = makeAdapter() ?: fail(ADAPTER_NOT_WIRED)".to_owned());
        lines.push("        a.reset()".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "        a.step({}, {})",
                literal(&step["action"], Language::Kotlin),
                literal(&step["params"], Language::Kotlin)
            ));
            lines.push(format!(
                "        assertPartial(a.observe(), {})",
                literal(states.get(index).unwrap_or(&Value::Null), Language::Kotlin)
            ));
        }
        append_forbidden(&mut lines, scenario, Language::Kotlin, "        ");
        lines.push("    }".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "            Triple({}, {}, {})",
                literal(&step["action"], Language::Kotlin),
                literal(&step["params"], Language::Kotlin),
                literal(&step["expected"], Language::Kotlin)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "listOf<Triple<String, Map<String, Any?>, Map<String, Any?>>>()".to_owned()
    } else {
        format!("listOf(\n{},\n        )", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        "    @Test fun randomWalkConformance() {".to_owned(),
        "        // random-walk conformance (baked oracle trace)".to_owned(),
        "        val a = makeAdapter() ?: fail(ADAPTER_NOT_WIRED)".to_owned(),
        "        a.reset()".to_owned(),
        format!(
            "        val initial: Map<String, Any?> = {}",
            literal(&walk["initial"], Language::Kotlin)
        ),
        "        assertPartial(a.observe(), initial)".to_owned(),
        format!(
            "        val walk: List<Triple<String, Map<String, Any?>, Map<String, Any?>>> = {walk_literal}"
        ),
        "        for ((action, params, expected) in walk) {".to_owned(),
        "            a.step(action, params)".to_owned(),
        "            assertPartial(a.observe(), expected)".to_owned(),
        "        }".to_owned(),
        "    }".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}
