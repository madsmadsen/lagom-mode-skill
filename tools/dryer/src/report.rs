// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// Ports dryer/src/dryer/report.py: text and EDN reports. The text layout matches dry4clj.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{Duplicate, Span};

pub fn format_text(candidates: &[Duplicate]) -> String {
    if candidates.is_empty() {
        return "No duplicate candidates found.\n".to_string();
    }
    let blocks: Vec<String> = candidates.iter().map(text_block).collect();
    blocks.join("\n\n") + "\n"
}

fn text_block(candidate: &Duplicate) -> String {
    let left = &candidate.left;
    let right = &candidate.right;
    format!(
        "DUPLICATE score={:.2}\n  {}:{}-{}\n  {}:{}-{}",
        candidate.score,
        left.file,
        left.start_line,
        left.end_line,
        right.file,
        right.start_line,
        right.end_line
    )
}

fn edn_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Python `_edn_float`: twelve decimals, trailing zeros and dot stripped, `.0` kept for whole numbers.
fn edn_float(value: f64) -> String {
    let mut text = format!("{value:.12}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

fn span_edn(span: &Span) -> String {
    format!(
        "{{:file {}, :start-line {}, :end-line {}}}",
        edn_string(&span.file),
        span.start_line,
        span.end_line
    )
}

fn candidate_edn(candidate: &Duplicate) -> String {
    [
        format!(" {{:score {}", edn_float(candidate.score)),
        format!("  :language {}", edn_string(candidate.language.name())),
        format!("  :left {}", span_edn(&candidate.left)),
        format!("  :right {}", span_edn(&candidate.right)),
        format!("  :left-nodes {}", candidate.left_nodes),
        format!("  :right-nodes {}}}", candidate.right_nodes),
    ]
    .join("\n")
}

pub fn render_edn(candidates: &[Duplicate]) -> String {
    if candidates.is_empty() {
        return "{:candidates []}\n".to_string();
    }
    let rows: Vec<String> = candidates.iter().map(candidate_edn).collect();
    format!("{{:candidates [\n{}\n]}}\n", rows.join("\n"))
}

pub fn metrics_path(root: &Path) -> PathBuf {
    root.join(".metrics").join("dry.edn")
}

pub fn write_metrics(candidates: &[Duplicate], root: &Path) -> io::Result<PathBuf> {
    let path = metrics_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, render_edn(candidates))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Language;

    fn span(file: &str, start_line: usize, end_line: usize) -> Span {
        Span {
            file: file.to_string(),
            start_line,
            end_line,
        }
    }

    fn scored(score: f64) -> Duplicate {
        Duplicate {
            score,
            language: Language::Python,
            left: span("a.py", 1, 4),
            right: span("b.py", 1, 4),
            left_nodes: 20,
            right_nodes: 20,
        }
    }

    fn invoice_receipt(score: f64) -> Duplicate {
        Duplicate {
            score,
            language: Language::Clojure,
            left: span("src/billing/invoice.clj", 3, 13),
            right: span("src/billing/receipt.clj", 3, 14),
            left_nodes: 158,
            right_nodes: 166,
        }
    }

    // DR-4: an empty report is the bare map.
    #[test]
    fn dr4_render_edn_empty_list_is_exact() {
        assert_eq!(render_edn(&[]), "{:candidates []}\n");
    }

    // DR-4: one candidate, laid out as dry4clj's EDN with the language key.
    #[test]
    fn dr4_render_edn_one_candidate_is_exact() {
        let expected = "{:candidates [\n\
             \x20{:score 0.890909090909\n\
             \x20 :language \"clojure\"\n\
             \x20 :left {:file \"src/billing/invoice.clj\", :start-line 3, :end-line 13}\n\
             \x20 :right {:file \"src/billing/receipt.clj\", :start-line 3, :end-line 14}\n\
             \x20 :left-nodes 158\n\
             \x20 :right-nodes 166}\n\
             ]}\n";
        assert_eq!(render_edn(&[invoice_receipt(0.890909090909)]), expected);
    }

    // DR-4: two candidates are separated by one newline, inside the same brackets.
    #[test]
    fn dr4_render_edn_two_candidates_are_exact() {
        let second = Duplicate {
            score: 0.5,
            language: Language::Python,
            left: span("a.py", 1, 4),
            right: span("b.py", 1, 4),
            left_nodes: 20,
            right_nodes: 21,
        };
        let expected = "{:candidates [\n\
             \x20{:score 0.890909090909\n\
             \x20 :language \"clojure\"\n\
             \x20 :left {:file \"src/billing/invoice.clj\", :start-line 3, :end-line 13}\n\
             \x20 :right {:file \"src/billing/receipt.clj\", :start-line 3, :end-line 14}\n\
             \x20 :left-nodes 158\n\
             \x20 :right-nodes 166}\n\
             \x20{:score 0.5\n\
             \x20 :language \"python\"\n\
             \x20 :left {:file \"a.py\", :start-line 1, :end-line 4}\n\
             \x20 :right {:file \"b.py\", :start-line 1, :end-line 4}\n\
             \x20 :left-nodes 20\n\
             \x20 :right-nodes 21}\n\
             ]}\n";
        assert_eq!(
            render_edn(&[invoice_receipt(0.890909090909), second]),
            expected
        );
    }

    // DR-4: floats print as dry4clj does: twelve places, trimmed, at least one decimal.
    #[test]
    fn dr4_edn_float_trims_to_the_python_form() {
        assert_eq!(edn_float(1.0), "1.0");
        assert_eq!(edn_float(0.890909090909), "0.890909090909");
        assert_eq!(edn_float(0.5), "0.5");
        assert_eq!(edn_float(0.0), "0.0");
    }

    // DR-4: strings escape backslash first, then double quote.
    #[test]
    fn dr4_edn_string_escapes_backslash_then_quote() {
        assert_eq!(edn_string("a\\b\"c"), "\"a\\\\b\\\"c\"");
    }

    // DR-12: text output has one block per candidate, separated by a blank line.
    #[test]
    fn dr12_format_text_joins_blocks_with_a_blank_line() {
        assert_eq!(format_text(&[]), "No duplicate candidates found.\n");
        let text = format_text(&[scored(0.5), scored(1.0)]);
        assert_eq!(
            text,
            "DUPLICATE score=0.50\n  a.py:1-4\n  b.py:1-4\n\nDUPLICATE score=1.00\n  a.py:1-4\n  b.py:1-4\n"
        );
    }

    // DR-12: the score prints with two places, round-half-even on the exact binary value.
    #[test]
    fn dr12_format_text_rounds_like_python_two_places() {
        assert!(format_text(&[scored(0.125)]).contains("score=0.12\n"));
        assert!(format_text(&[scored(0.375)]).contains("score=0.38\n"));
    }

    // DR-4: write_metrics creates .metrics/dry.edn and a second write replaces it.
    #[test]
    fn dr4_write_metrics_creates_and_replaces_the_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let path = write_metrics(&[scored(0.5)], root).expect("write");
        assert_eq!(path, root.join(".metrics").join("dry.edn"));
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            render_edn(&[scored(0.5)])
        );

        let again = write_metrics(&[], root).expect("write again");
        assert_eq!(again, path);
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            "{:candidates []}\n"
        );
    }

    // DR-NFR-1: rendering the same candidates twice gives identical bytes.
    #[test]
    fn dr_nfr1_two_renders_are_identical() {
        let items = vec![invoice_receipt(0.890909090909), scored(0.5)];
        assert_eq!(render_edn(&items), render_edn(&items));
        assert_eq!(format_text(&items), format_text(&items));
    }
}
