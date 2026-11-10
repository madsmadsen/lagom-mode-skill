// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: none
// license: MIT

//! Offline replay of the parity runs in `parity/coverage.toml`.
//! Each run executes the Rust binary in a copy of its fixture and compares the
//! result with what the original Python crapper recorded in `parity/recorded/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GIT_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "parity"),
    ("GIT_AUTHOR_EMAIL", "parity@example.invalid"),
    ("GIT_COMMITTER_NAME", "parity"),
    ("GIT_COMMITTER_EMAIL", "parity@example.invalid"),
    ("GIT_AUTHOR_DATE", "2000-01-01T00:00:00+0000"),
    ("GIT_COMMITTER_DATE", "2000-01-01T00:00:00+0000"),
];

struct Run {
    id: String,
    fixture: String,
    args: Vec<String>,
    setup: Vec<String>,
    requires: Vec<String>,
    covers: Vec<String>,
    env: Vec<String>,
    fix: String,
}

struct Produced {
    code: i32,
    stdout: Vec<u8>,
    edn: Option<Vec<u8>>,
}

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("parity")
}

fn recorded_dir(run_id: &str) -> PathBuf {
    parity_dir().join("recorded").join(run_id)
}

fn text_field(run: &toml::Value, key: &str) -> String {
    run.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn list_field(run: &toml::Value, key: &str) -> Vec<String> {
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

fn load_runs() -> Vec<Run> {
    let source =
        fs::read_to_string(parity_dir().join("coverage.toml")).expect("read coverage.toml");
    let table: toml::Table = toml::from_str(&source).expect("parse coverage.toml");
    table
        .get("run")
        .and_then(toml::Value::as_array)
        .expect("coverage.toml has [[run]] entries")
        .iter()
        .map(|run| Run {
            id: text_field(run, "id"),
            fixture: text_field(run, "fixture"),
            args: list_field(run, "args"),
            setup: list_field(run, "setup"),
            requires: list_field(run, "requires"),
            covers: list_field(run, "covers"),
            env: list_field(run, "env"),
            fix: text_field(run, "fix"),
        })
        .collect()
}

fn find_run(run_id: &str) -> Result<Run, String> {
    load_runs()
        .into_iter()
        .find(|run| run.id == run_id)
        .ok_or_else(|| format!("{run_id}: not in coverage.toml"))
}

fn ids_where(pred: impl Fn(&Run) -> bool) -> Vec<String> {
    load_runs()
        .into_iter()
        .filter(|run| run.fix.is_empty() && pred(run))
        .map(|run| run.id)
        .collect()
}

fn explicit(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

fn has_recording(run: &Run) -> bool {
    recorded_dir(&run.id).is_dir()
}

fn has_arg(run: &Run, flag: &str) -> bool {
    run.args.iter().any(|arg| arg == flag)
}

fn covers_language(run: &Run) -> bool {
    run.covers
        .iter()
        .any(|cover| cover.starts_with("language:"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn run_setup(commands: &[String], project: &Path) -> Result<(), String> {
    for command in commands {
        let status = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(project)
            .envs(GIT_ENV)
            .status()
            .map_err(|err| format!("setup `{command}` did not start: {err}"))?;
        if !status.success() {
            return Err(format!("setup `{command}` failed: {status}"));
        }
    }
    Ok(())
}

fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while !rest.is_empty() {
        if !needle.is_empty() && rest.starts_with(needle) {
            out.extend_from_slice(with);
            rest = &rest[needle.len()..];
        } else {
            out.push(rest[0]);
            rest = &rest[1..];
        }
    }
    out
}

fn normalize(stdout: &[u8], project: &Path) -> Vec<u8> {
    let mut variants = vec![project.to_string_lossy().into_owned()];
    if let Ok(resolved) = fs::canonicalize(project) {
        variants.push(resolved.to_string_lossy().into_owned());
    }
    variants.sort_by_key(|variant| std::cmp::Reverse(variant.len()));
    variants.iter().fold(stdout.to_vec(), |acc, variant| {
        replace_bytes(&acc, variant.as_bytes(), b"<ROOT>")
    })
}

fn metrics_path(copy: &Path, args: &[String]) -> PathBuf {
    let root = match args
        .iter()
        .position(|arg| arg == "--root")
        .and_then(|at| args.get(at + 1))
    {
        Some(value) => copy.join(value),
        None => copy.to_path_buf(),
    };
    root.join(".metrics").join("crap.edn")
}

/// PATH with the `.stubs` directory of the fixture first, when the fixture has one.
fn stubbed_path(project: &Path) -> std::ffi::OsString {
    let mut paths = vec![project.join(".stubs")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).expect("PATH entries have no separator")
}

/// The `NAME=VALUE` entries of a run. `{project}` is the project path.
fn run_env(entries: &[String], project: &Path) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|entry| entry.split_once('='))
        .map(|(name, value)| {
            (
                name.to_string(),
                value.replace("{project}", &project.to_string_lossy()),
            )
        })
        .collect()
}

fn produce(run: &Run) -> Result<Produced, String> {
    let root = tempfile::tempdir().map_err(|err| format!("tempdir: {err}"))?;
    let project = root.path().join("project");
    copy_dir(&parity_dir().join("fixtures").join(&run.fixture), &project)
        .map_err(|err| format!("{}: copy fixture: {err}", run.id))?;
    run_setup(&run.setup, &project)?;
    let done = Command::new(env!("CARGO_BIN_EXE_crapper"))
        .args(&run.args)
        .current_dir(&project)
        .env("PATH", stubbed_path(&project))
        .envs(run_env(&run.env, &project))
        .output()
        .map_err(|err| format!("{}: run crapper: {err}", run.id))?;
    Ok(Produced {
        code: done.status.code().unwrap_or(-1),
        stdout: normalize(&done.stdout, &project),
        edn: fs::read(metrics_path(&project, &run.args)).ok(),
    })
}

fn compare(run_id: &str, recorded: &Path, produced: &Produced) -> Result<(), String> {
    let read = |name: &str| {
        fs::read(recorded.join(name)).map_err(|err| format!("{run_id}: read {name}: {err}"))
    };
    let exit_text = fs::read_to_string(recorded.join("exit.txt"))
        .map_err(|err| format!("{run_id}: read exit.txt: {err}"))?;
    let expected_code: i32 = exit_text
        .trim()
        .parse()
        .map_err(|err| format!("{run_id}: bad exit.txt: {err}"))?;
    let mut problems = Vec::new();
    if produced.code != expected_code {
        problems.push(format!(
            "exit {} != recorded {expected_code}",
            produced.code
        ));
    }
    if produced.stdout != read("stdout.txt")? {
        problems.push("stdout differs from recorded stdout.txt".to_string());
    }
    match (fs::read(recorded.join("crap.edn")).ok(), &produced.edn) {
        (Some(want), Some(got)) if want != *got => {
            problems.push("crap.edn differs from recorded".to_string())
        }
        (Some(_), None) => problems.push("crap.edn not written; recorded one exists".to_string()),
        (None, Some(_)) => problems.push("crap.edn written; none recorded".to_string()),
        _ => {}
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("{run_id}: {}", problems.join("; ")))
    }
}

fn replay(run_id: &str) -> Result<(), String> {
    let run = find_run(run_id)?;
    let recorded = recorded_dir(run_id);
    if !recorded.is_dir() {
        return Err(format!(
            "{run_id}: no recorded run at {}",
            recorded.display()
        ));
    }
    let produced = produce(&run)?;
    compare(run_id, &recorded, &produced)
}

fn check_group(name: &str, run_ids: &[String]) {
    assert!(!run_ids.is_empty(), "{name}: no runs selected");
    let failures: Vec<String> = run_ids.iter().filter_map(|id| replay(id).err()).collect();
    assert!(
        failures.is_empty(),
        "{name}: {} of {} runs differ from the recorded original:\n{}",
        failures.len(),
        run_ids.len(),
        failures.join("\n")
    );
}

#[test]
fn ac_16_stored_reports_give_recorded_snapshots() {
    check_group(
        "AC-16",
        &ids_where(|run| covers_language(run) && has_arg(run, "--use-existing-coverage")),
    );
}

#[test]
fn ac_8_table_equals_recorded_original_table() {
    check_group(
        "AC-8",
        &ids_where(|run| run.requires.is_empty() && has_recording(run)),
    );
}

#[test]
fn ac_3_threshold_exit_codes_match_recorded() {
    check_group("AC-3", &ids_where(|run| run.id.starts_with("threshold")));
}

#[test]
fn ac_9_rejected_flags_match_recorded() {
    check_group("AC-9", &ids_where(|run| run.id.starts_with("reject")));
}

// CR-3, AC-26
#[test]
fn ac_26_crashes_exit_1_like_the_recorded_original() {
    check_group("AC-26", &ids_where(|run| run.id.starts_with("crash-")));
}

#[test]
fn ac_14_skipped_dirs_match_recorded() {
    check_group("AC-14", &explicit(&["dirs-skip"]));
}

#[test]
fn ac_4_changed_matches_recorded() {
    check_group("AC-4", &explicit(&["changed"]));
}

#[test]
fn ac_5_coverage_command_and_no_coverage_match_recorded() {
    check_group(
        "AC-5",
        &ids_where(|run| has_arg(run, "--no-coverage") || has_arg(run, "--coverage-command")),
    );
}

#[test]
fn ac_25_missing_function_matches_recorded() {
    check_group("AC-25", &explicit(&["missing-function", "cr10-no-data"]));
}

#[test]
fn ac_10_two_runs_give_identical_snapshots() {
    let run = find_run("python-existing").expect("python-existing is in coverage.toml");
    let first = produce(&run).expect("first run of python-existing");
    let second = produce(&run).expect("second run of python-existing");
    assert!(
        first.edn.is_some(),
        "AC-10: python-existing wrote no .metrics/crap.edn"
    );
    assert_eq!(
        first.edn, second.edn,
        "AC-10: two runs of python-existing wrote different snapshots"
    );
    assert_eq!(
        first.stdout, second.stdout,
        "AC-10: two runs of python-existing printed different tables"
    );
}

#[test]
fn ac_11_language_fixtures_pass_without_python() {
    check_group(
        "AC-11",
        &explicit(&[
            "clojure-existing",
            "java-existing",
            "go-existing",
            "typescript-existing",
            "rust-existing",
            "python-existing",
        ]),
    );
}

// CR-4: the snapshot lives under the --root value when given, else under the copy.
#[test]
fn cr_4_metrics_path_follows_root_flag() {
    let copy = Path::new("/work/copy");
    let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    assert_eq!(
        metrics_path(copy, &args(&["--root", "sub"])),
        PathBuf::from("/work/copy/sub/.metrics/crap.edn")
    );
    assert_eq!(
        metrics_path(copy, &args(&["--root", "/abs/dir"])),
        PathBuf::from("/abs/dir/.metrics/crap.edn")
    );
    assert_eq!(
        metrics_path(copy, &args(&["--threshold", "5"])),
        PathBuf::from("/work/copy/.metrics/crap.edn")
    );
}
