// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/crap.py

use std::cmp::Ordering;

use crate::model::{Entry, Function};

/// CRAP = CC² × (1 − coverage)³ + CC. `coverage_pct` is 0 to 100; `None` yields no score.
pub fn crap_score(complexity: u32, coverage_pct: Option<f64>) -> Option<f64> {
    let coverage_pct = coverage_pct?;
    let cc = f64::from(complexity);
    let uncovered = 1.0 - coverage_pct / 100.0;
    Some(cc * cc * uncovered * uncovered * uncovered + cc)
}

/// Python `round(x, 4)`. Formatting rounds the exact binary value half to even.
pub fn round_metric(value: Option<f64>) -> Option<f64> {
    value.map(|v| {
        format!("{v:.4}")
            .parse::<f64>()
            .expect("a formatted f64 always parses")
    })
}

pub fn make_entry(function: &Function, coverage: Option<f64>) -> Entry {
    let coverage = round_metric(coverage);
    Entry {
        name: function.name.clone(),
        namespace: function.namespace.clone(),
        complexity: function.complexity,
        coverage,
        crap: round_metric(crap_score(function.complexity, coverage)),
        path: function.path.clone(),
        language: function.language.clone(),
    }
}

/// Worst CRAP first. Unscored rows follow scored rows, most complex first.
/// Ties break by namespace, name, then path.
pub fn sort_entries(mut entries: Vec<Entry>) -> Vec<Entry> {
    entries.sort_by(compare_entries);
    entries
}

fn compare_entries(a: &Entry, b: &Entry) -> Ordering {
    let score_order = match (a.crap, b.crap) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b.complexity.cmp(&a.complexity),
    };
    score_order
        .then_with(|| a.namespace.cmp(&b.namespace))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.path.cmp(&b.path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(complexity: u32) -> Function {
        Function {
            name: "score".into(),
            namespace: "demo.core".into(),
            complexity,
            start_line: 1,
            end_line: 3,
            path: "src/demo/core.clj".into(),
            language: "clojure".into(),
            jacoco_class: None,
        }
    }

    fn row(name: &str, complexity: u32, coverage: Option<f64>, crap: Option<f64>) -> Entry {
        Entry {
            name: name.into(),
            namespace: "demo.core".into(),
            complexity,
            coverage,
            crap,
            path: "src/demo/core.clj".into(),
            language: "clojure".into(),
        }
    }

    // CR-10 AC-15: CC=1 cov 0 => 2.0
    #[test]
    fn cr_10_crap_of_one_uncovered_branch_is_two() {
        assert_eq!(crap_score(1, Some(0.0)), Some(2.0));
    }

    // CR-10 AC-15: CC=5 cov 50 => 8.125
    #[test]
    fn cr_10_crap_of_five_half_covered_is_8_125() {
        assert_eq!(crap_score(5, Some(50.0)), Some(8.125));
    }

    // CR-10 AC-15: CC=5 cov 100 => 5.0
    #[test]
    fn cr_10_crap_of_fully_covered_five_is_five() {
        assert_eq!(crap_score(5, Some(100.0)), Some(5.0));
    }

    // CR-10
    #[test]
    fn cr_10_crap_of_uncovered_five_is_thirty() {
        assert_eq!(crap_score(5, Some(0.0)), Some(30.0));
    }

    // CR-10
    #[test]
    fn cr_10_missing_coverage_has_no_crap_score() {
        assert_eq!(crap_score(3, None), None);
    }

    // CR-10
    #[test]
    fn cr_10_round_metric_keeps_four_decimals() {
        assert_eq!(round_metric(Some(1.23456)), Some(1.2346));
        assert_eq!(round_metric(None), None);
    }

    // CR-10
    #[test]
    fn cr_10_round_metric_breaks_exact_ties_to_even() {
        assert_eq!(round_metric(Some(0.03125)), Some(0.0312));
        assert_eq!(round_metric(Some(0.09375)), Some(0.0938));
    }

    // CR-10
    #[test]
    fn cr_10_make_entry_copies_identity_and_scores_five_half_covered() {
        let entry = make_entry(&function(5), Some(50.0));
        assert_eq!(entry.name, "score");
        assert_eq!(entry.namespace, "demo.core");
        assert_eq!(entry.language, "clojure");
        assert_eq!(entry.coverage, Some(50.0));
        assert_eq!(entry.crap, Some(8.125));
    }

    // CR-10
    #[test]
    fn cr_10_make_entry_without_coverage_is_unscored() {
        let entry = make_entry(&function(3), None);
        assert_eq!(entry.coverage, None);
        assert_eq!(entry.crap, None);
    }

    // CR-16
    #[test]
    fn cr_16_worst_score_first_and_unscored_last() {
        let rows = sort_entries(vec![
            row("missing", 1, None, None),
            row("mid", 5, Some(50.0), Some(5.0)),
            row("worst", 5, Some(0.0), Some(30.0)),
        ]);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["worst", "mid", "missing"]);
    }

    // CR-16
    #[test]
    fn cr_16_low_score_sorts_ahead_of_unscored_row_with_higher_complexity() {
        let rows = sort_entries(vec![
            row("large", 8, None, None),
            row("plain", 1, Some(100.0), Some(1.0)),
        ]);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["plain", "large"]);
    }

    // CR-16
    #[test]
    fn cr_16_unscored_rows_sort_by_complexity_descending() {
        let rows = sort_entries(vec![
            row("small", 1, None, None),
            row("large", 8, None, None),
        ]);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["large", "small"]);
    }

    // CR-16
    #[test]
    fn cr_16_equal_scores_break_ties_by_namespace_then_name_by_code_point() {
        let rows = sort_entries(vec![
            Entry {
                namespace: "b".into(),
                ..row("a", 1, Some(100.0), Some(1.0))
            },
            Entry {
                namespace: "a".into(),
                ..row("z", 1, Some(100.0), Some(1.0))
            },
            Entry {
                namespace: "a".into(),
                ..row("Y", 1, Some(100.0), Some(1.0))
            },
        ]);
        let keys: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.namespace.as_str(), r.name.as_str()))
            .collect();
        assert_eq!(keys, [("a", "Y"), ("a", "z"), ("b", "a")]);
    }
}
