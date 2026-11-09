// Derived from dryer by Robert C. Martin (https://github.com/unclebob/dryer), BSD Zero Clause License.
// Ports src/dryer/discover.py.

use crate::model::Language;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

// DR-5: extension to language, matched against Python's Path.suffix.
const EXTENSIONS: &[(&str, Language)] = &[
    (".clj", Language::Clojure),
    (".cljc", Language::Clojure),
    (".cljs", Language::Clojure),
    (".cljd", Language::Clojure),
    (".bb", Language::Clojure),
    (".java", Language::Java),
    (".go", Language::Go),
    (".ts", Language::TypeScript),
    (".tsx", Language::TypeScript),
    (".mts", Language::TypeScript),
    (".cts", Language::TypeScript),
    (".rs", Language::Rust),
    (".py", Language::Python),
];

// DR-6: directories that the walk never enters.
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

// DR-6: file-name suffixes that mark a test file.
const TEST_SUFFIXES: &[&str] = &[
    "_test.go",
    "_test.clj",
    "_test.cljc",
    "_test.cljs",
    "_test.cljd",
    "_test.bb",
    ".test.ts",
    ".spec.ts",
    ".test.tsx",
    ".spec.tsx",
    ".test.mts",
    ".spec.mts",
    ".test.cts",
    ".spec.cts",
    "_test.py",
];

// DR-5: a `.d.ts` declaration is never analyzed, and the suffix is looked up
// with Python Path.suffix rules.
pub fn language_of(path: &Path) -> Option<Language> {
    let name = file_name_of(path);
    if name.ends_with(".d.ts") {
        return None;
    }
    let ext = suffix_of(&name);
    EXTENSIONS
        .iter()
        .find(|(known, _)| *known == ext)
        .map(|(_, language)| *language)
}

// DR-6: test detection uses the file name and every path component.
pub fn is_test_file(path: &Path) -> bool {
    is_test_name(&file_name_of(path))
        || path
            .components()
            .any(|part| is_test_dir_part(part.as_os_str()))
}

pub fn is_skip_dir_name(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

// DR-6: `dryer` skips both skip-dirs and test-dirs during the walk.
fn skipped_dir(name: &str) -> bool {
    is_skip_dir_name(name) || TEST_DIRS.contains(&name)
}

fn is_test_name(name: &str) -> bool {
    name == "conftest.py"
        || (name.starts_with("test_") && name.ends_with(".py"))
        || TEST_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

fn is_test_dir_part(part: &OsStr) -> bool {
    part.to_str().is_some_and(|text| TEST_DIRS.contains(&text))
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// Python Path.suffix: the text from the last dot, unless the dot is the first
// or last byte of the name.
fn suffix_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(index) if index > 0 && index < name.len() - 1 => &name[index..],
        _ => "",
    }
}

// DR-6: os.walk semantics. Symlinked directories are listed but never entered.
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

fn walk_dir(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() && fs::metadata(&path).is_ok_and(|meta| meta.is_dir()) {
            continue;
        }
        if !kind.is_dir() {
            keep_file(path, found);
        } else if !skipped_dir(&entry.file_name().to_string_lossy()) {
            walk_dir(&path, found);
        }
    }
}

fn keep_file(path: PathBuf, found: &mut Vec<PathBuf>) {
    if language_of(&path).is_some() && !is_test_file(&path) {
        found.push(path);
    }
}

// DR-6: every root is walked, each file is resolved, then the list is
// deduplicated and sorted by its posix string.
pub fn iter_source_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| walk(root))
        .map(|path| resolve(&path))
        .collect();
    files.sort_by_cached_key(|path| posix(path));
    files.dedup_by_key(|path| posix(path.as_path()));
    files
}

// Python Path.resolve() (non-strict). Each prefix is canonicalized while it
// exists; `..` pops the already-resolved prefix.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut resolved = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
        if let Ok(real) = fs::canonicalize(&resolved) {
            resolved = real;
        }
    }
    resolved
}

pub fn posix(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, "x = 1\n").expect("write");
    }

    fn project() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let proj = tmp.path().join("proj");
        fs::create_dir(&proj).expect("proj");
        (tmp, proj)
    }

    // DR-5
    #[test]
    fn dr5_language_of_maps_every_known_extension() {
        for (ext, language) in EXTENSIONS {
            let name = format!("src/file{ext}");
            assert_eq!(language_of(Path::new(&name)), Some(*language), "{name}");
        }
    }

    // DR-5
    #[test]
    fn dr5_language_of_matches_python_language_detection() {
        assert_eq!(
            language_of(Path::new("src/app/core.clj")),
            Some(Language::Clojure)
        );
        assert_eq!(
            language_of(Path::new("src/app/core.cljd")),
            Some(Language::Clojure)
        );
        assert_eq!(language_of(Path::new("Widget.java")), Some(Language::Java));
        assert_eq!(language_of(Path::new("board.go")), Some(Language::Go));
        assert_eq!(
            language_of(Path::new("ui/view.tsx")),
            Some(Language::TypeScript)
        );
        assert_eq!(language_of(Path::new("src/lib.rs")), Some(Language::Rust));
        assert_eq!(
            language_of(Path::new("src/dryer/cli.py")),
            Some(Language::Python)
        );
        assert_eq!(language_of(Path::new("types.d.ts")), None);
        assert_eq!(language_of(Path::new("notes.md")), None);
    }

    // DR-5: Python Path.suffix has no suffix for a bare dot-name, and is case-sensitive.
    #[test]
    fn dr5_language_of_follows_python_suffix_rules() {
        assert_eq!(language_of(Path::new(".py")), None);
        assert_eq!(language_of(Path::new(".bashrc")), None);
        assert_eq!(language_of(Path::new("a.b.py")), Some(Language::Python));
        assert_eq!(language_of(Path::new("App.PY")), None);
    }

    // DR-6
    #[test]
    fn dr6_is_test_file_recognizes_test_names() {
        let names = [
            "pkg/foo_test.go",
            "src/app_test.clj",
            "src/app_test.cljc",
            "src/app_test.cljs",
            "src/app_test.cljd",
            "src/app_test.bb",
            "ui/a.test.ts",
            "ui/a.spec.ts",
            "ui/a.test.tsx",
            "ui/a.spec.tsx",
            "ui/a.test.mts",
            "ui/a.spec.mts",
            "ui/a.test.cts",
            "ui/a.spec.cts",
            "tests/conftest.py",
            "pkg/foo_test.py",
            "pkg/test_foo.py",
            "tests/app.py",
        ];
        for name in names {
            assert!(is_test_file(Path::new(name)), "{name}");
        }
        assert!(!is_test_file(Path::new("src/app.py")));
    }

    // DR-6: every path component counts, including the file's own directories.
    #[test]
    fn dr6_is_test_file_checks_every_path_component() {
        assert!(is_test_file(Path::new("spec/s.py")));
        assert!(is_test_file(Path::new("__tests__/a.ts")));
        assert!(is_test_file(Path::new("/x/tests/app.py")));
        assert!(!is_test_file(Path::new("testdata/x.go")));
    }

    // DR-6
    #[test]
    fn dr6_skip_dir_names_match_the_python_set() {
        assert!(is_skip_dir_name("node_modules"));
        assert!(is_skip_dir_name("testdata"));
        assert!(!is_skip_dir_name("src"));
        assert!(!is_skip_dir_name("tests"));
    }

    // DR-6: skipped and test-directory files never reach the result.
    #[test]
    fn dr6_iter_source_files_skips_skipped_dirs_and_test_files() {
        let (_tmp, proj) = project();
        for rel in [
            "src/app.py",
            "src/foo_test.py",
            "target/app.py",
            "tests/app.py",
            "src/app.test.cts",
            "src/app.spec.cts",
            "src/widget.cts",
            "node_modules/pkg/index.ts",
            "vendor/v.go",
            "testdata/t.go",
            "__pycache__/p.py",
            "pkg/bar_test.go",
            "web/app.spec.ts",
            "src/test_x.py",
            "conftest.py",
            "spec/s.py",
            "test/t.py",
            "notes.md",
        ] {
            write(&proj, rel);
        }
        let found = iter_source_files(std::slice::from_ref(&proj));
        assert_eq!(
            found,
            vec![
                resolve(&proj.join("src/app.py")),
                resolve(&proj.join("src/widget.cts"))
            ]
        );
    }

    // DR-6: a symlinked directory is listed by os.walk but never entered.
    #[cfg(unix)]
    #[test]
    fn dr6_iter_source_files_does_not_follow_symlinked_directories() {
        let (_tmp, proj) = project();
        write(&proj, "real/inner.py");
        std::os::unix::fs::symlink(proj.join("real"), proj.join("linked")).expect("symlink");
        let found = iter_source_files(std::slice::from_ref(&proj));
        assert_eq!(found, vec![resolve(&proj.join("real/inner.py"))]);
    }

    // DR-6: two paths that resolve to one file produce one entry.
    #[cfg(unix)]
    #[test]
    fn dr6_iter_source_files_keeps_one_entry_per_resolved_file() {
        let (_tmp, proj) = project();
        write(&proj, "real/inner.py");
        std::os::unix::fs::symlink(proj.join("real/inner.py"), proj.join("alias.py"))
            .expect("symlink");
        let found = iter_source_files(std::slice::from_ref(&proj));
        assert_eq!(found, vec![resolve(&proj.join("real/inner.py"))]);
    }

    // DR-6: a missing root contributes nothing.
    #[test]
    fn dr6_iter_source_files_returns_empty_for_missing_root() {
        let (_tmp, proj) = project();
        assert!(iter_source_files(&[proj.join("missing")]).is_empty());
    }

    // DR-6: a file root is returned without the language or test filter.
    #[test]
    fn dr6_iter_source_files_returns_a_file_root_without_filtering() {
        let (_tmp, proj) = project();
        write(&proj, "notes.md");
        assert_eq!(
            iter_source_files(&[proj.join("notes.md")]),
            vec![resolve(&proj.join("notes.md"))]
        );
    }

    // DR-6: resolve keeps a nonexistent child under the real parent.
    #[test]
    fn dr6_resolve_appends_a_nonexistent_child_to_the_real_parent() {
        let (_tmp, proj) = project();
        let real = fs::canonicalize(&proj).expect("canonical");
        assert_eq!(resolve(&proj.join("nope.py")), real.join("nope.py"));
    }

    // DR-6: resolve collapses `..` after the prefix has been resolved.
    #[test]
    fn dr6_resolve_collapses_parent_segments() {
        let (_tmp, proj) = project();
        fs::create_dir(proj.join("a")).expect("a");
        let real = fs::canonicalize(&proj).expect("canonical");
        assert_eq!(resolve(&proj.join("a/../b")), real.join("b"));
    }

    // DR-6
    #[test]
    fn dr6_posix_renders_forward_slashes() {
        assert_eq!(posix(Path::new("a/b.py")), "a/b.py");
    }
}
