// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_testgen_core::render::{
    Language, fill_template, literal, quoted, safe_ident, scenario_parts, unique_ident,
};
use fsl_testgen_core::{GeneratedFile, Options, Target, TestgenInput, UnwiredAdapter};
use serde_json::Value;

use super::forbidden::append_forbidden;
use super::{NOT_WIRED_MESSAGE, fail_header, one_file};

/// Standalone `PHPUnit` conformance scaffold.
pub struct PhpUnit;

impl Target for PhpUnit {
    fn name(&self) -> &str {
        "phpunit"
    }

    fn generate(
        &self,
        input: &TestgenInput,
        options: &Options,
    ) -> Result<Vec<GeneratedFile>, String> {
        Ok(one_file(
            format!("{}ConformanceTest.php", safe_ident(input.spec_name())),
            emit(input, options.unwired),
        ))
    }
}

fn emit(input: &TestgenInput, unwired: UnwiredAdapter) -> String {
    let walk = input.walk();
    let spec_name = input.spec_name();
    let (header, setup) = match unwired {
        UnwiredAdapter::Fail => (
            fail_header("// "),
            format!(
                "            $this->fail({} . ' (' . $e->getMessage() . ')');",
                quoted(NOT_WIRED_MESSAGE, Language::Php)
            ),
        ),
        UnwiredAdapter::Skip => (
            "// Wire `makeAdapter()` to your implementation. Until it is wired, setUp() marks\n\
             // every test skipped (mirroring the other targets' skip-when-unwired behaviour)."
                .to_owned(),
            "            $this->markTestSkipped('Adapter not wired: ' . $e->getMessage());"
                .to_owned(),
        ),
    };
    let preamble = fill_template(
        include_str!("php.txt"),
        input.source_name(),
        &[("__UNWIRED_HEADER__", header)],
    );
    let mut lines = vec![
        format!("final class {spec_name}ConformanceTest extends TestCase"),
        "{".to_owned(),
        fill_template(
            include_str!("php_helpers.txt"),
            "",
            &[("__UNWIRED_SETUP__", setup)],
        )
        .trim_end()
        .to_owned(),
    ];
    let mut seen = BTreeMap::new();
    for scenario in input.scenarios() {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "    public function {}(): void",
            unique_ident(name, &mut seen, "testScenario_")
        ));
        lines.push("    {".to_owned());
        lines.push("        $a = $this->adapter;".to_owned());
        lines.push("        $a->reset();".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "        $a->step({}, {});",
                literal(&step["action"], Language::Php),
                literal(&step["params"], Language::Php)
            ));
            lines.push(format!(
                "        $this->assertPartial({}, $a->observe());",
                literal(states.get(index).unwrap_or(&Value::Null), Language::Php)
            ));
        }
        append_forbidden(&mut lines, scenario, Language::Php, "        ");
        lines.push("    }".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "        ['action' => {}, 'params' => {}, 'expected' => {}]",
                literal(&step["action"], Language::Php),
                literal(&step["params"], Language::Php),
                literal(&step["expected"], Language::Php)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "[]".to_owned()
    } else {
        format!("[\n{},\n    ]", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        format!(
            "    private const INITIAL = {};",
            literal(&walk["initial"], Language::Php)
        ),
        format!("    private const WALK = {walk_literal};"),
        String::new(),
        "    public function testRandomWalkConformance(): void".to_owned(),
        "    {".to_owned(),
        "        // random-walk conformance (baked oracle trace)".to_owned(),
        "        $a = $this->adapter;".to_owned(),
        "        $a->reset();".to_owned(),
        "        $this->assertPartial(self::INITIAL, $a->observe());".to_owned(),
        "        foreach (self::WALK as $step) {".to_owned(),
        "            $a->step($step['action'], $step['params']);".to_owned(),
        "            $this->assertPartial($step['expected'], $a->observe());".to_owned(),
        "        }".to_owned(),
        "    }".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}
