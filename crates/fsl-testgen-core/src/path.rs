// SPDX-License-Identifier: Apache-2.0

//! Where the generated file and the spec are. The document does not say; the
//! caller does. Nothing here touches the file system: a caller that wants
//! canonical paths resolves them before it builds the context.

use std::path::{Path, PathBuf};

/// Whether the generated content is written to a caller-selected file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputContext {
    /// The content is returned or printed; no file location is known.
    WithoutOutput,
    /// The content is written to a file in `parent` (none for a bare root).
    WithOutput { parent: Option<PathBuf> },
}

/// Explicit path input for generated test presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathContext {
    pub spec_path: PathBuf,
    pub output: OutputContext,
}

fn portable_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

impl PathContext {
    /// The spec path a generated file refers to: relative to the directory of
    /// the output file when there is one, else as given.
    #[must_use]
    pub fn relative_spec_path(&self) -> String {
        let OutputContext::WithOutput {
            parent: Some(parent),
        } = &self.output
        else {
            return portable_path(&self.spec_path);
        };
        let left = parent.components().collect::<Vec<_>>();
        let right = self.spec_path.components().collect::<Vec<_>>();
        let common = left
            .iter()
            .zip(&right)
            .take_while(|(left, right)| left == right)
            .count();
        let mut result = PathBuf::new();
        for _ in common..left.len() {
            result.push("..");
        }
        for component in &right[common..] {
            result.push(component.as_os_str());
        }
        portable_path(&result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_path_is_relative_to_the_output_directory_only_when_there_is_one() {
        let without = PathContext {
            spec_path: PathBuf::from("missing/spec.fsl"),
            output: OutputContext::WithoutOutput,
        };
        assert_eq!(without.relative_spec_path(), "missing/spec.fsl");
        let with = PathContext {
            spec_path: PathBuf::from("/workspace/first/demo.fsl"),
            output: OutputContext::WithOutput {
                parent: Some(PathBuf::from("/workspace/generated")),
            },
        };
        assert_eq!(with.relative_spec_path(), "../first/demo.fsl");
        let parentless = PathContext {
            spec_path: PathBuf::from("/workspace/demo.fsl"),
            output: OutputContext::WithOutput { parent: None },
        };
        assert_eq!(parentless.relative_spec_path(), "/workspace/demo.fsl");
        assert_eq!(
            portable_path(&Path::new("parent").join("child")),
            "parent/child"
        );
    }
}
