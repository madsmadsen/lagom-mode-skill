mod common;

use common::*;
use std::fs;
use std::path::Path;

const SRC: &str = "def f(a, b):\n    return a < b\n\n\ndef g(x, y):\n    return x > y\n";
const EDITED_G: &str =
    "def f(a, b):\n    return a < b\n\n\ndef g(x, y):\n    # edited\n    return x > y\n";
const KILL: &str = "grep -q 'a < b' src/m.py && grep -q 'return x > y' src/m.py";
const FOOTER: &str = "* marks a function whose text changed since the last snapshot.";

/// Number of times the test command ran: each run appends one line to `calls`.
fn calls(calls_file: &Path) -> usize {
    fs::read_to_string(calls_file).map_or(0, |text| text.lines().count())
}

/// Runs mutator with `cmd` wrapped in a counter; returns the output and how many times `cmd` ran.
fn counted_run(dir: &Path, calls_file: &Path, extra: &[&str], cmd: &str) -> (Out, usize) {
    let before = calls(calls_file);
    let command = format!("echo call >> '{}'; {cmd}", calls_file.display());
    let mut args = vec!["--no-coverage"];
    args.extend_from_slice(extra);
    args.extend(["--test-command", command.as_str()]);
    let out = run(dir, &args);
    (out, calls(calls_file) - before)
}

/// Number of lines in a scan output that contain `tag`.
fn sites_tagged(scan: &str, tag: &str) -> usize {
    scan.lines().filter(|line| line.contains(tag)).count()
}

// AC-7 / MU-19: a survivor of the snapshot is rerun by the next run.
#[test]
fn mu_19_ac7_second_run_reruns_survivors() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    let counter = tempfile::tempdir().expect("tempdir");
    let calls_file = counter.path().join("calls");
    let (first, _) = counted_run(dir.path(), &calls_file, &[], "true");
    assert_eq!(first.code, 3, "{}", first.stderr);
    let scan = run(dir.path(), &["--scan", "--no-coverage"]);
    let sites = sites_tagged(&scan.stdout, "src/m.py:");
    let (second, ran) = counted_run(dir.path(), &calls_file, &[], "true");
    assert_eq!(second.code, 3, "{}", second.stderr);
    assert_eq!(ran, 1 + sites, "baseline plus every surviving site");
}

// AC-7 / MU-19: after one function changes, only its sites rerun; --mutate-all reruns every site.
#[test]
fn mu_19_ac7_changed_function_reruns_alone_and_mutate_all_reruns_all() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    let counter = tempfile::tempdir().expect("tempdir");
    let calls_file = counter.path().join("calls");
    let (first, _) = counted_run(dir.path(), &calls_file, &[], KILL);
    assert_eq!(first.code, 0, "{}", first.stderr);

    write(dir.path(), "src/m.py", EDITED_G);
    let scan = run(dir.path(), &["--scan", "--no-coverage"]);
    let starred: Vec<&str> = scan
        .stdout
        .lines()
        .filter(|line| line.starts_with("* src/"))
        .collect();
    let g_sites = sites_tagged(&scan.stdout, "[defn/g]");
    let f_sites = sites_tagged(&scan.stdout, "[defn/f]");
    assert_eq!(starred.len(), g_sites, "{}", scan.stdout);
    assert!(
        starred.iter().all(|line| line.contains("[defn/g]")),
        "{}",
        scan.stdout
    );
    assert_eq!(scan.stdout.lines().last(), Some(FOOTER));

    let (second, ran) = counted_run(dir.path(), &calls_file, &[], KILL);
    assert_eq!(second.code, 0, "{}", second.stderr);
    assert_eq!(ran, 1 + g_sites, "baseline plus the sites of g only");

    let (third, ran) = counted_run(dir.path(), &calls_file, &["--mutate-all"], KILL);
    assert_eq!(third.code, 0, "{}", third.stderr);
    assert_eq!(ran, 1 + f_sites + g_sites, "baseline plus every site");
}
