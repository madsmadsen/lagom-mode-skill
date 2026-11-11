// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// Ports the data shapes shared across the Python modules in src/dryer/*.py.

/// A source language that dryer can analyze.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Clojure,
    Java,
    Go,
    TypeScript,
    Rust,
    Python,
}

impl Language {
    /// The lowercase language name used in reports.
    pub fn name(self) -> &'static str {
        match self {
            Language::Clojure => "clojure",
            Language::Java => "java",
            Language::Go => "go",
            Language::TypeScript => "typescript",
            Language::Rust => "rust",
            Language::Python => "python",
        }
    }
}

/// A located region of a source file, with 1-based inclusive line numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// A pair of spans whose normalized forms are similar.
#[derive(Clone, Debug, PartialEq)]
pub struct Duplicate {
    pub score: f64,
    pub language: Language,
    pub left: Span,
    pub right: Span,
    pub left_nodes: usize,
    pub right_nodes: usize,
}

/// A normalized form ready to compare. `fingerprints` is sorted, deduplicated interned ids.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub language: Language,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub nodes: usize,
    pub fingerprints: Vec<u32>,
}

/// Normalized tree. Python: K(name) -> Kw, str -> Str, list -> List.
#[derive(Clone, Debug, PartialEq)]
pub enum Norm {
    Kw(&'static str),
    Str(String),
    List(Vec<Norm>),
}

#[cfg(test)]
mod tests {
    use super::Language;

    // DR-5, DR-8: each language reports its lowercase name.
    #[test]
    fn dr5_dr8_language_names_are_lowercase_identifiers() {
        assert_eq!(Language::Clojure.name(), "clojure");
        assert_eq!(Language::Java.name(), "java");
        assert_eq!(Language::Go.name(), "go");
        assert_eq!(Language::TypeScript.name(), "typescript");
        assert_eq!(Language::Rust.name(), "rust");
        assert_eq!(Language::Python.name(), "python");
    }
}
