mod common;

use common::*;
use std::fs;
use std::path::Path;
use std::process::Command;

const SRC: &str = "def ok(a, b):\n    return a >= b\n";
const EDITED: &str = "def ok(a, b):\n    return a > b\n";

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

/// The file names in the `Scan:` header lines, in output order.
fn scanned(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("Scan: "))
        .filter_map(|rest| rest.rsplit_once(" in "))
        .map(|(_, path)| path)
        .collect()
}

/// Committed a, b, c, e, test file and notes.txt; then b and c modified, c staged, d untracked,
/// e deleted, and the test file and notes.txt modified.
fn mixed_repo() -> tempfile::TempDir {
    let dir = project();
    for rel in [
        "src/a.py",
        "src/b.py",
        "src/c.py",
        "src/e.py",
        "test/test_b.py",
    ] {
        write(dir.path(), rel, SRC);
    }
    write(dir.path(), "notes.txt", "one\n");
    git_commit_all(dir.path());
    write(dir.path(), "src/b.py", EDITED);
    write(dir.path(), "src/c.py", EDITED);
    git(dir.path(), &["add", "src/c.py"]);
    write(dir.path(), "src/d.py", SRC);
    fs::remove_file(dir.path().join("src/e.py")).expect("delete e.py");
    write(dir.path(), "test/test_b.py", EDITED);
    write(dir.path(), "notes.txt", "two\n");
    dir
}

// AC-8 / MU-7: --changed selects the modified, staged, and untracked sources, and nothing else.
#[test]
fn mu_7_ac8_changed_scan_lists_modified_staged_and_untracked_sources_only() {
    let dir = mixed_repo();
    let out = run(dir.path(), &["--changed", "--scan", "--no-coverage"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(scanned(&out.stdout), ["src/b.py", "src/c.py", "src/d.py"]);
}

// AC-8 / MU-7: a changed file whose name has a space and non-ASCII letters is selected.
#[test]
fn mu_7_ac8_changed_selects_file_with_space_and_non_ascii_name() {
    let dir = project();
    write(dir.path(), "src/mé x.py", SRC);
    let out = run(dir.path(), &["--changed", "--scan", "--no-coverage"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(scanned(&out.stdout), ["src/mé x.py"]);
}

// MU-7: --changed outside a git repository exits non-zero and prints git's message.
#[test]
fn mu_7_changed_outside_git_repo_exits_with_git_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "src/a.py", SRC);
    let out = run_env(
        dir.path(),
        &["--changed", "--scan", "--no-coverage"],
        &[("LC_ALL", "C")],
    );
    assert_ne!(out.code, 0, "{}", out.stdout);
    assert!(
        out.stderr.contains("not a git repository"),
        "{}",
        out.stderr
    );
}
