// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/report.py
// license: MIT

use std::collections::BTreeSet;

use crate::model::Entry;

const NAME_MIN: usize = 30;
const NAME_MAX: usize = 48;
const NAMESPACE_MIN: usize = 35;
const NAMESPACE_MAX: usize = 72;

/// Column width: at least `minimum`, grows with the longest value up to `maximum`.
/// Widths count characters, as Python `len` counts code points.
fn column_width(minimum: usize, maximum: usize, lengths: impl Iterator<Item = usize>) -> usize {
    lengths.fold(minimum, |width, length| width.max(length.min(maximum)))
}

/// Formats a present value with `format`, or the fixed `na` text when the value is missing.
fn format_optional(value: Option<f64>, na: &str, format: impl FnOnce(f64) -> String) -> String {
    value.map_or_else(|| na.to_string(), format)
}

fn format_coverage(coverage: Option<f64>) -> String {
    format_optional(coverage, "  N/A ", |value| format!("{value:5.1}%"))
}

fn format_crap(score: Option<f64>) -> String {
    format_optional(score, "     N/A", |value| format!("{value:8.1}"))
}

fn format_row(entry: &Entry, name_width: usize, namespace_width: usize) -> String {
    format!(
        "{:<name_width$} {:<namespace_width$} {:>4} {} {}",
        entry.name,
        entry.namespace,
        entry.complexity,
        format_coverage(entry.coverage),
        format_crap(entry.crap),
    )
}

fn language_list(entries: &[Entry]) -> String {
    let languages: BTreeSet<&str> = entries.iter().map(|e| e.language.as_str()).collect();
    languages.into_iter().collect::<Vec<_>>().join(", ")
}

pub fn format_report(entries: &[Entry]) -> String {
    let name_width = column_width(
        NAME_MIN,
        NAME_MAX,
        entries.iter().map(|e| e.name.chars().count()),
    );
    let namespace_width = column_width(
        NAMESPACE_MIN,
        NAMESPACE_MAX,
        entries.iter().map(|e| e.namespace.chars().count()),
    );
    let header = format!(
        "{:<name_width$} {:<namespace_width$} {:>4} {:>7} {:>8}",
        "Function", "Namespace", "CC", "Cov%", "CRAP",
    );
    let separator = "-".repeat(header.chars().count());
    let languages = language_list(entries);

    let mut lines = vec!["CRAP Report".to_string(), "===========".to_string()];
    if !languages.is_empty() {
        lines.push(format!("Languages: {languages}"));
        lines.push(String::new());
    }
    lines.push(header);
    lines.push(separator);
    lines.extend(
        entries
            .iter()
            .map(|entry| format_row(entry, name_width, namespace_width)),
    );
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        name: &str,
        namespace: &str,
        language: &str,
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
            language: language.into(),
        }
    }

    fn tokens(line: &str) -> Vec<&str> {
        line.split_whitespace().collect()
    }

    // CR-16
    #[test]
    fn cr_16_scored_row_is_printed_under_its_header() {
        let text = format_report(&[entry(
            "score",
            "demo.core",
            "clojure",
            1,
            Some(100.0),
            Some(1.0),
        )]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0], "CRAP Report");
        assert_eq!(lines[1], "===========");
        assert_eq!(lines[2], "Languages: clojure");
        assert_eq!(lines[3], "");
        assert_eq!(
            tokens(lines[4]),
            ["Function", "Namespace", "CC", "Cov%", "CRAP"]
        );
        assert_eq!(lines[5], "-".repeat(88));
        assert_eq!(
            tokens(lines[6]),
            ["score", "demo.core", "1", "100.0%", "1.0"]
        );
        assert!(lines[6].ends_with("     1.0"));
        assert!(text.ends_with("\n"));
    }

    // CR-16
    #[test]
    fn cr_16_missing_coverage_and_crap_print_na() {
        let text = format_report(&[entry("missing", "demo.core", "clojure", 3, None, None)]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            tokens(lines[6]),
            ["missing", "demo.core", "3", "N/A", "N/A"]
        );
        assert!(lines[6].ends_with("     N/A"));
    }

    // CR-16
    #[test]
    fn cr_16_empty_list_prints_title_and_header_only() {
        let text = format_report(&[]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "CRAP Report");
        assert_eq!(lines[1], "===========");
        assert_eq!(
            tokens(lines[2]),
            ["Function", "Namespace", "CC", "Cov%", "CRAP"]
        );
        assert_eq!(lines[3], "-".repeat(88));
        assert!(text.ends_with("\n"));
    }

    // CR-16
    #[test]
    fn cr_16_languages_are_listed_sorted_and_unique() {
        let text = format_report(&[
            entry("a", "demo.core", "python", 1, Some(100.0), Some(1.0)),
            entry("b", "demo.core", "clojure", 1, Some(100.0), Some(1.0)),
            entry("c", "demo.core", "python", 1, Some(100.0), Some(1.0)),
        ]);
        assert_eq!(text.lines().nth(2), Some("Languages: clojure, python"));
    }

    // CR-16
    #[test]
    fn cr_16_name_column_grows_to_longest_name() {
        let long_name = "x".repeat(40);
        let text = format_report(&[entry(&long_name, "demo.core", "clojure", 1, None, None)]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[4].chars().count(), 98);
        assert_eq!(lines[5], "-".repeat(98));
    }
}
