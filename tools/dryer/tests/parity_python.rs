// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Runs the Python unit tests of `parity/` with `python3.12` (DR-NFR-2, AC-10).
//!
//! `tests/parity_expected.rs` replays the recorded results and needs no Python, which
//! keeps AC-8 true for a host with no Python. The Python tests of `run_side` then need a
//! check of their own, so this file runs them. A host with no `python3.12` skips them with
//! a printed note. The guest scripts install python3.12, so they run the tests.

use std::io;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// A hanging Python test must not block `cargo test`.
const TIMEOUT: Duration = Duration::from_secs(120);

/// The end of a `unittest` run: the status code and the output. `status` is `None` when
/// the deadline killed the run.
struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The count in the `unittest` summary line (`Ran 8 tests in 0.030s`), or `None` when the
/// output has no such line. The count tells a discovery that ran nothing from one that ran
/// the suite.
fn ran_tests(stderr: &str) -> Option<usize> {
    let line = stderr.lines().find(|l| l.starts_with("Ran "))?;
    line["Ran ".len()..].split_whitespace().next()?.parse().ok()
}

/// Reads a pipe to its end in its own thread, so a full pipe cannot block the child.
fn drain(pipe: Option<impl io::Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

/// Runs `command` for at most `TIMEOUT`. A longer run is killed and reports no status.
fn run(command: &mut Command) -> Run {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning the parity tests");
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait().expect("polling the parity tests") {
            Some(status) => break status.code(),
            None if started.elapsed() >= TIMEOUT => {
                child.kill().expect("killing the parity tests");
                child.wait().expect("reaping the parity tests");
                break None;
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    };
    Run {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    }
}

/// DR-NFR-2, AC-10: the Python tests of `run_side` pass, and the run executes at least one
/// test. An empty run would hide a test file that moved or broke. A host with no
/// `python3.12` skips with a note, so AC-8 still passes there.
#[test]
fn dr_nfr_2_ac_10_the_python_tests_of_the_parity_runner_pass() {
    let present = Command::new("python3.12")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match present {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!(
                "note: skipped, because python3.12 is not on this host. \
                 The guest scripts install it and run the tests there."
            );
            return;
        }
        Err(err) => panic!("running python3.12 -V: {err}"),
    }
    let mut command = Command::new("python3.12");
    command
        .args(["-m", "unittest", "discover", "-s", ".", "-p", "test_*.py"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PYTHONDONTWRITEBYTECODE", "1");
    let run = run(&mut command);
    let Some(status) = run.status else {
        panic!(
            "python3.12 -m unittest discover ran past {} s and was killed\nstdout:\n{}\nstderr:\n{}",
            TIMEOUT.as_secs(),
            run.stdout,
            run.stderr
        );
    };
    assert!(
        ran_tests(&run.stderr).is_some_and(|n| n > 0),
        "discovery ran no test\nstdout:\n{}\nstderr:\n{}",
        run.stdout,
        run.stderr
    );
    assert_eq!(
        status, 0,
        "python3.12 -m unittest discover failed\nstdout:\n{}\nstderr:\n{}",
        run.stdout, run.stderr
    );
}

/// DR-NFR-2, AC-10: the check for an empty run is not vacuous. It reads the summary line of
/// `unittest` and reports no test for a run that found nothing.
#[test]
fn dr_nfr_2_ac_10_a_run_that_found_no_test_is_detected() {
    assert_eq!(ran_tests("Ran 8 tests in 0.030s\n\nOK\n"), Some(8));
    assert_eq!(
        ran_tests("Ran 0 tests in 0.000s\n\nNO TESTS RAN\n"),
        Some(0)
    );
    assert_eq!(ran_tests("error: no module named parity\n"), None);
}
