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

/// AC-1 and MU-12: coverage.toml mentions every help flag and has a covers string for each language.
#[test]
fn mu_ac_1_coverage_toml_covers_every_flag_and_table_row() {
    let coverage = coverage_toml();
    let flags = help_flags();
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
