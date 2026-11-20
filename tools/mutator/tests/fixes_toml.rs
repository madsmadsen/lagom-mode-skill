// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Checks the parity records: `parity/fixes.toml` (MU-PB-3, MU-PB-4, AC-12) and
//! `parity/upstream.toml` (MU-PB-1). The crate has no TOML dependency, so the files
//! are read with plain string matching.

mod common;

use common::parity_file;
use std::fs;
use std::path::{Path, PathBuf};

/// One `[[fix]]` block: the keys it sets, and its `fixture` and `test` values.
#[derive(Default)]
struct FixBlock {
    keys: Vec<String>,
    fixture: Option<String>,
    test: Option<String>,
}

/// The value of `key = "value"` on one line, without the quotes.
fn quoted_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.strip_prefix(key)?
        .strip_prefix(" = \"")?
        .strip_suffix('"')
}

fn record_key(block: &mut FixBlock, line: &str) {
    let Some((key, _)) = line.split_once(" = ") else {
        return;
    };
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        return;
    }
    block.keys.push(key.to_string());
    match key {
        "fixture" => block.fixture = quoted_value(line, "fixture").map(str::to_string),
        "test" => block.test = quoted_value(line, "test").map(str::to_string),
        _ => {}
    }
}

/// Splits fixes.toml into its `[[fix]]` blocks. Lines inside a `"""` string are skipped.
fn fix_blocks(text: &str) -> Vec<FixBlock> {
    let mut blocks: Vec<FixBlock> = Vec::new();
    let mut in_string = false;
    for line in text.lines() {
        if !in_string {
            if line.trim() == "[[fix]]" {
                blocks.push(FixBlock::default());
            } else if let Some(block) = blocks.last_mut() {
                record_key(block, line);
            }
        }
        if line.matches("\"\"\"").count() % 2 == 1 {
            in_string = !in_string;
        }
    }
    blocks
}

/// AC-12, MU-PB-4: every fix names a test file under tests/ that contains the named fn.
#[test]
fn mu_pb_4_every_fix_names_a_test_that_exists() {
    let blocks = fix_blocks(&parity_file("fixes.toml"));
    assert!(!blocks.is_empty(), "fixes.toml has no [[fix]] entries");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for block in &blocks {
        let test = block
            .test
            .as_deref()
            .unwrap_or_else(|| panic!("[[fix]] has no test: {:?}", block.keys));
        let (file, name) = test
            .split_once("::")
            .unwrap_or_else(|| panic!("test is not FILE::FN: {test}"));
        let source = fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(
            source.contains(&format!("fn {name}(")),
            "{file} has no fn {name}("
        );
    }
}

/// MU-PB-3: every [[fix]] block records its id, requirement, description, changed
/// behavior, fixture, and test.
#[test]
fn mu_pb_3_every_fix_block_has_the_required_keys() {
    const REQUIRED: [&str; 6] = [
        "id",
        "requirement",
        "description",
        "changed_behavior",
        "fixture",
        "test",
    ];
    let blocks = fix_blocks(&parity_file("fixes.toml"));
    assert!(!blocks.is_empty(), "fixes.toml has no [[fix]] entries");
    for block in &blocks {
        for key in REQUIRED {
            assert!(
                block.keys.iter().any(|k| k.as_str() == key),
                "[[fix]] {:?} has no {key}",
                block.keys
            );
        }
    }
}

/// The path of a test file that a fix `fixture` value names in place of a parity fixture:
/// the part before the ` (note)`. `None` when the value names a parity fixture directory.
fn test_file_of(value: &str) -> Option<&str> {
    let path = value.split_once(" (").map_or(value, |(before, _)| before);
    path.starts_with("tests/").then_some(path)
}

/// The fixture problems of a fixes.toml text: a test file that a `fixture` value names must
/// exist below `root`, and it must hold the fn of the `test` value. One message per problem.
fn fixture_problems(text: &str, root: &Path) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    for block in fix_blocks(text) {
        let Some(value) = block.fixture.as_deref() else {
            problems.push(format!("[[fix]] {:?} has no fixture", block.keys));
            continue;
        };
        let Some(path) = test_file_of(value) else {
            continue;
        };
        let Ok(source) = fs::read_to_string(root.join(path)) else {
            problems.push(format!("fixture {path} of {:?} does not exist", block.keys));
            continue;
        };
        let name = block.test.as_deref().and_then(|test| test.split_once("::"));
        if let Some((_, name)) = name
            && !source.contains(&format!("fn {name}("))
        {
            problems.push(format!("fixture {path} has no fn {name}("));
        }
    }
    problems
}

/// MU-PB-3, MU-PB-4: a fix that names a test file in place of a parity fixture names a file
/// that exists and that holds the fn of its `test` value. The same check on a bad text fails.
#[test]
fn mu_pb_3_a_fixture_that_names_a_test_file_exists() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let text = parity_file("fixes.toml");
    assert_eq!(fixture_problems(&text, &root), Vec::<String>::new());
    let named = fix_blocks(&text)
        .iter()
        .filter_map(|block| block.fixture.as_deref())
        .filter_map(test_file_of)
        .count();
    assert!(named > 0, "no fix names a test file as its fixture");

    // The check is not vacuous: a missing file, and a file without the named fn, each fail.
    let missing = "[[fix]]\nid = \"x\"\nfixture = \"tests/gone.rs (inline)\"\n\
                   test = \"tests/memory_limit.rs::mu_25_ac17_a_bad_value_exits_one\"\n";
    assert_eq!(
        fixture_problems(missing, &root),
        ["fixture tests/gone.rs of [\"id\", \"fixture\", \"test\"] does not exist".to_string()]
    );
    let no_fn = "[[fix]]\nid = \"x\"\nfixture = \"tests/memory_limit.rs (inline)\"\n\
                 test = \"tests/memory_limit.rs::mu_25_no_such_fn\"\n";
    assert_eq!(
        fixture_problems(no_fn, &root),
        ["fixture tests/memory_limit.rs has no fn mu_25_no_such_fn(".to_string()]
    );
}

/// MU-PB-1: upstream.toml pins a full 40-character hex commit for the original and for crapper.
#[test]
fn mu_pb_1_upstream_toml_pins_both_commits() {
    let text = parity_file("upstream.toml");
    let mut section = String::new();
    let mut pins: Vec<(String, String)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            section = line.to_string();
        } else if let Some(commit) = quoted_value(line, "commit") {
            pins.push((section.clone(), commit.to_string()));
        }
    }
    for want in ["[original]", "[crapper]"] {
        let commit = pins
            .iter()
            .find(|(s, _)| s == want)
            .map(|(_, c)| c.as_str())
            .unwrap_or_else(|| panic!("{want} has no commit line"));
        assert_eq!(
            commit.len(),
            40,
            "{want} commit is not 40 characters: {commit}"
        );
        assert!(
            commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "{want} commit is not hex: {commit}"
        );
    }
}
