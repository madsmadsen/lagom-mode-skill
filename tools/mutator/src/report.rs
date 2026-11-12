// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Port of mutator/src/mutator/report.py.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::{FormResult, Site};

struct Widths {
    name: usize,
    namespace: usize,
}

struct Cells<'a> {
    name: &'a str,
    namespace: &'a str,
    killed: usize,
    survived: usize,
    uncovered: usize,
    sites: usize,
}

/// Render the per-function table, sorted by score, with a Total row.
pub fn format_results(forms: &[FormResult]) -> String {
    if forms.is_empty() {
        return "No functions to mutate.\n".to_owned();
    }
    let widths = Widths {
        name: column_width(forms.iter().map(|form| form.name.chars().count()), 8, 48),
        namespace: column_width(
            forms.iter().map(|form| form.namespace.chars().count()),
            9,
            72,
        ),
    };
    let header = format!(
        "{:<nw$} {:<sw$} {:>7} {:>9} {:>10} {:>6} {:>7}",
        "Function",
        "Namespace",
        "Killed",
        "Survived",
        "Uncovered",
        "Sites",
        "Score",
        nw = widths.name,
        sw = widths.namespace,
    );
    let rule = "-".repeat(header.chars().count());
    let mut lines = vec![header, rule.clone()];
    for form in ordered(forms) {
        lines.push(render_row(&widths, &form_cells(form), &score_text(form)));
    }
    let totals = totals(forms);
    lines.push(rule);
    lines.push(render_row(&widths, &totals, &total_score(&totals)));
    format!("{}\n", lines.join("\n"))
}

/// Render one log line per site that has a status, in site order.
pub fn format_site_log(sites: &[Site], status: &BTreeMap<String, String>) -> String {
    let lines: Vec<String> = sites
        .iter()
        .filter_map(|site| log_line(site, status))
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!("{}\n", lines.join("\n"))
}

/// Render the site inventory for one file; `*` marks changed functions.
pub fn format_scan(
    path: &str,
    sites: &[Site],
    changed: &HashSet<(String, String)>,
    covered: Option<&HashMap<String, bool>>,
) -> String {
    let mut lines = vec![format!("Scan: {} mutation sites in {path}", sites.len())];
    lines.extend(
        sites
            .iter()
            .map(|site| scan_line(path, site, changed, covered)),
    );
    if !changed.is_empty() {
        lines.push("* marks a function whose text changed since the last snapshot.".to_owned());
    }
    format!("{}\n", lines.join("\n"))
}

fn column_width(lengths: impl Iterator<Item = usize>, floor: usize, cap: usize) -> usize {
    lengths.fold(floor, usize::max).min(cap)
}

fn score_text(form: &FormResult) -> String {
    match form.score() {
        None => "   n/a".to_owned(),
        Some(score) => format!("{:5.1}%", score * 100.0),
    }
}

fn total_score(totals: &Cells) -> String {
    let executed = totals.killed + totals.survived;
    let score = if executed == 0 {
        "n/a".to_owned()
    } else {
        format!("{:.1}%", totals.killed as f64 / executed as f64 * 100.0)
    };
    format!("{score:>7}")
}

fn compare_forms(a: &&FormResult, b: &&FormResult) -> Ordering {
    let (score_a, score_b) = (a.score(), b.score());
    score_a
        .is_some()
        .cmp(&score_b.is_some())
        .then_with(|| score_a.unwrap_or(0.0).total_cmp(&score_b.unwrap_or(0.0)))
        .then_with(|| b.survived.cmp(&a.survived))
        .then_with(|| a.name.cmp(&b.name))
}

fn ordered(forms: &[FormResult]) -> Vec<&FormResult> {
    let mut ordered: Vec<&FormResult> = forms.iter().collect();
    ordered.sort_by(compare_forms);
    ordered
}

fn form_cells(form: &FormResult) -> Cells<'_> {
    Cells {
        name: &form.name,
        namespace: &form.namespace,
        killed: form.killed,
        survived: form.survived,
        uncovered: form.uncovered,
        sites: form.sites,
    }
}

fn totals(forms: &[FormResult]) -> Cells<'static> {
    Cells {
        name: "Total",
        namespace: "",
        killed: forms.iter().map(|form| form.killed).sum(),
        survived: forms.iter().map(|form| form.survived).sum(),
        uncovered: forms.iter().map(|form| form.uncovered).sum(),
        sites: forms.iter().map(|form| form.sites).sum(),
    }
}

fn render_row(widths: &Widths, cells: &Cells, score: &str) -> String {
    format!(
        "{:<nw$} {:<sw$} {:>7} {:>9} {:>10} {:>6} {score}",
        cells.name,
        cells.namespace,
        cells.killed,
        cells.survived,
        cells.uncovered,
        cells.sites,
        nw = widths.name,
        sw = widths.namespace,
    )
}

fn log_line(site: &Site, status: &BTreeMap<String, String>) -> Option<String> {
    let label = status.get(&site.mutation_id)?;
    Some(format!(
        "{:<9} {}:{} {}",
        label.to_uppercase(),
        site.file,
        site.line,
        site.description()
    ))
}

fn scan_line(
    path: &str,
    site: &Site,
    changed: &HashSet<(String, String)>,
    covered: Option<&HashMap<String, bool>>,
) -> String {
    let key = (site.namespace.clone(), site.form_id.clone());
    let mark = if changed.contains(&key) { "*" } else { " " };
    let uncovered = covered.is_some_and(|map| !map.get(&site.mutation_id).copied().unwrap_or(true));
    let coverage = if uncovered { " uncovered" } else { "" };
    format!(
        "{mark} {path}:{} {}{coverage}  [{}]",
        site.line,
        site.description(),
        site.form_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spaces(count: usize) -> String {
        " ".repeat(count)
    }

    fn form(name: &str, namespace: &str, counts: [usize; 4]) -> FormResult {
        let [killed, survived, uncovered, sites] = counts;
        FormResult {
            id: "defn/x".to_owned(),
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            private: false,
            file: "src/demo.py".to_owned(),
            line: 1,
            end_line: 1,
            digest: String::new(),
            killed,
            survived,
            uncovered,
            sites,
        }
    }

    fn site(mutation_id: &str) -> Site {
        Site {
            file: "src/demo.py".to_owned(),
            namespace: "demo".to_owned(),
            form_id: "defn/add".to_owned(),
            name: "add".to_owned(),
            line: 2,
            start: 20,
            end: 21,
            original: ">".to_owned(),
            mutant: ">=".to_owned(),
            category: "comparison".to_owned(),
            mutation_id: mutation_id.to_owned(),
            anchor_line: 2,
        }
    }

    // MU-20: an empty run reports that nothing was mutated.
    #[test]
    fn mu_20_format_results_reports_no_functions() {
        assert_eq!(format_results(&[]), "No functions to mutate.\n");
    }

    // MU-20: the table has exact column widths, a rule line, and a Total row.
    #[test]
    fn mu_20_format_results_renders_exact_table() {
        let forms = [form("add", "demo", [1, 3, 0, 4])];
        let header = "Function Namespace  Killed  Survived  Uncovered  Sites   Score";
        let row = format!(
            "add{}demo{}1{}3{}0{}4  25.0%",
            spaces(6),
            spaces(12),
            spaces(9),
            spaces(10),
            spaces(6)
        );
        let total = format!(
            "Total{}1{}3{}0{}4{}25.0%",
            spaces(20),
            spaces(9),
            spaces(10),
            spaces(6),
            spaces(3)
        );
        let rule = "-".repeat(62);
        let expected = format!("{header}\n{rule}\n{row}\n{rule}\n{total}\n");
        assert_eq!(format_results(&forms), expected);
    }

    // MU-20: a function with nothing executed shows n/a, and so does the Total.
    #[test]
    fn mu_20_format_results_shows_na_when_nothing_ran() {
        let forms = [form("add", "demo", [0, 0, 2, 2])];
        let row = format!(
            "add{}demo{}0{}0{}2{}2{}n/a",
            spaces(6),
            spaces(12),
            spaces(9),
            spaces(10),
            spaces(6),
            spaces(4)
        );
        let total = format!(
            "Total{}0{}0{}2{}2{}n/a",
            spaces(20),
            spaces(9),
            spaces(10),
            spaces(6),
            spaces(5)
        );
        let out = format_results(&forms);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[2], row);
        assert_eq!(lines[4], total);
    }

    // MU-20: unscored functions sort first, then by fewest survivors, then by name.
    #[test]
    fn mu_20_format_results_sorts_unscored_first() {
        let forms = [
            form("alpha", "demo", [1, 1, 0, 2]),
            form("zeta", "demo", [0, 0, 1, 1]),
        ];
        let out = format_results(&forms);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[2].starts_with("zeta"), "{out}");
        assert!(lines[3].starts_with("alpha"), "{out}");
    }

    // MU-20: each site with a status prints its upper-cased label; sites without one are skipped.
    #[test]
    fn mu_20_format_site_log_prints_labels_and_skips_unknown_sites() {
        let sites = [site("m1"), site("m2"), site("m3")];
        let status = BTreeMap::from([
            ("m1".to_owned(), "killed".to_owned()),
            ("m2".to_owned(), "survived".to_owned()),
        ]);
        assert_eq!(
            format_site_log(&sites, &status),
            "KILLED    src/demo.py:2 > -> >=\nSURVIVED  src/demo.py:2 > -> >=\n"
        );
    }

    // MU-20: an empty log prints nothing.
    #[test]
    fn mu_20_format_site_log_is_empty_without_statuses() {
        assert_eq!(format_site_log(&[site("m1")], &BTreeMap::new()), "");
    }

    // MU-16: a scan with no snapshot lists sites without marks or coverage flags.
    #[test]
    fn mu_16_format_scan_lists_sites_without_marks() {
        let out = format_scan("src/demo.py", &[site("m1")], &HashSet::new(), None);
        assert_eq!(
            out,
            "Scan: 1 mutation sites in src/demo.py\n  src/demo.py:2 > -> >=  [defn/add]\n"
        );
    }

    // MU-16 and MU-19: an uncovered site is flagged, and a changed function is starred.
    #[test]
    fn mu_19_format_scan_marks_changed_functions_and_uncovered_sites() {
        let changed = HashSet::from([("demo".to_owned(), "defn/add".to_owned())]);
        let covered = HashMap::from([("m1".to_owned(), false)]);
        let out = format_scan("src/demo.py", &[site("m1")], &changed, Some(&covered));
        assert_eq!(
            out,
            "Scan: 1 mutation sites in src/demo.py\n\
             * src/demo.py:2 > -> >= uncovered  [defn/add]\n\
             * marks a function whose text changed since the last snapshot.\n"
        );
    }

    // MU-20: a site missing from the coverage map counts as covered, so it is not flagged.
    #[test]
    fn mu_20_format_scan_treats_missing_coverage_as_covered() {
        let covered = HashMap::new();
        let out = format_scan(
            "src/demo.py",
            &[site("m1")],
            &HashSet::new(),
            Some(&covered),
        );
        assert_eq!(
            out,
            "Scan: 1 mutation sites in src/demo.py\n  src/demo.py:2 > -> >=  [defn/add]\n"
        );
    }
}
