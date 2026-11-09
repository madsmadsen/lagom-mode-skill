//! Port of crapper discover.py (BSD Zero Clause License, Robert C. Martin).

use crate::model::Language;
use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

/// Directory names the walker never enters.
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

/// Directory names that hold tests; the walker never enters them.
pub const TEST_DIRS: &[&str] = &["test", "tests", "spec", "specs", "__tests__"];

const EXTENSIONS: &[(&str, Language)] = &[
    ("clj", Language::Clojure),
    ("cljc", Language::Clojure),
    ("cljs", Language::Clojure),
    ("bb", Language::Clojure),
    ("java", Language::Java),
    ("go", Language::Go),
    ("ts", Language::TypeScript),
    ("tsx", Language::TypeScript),
    ("mts", Language::TypeScript),
    ("cts", Language::TypeScript),
    ("js", Language::TypeScript),
    ("jsx", Language::TypeScript),
    ("mjs", Language::TypeScript),
    ("cjs", Language::TypeScript),
    ("rs", Language::Rust),
    ("py", Language::Python),
];

const TEST_SUFFIXES: &[&str] = &[
    "_test.go",
    "_test.clj",
    "_test.cljc",
    "_test.cljs",
    "_test.bb",
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

/// Language of a source file by extension; `.d.ts` declaration files are not source.
pub fn language_of(path: &Path) -> Option<Language> {
    if name_of(path).ends_with(".d.ts") {
        return None;
    }
    let extension = path.extension()?.to_str()?;
    EXTENSIONS
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map(|(_, language)| *language)
}

/// True when the file name or any path part marks the file as a test.
pub fn is_test_file(path: &Path) -> bool {
    is_test_name(&name_of(path))
        || path
            .iter()
            .any(|part| part.to_str().is_some_and(|text| TEST_DIRS.contains(&text)))
}

/// Walks each root, keeping source files that are not tests; sorted, canonical, deduplicated.
pub fn iter_source_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| walk(root))
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .collect();
    files.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    files.dedup();
    files
}

/// Skipped directory names, sorted and joined as "a, b, and z" for --help.
pub fn skipped_directory_text() -> String {
    let mut names: Vec<&str> = SKIP_DIRS.iter().chain(TEST_DIRS).copied().collect();
    names.sort_unstable();
    names.dedup();
    names.split_last().map_or_else(String::new, |(last, rest)| {
        format!("{}, and {last}", rest.join(", "))
    })
}

fn name_of(path: &Path) -> Cow<'_, str> {
    path.file_name()
        .map_or(Cow::Borrowed(""), OsStr::to_string_lossy)
}

fn is_test_name(name: &str) -> bool {
    TEST_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) || is_python_test_name(name)
}

fn is_python_test_name(name: &str) -> bool {
    name == "conftest.py"
        || name.ends_with("_test.py")
        || (name.starts_with("test_") && name.ends_with(".py"))
}

fn is_skipped_dir(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|text| SKIP_DIRS.contains(&text) || TEST_DIRS.contains(&text))
}

fn walk(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        visit_dir(&dir, &mut pending, &mut found);
    }
    found
}

fn visit_dir(dir: &Path, pending: &mut Vec<PathBuf>, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            push_subdir(&entry, path, pending);
        } else {
            keep_source(path, found);
        }
    }
}

// Symlinked directories are classified as directories but never entered, as os.walk does.
fn push_subdir(entry: &DirEntry, path: PathBuf, pending: &mut Vec<PathBuf>) {
    let real_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
    if real_dir && !is_skipped_dir(&entry.file_name()) {
        pending.push(path);
    }
}

fn keep_source(path: PathBuf, found: &mut Vec<PathBuf>) {
    if language_of(&path).is_some() && !is_test_file(&path) {
        found.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(root: &Path, relative: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x\n").unwrap();
    }

    fn canonical(root: &Path, relative: &str) -> PathBuf {
        fs::canonicalize(root.join(relative)).unwrap()
    }

    #[test]
    // MU-5: language is detected by extension, as in the original.
    fn mu_5_language_of_maps_each_extension() {
        let cases = [
            ("a.clj", Some(Language::Clojure)),
            ("a.cljc", Some(Language::Clojure)),
            ("a.cljs", Some(Language::Clojure)),
            ("a.bb", Some(Language::Clojure)),
            ("Main.java", Some(Language::Java)),
            ("main.go", Some(Language::Go)),
            ("app.ts", Some(Language::TypeScript)),
            ("app.tsx", Some(Language::TypeScript)),
            ("app.mts", Some(Language::TypeScript)),
            ("app.cts", Some(Language::TypeScript)),
            ("lib.rs", Some(Language::Rust)),
            ("app.py", Some(Language::Python)),
            ("types.d.ts", None),
            ("notes.txt", None),
            ("Makefile", None),
            (".ts", None),
        ];
        for (name, expected) in cases {
            assert_eq!(language_of(Path::new(name)), expected, "{name}");
        }
    }

    #[test]
    // MU-5: the JavaScript extensions are handled as TypeScript.
    fn mu_5_javascript_extensions_are_typescript() {
        for name in ["a.js", "a.jsx", "a.mjs", "a.cjs"] {
            assert_eq!(
                language_of(Path::new(name)),
                Some(Language::TypeScript),
                "{name}"
            );
        }
    }

    #[test]
    // MU-6: the walker skips test, spec, vendor, node_modules and target directories.
    fn mu_6_walk_skips_test_spec_vendor_node_modules_and_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(root, "src/app.ts");
        for skipped in ["test", "spec", "vendor", "node_modules", "target"] {
            write_file(root, &format!("{skipped}/inside.ts"));
        }
        let found = iter_source_files(&[root.to_path_buf()]);
        assert_eq!(found, vec![canonical(root, "src/app.ts")]);
    }

    #[test]
    // MU-6: a file root is returned as is, even under a test directory.
    fn mu_6_file_root_is_returned_as_is() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "tests/only.py");
        let file = dir.path().join("tests/only.py");
        let found = iter_source_files(std::slice::from_ref(&file));
        assert_eq!(found, vec![canonical(dir.path(), "tests/only.py")]);
    }

    #[test]
    // MU-6: test files are not returned from a walk.
    fn mu_6_walk_skips_test_files_and_non_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in [
            "x_test.go",
            "app.test.ts",
            "test_x.py",
            "conftest.py",
            "notes.txt",
            "a.d.ts",
        ] {
            write_file(root, name);
        }
        write_file(root, "lib.rs");
        let found = iter_source_files(&[root.to_path_buf()]);
        assert_eq!(found, vec![canonical(root, "lib.rs")]);
    }

    #[test]
    // MU-6: the walk dedupes overlapping roots and sorts by path string.
    fn mu_6_iter_source_files_dedupes_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_file(root, "b/x.py");
        write_file(root, "a-c/y.py");
        write_file(root, "a/z.py");
        let roots = [root.to_path_buf(), root.join("a")];
        let found = iter_source_files(&roots);
        let expected = vec![
            canonical(root, "a-c/y.py"),
            canonical(root, "a/z.py"),
            canonical(root, "b/x.py"),
        ];
        assert_eq!(found, expected);
    }

    #[test]
    // MU-6: a missing root yields nothing.
    fn mu_6_missing_root_yields_nothing() {
        let missing = PathBuf::from("/definitely/not/a/real/path/for/mutator");
        assert!(iter_source_files(&[missing]).is_empty());
    }

    #[test]
    // MU-6: test files are recognized by name and by test directory.
    fn mu_6_is_test_file_recognizes_test_names_and_dirs() {
        let cases = [
            ("pkg/x_test.go", true),
            ("pkg/test_x.py", true),
            ("pkg/x_test.py", true),
            ("pkg/conftest.py", true),
            ("src/x.test.ts", true),
            ("src/x.spec.jsx", true),
            ("tests/helper.rs", true),
            ("src/app.ts", false),
            ("src/testing.py", false),
        ];
        for (path, expected) in cases {
            assert_eq!(is_test_file(Path::new(path)), expected, "{path}");
        }
    }

    #[test]
    // MU-6: the skipped-directory help text is the sorted union, joined with "and".
    fn mu_6_skipped_directory_text_is_sorted_union() {
        let text = skipped_directory_text();
        assert!(
            text.starts_with(".clj-kondo, .git, .hg, .idea, .metrics, .svn, .venv, __pycache__")
        );
        assert!(text.ends_with(", and venv"));
        assert!(text.contains(", vendor, and venv"));
        assert!(text.contains(", spec, specs, target, test, testdata, tests, "));
        assert!(text.contains(", and "));
    }
}
