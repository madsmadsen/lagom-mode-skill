// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: none

// Offline checks over the repository files: license, upstream headers, parity
// pin, last parity run, fixes, fixtures, and coverage mapping.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const LANGUAGES: &[(&str, &str)] = &[
    ("clojure", "clj"),
    ("java", "java"),
    ("go", "go"),
    ("typescript", "ts"),
    ("rust", "rs"),
    ("python", "py"),
];
const REPORT_DIRS: &[&str] = &["target", "coverage", "htmlcov", "jacoco"];
const APPROVER: &str = "Mads Madsen";
const HEADER_COPYRIGHT: &str = "// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>";
const HEADER_LICENSE: &str = "// SPDX-License-Identifier: MIT";
const HEADER_REPO: &str = "// upstream: https://github.com/unclebob/crapper";
const HEADER_COMMIT_PREFIX: &str = "// upstream-commit: ";
const HEADER_PATH_PREFIX: &str = "// upstream-path: ";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn load_toml(rel: &str) -> toml::Table {
    toml::from_str(&read(rel)).unwrap_or_else(|e| panic!("parse {rel}: {e}"))
}

fn str_field<'a>(t: &'a toml::Table, key: &str) -> &'a str {
    t.get(key).and_then(toml::Value::as_str).unwrap_or("")
}

fn sub<'a>(t: &'a toml::Table, key: &str) -> Option<&'a toml::Table> {
    t.get(key).and_then(toml::Value::as_table)
}

fn tables<'a>(t: &'a toml::Table, key: &str) -> Vec<&'a toml::Table> {
    t.get(key)
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_table).collect())
        .unwrap_or_default()
}

fn strings(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn upstream_commit() -> String {
    str_field(&load_toml("parity/upstream.toml"), "commit").to_owned()
}

/// Collects files under `dir`, skipping any directory whose name is in `skip`.
fn walk(dir: &Path, skip: &[&str], out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !skip.contains(&name) {
                walk(&path, skip, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn rs_files(dirs: &[&str]) -> Vec<PathBuf> {
    let mut all = Vec::new();
    for dir in dirs {
        walk(&root().join(dir), &[], &mut all);
    }
    all.retain(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"));
    all
}

fn header_ok(text: &str, commit: &str) -> bool {
    let lines: Vec<&str> = text.lines().take(5).collect();
    match lines.as_slice() {
        [copyright, license, repo, pin, path] => {
            *copyright == HEADER_COPYRIGHT
                && *license == HEADER_LICENSE
                && *repo == HEADER_REPO
                && pin.strip_prefix(HEADER_COMMIT_PREFIX) == Some(commit)
                && path
                    .strip_prefix(HEADER_PATH_PREFIX)
                    .is_some_and(|v| !v.trim().is_empty())
        }
        _ => false,
    }
}

#[test]
// AC-20, CR-NFR-5: MIT under the copyright of Mads Madsen, with credit to the original
fn ac_20_license_is_mit() {
    let license = read("LICENSE");
    assert!(license.starts_with("MIT License"));
    assert!(license.contains("Copyright (c) 2026 Mads Madsen <mads@onmadsen.com>"));
    assert!(license.contains("based on crapper by Robert C. Martin"));
}

#[test]
// AC-20, CR-NFR-5: every .rs file carries the upstream header with the pinned commit
fn ac_20_every_rs_file_has_upstream_header() {
    let commit = upstream_commit();
    let files = rs_files(&["src", "tests"]);
    assert!(!files.is_empty(), "no .rs files found under src/ or tests/");
    let violations: Vec<String> = files
        .iter()
        .filter(|p| !header_ok(&fs::read_to_string(p).unwrap_or_default(), &commit))
        .map(|p| {
            p.strip_prefix(root())
                .unwrap_or(p.as_path())
                .display()
                .to_string()
        })
        .collect();
    assert!(
        violations.is_empty(),
        "files without the upstream header:\n{}",
        violations.join("\n")
    );
}

#[test]
// AC-21: the upstream pin names the repository, a 40-hex commit, and both Python versions
fn ac_21_upstream_pin_complete() {
    let pin = load_toml("parity/upstream.toml");
    assert!(
        !str_field(&pin, "repository").is_empty(),
        "repository is empty"
    );
    let commit = str_field(&pin, "commit");
    assert!(
        commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit()),
        "commit is not 40 hex chars: {commit:?}"
    );
    let deps = sub(&pin, "python-dependencies");
    for dep in ["tree-sitter", "tree-sitter-language-pack"] {
        let version = deps
            .and_then(|d| d.get(dep))
            .and_then(toml::Value::as_str)
            .unwrap_or("");
        assert!(!version.is_empty(), "no version for {dep}");
    }
}

#[test]
// AC-22: the last parity run is for the pinned commit and passed
fn ac_22_last_run_matches_pin_and_passes() {
    let last = load_toml("parity/last-run.toml");
    let commit = upstream_commit();
    assert_eq!(str_field(&last, "commit"), commit);
    assert_eq!(str_field(&last, "result"), "pass");
}

#[test]
// AC-23: each approved-difference entry names a difference, behavior, and an existing fixture
fn ac_23_fixes_entries_complete() {
    let fixes = load_toml("parity/fixes.toml");
    for fix in tables(&fixes, "fix") {
        assert!(
            fix.contains_key("issue") || fix.contains_key("description"),
            "fix lacks issue or description: {fix:?}"
        );
        assert!(fix.contains_key("behavior"), "fix lacks behavior: {fix:?}");
        let fixture = str_field(fix, "fixture");
        assert!(
            !fixture.is_empty() && root().join("parity/fixtures").join(fixture).is_dir(),
            "fix names no existing fixture dir: {fix:?}"
        );
    }
}

// AC-24: every entry is approved by Mads Madsen. The new upstream-fix entries wait for his approval, so
// `cargo test` skips this check. Run it with `cargo test --test meta -- --ignored`, or with
// `python3.12 parity/run.py --check-meta`, which fails until he approves them.
#[test]
#[ignore = "entries wait for the approval of Mads Madsen (CR-PB-6)"]
fn ac_24_fixes_entries_approved() {
    let fixes = load_toml("parity/fixes.toml");
    for fix in tables(&fixes, "fix") {
        assert_eq!(
            str_field(fix, "approved_by"),
            APPROVER,
            "fix not approved: {fix:?}"
        );
    }
}

#[test]
// AC-24: an entry has no approved_by, or has the one of Mads Madsen. Nobody else approves.
fn ac_24_fixes_entries_name_no_other_approver() {
    let fixes = load_toml("parity/fixes.toml");
    for fix in tables(&fixes, "fix") {
        if let Some(approver) = fix.get("approved_by") {
            assert_eq!(approver.as_str(), Some(APPROVER), "wrong approver: {fix:?}");
        }
    }
}

#[test]
// AC-2: each language fixture has at least one source file of its extension outside report dirs
fn ac_2_fixture_for_each_language() {
    for &(language, ext) in LANGUAGES {
        let dir = root().join("parity/fixtures").join(language);
        let mut files = Vec::new();
        walk(&dir, REPORT_DIRS, &mut files);
        assert!(
            files
                .iter()
                .any(|p| p.extension().and_then(|e| e.to_str()) == Some(ext)),
            "fixture {language} has no .{ext} source file outside report dirs"
        );
    }
}

#[test]
// AC-1: every required coverage item is covered by a run; run ids are unique; fixtures exist
fn ac_1_coverage_toml_maps_every_required_item() {
    let cov = load_toml("parity/coverage.toml");
    let required = strings(sub(&cov, "required").and_then(|r| r.get("items")));
    let runs = tables(&cov, "run");
    let mut seen = BTreeSet::new();
    let mut covered = Vec::new();
    for run in &runs {
        let id = str_field(run, "id");
        assert!(seen.insert(id), "duplicate run id {id}");
        let fixture = str_field(run, "fixture");
        assert!(
            root().join("parity/fixtures").join(fixture).is_dir(),
            "run {id} names missing fixture {fixture:?}"
        );
        covered.extend(strings(run.get("covers")));
    }
    for item in required {
        assert!(
            covered.contains(&item),
            "required item {item} is covered by no run"
        );
    }
}

#[test]
// AC-12: every fix with an id has a test of that name in tests/fixes.rs
fn ac_12_every_fix_has_a_named_test() {
    let fixes = load_toml("parity/fixes.toml");
    let ids: Vec<&str> = tables(&fixes, "fix")
        .into_iter()
        .filter_map(|f| f.get("id").and_then(toml::Value::as_str))
        .collect();
    let source = fs::read_to_string(root().join("tests/fixes.rs")).unwrap_or_default();
    for id in ids {
        assert!(source.contains(id), "tests/fixes.rs has no test named {id}");
    }
}
