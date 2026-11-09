//! Checks the parity records: `parity/fixes.toml` (MU-PB-3, MU-PB-4, AC-12) and
//! `parity/upstream.toml` (MU-PB-1). The crate has no TOML dependency, so the files
//! are read with plain string matching.

use std::fs;
use std::path::PathBuf;

/// One `[[fix]]` block: the keys it sets, and its `test` value if it has one.
#[derive(Default)]
struct FixBlock {
    keys: Vec<String>,
    test: Option<String>,
}

fn parity_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("parity")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
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
    if key == "test" {
        block.test = quoted_value(line, "test").map(str::to_string);
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
