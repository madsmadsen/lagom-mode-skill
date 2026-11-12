// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: tests/test_cli.py, tests/test_edges.py

//! Black-box tests of the `crapper` binary's command-line behavior.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::{Command, Output};

const CHOOSE_SOURCE: &str = "def choose(x):\n    if x > 0:\n        return 1\n    elif x < 0:\n        return -1\n    return 0\n";

fn dial_source() -> String {
    let mut source = String::from("def dial(x):\n");
    for n in 1..=9 {
        source.push_str(&format!("    if x == {n}:\n        return {n}\n"));
    }
    source.push_str("    return 0\n");
    source
}

fn crapper(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crapper"))
        .args(args)
        .output()
        .expect("crapper binary runs")
}

fn crapper_in(root: &Path, args: &[&str]) -> Output {
    let root = root.to_str().expect("utf-8 temp path");
    let mut full = vec!["--root", root];
    full.extend_from_slice(args);
    crapper(&full)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("process exits normally")
}

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("file has parent")).expect("create dirs");
    fs::write(path, content).expect("write file");
}

fn lcov_record(path: &str, lines: u32, hits: u32) -> String {
    let mut record = format!("SF:{path}\n");
    for line in 1..=lines {
        record.push_str(&format!("DA:{line},{hits}\n"));
    }
    record.push_str("end_of_record\n");
    record
}

fn write_lcov(root: &Path, records: &str) {
    write_file(root, "target/coverage/python/lcov.info", records);
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

// CR-1, AC-13
#[test]
fn cr_1_ac_13_help_exits_zero_and_prints_usage() {
    let output = crapper(&["--help"]);
    assert_eq!(exit_code(&output), 0);
    assert!(stdout(&output).contains("Usage: crapper [options] [path-or-filter ...]"));
}

// CR-2, AC-9
#[test]
fn cr_2_ac_9_unknown_flag_exits_1_with_help_on_stderr() {
    let output = crapper(&["--bogus"]);
    assert_eq!(exit_code(&output), 1);
    let err = stderr(&output);
    assert!(err.contains("Unknown option: --bogus"));
    assert!(err.contains("Usage: crapper"));
}

// CR-2, AC-9
#[test]
fn cr_2_ac_9_flag_missing_its_value_exits_1() {
    let output = crapper(&["--root"]);
    assert_eq!(exit_code(&output), 1);
    assert!(stderr(&output).contains("--root requires a value"));
}

// CR-2, AC-9
#[test]
fn cr_2_ac_9_bad_threshold_exits_1() {
    let output = crapper(&["--threshold", "abc"]);
    assert_eq!(exit_code(&output), 1);
    assert!(stderr(&output).contains("--threshold requires a number"));
}

// CR-2, AC-9
#[test]
fn cr_2_ac_9_no_coverage_with_coverage_command_exits_1() {
    let output = crapper(&["--no-coverage", "--coverage-command", "true"]);
    assert_eq!(exit_code(&output), 1);
}

// CR-17, AC-3
#[test]
fn cr_17_ac_3_threshold_below_worst_score_exits_2() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_lcov(root, &lcov_record("src/app.py", 6, 0));

    let output = crapper_in(root, &["--use-existing-coverage", "--threshold", "10"]);
    assert_eq!(exit_code(&output), 2);
    assert!(stderr(&output).contains("CRAP threshold exceeded: 12.0 > 10.0"));
}

// CR-17, AC-3
#[test]
fn cr_17_ac_3_threshold_at_worst_score_exits_0() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_lcov(root, &lcov_record("src/app.py", 6, 0));

    let output = crapper_in(root, &["--use-existing-coverage", "--threshold", "12"]);
    assert_eq!(exit_code(&output), 0);
}

// CR-17, AC-3
#[test]
fn cr_17_ac_3_exact_score_10_0_exceeds_9_99_and_passes_10() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/dial.py", &dial_source());
    write_lcov(root, &lcov_record("src/dial.py", 21, 1));

    let exceeded = crapper_in(root, &["--use-existing-coverage", "--threshold", "9.99"]);
    assert_eq!(exit_code(&exceeded), 2);
    assert!(stderr(&exceeded).contains("CRAP threshold exceeded: 10.0 > 10.0"));

    let passes = crapper_in(root, &["--use-existing-coverage", "--threshold", "10"]);
    assert_eq!(exit_code(&passes), 0);
}

// CR-7, AC-4
#[test]
fn cr_7_ac_4_changed_selects_modified_staged_and_untracked() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    git(root, &["init", "-q"]);
    write_file(root, "committed.py", "def kept_fn(x):\n    return x\n");
    write_file(root, "modified.py", "def before_fn(x):\n    return x\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);

    write_file(root, "modified.py", "def changed_fn(x):\n    return x\n");
    write_file(root, "staged.py", "def staged_fn(x):\n    return x\n");
    git(root, &["add", "staged.py"]);
    write_file(root, "untracked.py", "def untracked_fn(x):\n    return x\n");

    let output = crapper_in(root, &["--changed", "--no-coverage"]);
    assert_eq!(exit_code(&output), 0);
    let out = stdout(&output);
    assert!(out.contains("changed_fn"));
    assert!(out.contains("staged_fn"));
    assert!(out.contains("untracked_fn"));
    assert!(!out.contains("kept_fn"));
    assert!(!out.contains("before_fn"));
}

// CR-20, AC-29
#[test]
fn cr_20_ac_29_changed_ignores_git_variables_of_another_repository() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("project");
    let other = temp.path().join("other");
    fs::create_dir_all(&root).expect("mkdir project");
    fs::create_dir_all(&other).expect("mkdir other");
    git(&root, &["init", "-q"]);
    git(&other, &["init", "-q"]);
    write_file(&root, "committed.py", "def kept_fn(x):\n    return x\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    write_file(
        &root,
        "untracked.py",
        "def untracked_fn(x):\n    return x\n",
    );

    let root_text = root.to_str().expect("utf-8 temp path");
    let output = Command::new(env!("CARGO_BIN_EXE_crapper"))
        .args(["--root", root_text, "--changed", "--no-coverage"])
        .env("GIT_DIR", other.join(".git"))
        .env("GIT_INDEX_FILE", other.join(".git/index"))
        .output()
        .expect("crapper binary runs");
    assert_eq!(exit_code(&output), 0, "stderr was: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("untracked_fn"), "stdout was: {out}");
    assert!(!out.contains("kept_fn"));
}

// CR-3
#[test]
fn cr_3_no_source_files_prints_message_and_exits_0() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = crapper_in(temp.path(), &["--no-coverage"]);
    assert_eq!(exit_code(&output), 0);
    assert_eq!(stdout(&output), "No source files to analyze.\n");
}

// CR-4
#[test]
fn cr_4_snapshot_written_at_root_metrics_crap_edn() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_lcov(root, &lcov_record("src/app.py", 6, 0));

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_eq!(exit_code(&output), 0);
    assert!(root.join(".metrics/crap.edn").is_file());
}

// CR-NFR-1, AC-10
#[test]
fn cr_nfr_1_ac_10_two_runs_write_identical_snapshots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(root, "src/dial.py", &dial_source());
    write_lcov(
        root,
        &format!(
            "{}{}",
            lcov_record("src/app.py", 6, 0),
            lcov_record("src/dial.py", 21, 1)
        ),
    );

    crapper_in(root, &["--use-existing-coverage"]);
    let first = fs::read(root.join(".metrics/crap.edn")).expect("first snapshot");
    crapper_in(root, &["--use-existing-coverage"]);
    let second = fs::read(root.join(".metrics/crap.edn")).expect("second snapshot");
    assert_eq!(first, second);
}

// CR-16
#[test]
fn cr_16_report_first_row_is_worst_score() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(root, "src/dial.py", &dial_source());
    write_lcov(
        root,
        &format!(
            "{}{}",
            lcov_record("src/app.py", 6, 0),
            lcov_record("src/dial.py", 21, 1)
        ),
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_eq!(exit_code(&output), 0);
    let out = stdout(&output);
    let first_row = out
        .lines()
        .skip_while(|line| !line.starts_with("---"))
        .nth(1)
        .expect("report has a first data row");
    assert!(
        first_row.starts_with("choose"),
        "first row was: {first_row}"
    );
    assert!(
        first_row.trim_end().ends_with("12.0"),
        "first row was: {first_row}"
    );
}

// CR-7, AC-4
#[test]
fn cr_7_ac_4_changed_with_root_skips_outside_deleted_and_non_source() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path();
    git(repo, &["init", "-q"]);
    write_file(repo, "sub/app.py", "def app_before(x):\n    return x\n");
    write_file(repo, "sub/gone.py", "def gone_fn(x):\n    return x\n");
    write_file(
        repo,
        "sub/test_x.py",
        "def test_x_before(x):\n    return x\n",
    );
    write_file(repo, "sub/README.md", "old\n");
    write_file(
        repo,
        "other/outside.py",
        "def outside_before(x):\n    return x\n",
    );
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "init"]);

    write_file(repo, "sub/app.py", "def app_fn(x):\n    return x\n");
    write_file(repo, "sub/test_x.py", "def test_x_fn(x):\n    return x\n");
    write_file(repo, "sub/README.md", "new\n");
    write_file(
        repo,
        "other/outside.py",
        "def outside_fn(x):\n    return x\n",
    );
    fs::remove_file(repo.join("sub/gone.py")).expect("delete file");

    let output = crapper_in(&repo.join("sub"), &["--changed", "--no-coverage"]);
    assert_eq!(exit_code(&output), 0);
    let out = stdout(&output);
    assert!(out.contains("app_fn"), "output was: {out}");
    assert!(!out.contains("outside_fn"), "output was: {out}");
    assert!(!out.contains("test_x_fn"), "output was: {out}");
    assert!(!out.contains("gone_fn"), "output was: {out}");
}

// CR-7, AC-4
#[test]
fn cr_7_ac_4_changed_ignores_deleted_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path();
    git(repo, &["init", "-q"]);
    write_file(repo, "sub/gone.py", "def gone_fn(x):\n    return x\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "init"]);
    fs::remove_file(repo.join("sub/gone.py")).expect("delete file");

    let output = crapper_in(&repo.join("sub"), &["--changed", "--no-coverage"]);
    assert_eq!(exit_code(&output), 0);
    assert_eq!(stdout(&output), "No source files to analyze.\n");
}

// CR-7, AC-4
#[test]
fn cr_7_ac_4_changed_ignores_files_outside_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path();
    git(repo, &["init", "-q"]);
    write_file(repo, "sub/notes.txt", "notes\n");
    write_file(
        repo,
        "other/outside.py",
        "def outside_before(x):\n    return x\n",
    );
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "init"]);
    write_file(
        repo,
        "other/outside.py",
        "def outside_fn(x):\n    return x\n",
    );

    let output = crapper_in(&repo.join("sub"), &["--changed", "--no-coverage"]);
    assert_eq!(exit_code(&output), 0);
    assert_eq!(stdout(&output), "No source files to analyze.\n");
}

// CR-7, AC-4
#[test]
fn cr_7_ac_4_changed_ignores_non_source_and_test_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path();
    git(repo, &["init", "-q"]);
    write_file(repo, "sub/README.md", "old\n");
    write_file(
        repo,
        "sub/test_x.py",
        "def test_x_before(x):\n    return x\n",
    );
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "init"]);
    write_file(repo, "sub/README.md", "new\n");
    write_file(repo, "sub/test_x.py", "def test_x_fn(x):\n    return x\n");

    let output = crapper_in(&repo.join("sub"), &["--changed", "--no-coverage"]);
    assert_eq!(exit_code(&output), 0);
    assert_eq!(stdout(&output), "No source files to analyze.\n");
}

// CR-3, CR-7
#[test]
fn cr_3_changed_without_git_on_path_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    let output = Command::new(env!("CARGO_BIN_EXE_crapper"))
        .env("PATH", root)
        .args([
            "--root",
            root.to_str().expect("utf-8 temp path"),
            "--changed",
            "--no-coverage",
        ])
        .output()
        .expect("crapper binary runs");
    assert_eq!(exit_code(&output), 1);
}

// CR-3, CR-4
#[test]
fn cr_3_unwritable_metrics_location_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(root, ".metrics", "a file, not a directory\n");

    let output = crapper_in(root, &["--no-coverage"]);
    assert_eq!(exit_code(&output), 1);
}

fn choose_row(out: &str) -> String {
    out.lines()
        .find(|line| line.starts_with("choose"))
        .expect("report has a choose row")
        .to_string()
}

// CR-13, CR-3
#[test]
fn cr_13_cr_3_failed_coverage_command_status_3_reads_no_reports() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_lcov(root, &lcov_record("src/app.py", 6, 1));

    let output = crapper_in(root, &["--coverage-command", "exit 3"]);
    assert_eq!(exit_code(&output), 0);
    let row = choose_row(&stdout(&output));
    assert!(row.contains("  0.0%"), "row was: {row}");
    assert!(!row.contains("100.0%"), "row was: {row}");
}

// CR-13, CR-3
#[test]
fn cr_13_cr_3_failed_coverage_command_status_1_reads_no_reports() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_lcov(root, &lcov_record("src/app.py", 6, 1));

    let output = crapper_in(root, &["--coverage-command", "exit 1"]);
    assert_eq!(exit_code(&output), 0);
    let row = choose_row(&stdout(&output));
    assert!(row.contains("  0.0%"), "row was: {row}");
    assert!(!row.contains("100.0%"), "row was: {row}");
}

// CR-13, CR-3
#[test]
fn cr_13_cr_3_successful_coverage_command_reads_reports() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_lcov(root, &lcov_record("src/app.py", 6, 1));

    let output = crapper_in(root, &["--coverage-command", "true"]);
    assert_eq!(exit_code(&output), 0);
    let row = choose_row(&stdout(&output));
    assert!(row.contains("100.0%"), "row was: {row}");
}

// CR-10, AC-25
#[test]
fn cr_10_ac_25_function_missing_from_the_report_scores_coverage_0_0() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(root, "src/other.py", "def other():\n    return 1\n");
    write_lcov(root, &lcov_record("src/other.py", 2, 1));

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_eq!(exit_code(&output), 0);

    let snapshot = fs::read_to_string(root.join(".metrics/crap.edn")).expect("snapshot written");
    assert_eq!(
        snapshot,
        "{:entries [\n  {:name \"choose\", :namespace \"app\", :complexity 3, :coverage 0.0, :crap 12.0}\n  {:name \"other\", :namespace \"other\", :complexity 1, :coverage 100.0, :crap 1.0}\n]}\n"
    );
    assert!(!snapshot.contains("nil"), "snapshot was: {snapshot}");

    let row = choose_row(&stdout(&output));
    assert!(row.contains("  0.0%"), "row was: {row}");
}

fn assert_crash_exit_1(output: &Output, root: &Path, needle: &str) {
    assert_eq!(exit_code(output), 1, "stderr was: {}", stderr(output));
    assert_eq!(stdout(output), "");
    let err = stderr(output);
    assert_eq!(err.trim_end().lines().count(), 1, "stderr was: {err}");
    assert!(err.contains(needle), "stderr was: {err}");
    assert!(!err.contains("panicked"), "stderr was: {err}");
    assert!(!root.join(".metrics/crap.edn").exists());
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_broken_symlink_source_exits_1_with_one_stderr_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    symlink("/nonexistent/missing.py", root.join("src/broken.py")).expect("symlink");

    let output = crapper_in(root, &["--no-coverage"]);
    assert_crash_exit_1(&output, root, "broken.py");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_source_file_without_read_permission_exits_1_with_one_stderr_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(root, "src/secret.py", CHOOSE_SOURCE);
    fs::set_permissions(
        root.join("src/secret.py"),
        fs::Permissions::from_mode(0o000),
    )
    .expect("chmod");
    // Root ignores permission bits, so an unreadable file is still readable here.
    if fs::read(root.join("src/secret.py")).is_ok() {
        return;
    }

    let output = crapper_in(root, &["--no-coverage"]);
    fs::set_permissions(
        root.join("src/secret.py"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("restore mode");
    assert_crash_exit_1(&output, root, "secret.py");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_symlink_loop_source_exits_1_with_one_stderr_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    symlink("loop.py", root.join("src/loop.py")).expect("symlink");

    let output = crapper_in(root, &["--no-coverage"]);
    assert_crash_exit_1(&output, root, "loop.py");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_go_profile_with_a_wrong_field_count_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py:1.1,2.2 1\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "coverage.out");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_go_profile_with_a_non_integer_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py:a.1,2.2 1 1\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "coverage.out");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_go_profile_block_without_a_comma_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py:1.1 1 1\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "coverage.out");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_go_profile_line_without_a_colon_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py 1 1\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "coverage.out");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_jacoco_report_that_is_not_well_formed_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/site/jacoco/jacoco.xml",
        "<report><class name=\"a/B\"",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "jacoco.xml");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_jacoco_counter_that_is_not_a_number_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/site/jacoco/jacoco.xml",
        "<report><package><class name=\"a/B\"><method name=\"f\" line=\"1\"><counter type=\"INSTRUCTION\" missed=\"x\" covered=\"1\"/></method></class></package></report>",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "jacoco.xml");
}

fn assert_unreadable_report_exits_1(relative: &str, content: &str, needle: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(root, relative, content);
    fs::set_permissions(root.join(relative), fs::Permissions::from_mode(0o000)).expect("chmod");
    // Root ignores permission bits, so an unreadable report is still readable here.
    if fs::read(root.join(relative)).is_ok() {
        return;
    }

    let output = crapper_in(root, &["--use-existing-coverage"]);
    fs::set_permissions(root.join(relative), fs::Permissions::from_mode(0o644))
        .expect("restore mode");
    assert_crash_exit_1(&output, root, needle);
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_an_unreadable_lcov_report_exits_1() {
    assert_unreadable_report_exits_1(
        "target/coverage/python/lcov.info",
        "SF:src/app.py\nDA:1,1\nend_of_record\n",
        "lcov.info",
    );
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_an_unreadable_go_profile_exits_1() {
    assert_unreadable_report_exits_1(
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py:1.1,2.2 1 1\n",
        "coverage.out",
    );
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_an_unreadable_jacoco_report_exits_1() {
    assert_unreadable_report_exits_1("target/site/jacoco/jacoco.xml", "<report/>", "jacoco.xml");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_an_unreadable_cloverage_html_exits_1() {
    assert_unreadable_report_exits_1(
        "target/coverage/app.html",
        "<span title=\"1 out of 2 forms covered\">1&nbsp;",
        "app.html",
    );
}

// CR-3
#[test]
fn cr_3_a_go_profile_with_python_style_integers_is_accepted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py:1_0.1,+2.2 1_0 +1\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_eq!(exit_code(&output), 0, "stderr was: {}", stderr(&output));
    assert!(root.join(".metrics/crap.edn").is_file());
}

// CR-3
#[test]
fn cr_3_a_go_profile_with_integers_beyond_64_bits_is_accepted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py:1.1,2.2 99999999999999999999999 99999999999999999999999\nsrc/app.py:3.1,4.2 1 99999999999999999999999\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_eq!(exit_code(&output), 0, "stderr was: {}", stderr(&output));
    assert!(root.join(".metrics/crap.edn").is_file());
}

// CR-3
#[test]
fn cr_3_a_jacoco_report_with_python_style_integers_is_accepted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/site/jacoco/jacoco.xml",
        "<report><package><class name=\"app\"><method name=\"f\" line=\"+0_1\"><counter type=\"INSTRUCTION\" missed=\"1_0\" covered=\"99999999999999999999999\"/></method></class></package></report>",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_eq!(exit_code(&output), 0, "stderr was: {}", stderr(&output));
    assert!(root.join(".metrics/crap.edn").is_file());
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_go_profile_with_a_misplaced_underscore_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/coverage/go/coverage.out",
        "mode: set\nsrc/app.py:1.1,2.2 1_ 1\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "coverage.out");
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_python_package_outside_the_root_exits_1_with_one_stderr_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    write_file(parent, "aaa-ext/app.py", "def f(x):\n    return x\n");
    write_file(
        parent,
        "aaa-ext/pyproject.toml",
        "[project]\nname = \"ext\"\n",
    );

    let root = parent.join("root");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    symlink(parent.join("aaa-ext/app.py"), root.join("src/ext_app.py")).expect("symlink");

    let output = crapper_in(&root, &[]);
    assert_crash_exit_1(&output, &root, "is not in the subpath of");
}

// CR-3
#[test]
fn cr_3_a_jacoco_report_with_garbage_in_the_doctype_is_accepted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/site/jacoco/jacoco.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE r\"&\\=2eport PUBLIC \"x\">\n<report/>\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_eq!(exit_code(&output), 0, "stderr was: {}", stderr(&output));
    assert!(root.join(".metrics/crap.edn").is_file());
}

// CR-3, AC-26
#[test]
fn cr_3_ac_26_a_jacoco_xml_declaration_with_a_bad_standalone_value_exits_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_file(root, "src/app.py", CHOOSE_SOURCE);
    write_file(
        root,
        "target/site/jacoco/jacoco.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yeses\"?>\n<report/>\n",
    );

    let output = crapper_in(root, &["--use-existing-coverage"]);
    assert_crash_exit_1(&output, root, "jacoco.xml");
}
