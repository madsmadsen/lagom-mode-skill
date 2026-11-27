// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
mod common;

use common::{project, run, snapshots, write};
use std::fs;

const SRC: &str = "def f(a):\n    return a > 1 and a < 9\n";

/// MU-27 / AC-21: a command that fails only inside a worker directory makes the file a
/// baseline failure. The run exits 2, names the lowest failing worker, and writes no
/// snapshot for the file.
#[test]
fn mu_27_ac_21_a_command_that_fails_only_in_a_worker_gives_exit_2() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    let scratch = tempfile::tempdir().expect("temp dir");
    let script = scratch.path().join("check.sh");
    fs::write(
        &script,
        "#!/bin/sh\ncase \"$(basename \"$PWD\")\" in worker-*) exit 1;; esac\nexit 0\n",
    )
    .expect("write script");
    let cmd = format!("sh {}", script.display());
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--max-workers",
            "2",
            "--test-command",
            &cmd,
        ],
    );
    assert_eq!(
        out.code, 2,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stderr
            .contains("Baseline failed for src/m.py in worker 0:"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.lines().any(|line| line == "exit code 1"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        snapshots(dir.path()).is_empty(),
        "a snapshot was written for the failed file: {:?}",
        snapshots(dir.path()).keys().collect::<Vec<_>>()
    );
}

/// MU-27 / AC-21: a command that passes alone but fails when two workers run at once
/// makes the file a baseline failure. The message tells the user to run with one worker.
#[test]
fn mu_27_ac_21_a_clash_between_workers_says_to_use_one_worker() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    let scratch = tempfile::tempdir().expect("temp dir");
    let lock = scratch.path().join("lock");
    let script = scratch.path().join("lock.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nmkdir '{lock}' 2>/dev/null || exit 1\nsleep 1\nrmdir '{lock}'\n",
            lock = lock.display()
        ),
    )
    .expect("write script");
    let cmd = format!("sh {}", script.display());
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--max-workers",
            "2",
            "--test-command",
            &cmd,
        ],
    );
    assert_eq!(
        out.code, 2,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stderr
            .contains("Baseline failed for src/m.py in worker "),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr
            .lines()
            .any(|line| line == "The command passes alone. Run with --max-workers 1."),
        "stderr: {}",
        out.stderr
    );
}
