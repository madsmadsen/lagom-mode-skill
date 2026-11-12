// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/metrics.py

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::Entry;

fn edn_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Four decimals, trailing zeros trimmed, always at least one decimal digit.
fn edn_number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.contains('.') {
        text.to_string()
    } else {
        format!("{text}.0")
    }
}

fn edn_value(value: Option<f64>) -> String {
    match value {
        None => "nil".to_string(),
        Some(number) => edn_number(number),
    }
}

fn render_row(entry: &Entry) -> String {
    format!(
        "  {{:name {}, :namespace {}, :complexity {}, :coverage {}, :crap {}}}",
        edn_string(&entry.name),
        edn_string(&entry.namespace),
        entry.complexity,
        edn_value(entry.coverage),
        edn_value(entry.crap),
    )
}

/// EDN map `{:entries [...]}` with crap4clj's keys, in crap4clj's order.
pub fn render_edn(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return "{:entries []}\n".to_string();
    }
    let rows: Vec<String> = entries.iter().map(render_row).collect();
    format!("{{:entries [\n{}\n]}}\n", rows.join("\n"))
}

/// Writes `root/.metrics/crap.edn`, replacing any previous snapshot.
pub fn write_metrics(entries: &[Entry], root: &Path) -> io::Result<PathBuf> {
    let dir = root.join(".metrics");
    fs::create_dir_all(&dir)?;
    let path = dir.join("crap.edn");
    fs::write(&path, render_edn(entries))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        name: &str,
        namespace: &str,
        complexity: u32,
        coverage: Option<f64>,
        crap: Option<f64>,
    ) -> Entry {
        Entry {
            name: name.into(),
            namespace: namespace.into(),
            complexity,
            coverage,
            crap,
            path: "src/demo/core.clj".into(),
            language: "clojure".into(),
        }
    }

    // CR-4
    #[test]
    fn cr_4_edn_snapshot_has_crap4clj_keys_and_order() {
        let text = render_edn(&[entry(
            "layout",
            "uml-viewer.layout",
            8,
            Some(82.0),
            Some(3.4),
        )]);
        assert_eq!(
            text,
            "{:entries [\n  {:name \"layout\", :namespace \"uml-viewer.layout\", :complexity 8, :coverage 82.0, :crap 3.4}\n]}\n"
        );
    }

    // CR-4
    #[test]
    fn cr_4_edn_of_no_entries_is_an_empty_vector() {
        assert_eq!(render_edn(&[]), "{:entries []}\n");
    }

    // CR-4
    #[test]
    fn cr_4_edn_writes_nil_for_missing_coverage_and_crap() {
        let text = render_edn(&[entry("score", "demo.core", 3, None, None)]);
        assert!(text.contains(":coverage nil, :crap nil}"));
    }

    // CR-4
    #[test]
    fn cr_4_edn_numbers_keep_one_decimal_and_trim_extra_zeros() {
        let text = render_edn(&[entry("score", "demo.core", 1, Some(100.0), Some(33.333_33))]);
        assert!(text.contains(":coverage 100.0, :crap 33.3333}"));
    }

    // CR-4
    #[test]
    fn cr_4_edn_zero_and_short_fractions_render_as_written() {
        let text = render_edn(&[entry("score", "demo.core", 5, Some(0.0), Some(8.125))]);
        assert!(text.contains(":coverage 0.0, :crap 8.125}"));
    }

    // CR-4
    #[test]
    fn cr_4_edn_strings_escape_backslash_and_quote() {
        let text = render_edn(&[entry("a\"b\\c", "d", 1, None, None)]);
        assert!(text.contains(":name \"a\\\"b\\\\c\", :namespace \"d\""));
    }

    // CR-4
    #[test]
    fn cr_4_write_metrics_twice_leaves_the_same_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rows = [entry("score", "demo.core", 1, Some(100.0), Some(1.0))];
        let first = write_metrics(&rows, dir.path()).expect("first write");
        let second = write_metrics(&rows, dir.path()).expect("second write");
        assert_eq!(first, second);
        assert_eq!(first, dir.path().join(".metrics").join("crap.edn"));
        assert_eq!(
            fs::read_to_string(&first).expect("read snapshot"),
            render_edn(&rows)
        );
    }
}
