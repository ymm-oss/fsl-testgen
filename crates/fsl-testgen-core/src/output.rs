// SPDX-License-Identifier: Apache-2.0

//! Writing generated files. Targets, built-in or external, only return
//! files; every write goes through here.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::target::GeneratedFile;

/// Check that `path` is a plain relative path with `/` separators.
///
/// # Errors
///
/// Rejects an empty path, an absolute path (a leading `/`, or a drive letter),
/// a `.` or `..` component, an empty component, a backslash and a NUL. The
/// message continues a sentence that names the path's position.
pub fn check_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("is empty".to_owned());
    }
    let reason = if path.contains('\0') {
        "contains a NUL"
    } else if path.contains('\\') {
        "contains a backslash; use '/' between components"
    } else if path.starts_with('/') {
        "is absolute"
    } else if path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic() {
        "starts with a drive letter"
    } else if path.split('/').any(|component| component == "..") {
        "has a '..' component"
    } else if path.split('/').any(|component| component == ".") {
        "has a '.' component"
    } else if path.split('/').any(str::is_empty) {
        "has an empty component"
    } else {
        return Ok(());
    };
    Err(format!("'{}' {reason}", path.escape_default()))
}

/// Check every path of `files`, and that no path is given twice.
///
/// # Errors
///
/// See [`check_relative_path`].
pub fn check_files(files: &[GeneratedFile]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for file in files {
        check_relative_path(&file.path).map_err(|error| format!("generated path {error}"))?;
        if !seen.insert(file.path.as_str()) {
            return Err(format!(
                "generated path '{}' is given more than once",
                file.path
            ));
        }
    }
    Ok(())
}

fn temporary_sibling(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file path", path.display()))?;
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".fsl-testgen-{}.tmp", std::process::id()));
    Ok(path.with_file_name(temporary))
}

/// Write `content` to `path` through a sibling temporary file, so that the
/// file is either the whole new content or untouched.
///
/// # Errors
///
/// Returns the reason the file could not be written; nothing is left behind.
pub fn write_file(path: &Path, content: &str) -> Result<(), String> {
    let temporary = temporary_sibling(path)?;
    let written = std::fs::write(&temporary, content)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| format!("cannot write {}: {error}", path.display()));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// What [`write_tree`] has put on disk so far, removed again on failure.
#[derive(Default)]
struct Staged {
    directories: Vec<PathBuf>,
    temporaries: Vec<PathBuf>,
}

impl Staged {
    fn discard(&self) {
        for temporary in &self.temporaries {
            let _ = std::fs::remove_file(temporary);
        }
        // Deepest first; a directory that got other content stays.
        for directory in self.directories.iter().rev() {
            let _ = std::fs::remove_dir(directory);
        }
    }
}

fn create_directories(root: &Path, relative: &Path, staged: &mut Staged) -> Result<(), String> {
    let mut directory = root.to_path_buf();
    for component in relative.components() {
        directory.push(component);
        match std::fs::create_dir(&directory) {
            Ok(()) => staged.directories.push(directory.clone()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!("cannot create {}: {error}", directory.display()));
            }
        }
    }
    Ok(())
}

/// Write `files` under the existing directory `root`, all or none.
///
/// Every path is checked first ([`check_files`]); then the directories are
/// created and each is checked to still be inside `root` after symbolic links
/// are resolved; then every file is written to a temporary sibling; only when
/// all of that succeeded are the temporaries renamed into place. A failure
/// before the renames removes what was created and leaves `root` as it was.
/// Each rename replaces one file as a whole. A failure during the renames
/// (rare: the files are already on the same file system) is reported with the
/// files that had been replaced by then.
///
/// # Errors
///
/// Returns the reason nothing, or not everything, was written.
pub fn write_tree(root: &Path, files: &[GeneratedFile]) -> Result<Vec<PathBuf>, String> {
    check_files(files)?;
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|error| format!("cannot write into {}: {error}", root.display()))?;
    if !canonical_root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let mut staged = Staged::default();
    let result = stage_and_commit(&canonical_root, root, files, &mut staged);
    if result.is_err() {
        staged.discard();
    }
    result
}

fn stage_and_commit(
    canonical_root: &Path,
    root: &Path,
    files: &[GeneratedFile],
    staged: &mut Staged,
) -> Result<Vec<PathBuf>, String> {
    let mut targets = Vec::new();
    for file in files {
        let relative = Path::new(&file.path);
        if let Some(parent) = relative.parent() {
            create_directories(canonical_root, parent, staged)?;
        }
        let target = canonical_root.join(relative);
        let parent = target.parent().unwrap_or(canonical_root);
        let resolved = std::fs::canonicalize(parent)
            .map_err(|error| format!("cannot resolve {}: {error}", parent.display()))?;
        if !resolved.starts_with(canonical_root) {
            return Err(format!(
                "generated path '{}' leaves {} through a symbolic link",
                file.path,
                root.display()
            ));
        }
        if target.is_dir() {
            return Err(format!(
                "generated path '{}' is an existing directory",
                file.path
            ));
        }
        targets.push(target);
    }
    for (file, target) in files.iter().zip(&targets) {
        let temporary = temporary_sibling(target)?;
        staged.temporaries.push(temporary.clone());
        std::fs::write(&temporary, &file.content)
            .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
    }
    let mut written = Vec::new();
    for ((file, target), temporary) in files.iter().zip(&targets).zip(&staged.temporaries) {
        if let Err(error) = std::fs::rename(temporary, target) {
            return Err(format!(
                "cannot write {}: {error}; already replaced: {}",
                target.display(),
                if written.is_empty() {
                    "nothing".to_owned()
                } else {
                    written
                        .iter()
                        .map(|path: &PathBuf| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ));
        }
        written.push(root.join(&file.path));
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_relative_paths_are_accepted() {
        for path in [
            "a",
            "a/b.txt",
            "a.b/c-d/e_f",
            "..a/b..",
            "a/...",
            "a b/c",
            "é/ü.kt",
        ] {
            assert_eq!(check_relative_path(path), Ok(()), "{path}");
        }
        for (path, reason) in [
            ("", "is empty"),
            ("/a", "'/a' is absolute"),
            ("//server/share", "'//server/share' is absolute"),
            ("C:/a", "'C:/a' starts with a drive letter"),
            ("c:a", "'c:a' starts with a drive letter"),
            ("..", "'..' has a '..' component"),
            ("../a", "'../a' has a '..' component"),
            ("a/../b", "'a/../b' has a '..' component"),
            ("a/..", "'a/..' has a '..' component"),
            (".", "'.' has a '.' component"),
            ("./a", "'./a' has a '.' component"),
            ("a//b", "'a//b' has an empty component"),
            ("a/", "'a/' has an empty component"),
            (
                "a\\b",
                "'a\\\\b' contains a backslash; use '/' between components",
            ),
            (
                "..\\a",
                "'..\\\\a' contains a backslash; use '/' between components",
            ),
            ("a\0b", "'a\\u{0}b' contains a NUL"),
        ] {
            assert_eq!(
                check_relative_path(path),
                Err(reason.to_owned()),
                "{path:?}"
            );
        }
    }

    #[test]
    fn a_path_given_twice_is_rejected() {
        let file = |path: &str| GeneratedFile {
            path: path.to_owned(),
            content: String::new(),
        };
        assert_eq!(check_files(&[file("a"), file("b/a")]), Ok(()));
        assert_eq!(
            check_files(&[file("a"), file("a")]),
            Err("generated path 'a' is given more than once".to_owned())
        );
        assert_eq!(
            check_files(&[file("a"), file("../b")]),
            Err("generated path '../b' has a '..' component".to_owned())
        );
    }
}
