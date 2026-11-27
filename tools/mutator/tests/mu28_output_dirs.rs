// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
mod common;

use common::{project, run, write};
use std::fs;

const SRC: &str = "def f(a):\n    return a > 1 and a < 9\n";

/// MU-28 / AC-22: a test command that writes `test-results` in a worker must not write into the
/// project. The worker does not link the output directory of the project.
#[test]
fn mu_28_ac_22_a_worker_does_not_write_test_results_into_the_project() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    write(dir.path(), "test-results/marker.txt", "keep");
    write(dir.path(), "src/test-results/marker.txt", "keep");
    let scratch = tempfile::tempdir().expect("temp dir");
    let script = scratch.path().join("report.sh");
    fs::write(
        &script,
        "#!/bin/sh\ncase \"$(basename \"$PWD\")\" in worker-*) mkdir -p test-results && echo x > test-results/new.txt ;; esac\nexit 0\n",
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
    assert!(
        !out.stderr.contains("panicked"),
        "stdout: {}\nstderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(
        !dir.path().join("test-results/new.txt").exists(),
        "a worker wrote into the project\nstdout: {}\nstderr: {}",
        out.stdout,
        out.stderr
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("test-results/marker.txt")).expect("read marker"),
        "keep"
    );
}
