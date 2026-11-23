// SPDX-License-Identifier: Apache-2.0

//! The interface every target implements: a checked document and options in,
//! generated files out.

use std::path::PathBuf;

use crate::input::TestgenInput;

/// What a generated test does while its adapter is not wired (ymm-oss/fsl
/// issue #1200).
///
/// An unwired suite never calls the implementation, so a green run of it is
/// not evidence that the implementation conforms. The default therefore fails
/// every generated test; skipping is an explicit, generation-time opt-in that
/// is visible in the generated file.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UnwiredAdapter {
    /// Fail every generated test with an "adapter not wired" message.
    #[default]
    Fail,
    /// Skip every generated test, selected by `--allow-unwired`.
    Skip,
}

/// What the caller decides about one generation.
///
/// The document does not say where the spec or the generated files are; the
/// caller may. Both paths are passed on as the caller gave them, unresolved.
/// No built-in target reads them: every built-in suite is self-contained.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Options {
    pub unwired: UnwiredAdapter,
    /// Where the caller says the spec is (`--spec-path`).
    pub spec_path: Option<PathBuf>,
    /// Where the caller writes the generated file (`-o`).
    pub output: Option<PathBuf>,
}

/// One generated file: a path relative to the output directory, and its text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedFile {
    pub path: String,
    pub content: String,
}

/// A test generator for one language and test framework.
pub trait Target {
    /// The name `--target` selects it by.
    fn name(&self) -> &str;

    /// Generate the files of one suite.
    ///
    /// # Errors
    ///
    /// Returns the reason the target refuses these options or this document.
    fn generate(
        &self,
        input: &TestgenInput,
        options: &Options,
    ) -> Result<Vec<GeneratedFile>, String>;
}
