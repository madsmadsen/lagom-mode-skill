// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Checks `parity/coverage.toml` against the CLI help (AC-1) and the README table
//! rows, one per language (MU-12).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// The six languages named in the README table.
const LANGUAGES: [&str; 6] = ["clojure", "java", "go", "typescript", "rust", "python"];

fn coverage_toml() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parity/coverage.toml");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The fixes registry, which names every difference from the original (MU-PB-3).
fn fixes_toml() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parity/fixes.toml");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Names of the `[[run]]` blocks in a coverage.toml text that set
/// `allow_stdout_diff = true`. A run must not skip the stdout check (MU-PB-6).
fn suppressing_runs(coverage: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in coverage.lines() {
        let line = line.trim();
        if line == "[[run]]" {
            current = Some(String::new());
        } else if let Some(name) = current.as_mut() {
            if let Some(rest) = line.strip_prefix("name = \"") {
                *name = rest.trim_end_matches('"').to_string();
            } else if line == "allow_stdout_diff = true" {
                if name.is_empty() {
                    *name = "?".to_string();
                }
                names.push(name.clone());
            }
        }
    }
    names
}

/// MU-PB-6: no run of coverage.toml skips the stdout check. A stdout difference needs
/// an approved fix, so a run that allows it on its own would hide the difference.
#[test]
fn mu_pb_6_no_run_skips_the_stdout_check() {
    let coverage = coverage_toml();
    assert_eq!(suppressing_runs(&coverage), Vec::<String>::new());

    // The check above is not vacuous: the helper flags a run that sets the field.
    let sample = "[[run]]\nname = \"sample\"\nallow_stdout_diff = true\n";
    assert_eq!(suppressing_runs(sample), vec!["sample".to_string()]);
    let clean = "[[run]]\nname = \"sample\"\nargs = [\"-h\"]\n";
    assert_eq!(suppressing_runs(clean), Vec::<String>::new());
}

/// The flags in the Options block of `--help`: `-h`, `-s`, and every `--name`.
fn help_flags() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_mutator"))
        .arg("--help")
        .output()
        .expect("running mutator --help");
    assert!(out.status.success(), "mutator --help exited {}", out.status);
    let help = String::from_utf8(out.stdout).expect("help is UTF-8");
    let mut flags = Vec::new();
    let mut in_options = false;
    for line in help.lines() {
        if line == "Options:" {
            in_options = true;
        } else if line.is_empty() {
            in_options = false;
        } else if in_options && line.starts_with("  -") {
            // The flag column ends at the first run of two spaces.
            let spec = line[2..].split("  ").next().unwrap_or("");
            flags.extend(
                spec.split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|t| t.starts_with("--") || *t == "-h" || *t == "-s")
                    .map(str::to_string),
            );
        }
    }
    flags
}

/// Flags that the original does not have. Each one has a fix in fixes.toml (MU-PB-3), and a
/// parity run cannot pass on it, so coverage.toml does not run it.
const PORT_ONLY_FLAGS: [(&str, &str); 1] = [("--max-memory-mb", "max-memory-limit")];

/// AC-1 and MU-12: coverage.toml mentions every help flag and has a covers string for each language.
/// MU-25: a flag that only the port has is named by a fix in fixes.toml instead.
#[test]
fn mu_ac_1_coverage_toml_covers_every_flag_and_table_row() {
    let coverage = coverage_toml();
    let fixes = fixes_toml();
    let flags = help_flags();
    for (flag, id) in PORT_ONLY_FLAGS {
        assert!(flags.iter().any(|f| f == flag), "help lists no {flag}");
        let entry = format!("id = \"{id}\"");
        assert!(
            fixes.lines().any(|l| l.trim() == entry),
            "parity/fixes.toml has no {entry} for {flag}"
        );
    }
    let flags: Vec<String> = flags
        .into_iter()
        .filter(|f| PORT_ONLY_FLAGS.iter().all(|(flag, _)| flag != f))
        .collect();
    assert!(
        flags.iter().any(|f| f == "--help"),
        "help lists no --help: {flags:?}"
    );
    for flag in &flags {
        assert!(
            coverage.contains(flag.as_str()),
            "parity/coverage.toml does not cover {flag}"
        );
    }
    for language in LANGUAGES {
        let prefix = format!("\"{language}:");
        assert!(
            coverage.contains(&prefix),
            "parity/coverage.toml has no covers string for {language}"
        );
    }
}
