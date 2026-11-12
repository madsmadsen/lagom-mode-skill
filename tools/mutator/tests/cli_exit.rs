// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
mod common;

use common::{project, run, snapshots, write};

const GE_SOURCE: &str = "def ge(a, b): return a >= b\n";

/// A project with one Python file that has a single `a >= b` comparison.
fn ge_project() -> tempfile::TempDir {
    let dir = project();
    write(dir.path(), "src/m.py", GE_SOURCE);
    dir
}

/// MU-1: the binary runs, and `--help` prints the usage on stdout with exit 0.
#[test]
fn mu_1_help_prints_usage_on_stdout_and_exits_0() {
    let dir = project();
    let out = run(dir.path(), &["--help"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("Usage: mutator"),
        "stdout: {}",
        out.stdout
    );
    assert!(out.stderr.is_empty(), "stderr: {}", out.stderr);
}

/// AC-10 / MU-2: every invocation the original rejects exits 1, names the problem on
/// stderr, prints the help text, and writes nothing to stdout.
#[test]
fn ac_10_rejected_invocations_exit_1_with_message_and_help() {
    let cases: &[(&[&str], &str)] = &[
        (&["--bogus"], "Unknown option: --bogus"),
        (&["--timeout-factor"], "--timeout-factor requires a value"),
        (
            &["--timeout-factor", "0"],
            "--timeout-factor requires a positive number",
        ),
        (
            &["--timeout-factor", "abc"],
            "could not convert string to float",
        ),
        (
            &["--max-workers", "0"],
            "--max-workers requires a positive integer",
        ),
        (&["--lines", "0"], "--lines requires positive line numbers"),
        (
            &["--mutate-all", "--since-last-run"],
            "--mutate-all cannot be combined with --since-last-run",
        ),
        (
            &["--no-coverage", "--coverage-command", "x"],
            "--no-coverage cannot be combined with --coverage-command",
        ),
        (
            &["--scan", "--mutate-all"],
            "--scan cannot be combined with --mutate-all",
        ),
        (
            &["--scan", "--max-workers", "2"],
            "--scan cannot be combined with --max-workers",
        ),
    ];
    let dir = project();
    for (args, message) in cases {
        let out = run(dir.path(), args);
        assert_eq!(out.code, 1, "args {args:?}: stderr {}", out.stderr);
        assert!(
            out.stderr.contains(message),
            "args {args:?}: stderr {}",
            out.stderr
        );
        assert!(
            out.stderr.contains("Usage: mutator"),
            "args {args:?}: stderr {}",
            out.stderr
        );
        assert!(
            out.stdout.is_empty(),
            "args {args:?}: stdout {}",
            out.stdout
        );
    }
}

/// MU-2: accepted flags and aliases parse; a scan run with each one exits 0.
#[test]
fn mu_2_accepted_flags_and_aliases_parse() {
    let dir = project();
    write(
        dir.path(),
        "src/app.py",
        "def f(a, b):\n    return a >= b\n",
    );
    let root = dir.path().to_str().expect("utf-8 path");
    let cases: &[&[&str]] = &[
        &["--scan", "--no-coverage", "--reuse-coverage"],
        &["--scan", "--no-coverage", "--use-existing-coverage"],
        &["--scan", "--no-coverage", "--root", root],
        &["--scan", "--no-coverage", "-s", "src"],
    ];
    for args in cases {
        let out = run(dir.path(), args);
        assert_eq!(out.code, 0, "args {args:?}: stderr {}", out.stderr);
        assert!(
            !out.stderr.contains("Usage: mutator"),
            "args {args:?}: stderr {}",
            out.stderr
        );
    }
}

/// MU-20: exit 0 when every executed mutant is killed.
#[test]
fn mu_20_exits_0_when_every_mutant_is_killed() {
    let dir = ge_project();
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--test-command",
            "grep -q 'a >= b' src/m.py",
        ],
    );
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
}

/// MU-3 / MU-20: exit 3 when a mutant survives.
#[test]
fn mu_3_exits_3_when_a_mutant_survives() {
    let dir = ge_project();
    let out = run(dir.path(), &["--no-coverage", "--test-command", "true"]);
    assert_eq!(
        out.code, 3,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
}

/// MU-3 / MU-20: exit 2 when the baseline test command fails.
#[test]
fn mu_3_exits_2_when_baseline_fails() {
    let dir = ge_project();
    let out = run(dir.path(), &["--no-coverage", "--test-command", "false"]);
    assert_eq!(
        out.code, 2,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
}

/// AC-3 / MU-14: a failing baseline exits 2 and leaves every snapshot byte-equal to its
/// previous content.
#[test]
fn mu_14_failed_baseline_exits_2_and_keeps_snapshots() {
    let dir = ge_project();
    let first = run(dir.path(), &["--no-coverage", "--test-command", "true"]);
    assert_eq!(
        first.code, 3,
        "stdout: {}\nstderr: {}",
        first.stdout, first.stderr
    );
    let before = snapshots(dir.path());
    assert!(!before.is_empty(), "first run wrote no snapshot");
    let second = run(
        dir.path(),
        &["--no-coverage", "--mutate-all", "--test-command", "false"],
    );
    assert_eq!(
        second.code, 2,
        "stdout: {}\nstderr: {}",
        second.stdout, second.stderr
    );
    assert_eq!(snapshots(dir.path()), before);
}

/// MU-3: an empty project prints the no-source message on stdout and exits 0.
#[test]
fn mu_3_empty_project_prints_no_source_message_and_exits_0() {
    let dir = project();
    let out = run(dir.path(), &["--no-coverage"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("No source files to mutate."),
        "stdout: {}",
        out.stdout
    );
}

/// MU-5: --scan detects .py, .rs, .go, .java, .ts, and .clj files, and ignores .txt.
#[test]
fn mu_5_scan_detects_six_languages_and_ignores_txt() {
    let dir = project();
    let files = [
        ("src/tiny.py", "def f(a, b):\n    return a >= b\n"),
        (
            "src/tiny.rs",
            "fn f(a: i32, b: i32) -> bool {\n    a >= b\n}\n",
        ),
        (
            "src/tiny.go",
            "package main\n\nfunc f(a, b int) bool {\n\treturn a >= b\n}\n",
        ),
        (
            "src/Tiny.java",
            "class Tiny {\n    boolean f(int a, int b) {\n        return a >= b;\n    }\n}\n",
        ),
        (
            "src/tiny.ts",
            "export function f(a: number, b: number): boolean {\n  return a >= b;\n}\n",
        ),
        ("src/tiny.clj", "(defn f [a b] (>= a b))\n"),
        ("src/notes.txt", "a >= b\n"),
    ];
    for (rel, text) in files {
        write(dir.path(), rel, text);
    }
    let out = run(dir.path(), &["--scan", "--no-coverage"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    for name in [
        "tiny.py",
        "tiny.rs",
        "tiny.go",
        "Tiny.java",
        "tiny.ts",
        "tiny.clj",
    ] {
        assert!(
            out.stdout.contains(name),
            "{name} not scanned: {}",
            out.stdout
        );
    }
    assert!(
        !out.stdout.contains("notes.txt"),
        "txt scanned: {}",
        out.stdout
    );
}

/// MU-6: --scan skips files under test, spec, vendor, node_modules, and target.
#[test]
fn mu_6_scan_skips_test_spec_vendor_node_modules_and_target() {
    let dir = project();
    write(
        dir.path(),
        "src/app.py",
        "def f(a, b):\n    return a >= b\n",
    );
    for skipped in ["test", "spec", "vendor", "node_modules", "target"] {
        write(
            dir.path(),
            &format!("{skipped}/hidden_{skipped}.py"),
            "def f(a, b):\n    return a >= b\n",
        );
    }
    let out = run(dir.path(), &["--scan", "--no-coverage"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("app.py"),
        "control file not scanned: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("hidden_"),
        "skipped directory scanned: {}",
        out.stdout
    );
}

/// MU-4: a run that kills a mutant prints the summary table on stdout, with a Killed column and a Total row.
#[test]
fn mu_4_run_prints_summary_table_on_stdout() {
    let dir = ge_project();
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--test-command",
            "grep -q 'a >= b' src/m.py",
        ],
    );
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(out.stdout.contains("Killed"), "stdout: {}", out.stdout);
    assert!(out.stdout.contains("Total"), "stdout: {}", out.stdout);
}

/// MU-4: a run that mutates no form prints no summary table and no "No functions" line.
#[test]
fn mu_4_run_without_forms_prints_no_summary_table() {
    let dir = project();
    write(dir.path(), "src/m.py", "x = 'a'\n");
    let out = run(dir.path(), &["--no-coverage", "--test-command", "true"]);
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(!out.stdout.contains("Total"), "stdout: {}", out.stdout);
    assert!(
        !out.stdout.contains("No functions to mutate"),
        "stdout: {}",
        out.stdout
    );
}
