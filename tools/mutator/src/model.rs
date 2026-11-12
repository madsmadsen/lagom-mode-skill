// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Port of src/mutator/model.py.

use std::collections::{BTreeMap, BTreeSet, HashMap};

pub type LineSet = BTreeSet<usize>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    Clojure,
    Java,
    Go,
    TypeScript,
    Rust,
    Python,
}

impl Language {
    /// The lowercase language name used in reports and messages.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub namespace: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawSite {
    pub line: usize,
    pub anchor_line: usize,
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub mutant: String,
    pub category: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    pub namespace: String,
    pub form_id: String,
    pub name: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub mutant: String,
    pub category: String,
    pub mutation_id: String,
    pub anchor_line: usize,
}

impl Site {
    /// Human text for the change: `delete X` when there is no mutant, else `X -> Y`.
    pub fn description(&self) -> String {
        if self.mutant.is_empty() {
            format!("delete {}", self.original)
        } else {
            format!("{} -> {}", self.original, self.mutant)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Killed,
    Survived,
}

impl Status {
    /// The text stored in snapshots.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Killed => "killed",
            Status::Survived => "survived",
        }
    }

    /// Reads the snapshot text back; unknown text gives None.
    pub fn parse(text: &str) -> Option<Status> {
        match text {
            "killed" => Some(Status::Killed),
            "survived" => Some(Status::Survived),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormResult {
    pub id: String,
    pub namespace: String,
    pub name: String,
    pub private: bool,
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub digest: String,
    pub killed: usize,
    pub survived: usize,
    pub uncovered: usize,
    pub sites: usize,
}

impl FormResult {
    /// Killed share of executed mutants; None when nothing was executed.
    pub fn score(&self) -> Option<f64> {
        let executed = self.killed + self.survived;
        if executed == 0 {
            None
        } else {
            Some(self.killed as f64 / executed as f64)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriorForm {
    pub namespace: String,
    pub id: String,
    pub digest: String,
    pub file: String,
}

/// Previous results for one source file, used for differential runs.
#[derive(Clone, Debug, Default)]
pub struct History {
    /// Keyed by (namespace, form id).
    pub forms: HashMap<(String, String), PriorForm>,
    /// Keyed by mutation id.
    pub outcomes: BTreeMap<String, Status>,
}

#[derive(Clone, Debug, Default)]
pub struct RunResult {
    pub path: String,
    pub forms: Vec<FormResult>,
    pub written: Vec<String>,
    pub sites: Vec<Site>,
    /// Mutation id to "killed", "survived", or "uncovered".
    pub statuses: BTreeMap<String, String>,
    pub baseline_failed: bool,
    pub baseline_message: String,
    pub skipped: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(original: &str, mutant: &str) -> Site {
        Site {
            file: "a.py".into(),
            namespace: "demo".into(),
            form_id: "defn/x".into(),
            name: "x".into(),
            line: 1,
            start: 0,
            end: 1,
            original: original.into(),
            mutant: mutant.into(),
            category: "operator".into(),
            mutation_id: "id".into(),
            anchor_line: 1,
        }
    }

    fn form(killed: usize, survived: usize) -> FormResult {
        FormResult {
            id: "defn/x".into(),
            namespace: "demo".into(),
            name: "x".into(),
            private: false,
            file: "a.py".into(),
            line: 1,
            end_line: 3,
            digest: "d".into(),
            killed,
            survived,
            uncovered: 0,
            sites: killed + survived,
        }
    }

    #[test]
    // MU-9: a site with an empty mutant reads as a deletion.
    fn mu_9_site_description_delete_when_mutant_empty() {
        assert_eq!(site("!", "").description(), "delete !");
    }

    #[test]
    // MU-9: a site with a mutant reads as "original -> mutant".
    fn mu_9_site_description_arrow_when_mutant_present() {
        assert_eq!(site(">", ">=").description(), "> -> >=");
    }

    #[test]
    // MU-4: a form with nothing executed has no score.
    fn mu_4_score_is_none_when_nothing_executed() {
        assert_eq!(form(0, 0).score(), None);
    }

    #[test]
    // MU-4: a form score is killed over executed.
    fn mu_4_score_is_killed_share_of_executed() {
        assert_eq!(form(3, 1).score(), Some(0.75));
        assert_eq!(form(0, 2).score(), Some(0.0));
    }

    #[test]
    // MU-4: statuses use the snapshot words and parse back.
    fn mu_4_status_round_trips_snapshot_words() {
        assert_eq!(Status::Killed.as_str(), "killed");
        assert_eq!(Status::Survived.as_str(), "survived");
        assert_eq!(Status::parse("killed"), Some(Status::Killed));
        assert_eq!(Status::parse("survived"), Some(Status::Survived));
    }

    #[test]
    // MU-4: unknown status words do not parse.
    fn mu_4_status_parse_rejects_unknown_words() {
        assert_eq!(Status::parse("uncovered"), None);
        assert_eq!(Status::parse("Killed"), None);
        assert_eq!(Status::parse(""), None);
    }

    #[test]
    // MU-9: language names are the lowercase report names.
    fn mu_9_language_names_are_lowercase() {
        assert_eq!(Language::Clojure.name(), "clojure");
        assert_eq!(Language::Java.name(), "java");
        assert_eq!(Language::Go.name(), "go");
        assert_eq!(Language::TypeScript.name(), "typescript");
        assert_eq!(Language::Rust.name(), "rust");
        assert_eq!(Language::Python.name(), "python");
    }
}
