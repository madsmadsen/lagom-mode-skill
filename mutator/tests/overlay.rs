mod common;

use common::*;
use std::fs;
use std::path::Path;
use std::process::Command;

const MUTATED_SRC: &str = "def ok(a, b):\n    return a >= b\n";
const GATE: &str = "grep -q 'a >= b' src/m.py";

const BACKUP: &str = "def ok(a, b):\n    return a >= b\n# restored\n";
const BACKUP_GATE: &str = "grep -q '# restored' src/m.py && grep -q 'a >= b' src/m.py";

/// Names of the `run-*` worker directories left under `target/mutation-workers`.
fn leftover_run_dirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir.join("target/mutation-workers")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("run-"))
        .collect()
}

fn git_status(dir: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain"])
        .output()
        .expect("git runs");
    String::from_utf8(out.stdout).expect("utf-8 status")
}

/// A project whose source is mutated and whose backup holds the restored text.
fn interrupted_project() -> tempfile::TempDir {
    let dir = project();
    write(dir.path(), "src/m.py", "def ok(a, b):\n    return a > b\n");
    write(dir.path(), "target/mutator-backup/src/m.py", BACKUP);
    dir
}

// AC-5 / MU-17: a mutating run with two workers leaves git status empty and no worker dirs behind.
#[test]
fn mu_17_ac5_mutating_run_leaves_git_status_empty_and_no_worker_dirs() {
    let dir = project();
    write(dir.path(), ".gitignore", "target/\n.metrics/\n");
    write(dir.path(), "src/m.py", MUTATED_SRC);
    git_commit_all(dir.path());
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--max-workers",
            "2",
            "--test-command",
            GATE,
        ],
    );
    assert!(
        out.code == 0 || out.code == 3,
        "exit {}\n{}{}",
        out.code,
        out.stdout,
        out.stderr
    );
    assert_eq!(git_status(dir.path()), "", "a source file changed");
    assert_eq!(leftover_run_dirs(dir.path()), Vec::<String>::new());
}

// AC-6 / MU-18, MU-NFR-5: a non-scan run restores the backup into src/m.py before any mutant runs.
#[test]
fn mu_18_ac6_non_scan_run_restores_backup_before_mutants() {
    let dir = interrupted_project();
    let out = run(
        dir.path(),
        &["--no-coverage", "--test-command", BACKUP_GATE],
    );
    assert!(
        out.code == 0 || out.code == 3,
        "exit {}\n{}{}",
        out.code,
        out.stdout,
        out.stderr
    );
    assert!(out.stderr.contains("Restored"), "{}", out.stderr);
    assert!(
        out.stderr.contains("from an interrupted mutation"),
        "{}",
        out.stderr
    );
    assert_eq!(read(dir.path(), "src/m.py"), BACKUP);
    assert!(!dir.path().join("target/mutator-backup/src/m.py").exists());
}

// AC-6 / MU-18: a scan run neither restores the backup nor deletes it.
#[test]
fn mu_18_ac6_scan_run_keeps_source_and_backup_as_they_are() {
    let dir = interrupted_project();
    let out = run(dir.path(), &["--scan", "--no-coverage"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        read(dir.path(), "src/m.py"),
        "def ok(a, b):\n    return a > b\n"
    );
    assert_eq!(read(dir.path(), "target/mutator-backup/src/m.py"), BACKUP);
}

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).expect("file reads")
}
