// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// Ports src/dryer/cli.py (run and main).

use std::io::Write;

use crate::args::{self, Action, Options};
use crate::discover;
use crate::report;
use crate::scan;
use crate::select;
use crate::shape;

const STACK_BYTES: usize = 512 * 1024 * 1024;
const PANIC_EXIT_CODE: i32 = 70;

/// Writes text, ignoring stream errors the way a closed pipe is ignored by the CLI.
fn emit(stream: &mut dyn Write, text: &str) {
    let _ = stream.write_all(text.as_bytes());
}

fn show_help(options: &Options, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let stream: &mut dyn Write = if options.exit_code == 0 { out } else { err };
    let mut text = options.message.clone();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    emit(stream, &text);
    options.exit_code
}

fn scan_project(options: &Options, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let root = discover::resolve(&options.project_root);
    let files = match select::select_files(options) {
        Ok(files) => files,
        Err(error) => {
            emit(err, &format!("{}\n", error.message));
            return error.code;
        }
    };
    if files.is_empty() {
        emit(out, "No source files to analyze.\n");
        return 0;
    }
    let mut interner = shape::Interner::new();
    let (entries, warnings) = scan::scan_files(
        &files,
        &root,
        options.min_lines,
        options.min_nodes,
        &mut interner,
    );
    for warning in &warnings {
        emit(err, &format!("dryer: {warning}\n"));
    }
    let candidates = scan::find_duplicates(&entries, options.threshold);
    let metrics = match report::write_metrics(&candidates, &root) {
        Ok(path) => path,
        Err(error) => {
            emit(err, &format!("dryer: {error}\n"));
            return 1;
        }
    };
    emit(out, &render_report(&candidates, &options.format));
    emit(err, &format!("Wrote {}\n", discover::posix(&metrics)));
    0
}

fn render_report(candidates: &[crate::model::Duplicate], format: &str) -> String {
    if format == "edn" {
        report::render_edn(candidates)
    } else {
        report::format_text(candidates)
    }
}

/// Runs the CLI with the given arguments (without the program name).
/// DR-3: returns the exit code; DR-13: 0 after report, 1 usage error, 2 unknown format.
pub fn run_with(argv: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let options = args::parse_args(argv);
    match options.action {
        Action::Help => show_help(&options, out, err),
        Action::Format => {
            emit(err, &options.message);
            options.exit_code
        }
        Action::Scan => scan_project(&options, out, err),
    }
}

/// Runs the CLI against the process's stdout and stderr.
pub fn run(argv: &[String]) -> i32 {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    run_with(argv, &mut out, &mut err)
}

/// DR-13: exit code of a finished worker thread, or 70 when the thread did not
/// start. A thread that could not start reports why on stderr first.
fn thread_exit(started: std::io::Result<std::thread::JoinHandle<i32>>, err: &mut dyn Write) -> i32 {
    match started {
        Ok(handle) => joined_exit(handle.join()),
        Err(error) => {
            emit(
                err,
                &format!("dryer: cannot start the worker thread: {error}\n"),
            );
            PANIC_EXIT_CODE
        }
    }
}

/// DR-13: exit code of the worker thread's join result, or 70 when it panicked.
/// The port installs no panic hook, so the default hook of Rust may print its own
/// message on stderr before this code is returned.
fn joined_exit(joined: std::thread::Result<i32>) -> i32 {
    joined.unwrap_or(PANIC_EXIT_CODE)
}

/// Process entry point: runs on a thread with a large stack and exits with its code.
pub fn main() -> ! {
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let started = std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || run(&argv));
    // A plain `Stderr` handle takes the lock per write, so the join below does not
    // wait on a lock that the worker thread also wants.
    let mut err = std::io::stderr();
    let code = thread_exit(started, &mut err);
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    const LEFT: &str =
        "def alpha(xs):\n    ys = filter(xs, odd)\n    zs = map(ys, inc)\n    return list(zs)\n";
    const RIGHT: &str = "def beta(items):\n    kept = filter(items, even)\n    out = map(kept, dec)\n    return list(out)\n";

    fn run_args(args: &[&str]) -> (i32, String, String) {
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let code = run_with(&argv, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    fn write_source(root: &Path, name: &str, text: &str) {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(path, text).expect("write source");
    }

    fn project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let proj = dir.path().join("proj");
        fs::create_dir_all(&proj).expect("create proj");
        (dir, proj)
    }

    fn seeded_project() -> (tempfile::TempDir, std::path::PathBuf) {
        let (dir, proj) = project();
        write_source(&proj, "src/left.py", LEFT);
        write_source(&proj, "src/right.py", RIGHT);
        write_source(&proj, "tests/test_left.py", LEFT);
        (dir, proj)
    }

    fn proj_str(proj: &Path) -> String {
        proj.to_string_lossy().into_owned()
    }

    // DR-13, AC-9: the tests of fix worker-thread-exit-70 in parity/fixes.toml, the
    // start-failure path and the panic path. A worker thread that cannot start reports
    // the error on stderr and exits 70. A worker thread that panics exits 70.
    #[test]
    fn dr13_thread_start_failure_reports_error_and_exits_seventy() {
        let failed = Err(std::io::Error::other("not enough memory"));
        let mut err: Vec<u8> = Vec::new();
        let code = thread_exit(failed, &mut err);
        assert_eq!(code, 70);
        let text = String::from_utf8_lossy(&err).into_owned();
        assert!(
            text.contains("cannot start the worker thread"),
            "stderr was {text:?}"
        );
        assert!(text.contains("not enough memory"), "stderr was {text:?}");
    }

    // DR-13: a worker thread that panics exits the process with 70. The exit
    // status is the part that DR-13 fixes. The port installs no panic hook, so
    // the default hook may print its own message on stderr, and this test does
    // not check that text. The 4 in the second assertion keeps the check honest:
    // a mapping that returned a constant would fail it.
    #[test]
    fn dr13_a_panicking_worker_thread_exits_seventy() {
        let panicked = std::thread::Builder::new()
            .spawn(|| -> i32 { panic!("worker thread failed") })
            .expect("spawn the worker thread")
            .join();
        assert!(panicked.is_err());
        assert_eq!(joined_exit(panicked), 70);
        assert_eq!(joined_exit(Ok(4)), 4);
    }

    // DR-13: a worker thread that runs keeps its exit code and writes nothing to stderr.
    #[test]
    fn dr13_thread_success_keeps_its_exit_code() {
        let started = std::thread::Builder::new().spawn(|| 3);
        let mut err: Vec<u8> = Vec::new();
        assert_eq!(thread_exit(started, &mut err), 3);
        assert!(err.is_empty());
    }

    // DR-13: --help exits 0 and prints usage to stdout without scanning.
    #[test]
    fn dr13_help_exits_zero_and_prints_usage() {
        let (code, out, err) = run_args(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("Usage: dryer"));
        assert!(out.contains("--threshold"));
        assert_eq!(err, "");
    }

    // DR-13: an unknown format exits 2 with the exact stderr message.
    #[test]
    fn dr13_unknown_format_exits_two() {
        let (code, out, err) = run_args(&["--format", "xml"]);
        assert_eq!(code, 2);
        assert_eq!(err, "Unknown format: xml\n");
        assert_eq!(out, "");
    }

    // DR-13: an unknown option exits 1 and names the option on stderr.
    #[test]
    fn dr13_unknown_option_exits_one() {
        let (code, _out, err) = run_args(&["--nope"]);
        assert_eq!(code, 1);
        assert!(err.contains("Unknown option: --nope"));
    }

    // DR-3, DR-4: two similar functions produce a text report, a metrics file, and a Wrote line.
    #[test]
    fn dr3_dr4_project_report_writes_snapshot() {
        let (_dir, proj) = seeded_project();
        let (code, out, err) = run_args(&[
            "--root",
            &proj_str(&proj),
            "--min-lines",
            "3",
            "--min-nodes",
            "1",
        ]);
        assert_eq!(code, 0);
        assert!(out.contains("DUPLICATE score=1.00"));
        assert!(out.contains("src/left.py:1-"));
        assert!(out.contains("src/right.py:1-"));
        assert!(!out.contains("test_left.py"));
        let metrics = report::metrics_path(&discover::resolve(&proj));
        let text = fs::read_to_string(&metrics).expect("metrics file");
        assert!(text.contains(":language \"python\""));
        assert!(text.contains(":file \"src/left.py\""));
        let expected = format!("Wrote {}\n", discover::posix(&metrics));
        assert!(err.ends_with(&expected), "stderr was {err:?}");
    }

    // DR-3: an empty project prints the no-sources message and writes no metrics directory.
    #[test]
    fn dr3_empty_project_reports_no_sources_and_writes_nothing() {
        let (_dir, proj) = project();
        let (code, out, _err) = run_args(&["--root", &proj_str(&proj)]);
        assert_eq!(code, 0);
        assert_eq!(out, "No source files to analyze.\n");
        assert!(!proj.join(".metrics").exists());
    }

    // DR-4, DR-12: --edn prints exactly the bytes written to .metrics/dry.edn.
    #[test]
    fn dr4_edn_stdout_equals_metrics_file_bytes() {
        let (_dir, proj) = seeded_project();
        let (code, out, _err) = run_args(&[
            "--root",
            &proj_str(&proj),
            "--edn",
            "--min-lines",
            "3",
            "--min-nodes",
            "1",
        ]);
        assert_eq!(code, 0);
        let file = fs::read_to_string(proj.join(".metrics").join("dry.edn")).expect("metrics");
        assert_eq!(out, file);
    }

    // DR-12: a threshold of 0.99 still reports the identical pair (score 1.00).
    #[test]
    fn dr12_threshold_below_score_reports_pair() {
        let (_dir, proj) = seeded_project();
        let (_code, out, _err) = run_args(&[
            "--root",
            &proj_str(&proj),
            "--threshold",
            "0.99",
            "--min-lines",
            "3",
            "--min-nodes",
            "1",
        ]);
        assert!(out.contains("DUPLICATE score=1.00"));
    }

    // DR-12: a --min-lines larger than the functions filters every form out.
    #[test]
    fn dr12_min_lines_above_form_length_reports_nothing() {
        let (_dir, proj) = seeded_project();
        let (code, out, _err) = run_args(&[
            "--root",
            &proj_str(&proj),
            "--min-lines",
            "50",
            "--min-nodes",
            "1",
        ]);
        assert_eq!(code, 0);
        assert!(!out.contains("DUPLICATE"));
    }

    // DR-NFR-1: two runs over the same project write byte-identical snapshots.
    #[test]
    fn dr_nfr1_two_runs_write_identical_bytes() {
        let (_dir, proj) = seeded_project();
        let args = [
            "--root",
            &proj_str(&proj),
            "--min-lines",
            "3",
            "--min-nodes",
            "1",
        ];
        run_args(&args);
        let first = fs::read(proj.join(".metrics").join("dry.edn")).expect("first");
        run_args(&args);
        let second = fs::read(proj.join(".metrics").join("dry.edn")).expect("second");
        assert_eq!(first, second);
    }

    // DR-7: --changed outside a git repository returns git's status code (128).
    #[test]
    fn dr7_changed_outside_git_returns_git_code() {
        let (_dir, proj) = project();
        let (code, out, err) = run_args(&["--changed", "--root", &proj_str(&proj)]);
        assert_eq!(code, 128);
        assert!(err.contains("not a git repository"));
        assert!(!out.contains("No source files"));
    }

    // DR-4: when .metrics cannot be created, the run reports the io error and exits 1.
    #[test]
    fn dr4_metrics_write_failure_exits_one() {
        let (_dir, proj) = seeded_project();
        fs::write(proj.join(".metrics"), "not a directory").expect("blocker file");
        let (code, _out, err) = run_args(&[
            "--root",
            &proj_str(&proj),
            "--min-lines",
            "3",
            "--min-nodes",
            "1",
        ]);
        assert_eq!(code, 1);
        assert!(err.starts_with("dryer: "), "stderr was {err:?}");
    }

    // DR-8: an unterminated Clojure list is reported as a warning; the run still succeeds.
    #[test]
    fn dr8_clojure_read_warning_is_reported_and_run_succeeds() {
        let (_dir, proj) = project();
        write_source(&proj, "bad.clj", "(defn f [x]\n  (inc x)");
        write_source(&proj, "good.clj", "(defn g [x]\n  (inc x))\n");
        let (code, _out, err) = run_args(&[
            "--root",
            &proj_str(&proj),
            "--min-lines",
            "1",
            "--min-nodes",
            "1",
        ]);
        assert_eq!(code, 0);
        assert!(err.contains("dryer: bad.clj:"), "stderr was {err:?}");
        assert!(err.contains("unterminated list"), "stderr was {err:?}");
    }
}
