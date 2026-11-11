// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! AC-9 (MU-10, MU-11) and MU-16, simulated.
//!
//! A fake `cargo` on a restricted PATH stands in for the toolchain. The PATH holds only the fake
//! `cargo`'s directory plus /usr/bin and /bin (for sh, git, grep), so cargo-llvm-cov, cargo-tarpaulin,
//! and rustup are not found. A real install on a host without cargo-llvm-cov is verified manually
//! (network needed).

mod common;

use common::{Out, git_commit_all, project, run_env, snapshots, write};
use mutator::edn::{self, Value};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

const CARGO_TOML: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
const LIB_RS: &str = "pub fn gt(a: i32, b: i32) -> bool { a > b }\n";
const INSTALL_NOTICE: &str =
    "Neither cargo-llvm-cov nor cargo-tarpaulin is installed. Installing cargo-llvm-cov.";
const INSTALL_ECHO: &str = "cargo install cargo-llvm-cov --locked";

// `{log}`, `{bin}`, and `{hits}` are replaced before the script is written.
// `install` drops a cargo-llvm-cov stub into the fake cargo's own dir.
// `llvm-cov --lcov --output-path F` writes an LCOV file for src/lib.rs with `DA:1,{hits}`.
const FAKE_CARGO: &str = r#"#!/bin/sh
echo "$*" >> "{log}"
case "$1" in
install)
  printf '#!/bin/sh\nexit 0\n' > "{bin}/cargo-llvm-cov"
  chmod +x "{bin}/cargo-llvm-cov"
  ;;
llvm-cov)
  while [ $# -gt 0 ]; do
    if [ "$1" = "--output-path" ]; then out="$2"; fi
    shift
  done
  printf 'SF:%s\nDA:1,{hits}\nend_of_record\n' "$(pwd -P)/src/lib.rs" > "$out"
  ;;
esac
exit 0
"#;

/// Runs `mutator --test-command true` in a demo crate with the fake cargo; `hits` is the LCOV hit
/// count for line 1. The returned dirs must stay alive while their files are read.
fn run_with_fake_cargo(hits: u8) -> (TempDir, TempDir, Out) {
    let proj = project();
    write(proj.path(), "Cargo.toml", CARGO_TOML);
    write(proj.path(), "src/lib.rs", LIB_RS);
    git_commit_all(proj.path());

    let tools = tempfile::tempdir().expect("tools dir");
    let bin = tools.path().display().to_string();
    let log = tools.path().join("cargo.log").display().to_string();
    let script = FAKE_CARGO
        .replace("{log}", &log)
        .replace("{bin}", &bin)
        .replace("{hits}", &hits.to_string());
    let cargo = tools.path().join("cargo");
    fs::write(&cargo, script).expect("write fake cargo");
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).expect("chmod fake cargo");

    let path = format!("{bin}:/usr/bin:/bin");
    let out = run_env(
        proj.path(),
        &["--test-command", "true"],
        &[("PATH", path.as_str())],
    );
    (proj, tools, out)
}

fn forms(snapshot: &Value) -> Vec<Value> {
    snapshot
        .get("forms")
        .and_then(|value| value.as_vec())
        .cloned()
        .unwrap_or_default()
}

/// Sums one per-form count (`uncovered` or `sites`) over every snapshot file.
fn form_total(snaps: &BTreeMap<String, String>, key: &str) -> i64 {
    snaps
        .values()
        .map(|text| edn::parse(text).expect("snapshot parses"))
        .flat_map(|snapshot| forms(&snapshot))
        .map(|form| form.get(key).and_then(|value| value.as_i64()).unwrap_or(0))
        .sum()
}

// AC-9, MU-10, MU-11: the missing tool is installed, the notice precedes the install echo on
// stderr, install runs before llvm-cov, and the LCOV hit for line 1 is read (no uncovered site).
#[test]
fn ac_9_mu_10_mu_11_missing_llvm_cov_is_installed_and_read() {
    let (proj, tools, out) = run_with_fake_cargo(1);

    let echo_line = out
        .stderr
        .lines()
        .find(|line| line.contains(INSTALL_ECHO))
        .expect("install command echoed on stderr");
    assert!(echo_line.starts_with("+ ("), "stderr: {}", out.stderr);
    let notice = out
        .stderr
        .find(INSTALL_NOTICE)
        .expect("install notice on stderr");
    let echo = out.stderr.find(echo_line).expect("echo position");
    assert!(
        notice < echo,
        "notice must precede the echo: {}",
        out.stderr
    );

    let log = fs::read_to_string(tools.path().join("cargo.log")).expect("fake cargo log");
    let install = log.find("install cargo-llvm-cov").expect("install logged");
    let llvm = log.find("llvm-cov --lcov").expect("llvm-cov logged");
    assert!(install < llvm, "install before llvm-cov: {log}");

    let snaps = snapshots(proj.path());
    assert!(!snaps.is_empty(), "a snapshot is written");
    assert_eq!(form_total(&snaps, "uncovered"), 0, "line 1 is covered");
    assert!(
        form_total(&snaps, "sites") > 0,
        "the site on line 1 is listed"
    );
    assert_eq!(
        out.code, 3,
        "the lone mutant survives under `true`: {}",
        out.stderr
    );
}

// MU-16: a line the LCOV report does not hit is an uncovered site; no mutant runs and exit is 0.
#[test]
fn mu_16_unhit_line_is_uncovered_and_no_mutant_runs() {
    let (proj, _tools, out) = run_with_fake_cargo(0);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let snaps = snapshots(proj.path());
    assert!(!snaps.is_empty(), "a snapshot is written");
    assert_eq!(form_total(&snaps, "uncovered"), 1, "line 1 is uncovered");
    let no_outcomes = snaps.values().all(|text| {
        let snapshot = edn::parse(text).expect("snapshot parses");
        matches!(snapshot.get("outcomes"), Some(Value::Map(pairs)) if pairs.is_empty())
    });
    assert!(no_outcomes, "no mutant ran, so no outcome is recorded");
}
