// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Path helpers shared across the crate.

use std::fs;
use std::path::{Path, PathBuf};

/// Makes `path` absolute, then canonicalizes the longest existing ancestor and appends the rest.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    canonical_prefix(&absolute)
}

fn canonical_prefix(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => canonical_prefix(parent).join(name),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // MU-9: an existing directory resolves to its canonical form.
    #[test]
    fn mu_9_resolve_existing_directory_is_canonical() {
        let dir = tempfile::tempdir().expect("tempdir");
        let canonical = fs::canonicalize(dir.path()).expect("canonical root");
        assert_eq!(resolve(dir.path()), canonical);
    }

    // MU-9: a missing leaf keeps its name under the canonical parent.
    #[test]
    fn mu_9_resolve_missing_leaf_joins_canonical_parent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let canonical = fs::canonicalize(dir.path()).expect("canonical root");
        assert_eq!(
            resolve(&dir.path().join("missing.txt")),
            canonical.join("missing.txt")
        );
    }

    // MU-9: several missing parents are all appended to the existing ancestor.
    #[test]
    fn mu_9_resolve_missing_nested_parents_keep_the_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let canonical = fs::canonicalize(dir.path()).expect("canonical root");
        assert_eq!(
            resolve(&dir.path().join("a/b/c.txt")),
            canonical.join("a/b/c.txt")
        );
    }

    // MU-10: a relative path with no existing ancestor becomes absolute.
    #[test]
    fn mu_10_resolve_relative_path_is_absolute() {
        let got = resolve(Path::new("mu10-missing-dir/file.txt"));
        assert!(got.is_absolute());
        assert!(got.ends_with("mu10-missing-dir/file.txt"));
    }
}
