// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! End-to-end check of MU-16: mutator reads `lcov.info` only below `coverage/` and
//! `target/coverage/`. A report at another path is not read, so each site is uncovered and the
//! run exits 0 with the no-coverage notice on stderr.

mod common;

use common::{Out, project, run, write};
use std::path::Path;
use tempfile::TempDir;

// Line 2 holds the site `> -> >=`, and line 6 holds the site `< -> <=`.
const M_PY: &str = "def gt(a, b):\n    return a > b\n\n\ndef lt(a, b):\n    return a < b\n";

// Line 2 is hit and line 6 is not, so the report separates a covered site from an uncovered one.
const LCOV: &str = "SF:src/m.py\nDA:2,1\nDA:6,0\nend_of_record\n";

const TEST_COMMAND: &str = "grep -q 'a > b' src/m.py";

const NOTICE: &str = "No coverage data for src/m.py; its sites are uncovered. Pass --no-coverage to run them anyway.";

/// A project with `src/m.py` and the LCOV report at `rel`.
fn project_with_report(rel: &str) -> TempDir {
    let dir = project();
    write(dir.path(), "src/m.py", M_PY);
    write(dir.path(), rel, LCOV);
    dir
}

/// Runs mutator on `src/m.py` and reads the coverage already on disk.
fn mutate(dir: &Path) -> Out {
    run(
        dir,
        &[
            "--use-existing-coverage",
            "--max-workers",
            "1",
            "--test-command",
            TEST_COMMAND,
            "src/m.py",
        ],
    )
}

/// True when the site log names `status` for the site on `line`.
fn logged(stdout: &str, status: &str, line: usize, site: &str) -> bool {
    let want = format!("{status:<9} src/m.py:{line} {site}");
    stdout.lines().any(|text| text == want)
}

/// True when the site log has no line for a mutant that ran.
fn no_mutant_ran(stdout: &str) -> bool {
    !stdout
        .lines()
        .any(|line| line.starts_with("KILLED") || line.starts_with("SURVIVED"))
}

// MU-16: `lcov.info` at the project root is not read. Both sites are uncovered, no mutant runs,
// the notice is on stderr, and the exit is 0. The same report below `coverage/`,
// `target/coverage/`, and a nested directory under `target/coverage/` is read: the site on line 2
// is covered and killed, the site on line 6 stays uncovered, and no notice is printed.
#[test]
fn mu_16_lcov_is_read_only_below_coverage_and_target_coverage() {
    let root = project_with_report("lcov.info");
    let out = mutate(root.path());
    assert_eq!(out.code, 0, "stderr:\n{}", out.stderr);
    assert!(
        out.stderr.contains(NOTICE),
        "the root report is not read, so the notice must appear:\n{}",
        out.stderr
    );
    assert!(
        logged(&out.stdout, "UNCOVERED", 2, "> -> >="),
        "stdout:\n{}",
        out.stdout
    );
    assert!(
        logged(&out.stdout, "UNCOVERED", 6, "< -> <="),
        "stdout:\n{}",
        out.stdout
    );
    assert!(no_mutant_ran(&out.stdout), "stdout:\n{}", out.stdout);

    for rel in [
        "coverage/lcov.info",
        "target/coverage/lcov.info",
        "target/coverage/rust/x/lcov.info",
    ] {
        let dir = project_with_report(rel);
        let out = mutate(dir.path());
        assert_eq!(out.code, 0, "{rel} stderr:\n{}", out.stderr);
        assert!(
            !out.stderr.contains("No coverage data"),
            "{rel} must be read:\n{}",
            out.stderr
        );
        assert!(
            logged(&out.stdout, "KILLED", 2, "> -> >="),
            "{rel} stdout:\n{}",
            out.stdout
        );
        assert!(
            logged(&out.stdout, "UNCOVERED", 6, "< -> <="),
            "{rel} stdout:\n{}",
            out.stdout
        );
    }
}
