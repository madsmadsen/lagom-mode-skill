// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/discover.py
// license: MIT

//! Find source files and decide which language each one is written in.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

const EXTENSIONS: &[(&str, &str)] = &[
    (".clj", "clojure"),
    (".cljc", "clojure"),
    (".cljs", "clojure"),
    (".bb", "clojure"),
    (".java", "java"),
    (".go", "go"),
    (".ts", "typescript"),
    (".tsx", "typescript"),
    (".mts", "typescript"),
    (".cts", "typescript"),
    (".js", "typescript"),
    (".jsx", "typescript"),
    (".mjs", "typescript"),
    (".cjs", "typescript"),
    (".rs", "rust"),
    (".py", "python"),
];

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".idea",
    ".venv",
    "venv",
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    ".metrics",
    "testdata",
    "__pycache__",
    ".clj-kondo",
];

pub const TEST_DIRS: &[&str] = &["test", "tests", "spec", "specs", "__tests__"];

pub const GO_TEST_SUFFIX: &str = "_test.go";
pub const CLOJURE_TEST_SUFFIXES: &[&str] = &["_test.clj", "_test.cljc", "_test.cljs", "_test.bb"];
pub const JS_TEST_SUFFIXES: &[&str] = &[
    ".test.ts",
    ".spec.ts",
    ".test.tsx",
    ".spec.tsx",
    ".test.mts",
    ".spec.mts",
    ".test.cts",
    ".spec.cts",
    ".test.js",
    ".spec.js",
    ".test.jsx",
    ".spec.jsx",
    ".test.mjs",
    ".spec.mjs",
    ".test.cjs",
    ".spec.cjs",
];

/// Python's `Path.name`.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Python's `Path.suffix`: text from the last dot, empty for a leading or trailing dot.
fn suffix(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => &name[i..],
        _ => "",
    }
}

pub fn language_of(path: &Path) -> Option<&'static str> {
    let name = file_name(path);
    if name.ends_with(".d.ts") {
        return None;
    }
    let ext = suffix(&name);
    EXTENSIONS
        .iter()
        .find(|(known, _)| *known == ext)
        .map(|(_, language)| *language)
}

pub fn is_test_file(path: &Path) -> bool {
    is_test_name(&file_name(path)) || in_test_dir(path)
}

fn is_test_name(name: &str) -> bool {
    name.ends_with(GO_TEST_SUFFIX)
        || CLOJURE_TEST_SUFFIXES
            .iter()
            .chain(JS_TEST_SUFFIXES)
            .any(|suffix| name.ends_with(*suffix))
        || name == "conftest.py"
        || name.ends_with("_test.py")
        || (name.starts_with("test_") && name.ends_with(".py"))
}

/// Every path component counts, including the file name and any root given by the caller.
fn in_test_dir(path: &Path) -> bool {
    path.components().any(|part| match part {
        Component::Normal(name) => name.to_str().is_some_and(|name| TEST_DIRS.contains(&name)),
        _ => false,
    })
}

fn is_pruned_dir(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| SKIP_DIRS.contains(&name) || TEST_DIRS.contains(&name))
}

/// Python's `Path.resolve()`: the canonical path, or the absolute path when the file does not exist.
pub fn resolve(path: &Path) -> PathBuf {
    fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn collect_source(path: PathBuf, found: &mut Vec<PathBuf>) {
    if language_of(&path).is_some() && !is_test_file(&path) {
        found.push(path);
    }
}

/// Mirrors `os.walk` with `followlinks=False`: a symlink to a directory is listed but never entered.
fn walk_dir(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !is_pruned_dir(&entry.file_name()) {
                walk_dir(&path, found);
            }
        } else if !(kind.is_symlink() && path.is_dir()) {
            collect_source(path, found);
        }
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    match fs::metadata(root) {
        Ok(meta) if meta.is_file() => vec![root.to_path_buf()],
        Ok(meta) if meta.is_dir() => {
            let mut found = Vec::new();
            walk_dir(root, &mut found);
            found
        }
        _ => Vec::new(),
    }
}

pub fn iter_source_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let files: BTreeSet<PathBuf> = roots
        .iter()
        .flat_map(|root| walk(root))
        .map(|path| resolve(&path))
        .collect();
    let mut sorted: Vec<PathBuf> = files.into_iter().collect();
    sorted.sort_by(|a, b| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()));
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn write(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "def f():\n    return 1\n").unwrap();
    }

    fn canonical(root: &Path, rel: &str) -> PathBuf {
        fs::canonicalize(root.join(rel)).unwrap()
    }

    // CR-5
    #[test]
    fn cr_5_each_extension_maps_to_its_language() {
        let cases = [
            ("core.clj", "clojure"),
            ("core.cljc", "clojure"),
            ("core.cljs", "clojure"),
            ("script.bb", "clojure"),
            ("Board.java", "java"),
            ("board.go", "go"),
            ("app.ts", "typescript"),
            ("view.tsx", "typescript"),
            ("app.mts", "typescript"),
            ("app.cts", "typescript"),
            ("app.js", "typescript"),
            ("view.jsx", "typescript"),
            ("app.mjs", "typescript"),
            ("app.cjs", "typescript"),
            ("lib.rs", "rust"),
            ("app.py", "python"),
        ];
        for (name, language) in cases {
            assert_eq!(language_of(Path::new(name)), Some(language), "{name}");
        }
    }

    // CR-5
    #[test]
    fn cr_5_declaration_files_and_unknown_extensions_have_no_language() {
        for name in [
            "types.d.ts",
            "src/types.d.ts",
            "notes.md",
            "README",
            ".py",
            "a.",
        ] {
            assert_eq!(language_of(Path::new(name)), None, "{name}");
        }
    }

    // CR-6
    #[test]
    fn cr_6_test_file_names_are_test_files() {
        let names = [
            "main_test.go",
            "core_test.clj",
            "core_test.cljc",
            "core_test.cljs",
            "core_test.bb",
            "app.test.ts",
            "app.spec.tsx",
            "app.test.js",
            "app.spec.mjs",
            "app.test.cts",
            "conftest.py",
            "test_board.py",
            "board_test.py",
        ];
        for name in names {
            assert!(is_test_file(Path::new(name)), "{name}");
        }
    }

    // CR-6
    #[test]
    fn cr_6_source_names_are_not_test_files() {
        let names = [
            "app.cts",
            "app.ts",
            "testing_board.py",
            "tests_x.py",
            "contest.py",
            "board.py",
        ];
        for name in names {
            assert!(!is_test_file(Path::new(name)), "{name}");
        }
    }

    // CR-6
    #[test]
    fn cr_6_any_test_directory_component_makes_a_test_file() {
        assert!(is_test_file(Path::new("tests/board.py")));
        assert!(is_test_file(Path::new("/x/test/a.py")));
        assert!(is_test_file(Path::new("src/__tests__/a.ts")));
        assert!(is_test_file(Path::new("src/specs/x.java")));
        assert!(!is_test_file(Path::new("src/testing/x.py")));
    }

    // CR-6
    #[test]
    fn cr_6_ac14_layout_selects_only_the_source_outside_skipped_and_test_dirs() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [
            "src/app.py",
            "test/a.py",
            "src/test/b.py",
            "spec/c.py",
            "vendor/d.py",
            "node_modules/e.py",
            "target/f.py",
            "testing/g.py",
        ];
        for rel in files {
            write(root, rel);
        }
        let found = iter_source_files(&[root.to_path_buf()]);
        assert_eq!(
            found,
            vec![
                canonical(root, "src/app.py"),
                canonical(root, "testing/g.py")
            ]
        );
    }

    // CR-6
    #[test]
    fn cr_6_missing_root_has_no_files() {
        let dir = tempdir().unwrap();
        assert!(iter_source_files(&[dir.path().join("missing")]).is_empty());
    }

    // CR-6
    #[test]
    fn cr_6_a_root_file_is_listed_by_its_resolved_path() {
        let dir = tempdir().unwrap();
        write(dir.path(), "src/app.py");
        let file = dir.path().join("src/app.py");
        assert_eq!(
            iter_source_files(&[file]),
            vec![canonical(dir.path(), "src/app.py")]
        );
    }

    // CR-6
    #[test]
    fn cr_6_symlinked_directories_are_not_descended() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        write(outside.path(), "z.py");
        let root = dir.path();
        write(root, "src/app.py");
        symlink(outside.path(), root.join("linked")).unwrap();
        assert_eq!(
            iter_source_files(&[root.to_path_buf()]),
            vec![canonical(root, "src/app.py")]
        );
    }

    // CR-6
    #[test]
    fn cr_6_overlapping_roots_and_aliases_give_each_file_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "src/app.py");
        symlink(root.join("src/app.py"), root.join("alias.py")).unwrap();
        let roots = [
            root.join("src"),
            root.join("src/app.py"),
            root.to_path_buf(),
        ];
        assert_eq!(
            iter_source_files(&roots),
            vec![canonical(root, "src/app.py")]
        );
    }
}
