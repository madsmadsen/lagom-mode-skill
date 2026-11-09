// Derived from dryer by Robert C. Martin (https://github.com/unclebob/dryer), BSD Zero Clause License.
// Ports src/dryer/cli.py: GitStatusError, _git, _changed_files, _positionals, _tracked_source,
// _changed_source, _explicit_files, _selected, _apply_filters, select_files.

use crate::args::Options;
use crate::discover::{is_test_file, iter_source_files, language_of, posix, resolve, SKIP_DIRS};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// git status could not be read. `code` is git's own status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatusError {
    pub code: i32,
    pub message: String,
}

impl fmt::Display for GitStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitStatusError {}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, GitStatusError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::inherit())
        .output()
        .map_err(|err| GitStatusError {
            code: 1,
            message: format!("git: {err}"),
        })?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let code = output.status.code().unwrap_or(1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    let message = if trimmed.is_empty() {
        "git status failed".to_string()
    } else {
        trimmed.to_string()
    };
    Err(GitStatusError { code, message })
}

/// Path for one `git status --porcelain -z` entry (`XY PATH`), relative to the repo top.
fn entry_path(toplevel: &Path, entry: &[u8]) -> PathBuf {
    let name = String::from_utf8_lossy(&entry[3..]);
    resolve(&toplevel.join(&*name))
}

/// Added and modified files under `root`, as paths from the repo root.
///
/// `-z` keeps spaces and non-ASCII names intact. A missing path, including a
/// deletion, is left out. A rename is the new file only.
pub fn changed_files(root: &Path) -> Result<Vec<PathBuf>, GitStatusError> {
    let toplevel_bytes = git(root, &["rev-parse", "--show-toplevel"])?;
    let toplevel = PathBuf::from(String::from_utf8_lossy(&toplevel_bytes).trim());
    let status = git(
        root,
        &[
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--",
            ".",
        ],
    )?;
    let root_resolved = resolve(root);
    Ok(status
        .split(|byte| *byte == 0)
        .filter(|entry| entry.len() > 3)
        .map(|entry| entry_path(&toplevel, entry))
        .filter(|path| path.is_file() && path.starts_with(&root_resolved))
        .collect())
}

/// Splits positionals into existing paths (resolved) and substring filters.
fn positionals(root: &Path, args: &[String]) -> (Vec<PathBuf>, Vec<String>) {
    let mut existing = Vec::new();
    let mut filters = Vec::new();
    for arg in args {
        let candidate = root.join(arg);
        if candidate.exists() {
            existing.push(resolve(&candidate));
        } else {
            filters.push(arg.clone());
        }
    }
    (existing, filters)
}

/// Normal path components as text. Root and prefix components are skipped, as are `.`/`..`.
fn part_names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Parts used for the skipped-directory check. With a root, only the part below it counts.
fn check_parts(path: &Path, root: Option<&Path>) -> Option<Vec<String>> {
    match root {
        None => Some(part_names(path)),
        Some(root) => {
            let resolved = resolve(path);
            resolved.strip_prefix(resolve(root)).ok().map(part_names)
        }
    }
}

/// Whether `path` is a source file that a scan should analyze.
pub fn tracked_source(path: &Path, root: Option<&Path>) -> bool {
    let Some(parts) = check_parts(path, root) else {
        return false;
    };
    if parts.iter().any(|part| SKIP_DIRS.contains(&part.as_str())) {
        return false;
    }
    if language_of(path).is_none() {
        return false;
    }
    !is_test_file(path)
}

fn changed_source(root: &Path) -> Result<Vec<PathBuf>, GitStatusError> {
    Ok(changed_files(root)?
        .into_iter()
        .filter(|path| tracked_source(path, Some(root)))
        .collect())
}

/// Named files with a known language, and every source file under named directories.
fn explicit_files(existing: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in existing {
        if path.is_dir() {
            files.extend(iter_source_files(std::slice::from_ref(path)));
        } else if language_of(path).is_some() {
            files.push(path.clone());
        }
    }
    files
}

fn selected(
    options: &Options,
    root: &Path,
    existing: &[PathBuf],
) -> Result<Vec<PathBuf>, GitStatusError> {
    if options.changed {
        return changed_source(root);
    }
    if !options.source_roots.is_empty() {
        let roots: Vec<PathBuf> = options
            .source_roots
            .iter()
            .map(|path| resolve(&root.join(path)))
            .collect();
        return Ok(iter_source_files(&roots));
    }
    if !existing.is_empty() {
        return Ok(explicit_files(existing));
    }
    Ok(iter_source_files(&[root.to_path_buf()]))
}

fn apply_filters(files: Vec<PathBuf>, filters: &[String]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|path| {
            let text = posix(path);
            filters.iter().any(|item| text.contains(item.as_str()))
        })
        .collect()
}

/// Resolves, removes duplicates, and sorts by posix string.
fn unique_sorted(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut resolved: Vec<PathBuf> = files.iter().map(|path| resolve(path)).collect();
    resolved.sort();
    resolved.dedup();
    resolved.sort_by_cached_key(|path| posix(path));
    resolved
}

/// Source files chosen by the options: changed, source roots, named paths, or the project root.
pub fn select_files(options: &Options) -> Result<Vec<PathBuf>, GitStatusError> {
    let root = resolve(&options.project_root);
    let (existing, filters) = positionals(&root, &options.positionals);
    let files = apply_filters(selected(options, &root, &existing)?, &filters);
    Ok(unique_sorted(&files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::parse_args;
    use std::fs;
    use tempfile::TempDir;

    fn proj(tmp: &TempDir) -> PathBuf {
        let dir = tmp.path().join("proj");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn root_arg(dir: &Path) -> String {
        dir.to_str().unwrap().to_string()
    }

    fn write(root: &Path, name: &str, text: &str) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init"]);
    }

    fn commit(dir: &Path, message: &str) {
        git(dir, &["add", "-A"]);
        git(
            dir,
            &[
                "-c",
                "user.email=a@b",
                "-c",
                "user.name=n",
                "commit",
                "-m",
                message,
            ],
        );
    }

    fn options(list: &[&str]) -> Options {
        let args: Vec<String> = list.iter().map(|item| item.to_string()).collect();
        parse_args(&args)
    }

    fn relative(files: &[PathBuf], base: &Path) -> Vec<String> {
        let base = resolve(base);
        files
            .iter()
            .map(|path| posix(path.strip_prefix(&base).unwrap()))
            .collect()
    }

    // DR-7
    #[test]
    fn dr7_changed_files_reads_real_status() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        init_repo(&dir);
        write(&dir, "src/a.py", "x = 1\n");
        write(&dir, "src/old.py", "x = 1\n");
        write(&dir, "gone.py", "x = 1\n");
        commit(&dir, "base");
        fs::remove_file(dir.join("gone.py")).unwrap();
        git(&dir, &["mv", "src/old.py", "src/b.py"]);
        write(&dir, "src/a.py", "x = 2\n");
        write(&dir, "src/c d.py", "x = 1\n");
        write(&dir, "src/café.py", "x = 1\n");
        write(&dir, "fresh/nested/new.py", "x = 1\n");
        let found = changed_files(&dir).unwrap();
        for name in [
            "src/a.py",
            "src/b.py",
            "src/c d.py",
            "src/café.py",
            "fresh/nested/new.py",
        ] {
            assert!(found.contains(&resolve(&dir.join(name))), "missing {name}");
        }
        assert!(!found.contains(&resolve(&dir.join("gone.py"))));
        assert!(!found.contains(&resolve(&dir.join("src/old.py"))));
    }

    // DR-7
    #[test]
    fn dr7_changed_files_reports_git_failure_outside_a_repo() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        let err = changed_files(&dir).unwrap_err();
        assert_eq!(err.code, 128);
        assert!(!err.message.is_empty());
    }

    // DR-7
    #[test]
    fn dr7_select_changed_reports_git_failure_outside_a_repo() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        let root = root_arg(&dir);
        let err = select_files(&options(&["--root", &root, "--changed"])).unwrap_err();
        assert_eq!(err.code, 128);
        assert!(!err.message.is_empty());
    }

    // DR-7
    #[test]
    fn dr7_changed_from_a_subdirectory_stays_inside_it() {
        let tmp = TempDir::new().unwrap();
        let repo = proj(&tmp);
        let sub = repo.join("sub");
        init_repo(&repo);
        write(&repo, "src/above.py", "x = 1\n");
        write(&sub, "src/below.py", "x = 1\n");
        let found = changed_files(&sub).unwrap();
        assert_eq!(found, vec![resolve(&sub.join("src/below.py"))]);
        let root = root_arg(&sub);
        let chosen = select_files(&options(&["--root", &root, "--changed"])).unwrap();
        assert_eq!(chosen, vec![resolve(&sub.join("src/below.py"))]);
    }

    // DR-7
    #[test]
    fn dr7_changed_selects_only_tracked_source_files() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        init_repo(&dir);
        write(&dir, "src/c.py", "x = 1\n");
        commit(&dir, "base");
        write(&dir, "src/a.py", "x = 2\n");
        write(&dir, "src/b.py", "x = 2\n");
        write(&dir, "tests/test_a.py", "x = 2\n");
        write(&dir, "README.md", "notes\n");
        let root = root_arg(&dir);
        let chosen = select_files(&options(&["--root", &root, "--changed"])).unwrap();
        assert_eq!(relative(&chosen, &dir), vec!["src/a.py", "src/b.py"]);
    }

    // DR-6
    #[test]
    fn dr6_tracked_source_skips_unknown_test_and_skipped_paths() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        assert!(!tracked_source(&dir.join("README.md"), None));
        assert!(!tracked_source(&dir.join("tests/test_a.py"), None));
        assert!(tracked_source(&dir.join("src/a.py"), None));
        assert!(!tracked_source(&dir.join("target/app.py"), None));
        assert!(tracked_source(&dir.join("src/a.py"), Some(dir.as_path())));
        assert!(!tracked_source(
            &tmp.path().join("elsewhere/src/a.py"),
            Some(dir.as_path())
        ));
    }

    // DR-6
    #[test]
    fn dr6_test_files_and_skipped_directories_are_left_out() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        for name in [
            "src/app.py",
            "src/foo_test.py",
            "target/app.py",
            "tests/app.py",
            "src/app.test.cts",
            "src/app.spec.cts",
            "src/widget.cts",
        ] {
            write(&dir, name, "x = 1\n");
        }
        let root = root_arg(&dir);
        let found = select_files(&options(&["--root", &root])).unwrap();
        assert_eq!(relative(&found, &dir), vec!["src/app.py", "src/widget.cts"]);
    }

    // DR-6
    #[test]
    fn dr6_a_positional_filter_matches_path_substrings() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        write(&dir, "src/a.py", "x = 1\n");
        write(&dir, "src/other/b.py", "x = 1\n");
        let root = root_arg(&dir);
        let found = select_files(&options(&["--root", &root, "other"])).unwrap();
        assert_eq!(relative(&found, &dir), vec!["src/other/b.py"]);
    }

    // DR-6
    #[test]
    fn dr6_a_positional_dir_or_file_selects_only_that_path() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        write(&dir, "src/a.py", "x = 1\n");
        write(&dir, "src/b.py", "x = 1\n");
        write(&dir, "lib/c.py", "x = 1\n");
        let root = root_arg(&dir);
        let by_dir = select_files(&options(&["--root", &root, "src"])).unwrap();
        assert_eq!(relative(&by_dir, &dir), vec!["src/a.py", "src/b.py"]);
        let by_file = select_files(&options(&["--root", &root, "lib/c.py"])).unwrap();
        assert_eq!(relative(&by_file, &dir), vec!["lib/c.py"]);
    }

    // DR-6
    #[test]
    fn dr6_source_root_limits_the_walk() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        write(&dir, "src/a.py", "x = 1\n");
        write(&dir, "extra/b.py", "x = 1\n");
        let root = root_arg(&dir);
        let found = select_files(&options(&["--root", &root, "--source-root", "src"])).unwrap();
        assert_eq!(relative(&found, &dir), vec!["src/a.py"]);
    }

    // DR-6
    #[test]
    fn dr6_a_repeated_positional_is_listed_once() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        write(&dir, "src/a.py", "x = 1\n");
        let root = root_arg(&dir);
        let found = select_files(&options(&["--root", &root, "src/a.py", "src/a.py"])).unwrap();
        assert_eq!(relative(&found, &dir), vec!["src/a.py"]);
    }

    // DR-2
    #[test]
    fn dr2_an_explicit_test_file_is_included_when_named() {
        let tmp = TempDir::new().unwrap();
        let dir = proj(&tmp);
        write(&dir, "tests/test_a.py", "x = 1\n");
        write(&dir, "src/a.py", "x = 1\n");
        let root = root_arg(&dir);
        let found = select_files(&options(&["--root", &root, "tests/test_a.py"])).unwrap();
        assert_eq!(relative(&found, &dir), vec!["tests/test_a.py"]);
    }
}
