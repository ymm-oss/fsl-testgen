// SPDX-License-Identifier: Apache-2.0

//! The built-in targets. Each is a port of the emitter of the same name in
//! ymm-oss/fsl (`fsl-tools`, `testgen.rs`).

use fsl_testgen_core::{GeneratedFile, Target};

mod dart;
mod forbidden;
mod kotlin;
mod phpunit;
mod pytest;
mod swift;
mod vitest;

/// The failure message of an unwired generated test, shared by every target
/// whose adapter entry point is `makeAdapter()`.
const NOT_WIRED_MESSAGE: &str = "adapter not wired: implement makeAdapter() to connect your implementation, or generate with `fslc testgen --allow-unwired` to skip instead";

/// Shared "until it is wired" header sentence for the failing default.
const FAIL_HEADER_LINES: [&str; 4] = [
    "Wire `makeAdapter()` to your implementation. Until it is wired, every test",
    "FAILS with \"adapter not wired\" (issue #1200): a suite that never calls the",
    "implementation is not evidence that it conforms. Generate with",
    "`fslc testgen --allow-unwired` to skip instead.",
];

fn fail_header(prefix: &str) -> String {
    FAIL_HEADER_LINES
        .iter()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn one_file(path: String, content: String) -> Vec<GeneratedFile> {
    vec![GeneratedFile { path, content }]
}

/// Every built-in target, in the order `fsl-testgen targets` lists them.
#[must_use]
pub fn builtin_targets() -> Vec<Box<dyn Target>> {
    vec![
        Box::new(pytest::Pytest),
        Box::new(vitest::Vitest),
        Box::new(swift::Swift),
        Box::new(kotlin::Kotlin),
        Box::new(dart::Dart),
        Box::new(phpunit::PhpUnit),
    ]
}

/// The built-in target called `name`.
#[must_use]
pub fn builtin_target(name: &str) -> Option<Box<dyn Target>> {
    builtin_targets()
        .into_iter()
        .find(|target| target.name() == name)
}

#[cfg(test)]
mod tests;
