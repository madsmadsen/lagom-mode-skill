// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Fix worker-cap-half-the-cores (MU-17, MU-PB-4, AC-12, AC-19).

use std::fs;
use std::path::Path;
use std::process::Command;

/// The cores of the host, and the worker count of a run that asks for 10000.
fn want_workers(site_count: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    (cores / 2).max(1).min(site_count)
}

/// MU-17, AC-19: the default worker count is half the cores, at least one, and an explicit cap is honored.
#[test]
fn mu_17_default_worker_count_is_half_the_cores_at_least_one() {
    let want = want_workers(10_000);
    assert_eq!(mutator::workers::worker_count(10_000, None), want);
    assert_eq!(mutator::workers::worker_count(10_000, Some(10_000)), want);
    assert_eq!(mutator::workers::worker_count(1, None), 1);
}

/// MU-17, AC-19: a binary run with `--max-workers 10000` makes half the cores of worker
/// directories, at least one. Each worker is a directory beside the mutated file, so the
/// test command counts them from the worker that runs it. No unit call is involved.
#[test]
fn ac_19_a_binary_run_with_max_workers_10000_uses_half_the_cores() {
    const SITES: usize = 12;
    let dir = tempfile::tempdir().expect("temp dir");
    let source: String = (0..SITES)
        .map(|n| format!("    x{n} = a > {n}\n"))
        .collect::<String>();
    fs::create_dir_all(dir.path().join("src")).expect("create src");
    fs::write(
        dir.path().join("src/m.py"),
        format!("def f(a):\n{source}    return a\n"),
    )
    .expect("write source");
    let counts = dir.path().join("workers.txt");
    let marker = dir.path().join("baseline.done");
    let script = dir.path().join("count.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             if [ -f {marker} ]; then\n\
             \x20 ls .. | grep -c '^worker-' >> {counts}\n\
             \x20 exit 1\n\
             fi\n\
             : > {marker}\n\
             exit 0\n",
            marker = marker.display(),
            counts = counts.display()
        ),
    )
    .expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_mutator"))
        .args([
            "--no-coverage",
            "--test-command",
            &format!("sh {}", script.display()),
            "--max-workers",
            "10000",
        ])
        .current_dir(dir.path())
        .output()
        .expect("spawn mutator");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let seen = read_counts(&counts);
    let want = want_workers(SITES);
    assert!(
        want < 10_000,
        "this host is too large for the test: {want} workers"
    );
    assert_eq!(
        seen.iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        [want]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        "a mutant run counted another number of worker directories: {seen:?}, wanted {want}"
    );
    assert!(
        seen.len() >= SITES,
        "only {} mutant runs counted the workers",
        seen.len()
    );
}

/// The counts that the test command wrote, one per mutant run, in file order.
fn read_counts(path: &Path) -> Vec<usize> {
    let text = fs::read_to_string(path).expect("count file");
    text.lines()
        .map(|line| line.trim().parse().expect("a count"))
        .collect()
}

/// MU-17: `--help` describes the half-the-cores default and no longer says one per core.
#[test]
fn mu_17_help_says_half_the_cores_not_one_per_core() {
    let out = Command::new(env!("CARGO_BIN_EXE_mutator"))
        .arg("--help")
        .output()
        .expect("running mutator --help");
    assert!(out.status.success(), "mutator --help exited {}", out.status);
    let help = String::from_utf8(out.stdout).expect("help is UTF-8");
    assert!(
        help.contains("Default: half the cores, at least one."),
        "help does not state the half-the-cores default:\n{help}"
    );
    assert!(
        !help.contains("one per core"),
        "help still says one per core:\n{help}"
    );
}
