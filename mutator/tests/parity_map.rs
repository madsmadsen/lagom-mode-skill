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

/// The fix id named by a comment, from the token after `Fix ` up to the next space.
fn named_fix(comment: &str) -> Option<&str> {
    if !comment.starts_with('#') {
        return None;
    }
    let (_, rest) = comment.split_once("Fix ")?;
    rest.split(' ').next()
}

/// MU-PB-3: every `allow_stdout_diff = true` run names, in the comment above it, a fix in fixes.toml.
#[test]
fn mu_pb_3_allow_stdout_diff_names_a_fix_in_fixes_toml() {
    let coverage = coverage_toml();
    let fixes = fixes_toml();
    let lines: Vec<&str> = coverage.lines().collect();
    let mut checked = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "allow_stdout_diff = true" {
            continue;
        }
        checked += 1;
        let comment = lines[..i]
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map_or("", |l| l.trim());
        let id = named_fix(comment).unwrap_or_else(|| {
            panic!(
                "line {} allows a stdout diff but the comment above names no fix: {comment:?}",
                i + 1
            )
        });
        let entry = format!("id = \"{id}\"");
        assert!(
            fixes.lines().any(|l| l.trim() == entry),
            "parity/fixes.toml has no {entry}, named above line {}",
            i + 1
        );
    }
    assert!(
        checked > 0,
        "parity/coverage.toml has no allow_stdout_diff line"
    );
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
