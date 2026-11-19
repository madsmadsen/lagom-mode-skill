// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Checks `parity/coverage.toml` against the CLI help (AC-1) and the README table
//! rows, one per language (MU-12).

use std::fs;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

/// Seconds that the Python parity tests may run. A test that starts a subprocess could
/// otherwise block `cargo test` for ever.
const PARITY_TIMEOUT_SECS: u64 = 120;

/// The six languages named in the README table.
const LANGUAGES: [&str; 6] = ["clojure", "java", "go", "typescript", "rust", "python"];

/// The text of a parity registry file: `coverage.toml` holds the runs, and `fixes.toml`
/// names every difference from the original (MU-PB-3).
fn parity_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("parity")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Names of the `[[run]]` blocks in a coverage.toml text that set
/// `allow_stdout_diff = true`. A run must not skip the stdout check (MU-PB-6).
fn suppressing_runs(coverage: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in coverage.lines() {
        let line = line.trim();
        if line == "[[run]]" {
            current = Some(String::new());
        } else if let Some(name) = current.as_mut() {
            if let Some(rest) = line.strip_prefix("name = \"") {
                *name = rest.trim_end_matches('"').to_string();
            } else if line == "allow_stdout_diff = true" {
                if name.is_empty() {
                    *name = "?".to_string();
                }
                names.push(name.clone());
            }
        }
    }
    names
}

/// MU-PB-6: no run of coverage.toml skips the stdout check. A stdout difference needs
/// an approved fix, so a run that allows it on its own would hide the difference.
#[test]
fn mu_pb_6_no_run_skips_the_stdout_check() {
    let coverage = parity_file("coverage.toml");
    assert_eq!(suppressing_runs(&coverage), Vec::<String>::new());

    // The check above is not vacuous: the helper flags a run that sets the field.
    let sample = "[[run]]\nname = \"sample\"\nallow_stdout_diff = true\n";
    assert_eq!(suppressing_runs(sample), vec!["sample".to_string()]);
    let clean = "[[run]]\nname = \"sample\"\nargs = [\"-h\"]\n";
    assert_eq!(suppressing_runs(clean), Vec::<String>::new());
}

/// The Python that can run `parity/test_status.py`: it imports `tomllib`, so it needs
/// Python 3.11 or later. `python3.12` comes first, because `python3` is 3.9 on some hosts.
fn parity_python() -> Option<&'static str> {
    ["python3.12", "python3"].into_iter().find(|name| {
        Command::new(name)
            .args(["-c", "import tomllib"])
            .output()
            .is_ok_and(|out| out.status.success())
    })
}

/// A parity run: how it ended, and what it printed.
struct ParityRun {
    /// `None` when the run was killed at `PARITY_TIMEOUT_SECS`.
    status: Option<ExitStatus>,
    stdout: String,
    stderr: String,
}

/// Reads a pipe to its end in its own thread, so that a full pipe cannot block the child.
fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

/// Kills the process group of `child`, which was spawned with `process_group(0)`, and reaps it.
fn kill_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `pid` is the child's pid and the child leads its own process group, so
        // `-pid` names that group. The child is not reaped yet, so the id cannot be reused.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    let _ = child.wait();
}

/// Runs `command` for at most `limit`. A run that takes longer is killed, and its status is
/// `None`.
fn run_with_deadline(command: &mut Command, limit: Duration) -> ParityRun {
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .expect("running the parity tests");
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait().expect("polling the parity tests") {
            Some(status) => break Some(status),
            None if started.elapsed() >= limit => {
                kill_group(&mut child);
                break None;
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    };
    ParityRun {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    }
}

/// MU-PB-6, AC-1: a parity run that takes longer than its deadline is killed, so that a
/// hanging Python test cannot block `cargo test`.
#[test]
fn mu_pb_6_a_parity_run_that_takes_too_long_is_killed() {
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 30"]);
    let started = Instant::now();
    let run = run_with_deadline(&mut command, Duration::from_secs(1));
    assert!(run.status.is_none(), "the killed run reported a status");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the deadline did not end the run: {:?}",
        started.elapsed()
    );
}

/// MU-PB-6, AC-1: the Python tests of the parity runner pass. No other automatic check
/// runs them, so a broken `status_for` would pass every other check. The test skips when
/// no interpreter has `tomllib`, and it fails when the run takes longer than
/// `PARITY_TIMEOUT_SECS`.
#[test]
fn mu_pb_6_ac_1_the_parity_status_tests_pass() {
    let Some(python) = parity_python() else {
        println!(
            "note: skipped, because neither python3.12 nor python3 imports tomllib. \
             Run `python3.12 -m unittest discover -s . -p 'test_*.py'` in tools/mutator."
        );
        return;
    };
    let mut command = Command::new(python);
    command
        .args(["-m", "unittest", "discover", "-s", ".", "-p", "test_*.py"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PYTHONDONTWRITEBYTECODE", "1");
    let run = run_with_deadline(&mut command, Duration::from_secs(PARITY_TIMEOUT_SECS));
    let Some(status) = run.status else {
        panic!(
            "{python} -m unittest discover ran past {PARITY_TIMEOUT_SECS} s and was killed\n\
             stdout:\n{}\nstderr:\n{}",
            run.stdout, run.stderr
        );
    };
    assert!(
        status.success(),
        "{python} -m unittest discover exited {:?}\nstdout:\n{}\nstderr:\n{}",
        status.code(),
        run.stdout,
        run.stderr
    );
}

/// The flags in the Options block of `--help`: `-h`, `-s`, and every `--name`.
fn help_flags() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_mutator"))
        .arg("--help")
        .output()
        .expect("running mutator --help");
    assert!(out.status.success(), "mutator --help exited {}", out.status);
    let help = String::from_utf8(out.stdout).expect("help is UTF-8");
    let mut flags = Vec::new();
    let mut in_options = false;
    for line in help.lines() {
        if line == "Options:" {
            in_options = true;
        } else if line.is_empty() {
            in_options = false;
        } else if in_options && line.starts_with("  -") {
            // The flag column ends at the first run of two spaces.
            let spec = line[2..].split("  ").next().unwrap_or("");
            flags.extend(
                spec.split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|t| t.starts_with("--") || *t == "-h" || *t == "-s")
                    .map(str::to_string),
            );
        }
    }
    flags
}

/// Flags that the original does not have. Each one has a fix in fixes.toml (MU-PB-3), and a
/// parity run cannot pass on it, so coverage.toml does not run it.
const PORT_ONLY_FLAGS: [(&str, &str); 1] = [("--max-memory-mb", "max-memory-limit")];

/// AC-1 and MU-12: coverage.toml mentions every help flag and has a covers string for each language.
/// MU-25: a flag that only the port has is named by a fix in fixes.toml instead.
#[test]
fn mu_ac_1_coverage_toml_covers_every_flag_and_table_row() {
    let coverage = parity_file("coverage.toml");
    let fixes = parity_file("fixes.toml");
    let flags = help_flags();
    for (flag, id) in PORT_ONLY_FLAGS {
        assert!(flags.iter().any(|f| f == flag), "help lists no {flag}");
        let entry = format!("id = \"{id}\"");
        assert!(
            fixes.lines().any(|l| l.trim() == entry),
            "parity/fixes.toml has no {entry} for {flag}"
        );
    }
    let flags: Vec<String> = flags
        .into_iter()
        .filter(|f| PORT_ONLY_FLAGS.iter().all(|(flag, _)| flag != f))
        .collect();
    assert!(
        flags.iter().any(|f| f == "--help"),
        "help lists no --help: {flags:?}"
    );
    for flag in &flags {
        assert!(
            coverage.contains(flag.as_str()),
            "parity/coverage.toml does not cover {flag}"
        );
    }
    for language in LANGUAGES {
        let prefix = format!("\"{language}:");
        assert!(
            coverage.contains(&prefix),
            "parity/coverage.toml has no covers string for {language}"
        );
    }
}
