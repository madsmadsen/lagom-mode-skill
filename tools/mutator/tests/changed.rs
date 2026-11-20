// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
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

// MU-7 / MU-20: --changed outside a Git repository exits with the code of Git, which is 128,
// and prints Git's message. `GIT_CEILING_DIRECTORIES` stops git from finding a work tree above
// the temp dir, so the test does not depend on where TMPDIR lies.
#[test]
fn mu_7_mu_20_changed_outside_git_repo_exits_with_git_code_128() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "src/a.py", SRC);
    let ceiling = dir.path().to_string_lossy().into_owned();
    let out = run_env(
        dir.path(),
        &["--changed", "--scan", "--no-coverage"],
        &[("LC_ALL", "C"), ("GIT_CEILING_DIRECTORIES", &ceiling)],
    );
    assert_eq!(out.code, 128, "{}", out.stdout);
    assert!(
        out.stderr.contains("not a git repository"),
        "{}",
        out.stderr
    );
}

// MU-20: when Git cannot start at all, for example when `git` is not on PATH, the port exits
// 1, which is not a code of Git, and prints the error of the system on stderr.
#[test]
fn mu_20_a_git_that_cannot_start_exits_one_and_prints_the_system_error() {
    let dir = project();
    write(dir.path(), "src/a.py", SRC);
    let out = run_env(
        dir.path(),
        &["--changed", "--scan", "--no-coverage"],
        &[("PATH", "/nonexistent-mutator-bin")],
    );
    assert_eq!(out.code, 1, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("No such file or directory")
            || out.stderr.contains("not found")
            || out.stderr.contains("os error 2"),
        "stderr names no system error: {}",
        out.stderr
    );
    assert!(
        out.stdout.is_empty(),
        "a failing Git still printed a selection: {}",
        out.stdout
    );
}
