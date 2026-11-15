// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: none

//! One test for each entry of `parity/fixes.toml` (CR-PB-4, AC-12).
//!
//! Each fix has a run `fix-<id>` in `parity/coverage.toml`. The original at the
//! pinned commit recorded its output for that run in `parity/recorded/`. A test
//! states one property of the fixed behavior. The property is false for the
//! recorded output of the original and true for the output of the port. The
//! tests need no Python.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Outcome {
    code: i32,
    stdout: String,
}

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("parity")
}

fn run_table(fix: &str) -> toml::Value {
    let source =
        fs::read_to_string(parity_dir().join("coverage.toml")).expect("read coverage.toml");
    let table: toml::Table = toml::from_str(&source).expect("parse coverage.toml");
    let run_id = format!("fix-{fix}");
    table["run"]
        .as_array()
        .expect("coverage.toml has runs")
        .iter()
        .find(|run| run["id"].as_str() == Some(&run_id))
        .unwrap_or_else(|| panic!("{run_id} is not in coverage.toml"))
        .clone()
}

fn strings(run: &toml::Value, key: &str) -> Vec<String> {
    run.get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("dir entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

fn setup(commands: &[String], project: &Path) {
    for command in commands {
        let status = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(project)
            .env("GIT_AUTHOR_NAME", "parity")
            .env("GIT_AUTHOR_EMAIL", "parity@example.invalid")
            .env("GIT_COMMITTER_NAME", "parity")
            .env("GIT_COMMITTER_EMAIL", "parity@example.invalid")
            .status()
            .expect("setup starts");
        assert!(status.success(), "setup `{command}` failed");
    }
}

fn stubbed_path(project: &Path) -> OsString {
    let mut paths = vec![project.join(".stubs")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).expect("PATH entries have no separator")
}

fn produced(fix: &str) -> Outcome {
    let run = run_table(fix);
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("project");
    let fixture = run["fixture"].as_str().expect("run has a fixture");
    copy_dir(&parity_dir().join("fixtures").join(fixture), &project);
    setup(&strings(&run, "setup"), &project);
    let mut command = Command::new(env!("CARGO_BIN_EXE_crapper"));
    command
        .args(strings(&run, "args"))
        .current_dir(&project)
        .env("PATH", stubbed_path(&project));
    for entry in strings(&run, "env") {
        let (name, value) = entry.split_once('=').expect("env entry is NAME=VALUE");
        command.env(name, value.replace("{project}", &project.to_string_lossy()));
    }
    let done = command.output().expect("crapper runs");
    Outcome {
        code: done.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&done.stdout).into_owned(),
    }
}

fn recorded(fix: &str) -> Outcome {
    let dir = parity_dir().join("recorded").join(format!("fix-{fix}"));
    let code = fs::read_to_string(dir.join("exit.txt")).expect("recorded exit.txt");
    Outcome {
        code: code.trim().parse().expect("exit.txt holds a number"),
        stdout: fs::read_to_string(dir.join("stdout.txt")).expect("recorded stdout.txt"),
    }
}

/// `holds` is false for the recorded original and true for the port.
fn check(fix: &str, holds: impl Fn(&Outcome) -> bool) {
    assert!(
        !holds(&recorded(fix)),
        "{fix}: the property holds for the recorded original"
    );
    let port = produced(fix);
    assert!(
        holds(&port),
        "{fix}: the property fails for the port (exit {}):\n{}",
        port.code,
        port.stdout
    );
}

fn row_of<'a>(outcome: &'a Outcome, name: &str) -> Option<&'a str> {
    outcome
        .stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some(name))
}

fn has_coverage(outcome: &Outcome, name: &str, coverage: &str) -> bool {
    outcome.code == 0
        && row_of(outcome, name).is_some_and(|row| row.split_whitespace().any(|c| c == coverage))
}

// fix: vitest-package-manager (CR-18, AC-27). On the pnpm fixture, `big` has coverage 100.0.
#[test]
fn fix_vitest_package_manager_installs_with_pnpm_on_a_pnpm_project() {
    check("vitest-package-manager", |out| {
        has_coverage(out, "big", "100.0%")
    });
}

// fix: vitest-npm-lockfile (CR-19, AC-28). npm keeps the locked semver, so the stub run covers `big` fully.
#[test]
fn fix_vitest_npm_lockfile_keeps_the_locked_dependency_version() {
    check("vitest-npm-lockfile", |out| {
        has_coverage(out, "big", "100.0%")
    });
}

// fix: vitest-untrusted-install (CR-23, AC-32). A file: version is not pinned and no install script runs.
#[test]
fn fix_vitest_untrusted_install_ignores_a_bad_version_and_install_scripts() {
    check("vitest-untrusted-install", |out| {
        has_coverage(out, "big", "100.0%")
    });
}

// fix: git-env (CR-20, AC-29). With GIT_DIR set to another repository, --changed still reads the project.
#[test]
fn fix_git_env_changed_ignores_git_variables_of_another_repository() {
    check("git-env", |out| {
        out.code == 0 && row_of(out, "staged_extra").is_some() && row_of(out, "unchanged").is_none()
    });
}

// fix: ts-shapes (CR-21, AC-30). The file of issue 20 gives seven entries.
#[test]
fn fix_ts_shapes_gives_seven_entries_for_the_file_of_issue_20() {
    check("ts-shapes", |out| {
        [
            "plain",
            "viaVar",
            "objectMethod",
            "method",
            "field",
            "legacy",
            "default",
        ]
        .iter()
        .all(|name| row_of(out, name).is_some())
    });
}

// fix: python-routes (CR-22, AC-31). Route handlers in a factory are entries, and the factory does not count them.
#[test]
fn fix_python_routes_makes_route_handlers_their_own_entries() {
    check("python-routes", |out| {
        let factory = row_of(out, "build_router").unwrap_or_default();
        out.stdout.contains("GET /users/{user_id}")
            && out.stdout.contains("POST /users")
            && out.stdout.contains("GET|POST /sync")
            && out.stdout.contains("GET /users#2")
            && !out.stdout.contains("patched")
            && factory.split_whitespace().any(|c| c == "3")
    });
}

// fix: jacoco-doctype-subset (CR-24, AC-33). A DOCTYPE with an internal subset gives 75.0 for place.
#[test]
fn fix_jacoco_doctype_subset_reads_a_report_with_an_internal_subset() {
    check("jacoco-doctype-subset", |out| {
        has_coverage(out, "place", "75.0%")
    });
}

// fix: rust-test-code (CR-25, AC-34). Functions in a cfg(test) module and functions with #[test] or #[cfg(test)] are test code.
#[test]
fn fix_rust_test_code_leaves_out_cfg_test_modules_and_test_functions() {
    check("rust-test-code", |out| {
        let reported = |name| row_of(out, name).is_some();
        out.code == 0
            && reported("keep")
            && reported("prod_only")
            && !["unit_check", "unit_helper", "plain_check", "test_helper"]
                .into_iter()
                .any(reported)
    });
}
