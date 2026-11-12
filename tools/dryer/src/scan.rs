// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// Ports dryer/src/dryer/scan.py: read source files and pair the forms that clear the threshold.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::discover;
use crate::extract;
use crate::model::{Duplicate, Entry, Span};
use crate::shape::{jaccard, Interner};

/// Resolved `path` relative to resolved `root`, in posix form. Outside `root`, the resolved posix path.
pub fn display_path(path: &Path, root: &Path) -> String {
    let resolved = discover::resolve(path);
    match resolved.strip_prefix(discover::resolve(root)) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_string(),
        Ok(relative) => discover::posix(relative),
        Err(_) => discover::posix(&resolved),
    }
}

/// Python universal-newline translation: `\r\n` and lone `\r` become `\n`.
fn universal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Python `_keep`: `end - start + 1 >= min_lines`, rearranged so it cannot underflow.
fn keep(entry: &Entry, min_lines: usize, min_nodes: usize) -> bool {
    entry.end_line + 1 >= entry.start_line + min_lines && entry.nodes >= min_nodes
}

fn read_source(path: &Path, display: &str) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(universal_newlines(&String::from_utf8_lossy(&bytes))),
        Err(err) => Err(format!("{display}: {err}")),
    }
}

fn scan_one(
    path: &Path,
    root: &Path,
    min_lines: usize,
    min_nodes: usize,
    interner: &mut Interner,
    found: &mut (Vec<Entry>, Vec<String>),
) {
    let Some(language) = discover::language_of(path) else {
        return;
    };
    let display = display_path(path, root);
    let source = match read_source(path, &display) {
        Ok(source) => source,
        Err(warning) => {
            found.1.push(warning);
            return;
        }
    };
    let (entries, warning) = extract::entries_in_source(
        language,
        &source,
        &discover::posix(path),
        &display,
        interner,
    );
    if let Some(warning) = warning.filter(|text| !text.is_empty()) {
        found.1.push(format!("{display}:{warning}"));
    }
    found.0.extend(
        entries
            .into_iter()
            .filter(|entry| keep(entry, min_lines, min_nodes)),
    );
}

pub fn scan_files(
    files: &[PathBuf],
    root: &Path,
    min_lines: usize,
    min_nodes: usize,
    interner: &mut Interner,
) -> (Vec<Entry>, Vec<String>) {
    let mut found = (Vec::new(), Vec::new());
    for path in files {
        scan_one(path, root, min_lines, min_nodes, interner, &mut found);
    }
    found
}

fn same_span(left: &Entry, right: &Entry) -> bool {
    left.file == right.file
        && left.start_line == right.start_line
        && left.end_line == right.end_line
}

fn span_of(entry: &Entry) -> Span {
    Span {
        file: entry.file.clone(),
        start_line: entry.start_line,
        end_line: entry.end_line,
    }
}

fn pair(left: &Entry, right: &Entry, threshold: f64) -> Option<Duplicate> {
    if same_span(left, right) {
        return None;
    }
    let score = jaccard(&left.fingerprints, &right.fingerprints);
    // Keep the `<` form: a NaN threshold keeps every pair, as in Python.
    if score < threshold {
        return None;
    }
    Some(Duplicate {
        score,
        language: left.language,
        left: span_of(left),
        right: span_of(right),
        left_nodes: left.nodes,
        right_nodes: right.nodes,
    })
}

/// Entries grouped by language, groups in first-seen order.
fn group_by_language(entries: &[Entry]) -> Vec<Vec<&Entry>> {
    let mut groups: Vec<Vec<&Entry>> = Vec::new();
    for entry in entries {
        match groups
            .iter()
            .position(|group| group[0].language == entry.language)
        {
            Some(index) => groups[index].push(entry),
            None => groups.push(vec![entry]),
        }
    }
    groups
}

/// Every unordered pair of the group, each pair once, in order of first member.
/// `split_first` walks the group so that no start index is a mutation site that
/// the same-span rule makes equivalent.
fn pairs_in(group: &[&Entry], threshold: f64) -> Vec<Duplicate> {
    let mut found = Vec::new();
    let mut rest = group;
    while let Some((left, tail)) = rest.split_first() {
        for right in tail {
            found.extend(pair(left, right, threshold));
        }
        rest = tail;
    }
    found
}

/// Python `_duplicate_key` ordering: descending score, then language and span keys.
fn compare_duplicates(a: &Duplicate, b: &Duplicate) -> Ordering {
    (-a.score)
        .total_cmp(&-b.score)
        .then_with(|| a.language.name().cmp(b.language.name()))
        .then_with(|| a.left.file.cmp(&b.left.file))
        .then_with(|| a.left.start_line.cmp(&b.left.start_line))
        .then_with(|| a.right.file.cmp(&b.right.file))
        .then_with(|| a.right.start_line.cmp(&b.right.start_line))
}

pub fn find_duplicates(entries: &[Entry], threshold: f64) -> Vec<Duplicate> {
    let mut found: Vec<Duplicate> = group_by_language(entries)
        .iter()
        .flat_map(|group| pairs_in(group, threshold))
        .collect();
    found.sort_by(compare_duplicates);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Language;
    use std::fs;

    fn entry(
        interner: &mut Interner,
        file: &str,
        (start, end): (usize, usize),
        nodes: usize,
        names: &[&str],
        language: Language,
    ) -> Entry {
        let mut fingerprints: Vec<u32> = names.iter().map(|name| interner.id(name)).collect();
        fingerprints.sort_unstable();
        fingerprints.dedup();
        Entry {
            language,
            file: file.to_string(),
            start_line: start,
            end_line: end,
            nodes,
            fingerprints,
        }
    }

    fn py(interner: &mut Interner, file: &str, span: (usize, usize), names: &[&str]) -> Entry {
        entry(interner, file, span, 20, names, Language::Python)
    }

    // DR-10: a form is never paired with its own span.
    #[test]
    fn dr10_the_same_span_is_not_a_pair() {
        let mut interner = Interner::new();
        let same = vec![
            py(&mut interner, "a.py", (1, 4), &["q"]),
            py(&mut interner, "a.py", (1, 4), &["q"]),
        ];
        assert!(find_duplicates(&same, 0.0).is_empty());

        let other_file = vec![
            py(&mut interner, "a.py", (1, 4), &["q"]),
            py(&mut interner, "b.py", (1, 4), &["q"]),
        ];
        assert_eq!(find_duplicates(&other_file, 0.5).len(), 1);

        let overlapping = vec![
            py(&mut interner, "a.py", (1, 5), &["q"]),
            py(&mut interner, "a.py", (3, 5), &["q"]),
        ];
        assert_eq!(find_duplicates(&overlapping, 0.5).len(), 1);

        let nested = vec![
            py(&mut interner, "a.py", (1, 4), &["q"]),
            py(&mut interner, "a.py", (1, 8), &["q"]),
        ];
        assert_eq!(find_duplicates(&nested, 0.5).len(), 1);
    }

    // DR-12: a score exactly equal to --threshold is kept; a higher threshold drops it.
    #[test]
    fn dr12_a_score_equal_to_the_threshold_is_kept() {
        let mut interner = Interner::new();
        let pair_entries = vec![
            entry(
                &mut interner,
                "a.rs",
                (1, 4),
                20,
                &["x", "y"],
                Language::Rust,
            ),
            entry(
                &mut interner,
                "b.rs",
                (1, 4),
                20,
                &["x", "z"],
                Language::Rust,
            ),
        ];
        assert_eq!(find_duplicates(&pair_entries, 1.0 / 3.0).len(), 1);
        assert!(find_duplicates(&pair_entries, 1.0 / 3.0 + 0.01).is_empty());
    }

    // DR-10: higher scores come first.
    #[test]
    fn dr10_the_higher_score_is_reported_first() {
        let mut interner = Interner::new();
        let entries = vec![
            py(&mut interner, "a.py", (1, 4), &["only"]),
            py(&mut interner, "b.py", (1, 4), &["only"]),
            entry(
                &mut interner,
                "a.rs",
                (1, 4),
                20,
                &["x", "y"],
                Language::Rust,
            ),
            entry(
                &mut interner,
                "b.rs",
                (10, 14),
                20,
                &["x", "z"],
                Language::Rust,
            ),
        ];
        let found = find_duplicates(&entries, 0.3);
        let scores: Vec<f64> = found.iter().map(|item| item.score).collect();
        assert_eq!(scores, vec![1.0, 1.0 / 3.0]);
    }

    // DR-10: higher scores come first even when the lower score is found first.
    #[test]
    fn dr10_the_higher_score_is_reported_first_when_found_last() {
        let mut interner = Interner::new();
        let entries = vec![
            entry(
                &mut interner,
                "a.rs",
                (1, 4),
                20,
                &["x", "y"],
                Language::Rust,
            ),
            entry(
                &mut interner,
                "b.rs",
                (10, 14),
                20,
                &["x", "z"],
                Language::Rust,
            ),
            py(&mut interner, "a.py", (1, 4), &["only"]),
            py(&mut interner, "b.py", (1, 4), &["only"]),
        ];
        let found = find_duplicates(&entries, 0.3);
        let scores: Vec<f64> = found.iter().map(|item| item.score).collect();
        assert_eq!(scores, vec![1.0, 1.0 / 3.0]);
    }

    // DR-10: equal scores break ties by language, then left and right file and line.
    #[test]
    fn dr10_equal_scores_are_ordered_by_file_keys() {
        let mut interner = Interner::new();
        let entries = vec![
            py(&mut interner, "x.py", (1, 4), &["q"]),
            py(&mut interner, "y.py", (1, 4), &["q"]),
            py(&mut interner, "z.py", (1, 4), &["q"]),
        ];
        let found = find_duplicates(&entries, 0.5);
        let pairs: Vec<(String, String)> = found
            .iter()
            .map(|item| (item.left.file.clone(), item.right.file.clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("x.py".to_string(), "y.py".to_string()),
                ("x.py".to_string(), "z.py".to_string()),
                ("y.py".to_string(), "z.py".to_string()),
            ]
        );
    }

    // DR-10: only forms of the same language are compared.
    #[test]
    fn dr10_cross_language_forms_are_never_paired() {
        let mut interner = Interner::new();
        let entries = vec![
            py(&mut interner, "a.py", (1, 4), &["q"]),
            entry(&mut interner, "b.rs", (1, 4), 20, &["q"], Language::Rust),
        ];
        assert!(find_duplicates(&entries, 0.0).is_empty());
    }

    // DR-12: --min-lines and --min-nodes are inclusive minimums.
    #[test]
    fn dr12_keep_uses_the_line_and_node_minimums() {
        let mut interner = Interner::new();
        let four_lines = entry(&mut interner, "a.py", (1, 4), 20, &["q"], Language::Python);
        let three_lines = entry(&mut interner, "a.py", (1, 3), 20, &["q"], Language::Python);
        let small = entry(&mut interner, "a.py", (1, 4), 19, &["q"], Language::Python);
        assert!(keep(&four_lines, 4, 20));
        assert!(!keep(&three_lines, 4, 20));
        assert!(!keep(&small, 4, 20));
    }

    // DR-4: the reported file is the path relative to the project root.
    #[test]
    fn dr4_display_path_is_relative_to_the_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let proj = dir.path().join("proj");
        let inside = proj.join("src/a.py");
        assert_eq!(display_path(&inside, &proj), "src/a.py");

        let outside = dir.path().join("outside.py");
        assert!(display_path(&outside, &proj).ends_with("/outside.py"));
    }

    // DR-5: files whose extension has no language are skipped silently.
    #[test]
    fn dr5_unknown_extensions_are_skipped_without_warnings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let proj = dir.path().join("proj");
        fs::create_dir_all(&proj).expect("mkdir");
        fs::write(proj.join("notes.md"), "# notes\n").expect("write");
        let mut interner = Interner::new();
        let (entries, warnings) = scan_files(&[proj.join("notes.md")], &proj, 1, 1, &mut interner);
        assert!(entries.is_empty());
        assert!(warnings.is_empty());
    }

    // DR-8: an unreadable file is a warning, not a crash.
    #[test]
    fn dr8_an_unreadable_file_becomes_a_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let proj = dir.path().join("proj");
        fs::create_dir_all(&proj).expect("mkdir");
        let mut interner = Interner::new();
        let (entries, warnings) =
            scan_files(&[proj.join("missing.py")], &proj, 1, 1, &mut interner);
        assert!(entries.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("missing.py: "), "{}", warnings[0]);
    }

    fn spans_of(
        path: &Path,
        proj: &Path,
        interner: &mut Interner,
    ) -> Vec<(usize, usize, usize, Vec<u32>)> {
        let (entries, warnings) = scan_files(&[path.to_path_buf()], proj, 1, 1, interner);
        assert!(warnings.is_empty(), "{warnings:?}");
        entries
            .into_iter()
            .map(|entry| {
                (
                    entry.start_line,
                    entry.end_line,
                    entry.nodes,
                    entry.fingerprints,
                )
            })
            .collect()
    }

    // DR-8: CRLF and lone CR line endings give the same line numbers and shapes as LF.
    #[test]
    fn dr8_crlf_and_cr_sources_keep_the_lf_line_numbers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let proj = dir.path().join("proj");
        fs::create_dir_all(&proj).expect("mkdir");
        let lf = "def alpha(xs):\n    ys = filter(xs, odd)\n    zs = map(ys, inc)\n    return list(zs)\n\n\
                  def beta(xs):\n    ys = filter(xs, odd)\n    zs = map(ys, inc)\n    return list(zs)\n";
        fs::write(proj.join("lf.py"), lf).expect("write");
        fs::write(proj.join("crlf.py"), lf.replace('\n', "\r\n")).expect("write");
        fs::write(proj.join("cr.py"), lf.replace('\n', "\r")).expect("write");

        let mut interner = Interner::new();
        let expected = spans_of(&proj.join("lf.py"), &proj, &mut interner);
        let spans: Vec<(usize, usize)> = expected.iter().map(|item| (item.0, item.1)).collect();
        assert_eq!(spans, vec![(1, 4), (6, 9)]);
        assert_eq!(
            spans_of(&proj.join("crlf.py"), &proj, &mut interner),
            expected
        );
        assert_eq!(
            spans_of(&proj.join("cr.py"), &proj, &mut interner),
            expected
        );
    }
}
