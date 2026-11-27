// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
mod common;

use common::{project, run, write};
use std::fs;

const SRC: &str = "def f(a):\n    return a > 1 and a < 9\n";

/// MU-29 / AC-23: a worker holds real copies of the HTML files of the project. A test command
/// that fails when `index.html` or `pages/page.html` is a link passes in every worker, so every
/// mutant survives. The run exits 3 and prints no baseline failure.
#[test]
fn mu_29_ac_23_a_worker_holds_real_copies_of_html_files() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    write(dir.path(), "index.html", "<html></html>\n");
    write(dir.path(), "pages/page.html", "<p>page</p>\n");
    write(dir.path(), "pages/notes.txt", "notes\n");
    let scratch = tempfile::tempdir().expect("temp dir");
    let script = scratch.path().join("links.sh");
    fs::write(
        &script,
        "#!/bin/sh\ncase \"$(basename \"$PWD\")\" in worker-*) if test -L index.html || test -L pages/page.html; then exit 1; fi ;; esac\nexit 0\n",
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
        out.code, 3,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        !out.stderr.contains("Baseline failed"),
        "stderr: {}",
        out.stderr
    );
}
