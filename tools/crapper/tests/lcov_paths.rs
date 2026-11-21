// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: none

//! Black-box checks of the coverage reports that `--use-existing-coverage`
//! reads (CR-12, AC-16). crapper reads `lcov.info` only below `coverage/` and
//! `target/coverage/` of the project root, at any depth. A report at another
//! path, for example the root, gives each function 0% coverage and no warning.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const CHOOSE_SOURCE: &str = "def choose(x):\n    if x > 0:\n        return 1\n    elif x < 0:\n        return -1\n    return 0\n";

fn write_text(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("file has parent")).expect("create dirs");
    fs::write(path, text).expect("write file");
}

fn fully_hit_record(path: &str) -> String {
    let mut record = format!("SF:{path}\n");
    for line in 1..=6 {
        record.push_str(&format!("DA:{line},1\n"));
    }
    record.push_str("end_of_record\n");
    record
}

fn crapper_in(root: &Path, args: &[&str]) -> Output {
    let root = root.to_str().expect("utf-8 temp path");
    let mut full = vec!["--root", root];
    full.extend_from_slice(args);
    Command::new(env!("CARGO_BIN_EXE_crapper"))
        .args(&full)
        .output()
        .expect("crapper binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The stderr lines that are not the snapshot notice, which every successful run prints.
fn warnings(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| !line.starts_with("Wrote "))
        .map(str::to_string)
        .collect()
}

/// The `Cov%` cell of the row of `name`, as its own token, so "100.0%" does not match "0.0%".
fn coverage_cell(output: &Output, name: &str) -> String {
    let report = stdout(output);
    let row = report
        .lines()
        .find(|line| line.starts_with(name))
        .unwrap_or_else(|| panic!("report has no row for {name}:\n{report}"));
    row.split_whitespace()
        .nth(3)
        .unwrap_or_else(|| panic!("row of {name} has no coverage cell: {row}"))
        .to_string()
}

/// A project with one Python source file and one LCOV report at `relative`.
/// The report says that every line of the function ran.
fn project_with_report(relative: &str) -> (tempfile::TempDir, Output) {
    let temp = tempfile::tempdir().expect("tempdir");
    write_text(temp.path(), "src/app.py", CHOOSE_SOURCE);
    write_text(temp.path(), relative, &fully_hit_record("src/app.py"));
    let output = crapper_in(temp.path(), &["--use-existing-coverage"]);
    (temp, output)
}

// CR-12, AC-16
#[test]
fn cr_12_lcov_is_read_only_below_coverage_and_target_coverage() {
    let (_root_dir, root_output) = project_with_report("lcov.info");
    let root_cell = coverage_cell(&root_output, "choose");
    assert_eq!(
        warnings(&root_output),
        Vec::<String>::new(),
        "CR-12 case (a), report at the project root: stderr must hold no warning"
    );
    assert_eq!(
        root_cell, "0.0%",
        "CR-12 case (a), a report at the project root is not read, so coverage is 0.0% and not the report value"
    );

    let (_coverage_dir, coverage_output) = project_with_report("coverage/lcov.info");
    let coverage_value = coverage_cell(&coverage_output, "choose");
    assert_eq!(
        coverage_value, "100.0%",
        "CR-12 case (b), a report below coverage/ is read, so coverage is 100.0%"
    );

    let (_target_dir, target_output) = project_with_report("target/coverage/lcov.info");
    let target_value = coverage_cell(&target_output, "choose");
    assert_eq!(
        target_value, "100.0%",
        "CR-12 case (c), a report below target/coverage/ is read, so coverage is 100.0%"
    );

    let (_nested_dir, nested_output) = project_with_report("target/coverage/rust/src-x/lcov.info");
    let nested_value = coverage_cell(&nested_output, "choose");
    assert_eq!(
        nested_value, "100.0%",
        "CR-12 case (d), a nested report below target/coverage/rust/src-x/ is read, so coverage is 100.0%"
    );
}

// CR-12, AC-16
#[test]
fn cr_12_no_report_at_all_gives_zero_coverage_and_no_warning() {
    let temp = tempfile::tempdir().expect("tempdir");
    write_text(temp.path(), "src/app.py", CHOOSE_SOURCE);
    let output = crapper_in(temp.path(), &["--use-existing-coverage"]);
    assert_eq!(
        warnings(&output),
        Vec::<String>::new(),
        "CR-12 no report: stderr must hold no warning"
    );
    assert_eq!(
        coverage_cell(&output, "choose"),
        "0.0%",
        "CR-12 no report: coverage is 0.0%"
    );
}
