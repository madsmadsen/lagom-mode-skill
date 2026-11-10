//! Fix max-memory-limit (MU-25, AC-17, MU-PB-4, AC-12). The original has no memory limit and no
//! `--max-memory-mb` flag, so each test here fails on its behavior.

mod common;

use common::{git_commit_all, project, run, write};
use std::fs;
use std::time::Instant;

const M_PY: &str = "def f(a, b):\n    return a < b\n";

/// The baseline sleeps 1 s, so `--timeout-factor 100` gives a timeout above 60 s. The mutant
/// `<` to `<=` allocates 8 MiB at a time, copying the whole string each time. The loop stops
/// at 512 MiB and exits 0, so the allocator ends by itself even when no limit kills it.
fn command(counter: &std::path::Path) -> String {
    format!(
        "sleep 1; if grep -q 'a <= b' src/m.py; then echo x >> {c}; \
         chunk=$(head -c 8388608 /dev/zero | tr '\\0' a); s=; i=0; \
         while [ $i -lt 64 ]; do s=$s$chunk; i=$((i+1)); done; fi; exit 0",
        c = counter.display()
    )
}

/// The same, but it doubles a 10 MiB string 5 times, so it reaches 320 MiB in about a second
/// and exits 0 by itself. That is over 256 MiB and fast to run.
fn doubling_command(counter: &std::path::Path) -> String {
    format!(
        "sleep 1; if grep -q 'a <= b' src/m.py; then echo x >> {c}; \
         s=$(head -c 10485760 /dev/zero | tr '\\0' a); i=0; \
         while [ $i -lt 5 ]; do s=$s$s; i=$((i+1)); done; fi; exit 0",
        c = counter.display()
    )
}

fn args<'a>(command: &'a str, memory: &'a [&'a str]) -> Vec<&'a str> {
    let mut all = vec![
        "--no-coverage",
        "--max-workers",
        "1",
        "--timeout-factor",
        "100",
    ];
    all.extend_from_slice(memory);
    all.extend_from_slice(&["--test-command", command]);
    all
}

// AC-17 (MU-25): with `--max-memory-mb 256` the allocating mutant is killed within 10 s of the
// start of its test run, and it runs once (a kill for memory is not a MU-21 second run).
#[test]
fn mu_25_ac17_a_mutant_over_the_memory_limit_is_killed_once_and_fast() {
    let scratch = tempfile::tempdir().unwrap();
    let counter = scratch.path().join("counter");
    let dir = project();
    write(dir.path(), "src/m.py", M_PY);
    git_commit_all(dir.path());
    let test_command = command(&counter);
    let started = Instant::now();
    let out = run(
        dir.path(),
        &args(&test_command, &["--max-memory-mb", "256"]),
    );
    let seconds = started.elapsed().as_secs_f64();
    assert_eq!(
        out.code, 0,
        "stdout:\n{}\nstderr:\n{}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .lines()
            .any(|l| l == "KILLED    src/m.py:2 < -> <="),
        "stdout:\n{}",
        out.stdout
    );
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
    // The baseline takes 1 s and the mutant 1 s before it allocates, so 10 s of allocation fit
    // in 12 s. The timeout is above 60 s, so only the memory limit can end the run this early.
    assert!(seconds < 12.0, "the run took {seconds} s");
}

// AC-17 (MU-25): `--max-memory-mb 0` turns the limit off, so a mutant that uses 320 MiB survives.
#[test]
fn mu_25_ac17_limit_zero_turns_the_memory_limit_off() {
    let scratch = tempfile::tempdir().unwrap();
    let counter = scratch.path().join("counter");
    let dir = project();
    write(dir.path(), "src/m.py", M_PY);
    git_commit_all(dir.path());
    let test_command = doubling_command(&counter);
    let out = run(dir.path(), &args(&test_command, &["--max-memory-mb", "0"]));
    assert_eq!(
        out.code, 3,
        "stdout:\n{}\nstderr:\n{}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .lines()
            .any(|l| l == "SURVIVED  src/m.py:2 < -> <="),
        "stdout:\n{}",
        out.stdout
    );
}

// AC-17 (MU-25): a negative value and a value that is not an integer exit 1.
#[test]
fn mu_25_ac17_a_bad_value_exits_one() {
    let dir = project();
    for value in ["-1", "abc", "1.5", ""] {
        let out = run(dir.path(), &["--max-memory-mb", value]);
        assert_eq!(out.code, 1, "value {value:?}, stderr:\n{}", out.stderr);
    }
    let out = run(dir.path(), &["--max-memory-mb"]);
    assert_eq!(out.code, 1, "stderr:\n{}", out.stderr);
}

// MU-25: `--help` lists the flag with its default.
#[test]
fn mu_25_help_lists_the_flag_and_its_default() {
    let dir = project();
    let out = run(dir.path(), &["--help"]);
    assert_eq!(out.code, 0);
    assert!(
        out.stdout.contains("--max-memory-mb <number>"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("Default: 4096."), "{}", out.stdout);
}
