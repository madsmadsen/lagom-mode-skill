//! Fix worker-cap-half-the-cores (MU-17, MU-PB-4, AC-12).

use std::process::Command;

/// MU-17: the default worker count is half the cores, at least one, and an explicit cap is honored.
#[test]
fn mu_17_default_worker_count_is_half_the_cores_at_least_one() {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let want = (cores / 2).max(1);
    assert_eq!(mutator::workers::worker_count(10_000, None), want);
    assert_eq!(mutator::workers::worker_count(10_000, Some(10_000)), want);
    assert_eq!(mutator::workers::worker_count(1, None), 1);
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
