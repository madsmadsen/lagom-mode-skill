// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! End-to-end checks of MU-21 second runs (AC-13) and MU-NFR-1 determinism.

mod common;

use common::{git_commit_all, project, run, snapshot_text, snapshots, write};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const M_PY: &str = "def f(a, b):\n    return a < b and a != 0\n";

/// True when the run logged `status` for the mutant `site` on `src/m.py` line 2.
fn logged(stdout: &str, status: &str, site: &str) -> bool {
    let want = format!("{status:<9} src/m.py:2 {site}");
    stdout.lines().any(|line| line == want)
}

fn project_with_m() -> TempDir {
    let dir = project();
    write(dir.path(), "src/m.py", M_PY);
    git_commit_all(dir.path());
    dir
}

/// The text of the one snapshot file, with its `:tested-at` line kept.
fn snapshot_bytes(dir: &Path) -> String {
    let files = snapshots(dir);
    assert_eq!(files.len(), 1, "expected one snapshot file: {files:?}");
    let (rel, _) = files.iter().next().unwrap();
    snapshot_text(dir, rel)
}

// AC-13 (MU-21): the first run of the `<` -> `<=` mutant times out, so it runs again with 2 times
// the timeout. The second run exits 0, and that outcome decides: SURVIVED. The counter file
// outside the project shows two runs.
#[test]
fn mu_21_second_run_decides_the_outcome() {
    let scratch = tempfile::tempdir().unwrap();
    let counter = scratch.path().join("counter");
    let command = format!(
        "if grep -q 'a <= b' src/m.py; then echo x >> {c}; \
         if [ $(grep -c x {c}) -eq 1 ]; then sleep 3; fi; fi; exit 0",
        c = counter.display()
    );
    let dir = project_with_m();
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--max-workers",
            "1",
            "--test-command",
            command.as_str(),
        ],
    );
    assert_eq!(out.code, 3, "stderr:\n{}", out.stderr);
    assert!(
        logged(&out.stdout, "SURVIVED", "< -> <="),
        "stdout:\n{}",
        out.stdout
    );
    let runs = fs::read_to_string(&counter).unwrap();
    assert_eq!(runs.lines().count(), 2, "the mutant ran twice");
}

// AC-13 (MU-21, MU-NFR-1): a mutant that takes 1.9 s against a 2 s timeout is within 10% of its
// timeout, so it runs again and passes. Five runs give the same exit code, stdout and snapshot
// bytes.
#[test]
fn mu_nfr_1_five_runs_give_identical_snapshots() {
    let command = "if grep -q 'a <= b' src/m.py; then sleep 1.9; fi; exit 0";
    let dir = project_with_m();
    let args = [
        "--mutate-all",
        "--no-coverage",
        "--max-workers",
        "1",
        "--test-command",
        command,
    ];
    let first = run(dir.path(), &args);
    assert_eq!(first.code, 3, "stderr:\n{}", first.stderr);
    assert!(
        logged(&first.stdout, "SURVIVED", "< -> <="),
        "stdout:\n{}",
        first.stdout
    );
    let first_snapshot = snapshot_bytes(dir.path());
    for _ in 1..5 {
        let again = run(dir.path(), &args);
        assert_eq!(again.code, first.code, "stderr:\n{}", again.stderr);
        assert_eq!(again.stdout, first.stdout);
        assert_eq!(snapshot_bytes(dir.path()), first_snapshot);
    }
}
