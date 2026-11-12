// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! End-to-end checks of MU-15 timeouts and the compile-failure rule (AC-4).

mod common;
use common::{Out, git_commit_all, project, run, write};
use std::time::{Duration, Instant};
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

/// Runs the mutator on the Python fixture with a shell test command.
fn mutate(test_command: &str, extra: &[&str]) -> (TempDir, Out) {
    let dir = project_with_m();
    let mut args = vec!["--no-coverage", "--test-command", test_command];
    args.extend_from_slice(extra);
    let out = run(dir.path(), &args);
    (dir, out)
}

// AC-4 (MU-15): a mutant that times out counts as killed, and the run ends in under 20 s.
#[test]
fn mu_15_timed_out_mutant_is_killed_within_twenty_seconds() {
    let command = "if grep -q 'a <= b' src/m.py; then sleep 30; fi; exit 0";
    let started = Instant::now();
    let (_dir, out) = mutate(command, &[]);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(20), "run took {elapsed:?}");
    assert!(
        logged(&out.stdout, "KILLED", "< -> <="),
        "stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
}

// AC-4 (MU-15): a mutant whose test command fails, as a compile failure does, counts as killed.
#[test]
fn mu_15_mutant_that_fails_to_compile_is_killed() {
    let command = "if grep -q 'a == 0' src/m.py; then exit 1; fi; exit 0";
    let (_dir, out) = mutate(command, &[]);
    assert!(
        logged(&out.stdout, "KILLED", "!= -> =="),
        "stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
}

// AC-4 (MU-15): with a baseline under 0.2 s and --timeout-factor 10, the timeout is the 2 s floor.
// A 5 s mutant is killed inside the 2 s + 4 s runs. A 1 s mutant survives, which a 0.1 s
// timeout would have killed.
#[test]
fn mu_15_timeout_floor_is_two_seconds() {
    let command = "if grep -q 'a <= b' src/m.py; then sleep 5; fi; \
                   if grep -q 'a == 0' src/m.py; then sleep 1; fi; exit 0";
    let started = Instant::now();
    let (_dir, out) = mutate(command, &["--timeout-factor", "10"]);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(14), "run took {elapsed:?}");
    assert!(
        logged(&out.stdout, "KILLED", "< -> <="),
        "stdout:\n{}",
        out.stdout
    );
    assert!(
        logged(&out.stdout, "SURVIVED", "!= -> =="),
        "stdout:\n{}",
        out.stdout
    );
}
