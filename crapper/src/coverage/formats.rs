// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/coverage.py
// license: MIT

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::paths::{lookup, Keyed};

/// Line number to `(covered, total)`.
pub type LineHits = HashMap<i64, (i64, i64)>;

/// One source file's LCOV record: line hits and branch hits.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileCoverage {
    pub lines: LineHits,
    pub branches: LineHits,
}

/// One Go profile block: a source range with its statement count and hit count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoSegment {
    pub start: i64,
    pub end: i64,
    pub statements: i64,
    pub hits: i64,
}

/// One JaCoCo method with its instruction counts and declaration line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JacocoMethod {
    pub missed: i64,
    pub covered: i64,
    pub line: i64,
}

/// Python `_coverage_percent`: percent covered, or 0.0 when there is nothing to count.
pub fn coverage_percent(covered: i64, total: i64) -> f64 {
    if total <= 0 {
        return 0.0;
    }
    100.0 * covered as f64 / total as f64
}

fn span_totals(hits: &LineHits, start: u32, end: u32) -> (i64, i64) {
    (i64::from(start)..=i64::from(end))
        .filter_map(|line| hits.get(&line))
        .fold((0, 0), |(covered, total), (hit, count)| {
            (covered + hit, total + count)
        })
}

/// Python `_percent_for_range`: line coverage across a function's span.
pub fn percent_for_range(lines: &LineHits, start: u32, end: u32) -> f64 {
    let (covered, total) = span_totals(lines, start, end);
    coverage_percent(covered, total)
}

/// Python `_branch_percent`: branch coverage across a span, or None when the span has no branches.
pub fn branch_percent(record: &FileCoverage, start: u32, end: u32) -> Option<f64> {
    let (covered, total) = span_totals(&record.branches, start, end);
    (total != 0).then(|| coverage_percent(covered, total))
}

static SPAN_FORMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<span[^>]*title="([0-9]+) out of ([0-9]+) forms covered"[^>]*>\s*([0-9]+)&nbsp;"#)
        .expect("valid form regex")
});

/// Python `_parse_form_coverage`: cloverage per-line form counts, keyed by line number.
pub fn parse_form_coverage(html: &str) -> LineHits {
    let mut found = HashMap::new();
    for caps in SPAN_FORMS.captures_iter(html) {
        let total = digits(&caps[2]);
        if total > 0 {
            found.insert(digits(&caps[3]), (digits(&caps[1]), total));
        }
    }
    found
}

fn digits(text: &str) -> i64 {
    text.bytes().fold(0_i64, |acc, byte| {
        acc.saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'))
    })
}

static DA_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^DA:([0-9]+),([0-9]+)").expect("valid DA regex"));
static BRDA_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^BRDA:([0-9]+),([^,]*),([^,]*),(-|[0-9]+)").expect("valid BRDA regex")
});

/// Python `_parse_lcov`: LCOV records keyed by the `SF:` source path.
pub fn parse_lcov(text: &str) -> Keyed<FileCoverage> {
    let mut out = Keyed::default();
    let mut current_file: Option<String> = None;
    let mut current = FileCoverage::default();
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            store_lcov(
                &mut out,
                current_file.replace(path.to_string()),
                std::mem::take(&mut current),
            );
        } else if line == "end_of_record" {
            store_lcov(&mut out, current_file.take(), std::mem::take(&mut current));
        } else if current_file.is_some() {
            read_lcov_line(&mut current, line);
        }
    }
    store_lcov(&mut out, current_file, current);
    out
}

fn store_lcov(out: &mut Keyed<FileCoverage>, file: Option<String>, record: FileCoverage) {
    if let Some(name) = file {
        out.insert(name, record);
    }
}

fn read_lcov_line(record: &mut FileCoverage, line: &str) {
    if let Some(caps) = DA_LINE.captures(line) {
        let hit = is_hit(&caps[2]);
        record.lines.insert(digits(&caps[1]), (i64::from(hit), 1));
    } else if let Some(caps) = BRDA_LINE.captures(line) {
        let entry = record.branches.entry(digits(&caps[1])).or_insert((0, 0));
        entry.0 += i64::from(branch_hit(&caps[4]));
        entry.1 += 1;
    }
}

fn is_hit(count: &str) -> bool {
    count.bytes().any(|byte| byte != b'0')
}

fn branch_hit(taken: &str) -> bool {
    taken != "-" && is_hit(taken)
}

/// Python `_parse_go_profile`: Go coverage blocks keyed by file path. Errors name the bad line.
pub fn parse_go_profile(text: &str) -> Result<Keyed<Vec<GoSegment>>, String> {
    let mut out: Keyed<Vec<GoSegment>> = Keyed::default();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("mode:") {
            continue;
        }
        let (file, segment) = parse_go_segment(line)
            .ok_or_else(|| format!("invalid coverage segment on line {}: {raw:?}", index + 1))?;
        out.get_mut_or_default(file.to_string()).push(segment);
    }
    Ok(out)
}

fn parse_go_segment(line: &str) -> Option<(&str, GoSegment)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let &[location, statements, hits] = fields.as_slice() else {
        return None;
    };
    let (file, range) = location.split_once(':')?;
    let (start, end) = range.split_once(',')?;
    let segment = GoSegment {
        start: leading_int(start)?,
        end: leading_int(end)?,
        statements: statements.parse().ok()?,
        hits: hits.parse().ok()?,
    };
    Some((file, segment))
}

fn leading_int(text: &str) -> Option<i64> {
    text.split('.').next()?.parse().ok()
}

/// Python `_go_percent`: statement coverage over the Go blocks that overlap the function's lines.
pub fn go_percent(
    profile: &Keyed<Vec<GoSegment>>,
    path: &str,
    start: u32,
    end: u32,
    source_parts: &[Vec<String>],
) -> Option<f64> {
    let segments = lookup(profile, path, source_parts)?;
    let (covered, total) = segments
        .iter()
        .filter(|segment| segment.end >= i64::from(start) && segment.start <= i64::from(end))
        .fold((0, 0), |(covered, total), segment| {
            let hit = if segment.hits > 0 {
                segment.statements
            } else {
                0
            };
            (covered + hit, total + segment.statements)
        });
    Some(coverage_percent(covered, total))
}

static DOCTYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<!DOCTYPE[^>]*>").expect("valid DOCTYPE regex"));

/// Python `_parse_jacoco_index`: JaCoCo methods keyed by `class#method`.
pub fn parse_jacoco_index(text: &str) -> Result<Keyed<Vec<JacocoMethod>>, String> {
    let cleaned = DOCTYPE.replace_all(text, "");
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let document = roxmltree::Document::parse_with_options(&cleaned, options)
        .map_err(|error| error.to_string())?;
    let mut found: Keyed<Vec<JacocoMethod>> = Keyed::default();
    for class in document
        .descendants()
        .filter(|node| is_element(*node, "class"))
    {
        let class_name = class.attribute("name").unwrap_or("").replace('/', ".");
        for method in class.children().filter(|node| is_element(*node, "method")) {
            record_method(&mut found, &class_name, method)?;
        }
    }
    Ok(found)
}

fn is_element(node: roxmltree::Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace().is_none()
}

fn record_method(
    found: &mut Keyed<Vec<JacocoMethod>>,
    class_name: &str,
    method: roxmltree::Node,
) -> Result<(), String> {
    let instruction = method
        .children()
        .find(|node| is_element(*node, "counter") && node.attribute("type") == Some("INSTRUCTION"));
    let Some(counter) = instruction else {
        return Ok(());
    };
    let key = format!("{class_name}#{}", method.attribute("name").unwrap_or(""));
    let entry = JacocoMethod {
        missed: count_attr(counter, "missed")?,
        covered: count_attr(counter, "covered")?,
        line: attr_or_zero(method, "line").trim().parse().unwrap_or(0),
    };
    found.get_mut_or_default(key).push(entry);
    Ok(())
}

fn count_attr(counter: roxmltree::Node, name: &str) -> Result<i64, String> {
    let text = attr_or_zero(counter, name).trim();
    text.parse()
        .map_err(|_| format!("invalid JaCoCo {name} count: {text:?}"))
}

fn attr_or_zero<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &'a str) -> &'a str {
    node.attribute(name)
        .filter(|value| !value.is_empty())
        .unwrap_or("0")
}

/// Python `_jacoco_percent`: the method at the function's line, else the nearest method by line.
pub fn jacoco_percent(
    index: &Keyed<Vec<JacocoMethod>>,
    class_names: &[&str],
    method_name: &str,
    line: u32,
) -> Option<f64> {
    let pool: Vec<JacocoMethod> = class_names
        .iter()
        .filter_map(|class| index.get(&format!("{class}#{method_name}")))
        .flatten()
        .copied()
        .collect();
    let line = i64::from(line);
    if let Some(exact) = pool.iter().find(|method| method.line == line) {
        return Some(method_percent(exact));
    }
    pool.iter()
        .min_by_key(|method| (method.line - line).abs())
        .map(method_percent)
}

fn method_percent(method: &JacocoMethod) -> f64 {
    coverage_percent(method.covered, method.missed + method.covered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::paths::source_parts;

    fn profile(text: &str) -> Keyed<Vec<GoSegment>> {
        parse_go_profile(text).expect("valid profile")
    }

    // CR-12: cloverage form counts are read per line from the span title and the line number.
    #[test]
    fn cr_12_cloverage_form_counts_are_read_per_line() {
        let html =
            r#"<span class="pc" title="1 out of 2 forms covered">   4&nbsp;&nbsp;(if x 1)</span>"#;
        assert_eq!(parse_form_coverage(html), HashMap::from([(4, (1, 2))]));
    }

    // CR-12: a span with no forms (total 0) contributes nothing.
    #[test]
    fn cr_12_form_coverage_ignores_a_span_with_no_forms() {
        let html =
            r#"<span class="pc" title="0 out of 0 forms covered">   4&nbsp;&nbsp;(if x 1)</span>"#;
        assert!(parse_form_coverage(html).is_empty());
    }

    // CR-12: LCOV DA records give line hits, and a record without end_of_record is still stored.
    #[test]
    fn cr_12_lcov_keeps_a_file_that_has_no_end_record() {
        let parsed = parse_lcov("SF:src/demo/app.ts\nDA:2,1\nDA:3,0\n");
        let record = parsed.get("src/demo/app.ts").expect("record stored");
        assert_eq!(record.lines[&2], (1, 1));
        assert_eq!(record.lines[&3], (0, 1));
    }

    // CR-10: LCOV branch records give branch hits, and a '-' taken count is not a hit.
    #[test]
    fn cr_10_lcov_branch_records_are_counted_and_dash_is_not_a_hit() {
        let parsed = parse_lcov(
            "SF:src/demo/app.ts\nBRDA:3,0,0,1\nBRDA:3,0,1,0\nBRDA:3,0,2,-\nend_of_record\n",
        );
        let record = parsed.get("src/demo/app.ts").expect("record stored");
        assert_eq!(record.branches[&3], (1, 3));
    }

    // CR-10: a zero-total span has no branch percentage, and a span with no lines is 0.0.
    #[test]
    fn cr_10_coverage_percent_is_zero_when_there_is_nothing_to_count() {
        assert_eq!(coverage_percent(0, 0), 0.0);
        assert_eq!(percent_for_range(&LineHits::new(), 1, 3), 0.0);
        assert_eq!(branch_percent(&FileCoverage::default(), 1, 3), None);
    }

    // CR-12: Go profile blocks score statement ranges: the first block is covered, the second is not.
    #[test]
    fn cr_12_go_profile_matches_statement_ranges() {
        let parsed = profile(
            "mode: set\ngithub.com/acme/demo/board.go:4.1,6.2 2 1\ngithub.com/acme/demo/board.go:8.1,9.2 1 0\n",
        );
        assert_eq!(
            go_percent(&parsed, "board.go", 4, 6, &source_parts(&[])),
            Some(100.0)
        );
        assert_eq!(
            go_percent(&parsed, "board.go", 8, 9, &source_parts(&[])),
            Some(0.0)
        );
        assert_eq!(
            go_percent(&parsed, "other.go", 1, 3, &source_parts(&[])),
            None
        );
    }

    // CR-12: a Go function whose lines overlap no block is 0.0, not None.
    #[test]
    fn cr_12_go_percent_is_zero_when_no_segment_overlaps() {
        let parsed = profile("mode: set\ndemo/board.go:4.1,6.2 2 1\n");
        assert_eq!(
            go_percent(&parsed, "demo/board.go", 20, 22, &source_parts(&[])),
            Some(0.0)
        );
    }

    // CR-12: a malformed Go profile line is an error that names the segment.
    #[test]
    fn cr_12_go_profile_rejects_a_broken_line() {
        let error = parse_go_profile("mode: set\nnot a segment\n").expect_err("broken line");
        assert!(error.contains("invalid coverage segment"), "{error}");
    }

    // CR-12: the most specific Go profile key wins, a tie matches nothing, and "/" matches nothing.
    #[test]
    fn cr_12_the_most_specific_prefix_wins_and_a_tie_matches_nothing() {
        let specific =
            profile("mode: set\nother/proj/src/a.go:1.1,2.2 1 0\nproj/src/a.go:1.1,2.2 1 1\n");
        assert_eq!(
            go_percent(&specific, "src/a.go", 1, 2, &source_parts(&[])),
            Some(100.0)
        );
        let tied = profile("mode: set\nproj/a.go:1.1,2.2 1 1\nother/a.go:1.1,2.2 1 0\n");
        assert_eq!(go_percent(&tied, "a.go", 1, 2, &source_parts(&[])), None);
        assert_eq!(go_percent(&specific, "/", 1, 2, &source_parts(&[])), None);
    }

    // CR-12: an exact Go profile key wins over a suffix-matching key.
    #[test]
    fn cr_12_exact_profile_key_wins_over_a_suffix() {
        let parsed = profile("mode: set\na/b/main.go:1.1,2.2 1 1\nb/main.go:1.1,2.2 1 0\n");
        assert_eq!(
            go_percent(&parsed, "b/main.go", 1, 2, &source_parts(&[])),
            Some(0.0)
        );
        assert_eq!(
            go_percent(&parsed, "a/b/main.go", 1, 2, &source_parts(&[])),
            Some(100.0)
        );
    }

    // CR-12: a JaCoCo method counts by its class and method name, and a counter with no fields is 0.
    #[test]
    fn cr_12_jacoco_index_reads_class_and_method_counters() {
        let xml = r#"<report><package><class name="demo/pkg/Board">
            <method name="place" line="4"><counter type="INSTRUCTION" missed="1" covered="3"/></method>
            <method name="empty" line="6"><counter type="INSTRUCTION"/></method>
            <method name="skip" line="7"><counter type="LINE" missed="9" covered="9"/></method>
        </class></package></report>"#;
        let index = parse_jacoco_index(xml).expect("valid jacoco");
        assert_eq!(
            index.get("demo.pkg.Board#place"),
            Some(&vec![JacocoMethod {
                missed: 1,
                covered: 3,
                line: 4
            }])
        );
        assert_eq!(
            index.get("demo.pkg.Board#empty"),
            Some(&vec![JacocoMethod {
                missed: 0,
                covered: 0,
                line: 6
            }])
        );
        assert!(index.get("demo.pkg.Board#skip").is_none());
    }

    // CR-12: a JaCoCo method without an instruction counter is skipped, and a non-numeric line becomes 0.
    #[test]
    fn cr_12_jacoco_skips_methods_without_an_instruction_counter() {
        let xml = r#"<report>
          <class name="demo/pkg/Board">
            <method name="skip" line="2"><counter type="LINE" missed="1" covered="0"/></method>
            <method name="bad" line="nope"><counter type="INSTRUCTION" missed="1" covered="1"/></method>
            <counter type="INSTRUCTION" missed="0" covered="1"/>
          </class>
        </report>"#;
        let found = parse_jacoco_index(xml).expect("valid jacoco");
        assert_eq!(
            found
                .get("demo.pkg.Board#bad")
                .map(|methods| methods[0].line),
            Some(0)
        );
        assert!(found.get("demo.pkg.Board#skip").is_none());
    }

    // CR-12: a JaCoCo DOCTYPE is stripped before parsing.
    #[test]
    fn cr_12_jacoco_doctype_is_stripped_before_parsing() {
        let xml = r#"<?xml version="1.0"?><!DOCTYPE report PUBLIC "x" "report.dtd"><report/>"#;
        assert_eq!(
            parse_jacoco_index(xml)
                .expect("valid jacoco")
                .into_iter()
                .count(),
            0
        );
    }

    // CR-12: when no method sits on the function's line, the nearest method by line supplies the value.
    #[test]
    fn cr_12_jacoco_uses_the_nearest_method() {
        let index = parse_jacoco_index(
            r#"<report><class name="demo/Board"><method name="place" line="20"><counter type="INSTRUCTION" missed="4" covered="0"/></method></class></report>"#,
        )
        .expect("valid jacoco");
        assert_eq!(
            jacoco_percent(&index, &["demo.Board"], "place", 18),
            Some(0.0)
        );
    }

    // CR-12: a JaCoCo lookup with no matching method is None.
    #[test]
    fn cr_12_jacoco_is_none_when_no_method_matches() {
        let index = parse_jacoco_index("<report/>").expect("valid jacoco");
        assert_eq!(jacoco_percent(&index, &["demo.Board"], "place", 4), None);
    }
}
