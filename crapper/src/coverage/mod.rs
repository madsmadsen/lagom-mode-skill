// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/coverage.py
// license: MIT

mod formats;
mod paths;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::Function;

use formats::{
    branch_percent, go_percent, jacoco_percent, parse_form_coverage, parse_go_profile,
    parse_jacoco_index, parse_lcov, percent_for_range, FileCoverage, GoSegment, JacocoMethod,
    LineHits,
};
use paths::{lookup, resolve, source_parts, Keyed};

/// Coverage reports loaded from a project tree, scored against functions.
#[derive(Default)]
pub struct CoverageBundle {
    lcov: Keyed<FileCoverage>,
    form_html: Keyed<LineHits>,
    go_profile: Keyed<Vec<GoSegment>>,
    jacoco: Keyed<Vec<JacocoMethod>>,
    source_parts: Vec<Vec<String>>,
}

impl CoverageBundle {
    /// Python `bind_sources`: the source paths that report keys are matched against.
    pub fn bind_sources(&mut self, paths: &[String]) {
        self.source_parts = source_parts(paths);
    }

    /// Python `percent_for`: the coverage percent for a function, or None when no report covers it.
    pub fn percent_for(&self, function: &Function) -> Option<f64> {
        match function.language.as_str() {
            "java" => self.java_percent(function),
            "go" => go_percent(
                &self.go_profile,
                &function.path,
                function.start_line,
                function.end_line,
                &self.source_parts,
            ),
            _ => self.line_percent(function),
        }
    }

    fn java_percent(&self, function: &Function) -> Option<f64> {
        let names = java_class_names(function);
        jacoco_percent(&self.jacoco, &names, &function.name, function.start_line)
    }

    fn line_percent(&self, function: &Function) -> Option<f64> {
        let (start, end) = (function.start_line, function.end_line);
        if let Some(hits) = lookup(&self.form_html, &function.path, &self.source_parts) {
            return Some(percent_for_range(hits, start, end));
        }
        let record = lookup(&self.lcov, &function.path, &self.source_parts)?;
        branch_percent(record, start, end)
            .or_else(|| Some(percent_for_range(&record.lines, start, end)))
    }
}

/// Python `_java_class_names`: the function's JaCoCo class, then its namespace if not already present.
fn java_class_names(function: &Function) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    if let Some(class) = function
        .jacoco_class
        .as_deref()
        .filter(|class| !class.is_empty())
    {
        names.push(class);
    }
    if !names.contains(&function.namespace.as_str()) {
        names.push(&function.namespace);
    }
    names
}

/// Python `load_coverage`: read every report under `root` and merge them.
/// A malformed Go profile or JaCoCo index is fatal, as it is in the Python original.
pub fn load_bundle(root: &Path) -> CoverageBundle {
    CoverageBundle {
        lcov: merge_lcov(root),
        form_html: merge_forms(root),
        go_profile: merge_go(root),
        jacoco: merge_jacoco(root),
        source_parts: Vec::new(),
    }
}

fn merge_lcov(root: &Path) -> Keyed<FileCoverage> {
    let mut merged: Keyed<FileCoverage> = Keyed::default();
    let mut lcov_files = find_named(&root.join("target").join("coverage"), &|name| {
        name == "lcov.info"
    });
    lcov_files.extend(find_named(&root.join("coverage"), &|name| {
        name == "lcov.info"
    }));
    for text in merge_texts(&lcov_files) {
        for (key, record) in parse_lcov(&text) {
            merged.insert(key, record);
        }
    }
    merged
}

fn merge_forms(root: &Path) -> Keyed<LineHits> {
    let coverage_dir = root.join("target").join("coverage");
    let mut found: Keyed<LineHits> = Keyed::default();
    for html_path in find_named(&coverage_dir, &|name| name.ends_with(".html")) {
        let Some(text) = read_text(&html_path).filter(|text| text.contains("forms covered")) else {
            continue;
        };
        let Ok(relative) = html_path.strip_prefix(&coverage_dir) else {
            continue;
        };
        let key = as_posix(relative);
        found.insert(
            key.trim_end_matches(".html").to_string(),
            parse_form_coverage(&text),
        );
    }
    found
}

fn merge_go(root: &Path) -> Keyed<Vec<GoSegment>> {
    let mut merged: Keyed<Vec<GoSegment>> = Keyed::default();
    let mut go_files = vec![
        root.join("target/coverage/coverage.out"),
        root.join("target/coverage/go/coverage.out"),
        root.join("coverage.out"),
    ];
    go_files.extend(project_reports(root, "target/coverage/go/coverage.out"));
    for text in merge_texts(&go_files) {
        let profile = parse_go_profile(&text).unwrap_or_else(|error| panic!("{error}"));
        for (key, segments) in profile {
            merged.get_mut_or_default(key).extend(segments);
        }
    }
    merged
}

fn merge_jacoco(root: &Path) -> Keyed<Vec<JacocoMethod>> {
    let mut merged: Keyed<Vec<JacocoMethod>> = Keyed::default();
    let mut jacoco_files = vec![root.join("target/site/jacoco/jacoco.xml")];
    jacoco_files.extend(project_reports(root, "target/site/jacoco/jacoco.xml"));
    for text in merge_texts(&jacoco_files) {
        let index = parse_jacoco_index(&text).unwrap_or_else(|error| panic!("{error}"));
        for (key, methods) in index {
            merged.get_mut_or_default(key).extend(methods);
        }
    }
    merged
}

/// Python `_merge_texts`: read each distinct file (by resolved path, first wins) that is non-empty.
fn merge_texts(paths: &[PathBuf]) -> Vec<String> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut texts = Vec::new();
    for path in paths {
        if !seen.insert(resolve(path)) {
            continue;
        }
        if let Some(text) = read_text(path).filter(|text| !text.is_empty()) {
            texts.push(text);
        }
    }
    texts
}

fn read_text(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Python `*/<tail>` and `*/*/<tail>`: the tail under every child directory, then every grandchild.
fn project_reports(root: &Path, tail: &str) -> Vec<PathBuf> {
    let children = child_dirs(root);
    let grandchildren: Vec<PathBuf> = children
        .iter()
        .flat_map(|child| child_dirs(child))
        .collect();
    children
        .iter()
        .chain(grandchildren.iter())
        .map(|dir| dir.join(tail))
        .collect()
}

fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    sorted_entries(dir)
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

/// Python `**/<name>`: every file under `dir` whose name matches, parent directory first, sorted at each level.
/// Directory symlinks are not followed.
fn find_named(dir: &Path, matches: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_named(dir, matches, &mut found);
    found
}

fn collect_named(dir: &Path, matches: &dyn Fn(&str) -> bool, found: &mut Vec<PathBuf>) {
    let entries = sorted_entries(dir);
    for entry in &entries {
        if matches(&entry.file_name().to_string_lossy()) {
            found.push(entry.path());
        }
    }
    for entry in &entries {
        let is_real_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_real_dir {
            collect_named(&entry.path(), matches, found);
        }
    }
}

fn sorted_entries(dir: &Path) -> Vec<fs::DirEntry> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)
        .map(|listing| listing.filter_map(Result::ok).collect())
        .unwrap_or_default();
    entries.sort_by_key(|entry| entry.file_name());
    entries
}

fn as_posix(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn function(language: &str, path: &str, start: u32, end: u32) -> Function {
        Function {
            name: "choose".to_string(),
            namespace: "demo.core".to_string(),
            complexity: 2,
            start_line: start,
            end_line: end,
            path: path.to_string(),
            language: language.to_string(),
            jacoco_class: None,
        }
    }

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let mut file = fs::File::create(path).expect("create");
        file.write_all(text.as_bytes()).expect("write");
    }

    // CR-10: form HTML wins over LCOV for Clojure, so the form counts give the value.
    #[test]
    fn cr_10_cloverage_form_counts_and_lcov_lines() {
        let html =
            r#"<span class="pc" title="1 out of 2 forms covered">   4&nbsp;&nbsp;(if x 1)</span>"#;
        let mut bundle = CoverageBundle::default();
        bundle.form_html.insert(
            "src/demo/core.clj".to_string(),
            formats::parse_form_coverage(html),
        );
        bundle.lcov =
            formats::parse_lcov("SF:src/demo/core.clj\nDA:3,1\nDA:4,0\nDA:9,3\nend_of_record\n");
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 3, 5)),
            Some(50.0)
        );
    }

    // CR-10: with no form HTML, LCOV line hits give the value, and a missing path is None.
    #[test]
    fn cr_10_lcov_percentage_when_there_is_no_html() {
        let bundle = CoverageBundle {
            lcov: formats::parse_lcov("SF:src/demo/core.clj\nDA:3,1\nDA:4,0\nend_of_record\n"),
            ..Default::default()
        };
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 3, 4)),
            Some(50.0)
        );
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/missing.clj", 3, 4)),
            None
        );
    }

    // CR-10: LCOV branch records override line hits for a span that has branches.
    #[test]
    fn cr_10_lcov_branch_records_override_line_hits() {
        let bundle = CoverageBundle {
            lcov: formats::parse_lcov(
                "SF:src/demo/app.ts\nDA:2,1\nDA:3,1\nBRDA:3,0,0,1\nBRDA:3,0,1,-\nend_of_record\n",
            ),
            ..Default::default()
        };
        assert_eq!(
            bundle.percent_for(&function("typescript", "src/demo/app.ts", 2, 3)),
            Some(50.0)
        );
    }

    // CR-10: a span with no branch records keeps its line coverage.
    #[test]
    fn cr_10_a_branchless_span_keeps_line_coverage() {
        let bundle = CoverageBundle {
            lcov: formats::parse_lcov(
                "SF:src/demo/app.ts\nDA:2,1\nDA:3,0\nBRDA:8,0,0,1\nBRDA:8,0,1,0\nend_of_record\n",
            ),
            ..Default::default()
        };
        assert_eq!(
            bundle.percent_for(&function("typescript", "src/demo/app.ts", 2, 3)),
            Some(50.0)
        );
    }

    // CR-12: Go statement ranges score a function, and an unrelated Go file is None.
    #[test]
    fn cr_12_go_profile_matches_statement_ranges() {
        let bundle = CoverageBundle {
            go_profile: formats::parse_go_profile(
                "mode: set\ngithub.com/acme/demo/board.go:4.1,6.2 2 1\ngithub.com/acme/demo/board.go:8.1,9.2 1 0\n",
            )
            .expect("valid profile"),
            ..Default::default()
        };
        assert_eq!(
            bundle.percent_for(&function("go", "board.go", 4, 6)),
            Some(100.0)
        );
        assert_eq!(
            bundle.percent_for(&function("go", "board.go", 8, 9)),
            Some(0.0)
        );
        assert_eq!(bundle.percent_for(&function("go", "other.go", 1, 3)), None);
    }

    // CR-10: a JaCoCo method joins on its class name, and an inner class joins on its own name.
    #[test]
    fn cr_10_jacoco_instruction_coverage_joins_on_class_and_method() {
        let xml = r#"<?xml version="1.0"?>
    <report>
      <package>
        <class name="demo/pkg/Board">
          <method name="place" line="4">
            <counter type="INSTRUCTION" missed="1" covered="3"/>
          </method>
        </class>
        <class name="demo/pkg/Board$Inner">
          <method name="tick" line="9">
            <counter type="INSTRUCTION" missed="0" covered="2"/>
          </method>
        </class>
      </package>
    </report>
    "#;
        let bundle = CoverageBundle {
            jacoco: formats::parse_jacoco_index(xml).expect("valid jacoco"),
            ..Default::default()
        };
        let outer = Function {
            name: "place".to_string(),
            namespace: "demo.pkg.Board".to_string(),
            jacoco_class: Some("demo.pkg.Board".to_string()),
            ..function("java", "src/demo/pkg/Board.java", 4, 5)
        };
        let inner = Function {
            name: "tick".to_string(),
            namespace: "demo.pkg.Board.Inner".to_string(),
            jacoco_class: Some("demo.pkg.Board$Inner".to_string()),
            ..function("java", "src/demo/pkg/Board.java", 9, 10)
        };
        assert_eq!(bundle.percent_for(&outer), Some(75.0));
        assert_eq!(bundle.percent_for(&inner), Some(100.0));
    }

    // CR-10: with no Go profile loaded, a Go function scores None.
    #[test]
    fn cr_10_missing_go_profile_is_none() {
        let bundle = CoverageBundle::default();
        assert_eq!(bundle.percent_for(&function("go", "board.go", 1, 3)), None);
    }

    // CR-10: a shorter source path does not take a longer file's report.
    #[test]
    fn cr_10_a_shorter_file_does_not_take_a_longer_files_coverage() {
        let mut bundle = CoverageBundle {
            go_profile: formats::parse_go_profile("mode: set\na/b/main.go:1.1,2.2 1 1\n")
                .expect("valid profile"),
            ..Default::default()
        };
        bundle.bind_sources(&["b/main.go".to_string(), "a/b/main.go".to_string()]);
        assert_eq!(bundle.percent_for(&function("go", "b/main.go", 1, 2)), None);
        assert_eq!(
            bundle.percent_for(&function("go", "a/b/main.go", 1, 2)),
            Some(100.0)
        );
    }

    // CR-10: a short report key matches one longer source, and a tie with another source matches nothing.
    #[test]
    fn cr_10_a_short_report_key_matches_one_longer_source() {
        let mut bundle = CoverageBundle {
            go_profile: formats::parse_go_profile("mode: set\nboard.go:4.1,6.2 2 1\n")
                .expect("valid profile"),
            ..Default::default()
        };
        bundle.bind_sources(&["cmd/board.go".to_string()]);
        assert_eq!(
            bundle.percent_for(&function("go", "cmd/board.go", 4, 6)),
            Some(100.0)
        );
        bundle.bind_sources(&["cmd/board.go".to_string(), "other/board.go".to_string()]);
        assert_eq!(
            bundle.percent_for(&function("go", "cmd/board.go", 4, 6)),
            None
        );
        assert_eq!(
            bundle.percent_for(&function("go", "other/board.go", 4, 6)),
            None
        );
    }

    // CR-10: a suffix lookup finds a report keyed by a longer path.
    #[test]
    fn cr_10_suffix_lookup_finds_a_report_keyed_by_a_longer_path() {
        let bundle = CoverageBundle {
            lcov: formats::parse_lcov("SF:proj/src/demo/core.clj\nDA:3,1\nDA:4,0\nend_of_record\n"),
            ..Default::default()
        };
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 3, 4)),
            Some(50.0)
        );
    }

    // AC-16: a cloverage HTML report, loaded from a tempdir, scores a function by its form counts.
    #[test]
    fn ac_16_cloverage_html_report_loads_and_scores() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "target/coverage/src/demo/core.clj.html",
            "<span class=\"pc\" title=\"1 out of 2 forms covered\">   4&nbsp;&nbsp;(if x 1)</span>\n\
             <span class=\"pc\" title=\"3 out of 4 forms covered\">   6&nbsp;&nbsp;(if y 2)</span>\n",
        );
        let bundle = load_bundle(dir.path());
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 3, 6)),
            Some(66.66666666666667)
        );
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 3, 5)),
            Some(50.0)
        );
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 7, 9)),
            Some(0.0)
        );
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/other.clj", 3, 6)),
            None
        );
    }

    // AC-16: an LCOV report, loaded from a tempdir, uses branch coverage where a span has branches.
    #[test]
    fn ac_16_lcov_report_loads_and_scores() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "target/coverage/lcov.info",
            "SF:src/demo/app.ts\nDA:2,1\nDA:3,0\nDA:4,1\nBRDA:2,0,0,1\nBRDA:2,0,1,0\nBRDA:2,0,2,-\nend_of_record\n",
        );
        let bundle = load_bundle(dir.path());
        assert_eq!(
            bundle.percent_for(&function("typescript", "src/demo/app.ts", 2, 4)),
            Some(33.333333333333336)
        );
        assert_eq!(
            bundle.percent_for(&function("typescript", "src/demo/app.ts", 3, 4)),
            Some(50.0)
        );
        assert_eq!(
            bundle.percent_for(&function("typescript", "src/nope.ts", 3, 4)),
            None
        );
    }

    // AC-16: a JaCoCo XML report, loaded from a tempdir, scores a Java method by class and line.
    #[test]
    fn ac_16_jacoco_xml_report_loads_and_scores() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "target/site/jacoco/jacoco.xml",
            "<?xml version=\"1.0\"?>\n<!DOCTYPE report PUBLIC \"-//JACOCO//DTD Report 1.1//EN\" \"report.dtd\">\n\
             <report><package>\n\
             <class name=\"demo/pkg/Board\">\n\
             <method name=\"place\" line=\"4\"><counter type=\"INSTRUCTION\" missed=\"1\" covered=\"3\"/></method>\n\
             <method name=\"place\" line=\"20\"><counter type=\"INSTRUCTION\" missed=\"4\" covered=\"0\"/></method>\n\
             </class>\n\
             <class name=\"demo/pkg/Board$Inner\">\n\
             <method name=\"tick\" line=\"9\"><counter type=\"INSTRUCTION\" missed=\"0\" covered=\"2\"/></method>\n\
             </class>\n\
             </package></report>\n",
        );
        let bundle = load_bundle(dir.path());
        let java = |name: &str, class: &str, line: u32| Function {
            name: name.to_string(),
            namespace: "demo.pkg.Board".to_string(),
            jacoco_class: Some(class.to_string()),
            ..function("java", "src/demo/pkg/Board.java", line, line)
        };
        assert_eq!(
            bundle.percent_for(&java("place", "demo.pkg.Board", 4)),
            Some(75.0)
        );
        assert_eq!(
            bundle.percent_for(&java("place", "demo.pkg.Board", 18)),
            Some(0.0)
        );
        assert_eq!(
            bundle.percent_for(&java("tick", "demo.pkg.Board$Inner", 9)),
            Some(100.0)
        );
        assert_eq!(
            bundle.percent_for(&java("missing", "demo.pkg.Board", 4)),
            None
        );
    }

    // AC-16: a Go coverage profile, loaded from a tempdir, scores a function by its statement ranges.
    #[test]
    fn ac_16_go_profile_report_loads_and_scores() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "target/coverage/go/coverage.out",
            "mode: set\ndemo/board.go:4.1,6.2 2 1\ndemo/board.go:8.1,9.2 1 0\n",
        );
        let bundle = load_bundle(dir.path());
        assert_eq!(
            bundle.percent_for(&function("go", "demo/board.go", 4, 9)),
            Some(66.66666666666667)
        );
        assert_eq!(
            bundle.percent_for(&function("go", "demo/board.go", 8, 9)),
            Some(0.0)
        );
        assert_eq!(bundle.percent_for(&function("go", "other.go", 1, 3)), None);
    }

    // CR-10: load_bundle merges every report kind, and an index.html beside the forms is ignored.
    #[test]
    fn cr_10_load_bundle_merges_each_report() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "target/coverage/lcov.info",
            "SF:src/demo/core.clj\nDA:3,1\nDA:4,0\nend_of_record\n",
        );
        write(
            dir.path(),
            "target/coverage/go/coverage.out",
            "mode: set\nboard.go:1.1,2.2 1 1\n",
        );
        write(
            dir.path(),
            "target/site/jacoco/jacoco.xml",
            "<report><class name=\"demo/Board\"><method name=\"place\" line=\"4\"><counter type=\"INSTRUCTION\" missed=\"0\" covered=\"1\"/></method></class></report>",
        );
        write(
            dir.path(),
            "target/coverage/src/demo/core.clj.html",
            "<span title=\"1 out of 2 forms covered\">   4&nbsp;&nbsp;(if x 1)</span>",
        );
        write(dir.path(), "target/coverage/index.html", "<html></html>");
        let bundle = load_bundle(dir.path());
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 3, 4)),
            Some(50.0)
        );
        assert_eq!(
            bundle.percent_for(&function("go", "board.go", 1, 2)),
            Some(100.0)
        );
        let java = Function {
            name: "place".to_string(),
            namespace: "demo.Board".to_string(),
            jacoco_class: Some("demo.Board".to_string()),
            ..function("java", "src/demo/Board.java", 4, 4)
        };
        assert_eq!(bundle.percent_for(&java), Some(100.0));
    }

    // AC-25: an empty tree loads a bundle that scores every function None.
    #[test]
    fn ac_25_load_bundle_on_an_empty_tree_scores_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bundle = load_bundle(dir.path());
        assert_eq!(
            bundle.percent_for(&function("clojure", "src/demo/core.clj", 3, 6)),
            None
        );
        assert_eq!(
            bundle.percent_for(&function("java", "src/demo/core.java", 3, 6)),
            None
        );
        assert_eq!(
            bundle.percent_for(&function("go", "demo/board.go", 3, 6)),
            None
        );
    }
}
