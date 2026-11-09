//! Port of mutator/src/mutator/coverage.py and the crapper/src/crapper/coverage.py loaders
//! it uses (BSD Zero Clause License, Robert C. Martin).
//!
//! Reports are read with lossy UTF-8 and sorted before merging, so the result does not depend
//! on directory order. A report that cannot be used is skipped whole: an unreadable file, or a
//! Go profile with a malformed segment line (Python raised ValueError and aborted the run).
//! The JaCoCo scanner is tolerant and does not validate XML.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::bind::splitlines;
use crate::fs_util::resolve;
use crate::model::{Language, LineSet};

/// Reports keyed by the path text they were written with.
type Keyed<V> = BTreeMap<String, V>;
/// Rank of a report key against the source: `(0, extra)` key ends with the source,
/// `(1, missing)` source ends with the key. Lower is better.
type Rank = (u8, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GoSegment {
    start: usize,
    end: usize,
    hits: i64,
}

type GoProfile = Keyed<Vec<GoSegment>>;

static FORM_SPAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<span[^>]*title="(\d+) out of (\d+) forms covered"[^>]*>\s*(\d+)&nbsp;"#)
        .expect("literal pattern is valid")
});
static LCOV_DA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^DA:([0-9]+),([0-9]+)").expect("literal pattern is valid"));
static DOCTYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<!DOCTYPE[^>]*>").expect("literal pattern is valid"));
static XML_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<(/?)([A-Za-z][\w.:-]*)((?:[^>"']|"[^"]*"|'[^']*')*)>"#)
        .expect("literal pattern is valid")
});
static XML_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([A-Za-z_:][\w.:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
        .expect("literal pattern is valid")
});

/// Lines with a hit, or None when this file is absent from coverage.
pub fn covered_lines(root: &Path, source_path: &Path, language: Language) -> Option<LineSet> {
    let path = resolve(source_path).to_string_lossy().into_owned();
    match language {
        Language::Java => lookup(&jacoco_index(root), &path).cloned(),
        Language::Go => go_lines(&go_profile(root), &path),
        Language::Clojure => {
            let html = lookup(&form_html(root), &path).cloned();
            html.or_else(|| lookup(&lcov_index(root), &path).cloned())
        }
        _ => lookup(&lcov_index(root), &path).cloned(),
    }
}

fn read_lossy(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Report texts in order, each real file once (by resolved path). Unreadable files are dropped.
fn unique_texts(paths: &[PathBuf]) -> Vec<String> {
    let mut seen = HashSet::new();
    paths
        .iter()
        .filter(|path| seen.insert(resolve(path)))
        .filter_map(|path| read_lossy(path))
        .collect()
}

fn percent_decode(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        rest = tail;
        match hex_pair(tail) {
            Some(decoded) if byte == b'%' => {
                out.push(decoded);
                rest = &tail[2..];
            }
            _ => out.push(byte),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_pair(bytes: &[u8]) -> Option<u8> {
    match bytes {
        [high, low, ..] => Some(hex_digit(*high)? * 16 + hex_digit(*low)?),
        _ => None,
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

/// Percent-decodes, makes backslashes slashes, drops `file:` and leading `./`, and collapses `//`.
fn normalize_path(path: &str) -> String {
    let decoded = percent_decode(path).replace('\\', "/");
    let mut rest = decoded.as_str();
    if let Some(stripped) = rest.strip_prefix("file:") {
        rest = stripped;
    }
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    let mut text = rest.to_string();
    while text.contains("//") {
        text = text.replace("//", "/");
    }
    text
}

/// The source path as written and as resolved, normalized, without empty or repeated values.
fn candidate_paths(source: &str) -> Vec<String> {
    if source.is_empty() {
        return Vec::new();
    }
    let relative = normalize_path(source);
    let absolute = normalize_path(&resolve(Path::new(source)).to_string_lossy());
    let mut found: Vec<String> = Vec::new();
    for value in [relative, absolute] {
        if !value.is_empty() && !found.contains(&value) {
            found.push(value);
        }
    }
    found
}

fn path_parts(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

fn source_parts(candidates: &[String]) -> Vec<Vec<&str>> {
    let mut sources: Vec<Vec<&str>> = Vec::new();
    for parts in candidates.iter().map(|candidate| path_parts(candidate)) {
        if !sources.contains(&parts) {
            sources.push(parts);
        }
    }
    sources
}

/// Finds the report value for a source path: an exact candidate first, then the best suffix.
fn lookup<'a, V>(index: &'a Keyed<V>, source: &str) -> Option<&'a V> {
    if index.is_empty() {
        return None;
    }
    let normalized: BTreeMap<String, &'a V> = index
        .iter()
        .map(|(key, value)| (normalize_path(key), value))
        .collect();
    let candidates = candidate_paths(source);
    exact_match(&normalized, &candidates)
        .or_else(|| best_match(&normalized, &source_parts(&candidates)))
}

fn exact_match<V: Copy>(normalized: &BTreeMap<String, V>, candidates: &[String]) -> Option<V> {
    candidates
        .iter()
        .find_map(|candidate| normalized.get(candidate).copied())
}

/// How many extra leading parts `longer` has before `shorter`, when it ends with it.
fn extra_parts(longer: &[&str], shorter: &[&str]) -> Option<usize> {
    (longer.len() >= shorter.len() && longer.ends_with(shorter))
        .then(|| longer.len() - shorter.len())
}

fn suffix_rank(key: &[&str], source: &[&str]) -> Option<Rank> {
    if key.is_empty() || source.is_empty() {
        return None;
    }
    if let Some(extra) = extra_parts(key, source) {
        return Some((0, extra));
    }
    extra_parts(source, key).map(|missing| (1, missing))
}

fn best_rank(key: &[&str], sources: &[Vec<&str>]) -> Option<Rank> {
    sources
        .iter()
        .filter_map(|source| suffix_rank(key, source))
        .min()
}

/// The single best-ranked value. Two different key paths at the best rank are ambiguous: None.
fn best_match<V: Copy>(normalized: &BTreeMap<String, V>, sources: &[Vec<&str>]) -> Option<V> {
    let mut best: Option<Rank> = None;
    let mut matches: Vec<(Vec<&str>, V)> = Vec::new();
    for (key, value) in normalized {
        let parts = path_parts(key);
        let Some(rank) = best_rank(&parts, sources) else {
            continue;
        };
        if best.is_some_and(|current| rank > current) {
            continue;
        }
        if best != Some(rank) {
            best = Some(rank);
            matches.clear();
        }
        matches.push((parts, *value));
    }
    let (first, value) = matches.first()?;
    matches
        .iter()
        .all(|(parts, _)| parts == first)
        .then_some(*value)
}

fn record(lines: &mut LineSet, number: usize, hit: bool) {
    if hit {
        lines.insert(number);
    } else {
        lines.remove(&number);
    }
}

fn store(out: &mut Keyed<LineSet>, record: Option<(String, LineSet)>) {
    if let Some((path, lines)) = record {
        out.insert(path, lines);
    }
}

fn read_da(lines: &mut LineSet, line: &str) {
    let Some(caps) = LCOV_DA.captures(line) else {
        return;
    };
    let Ok(number) = caps[1].parse::<usize>() else {
        return;
    };
    record(lines, number, caps[2].bytes().any(|digit| digit != b'0'));
}

/// LCOV `SF:` records to the lines whose last `DA` hit count is above zero.
fn parse_lcov(text: &str) -> Keyed<LineSet> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, LineSet)> = None;
    for raw in splitlines(text) {
        let line = raw.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            store(&mut out, current.take());
            current = Some((path.to_string(), LineSet::new()));
        } else if line == "end_of_record" {
            store(&mut out, current.take());
        } else if let Some((_, lines)) = current.as_mut() {
            read_da(lines, line);
        }
    }
    store(&mut out, current.take());
    out
}

fn parse_form_coverage(html: &str) -> LineSet {
    let mut lines = LineSet::new();
    for caps in FORM_SPAN.captures_iter(html) {
        let (Ok(covered), Ok(total), Ok(line)) = (
            caps[1].parse::<usize>(),
            caps[2].parse::<usize>(),
            caps[3].parse::<usize>(),
        ) else {
            continue;
        };
        if total > 0 {
            record(&mut lines, line, covered > 0);
        }
    }
    lines
}

fn parse_go_segment(line: &str) -> Option<(String, GoSegment)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [position, statements, hits] = fields.as_slice() else {
        return None;
    };
    let (file, range) = position.split_once(':')?;
    let (start, end) = range.split_once(',')?;
    statements.parse::<i64>().ok()?;
    let segment = GoSegment {
        start: go_line_number(start)?,
        end: go_line_number(end)?,
        hits: hits.parse().ok()?,
    };
    Some((file.to_string(), segment))
}

fn go_line_number(position: &str) -> Option<usize> {
    position
        .split_once('.')
        .map_or(position, |(line, _)| line)
        .parse()
        .ok()
}

/// Parses a Go profile. A malformed segment line makes the whole profile an error.
fn parse_go_profile(text: &str) -> Result<GoProfile, String> {
    let mut out: GoProfile = BTreeMap::new();
    for (index, raw) in splitlines(text).into_iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("mode:") {
            continue;
        }
        let (file, segment) = parse_go_segment(line)
            .ok_or_else(|| format!("invalid coverage segment on line {}: {raw:?}", index + 1))?;
        out.entry(file).or_default().push(segment);
    }
    Ok(out)
}

fn go_lines(profile: &GoProfile, path: &str) -> Option<LineSet> {
    let segments = lookup(profile, path)?;
    Some(
        segments
            .iter()
            .filter(|segment| segment.hits > 0)
            .flat_map(|segment| segment.start..=segment.end)
            .collect(),
    )
}

/// One `<tag ...>` from the JaCoCo XML.
struct Tag<'t> {
    closing: bool,
    self_closing: bool,
    name: &'t str,
    attrs: BTreeMap<&'t str, String>,
}

impl<'t> Tag<'t> {
    fn from_captures(caps: &Captures<'t>) -> Tag<'t> {
        let body = caps.get(3).map_or("", |m| m.as_str());
        Tag {
            closing: caps.get(1).is_some_and(|m| m.as_str() == "/"),
            self_closing: body.trim_end().ends_with('/'),
            name: caps.get(2).map_or("", |m| m.as_str()),
            attrs: attributes(body),
        }
    }

    fn attr(&self, name: &str) -> &str {
        self.attrs.get(name).map_or("", String::as_str)
    }
}

fn attributes(body: &str) -> BTreeMap<&str, String> {
    XML_ATTR
        .captures_iter(body)
        .map(|caps| {
            let name = caps.get(1).map_or("", |m| m.as_str());
            let value = caps
                .get(2)
                .or_else(|| caps.get(3))
                .map_or("", |m| m.as_str());
            (name, unescape(value))
        })
        .collect()
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// A JaCoCo count attribute. A missing or empty value counts as 0; other text is invalid.
fn int_attr(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        Some(0)
    } else {
        text.parse().ok()
    }
}

/// The line number of a JaCoCo `<line>` that has covered instructions or branches.
fn covered_line(tag: &Tag<'_>) -> Option<usize> {
    let number = int_attr(tag.attr("nr"))?;
    let instructions = int_attr(tag.attr("ci"))?;
    let branches = int_attr(tag.attr("cb"))?;
    if instructions > 0 || branches > 0 {
        usize::try_from(number).ok()
    } else {
        None
    }
}

#[derive(Default)]
struct JacocoScan {
    package: Option<String>,
    source: Option<(String, LineSet)>,
    found: Keyed<LineSet>,
}

impl JacocoScan {
    fn tag(&mut self, tag: &Tag<'_>) {
        match tag.name {
            "package" => self.package_tag(tag),
            "sourcefile" => self.sourcefile_tag(tag),
            "line" => self.line_tag(tag),
            _ => {}
        }
    }

    fn package_tag(&mut self, tag: &Tag<'_>) {
        if tag.closing {
            self.package = None;
        } else if !tag.self_closing {
            self.package = Some(tag.attr("name").trim_matches('/').to_string());
        }
    }

    fn sourcefile_tag(&mut self, tag: &Tag<'_>) {
        if tag.closing {
            self.commit();
        } else if let Some(key) = self.source_key(tag.attr("name")) {
            self.source = Some((key, LineSet::new()));
            if tag.self_closing {
                self.commit();
            }
        }
    }

    fn line_tag(&mut self, tag: &Tag<'_>) {
        let Some(number) = covered_line(tag) else {
            return;
        };
        if let Some((_, lines)) = self.source.as_mut() {
            lines.insert(number);
        }
    }

    /// `package/file` for a source inside a package; a source outside any package is not indexed.
    fn source_key(&self, filename: &str) -> Option<String> {
        let package = self.package.as_deref()?;
        Some(if package.is_empty() {
            filename.to_string()
        } else {
            format!("{package}/{filename}")
        })
    }

    fn commit(&mut self) {
        if let Some((key, lines)) = self.source.take() {
            self.found.insert(key, lines);
        }
    }
}

/// JaCoCo lines keyed by `package/file`, after removing `<!DOCTYPE ...>`.
fn scan_jacoco(text: &str) -> Keyed<LineSet> {
    let cleaned = DOCTYPE.replace_all(text, "");
    let mut scan = JacocoScan::default();
    for caps in XML_TAG.captures_iter(&cleaned) {
        scan.tag(&Tag::from_captures(&caps));
    }
    scan.found
}

fn child_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = dir_entries(dir).iter().map(fs::DirEntry::path).collect();
    paths.sort();
    paths
}

fn dir_entries(dir: &Path) -> Vec<fs::DirEntry> {
    fs::read_dir(dir)
        .map(|reader| reader.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Every regular file under `dir`, sorted. Symlinked directories are not followed.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in dir_entries(&current) {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
            } else if path.is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn named_files_under(dir: &Path, name: &str) -> Vec<PathBuf> {
    files_under(dir)
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|file| file == name))
        .collect()
}

/// `relative` under `root`, then under each child, then under each grandchild (`*/x`, `*/*/x`).
fn nested_reports(root: &Path, relative: &str) -> Vec<PathBuf> {
    let children = child_paths(root);
    let mut paths: Vec<PathBuf> = children.iter().map(|dir| dir.join(relative)).collect();
    paths.extend(
        children
            .iter()
            .flat_map(|dir| child_paths(dir))
            .map(|dir| dir.join(relative)),
    );
    paths
}

const JACOCO_REPORT: &str = "target/site/jacoco/jacoco.xml";
const GO_PROFILE: &str = "target/coverage/go/coverage.out";

fn jacoco_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![root.join(JACOCO_REPORT)];
    paths.extend(nested_reports(root, JACOCO_REPORT));
    paths
}

fn go_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        root.join("target/coverage/coverage.out"),
        root.join(GO_PROFILE),
        root.join("coverage.out"),
    ];
    paths.extend(nested_reports(root, GO_PROFILE));
    paths
}

fn lcov_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = named_files_under(&root.join("target/coverage"), "lcov.info");
    paths.extend(named_files_under(&root.join("coverage"), "lcov.info"));
    paths
}

fn jacoco_index(root: &Path) -> Keyed<LineSet> {
    let mut found = BTreeMap::new();
    for text in unique_texts(&jacoco_paths(root)) {
        found.extend(scan_jacoco(&text));
    }
    found
}

fn lcov_index(root: &Path) -> Keyed<LineSet> {
    let mut merged = BTreeMap::new();
    for text in unique_texts(&lcov_paths(root)) {
        merged.extend(parse_lcov(&text));
    }
    merged
}

fn go_profile(root: &Path) -> GoProfile {
    let mut merged: GoProfile = BTreeMap::new();
    for text in unique_texts(&go_paths(root)) {
        // A profile with a malformed segment is skipped whole, not merged partly.
        let Ok(profile) = parse_go_profile(&text) else {
            continue;
        };
        for (file, segments) in profile {
            merged.entry(file).or_default().extend(segments);
        }
    }
    merged
}

/// Cloverage form counts for one `.html` under `target/coverage`, keyed by its path without `.html`.
fn form_entry(coverage: &Path, path: &Path) -> Option<(String, LineSet)> {
    let relative = path.strip_prefix(coverage).ok()?.to_string_lossy();
    let key = relative.strip_suffix(".html")?;
    let text = read_lossy(path)?;
    text.contains("forms covered")
        .then(|| (key.to_string(), parse_form_coverage(&text)))
}

fn form_html(root: &Path) -> Keyed<LineSet> {
    let coverage = root.join("target").join("coverage");
    files_under(&coverage)
        .into_iter()
        .filter_map(|path| form_entry(&coverage, &path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const PY_LCOV: &str = "target/coverage/python/lcov.info";
    const GO_COVER: &str = "target/coverage/coverage.out";
    const APP: &str = "def place():\n    return 1\n";

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap_or(root)).expect("create test dirs");
        fs::write(path, text).expect("write test file");
    }

    fn write_source(root: &Path, relative: &str) -> PathBuf {
        write(root, relative, APP);
        root.join(relative)
    }

    fn absolute(path: &Path) -> String {
        fs::canonicalize(path)
            .expect("canonical test path")
            .to_string_lossy()
            .into_owned()
    }

    fn lines(numbers: &[usize]) -> Option<LineSet> {
        Some(numbers.iter().copied().collect())
    }

    #[test]
    fn mu_10_lcov_exact_absolute_match() {
        // MU-10: an LCOV SF with the absolute source path matches that source.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        let report = format!("SF:{}\nDA:1,3\nend_of_record\n", absolute(&source));
        write(dir.path(), PY_LCOV, &report);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[1])
        );
    }

    #[test]
    fn mu_10_lcov_report_path_shorter_than_source_matches() {
        // MU-10: a report key that is a suffix of the absolute source path matches it.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        write(
            dir.path(),
            PY_LCOV,
            "SF:src/demo/app.py\nDA:1,1\nend_of_record\n",
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[1])
        );
    }

    #[test]
    fn mu_10_lcov_report_path_longer_than_source_matches() {
        // MU-10: a report key that ends with the source path matches it; `//` is collapsed.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        let report = format!("SF:project/{}\nDA:1,1\nend_of_record\n", absolute(&source));
        write(dir.path(), PY_LCOV, &report);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[1])
        );
    }

    #[test]
    fn mu_10_ambiguous_suffix_has_no_coverage() {
        // MU-10: two different report keys at the same best rank are ambiguous: None.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        let abs = absolute(&source);
        let report =
            format!("SF:one/{abs}\nDA:1,1\nend_of_record\nSF:two/{abs}\nDA:1,1\nend_of_record\n");
        write(dir.path(), PY_LCOV, &report);
        assert_eq!(covered_lines(dir.path(), &source, Language::Python), None);
    }

    #[test]
    fn mu_10_shorter_report_path_does_not_steal_longer_hits() {
        // MU-10: a shorter key never takes the hits of a longer key that also ends the source.
        let dir = TempDir::new().expect("temp dir");
        let short = write_source(dir.path(), "b/app.py");
        let long = write_source(dir.path(), "a/b/app.py");
        let report = "SF:b/app.py\nDA:2,1\nend_of_record\nSF:a/b/app.py\nDA:1,1\nend_of_record\n";
        write(dir.path(), PY_LCOV, report);
        assert_eq!(
            covered_lines(dir.path(), &long, Language::Python),
            lines(&[1])
        );
        assert_eq!(
            covered_lines(dir.path(), &short, Language::Python),
            lines(&[2])
        );
    }

    #[test]
    fn mu_10_percent_encoded_report_path_matches() {
        // MU-10: a percent-encoded SF path is decoded before matching the source file name.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/my demo/app.py");
        write(
            dir.path(),
            PY_LCOV,
            "SF:src/my%20demo/app.py\nDA:1,1\nend_of_record\n",
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[1])
        );
    }

    #[test]
    fn mu_10_file_missing_from_report_is_none() {
        // MU-10: a source file that no report names has no coverage data.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        write(
            dir.path(),
            PY_LCOV,
            "SF:src/other.py\nDA:1,1\nend_of_record\n",
        );
        assert_eq!(covered_lines(dir.path(), &source, Language::Python), None);
    }

    #[test]
    fn mu_10_normalize_path_decodes_scheme_dot_prefix_and_slashes() {
        // MU-10: percent-decoding, backslashes, `file:`, `./` and `//` are normalized.
        assert_eq!(
            normalize_path("file:./src//demo\\app%2Epy"),
            "src/demo/app.py"
        );
    }

    #[test]
    fn mu_10_candidate_paths_skip_empty_and_repeated_values() {
        // MU-10: no candidates for an empty path; an absolute path that does not exist is one.
        assert!(candidate_paths("").is_empty());
        assert_eq!(candidate_paths("/no/such/app.py"), vec!["/no/such/app.py"]);
    }

    #[test]
    fn mu_10_go_profile_ranges_expand_to_hit_lines() {
        // MU-10: each hit segment marks every line in its range; a missing file is None.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "demo/widget.go");
        let profile = "mode: set\ndemo/widget.go:2.1,4.2 1 1\ndemo/widget.go:5.1,6.2 1 0\n";
        write(dir.path(), GO_COVER, profile);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Go),
            lines(&[2, 3, 4])
        );
        let missing = dir.path().join("other/missing.go");
        assert_eq!(covered_lines(dir.path(), &missing, Language::Go), None);
    }

    #[test]
    fn mu_10_go_without_profile_is_none() {
        // MU-10: with no profile at all, every Go file is absent from coverage.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "demo/widget.go");
        assert_eq!(covered_lines(dir.path(), &source, Language::Go), None);
    }

    #[test]
    fn mu_10_go_profile_does_not_cross_files() {
        // MU-10: a shorter profile path never takes the segments of a longer one.
        let dir = TempDir::new().expect("temp dir");
        let short = write_source(dir.path(), "b/widget.go");
        let long = write_source(dir.path(), "a/b/widget.go");
        write(
            dir.path(),
            GO_COVER,
            "mode: set\nb/widget.go:2.1,2.2 1 1\na/b/widget.go:3.1,3.2 1 0\n",
        );
        assert_eq!(covered_lines(dir.path(), &short, Language::Go), lines(&[2]));
        assert_eq!(covered_lines(dir.path(), &long, Language::Go), lines(&[]));
    }

    #[test]
    fn mu_10_go_invalid_segment_skips_only_its_report() {
        // MU-10: a profile with a malformed segment is skipped whole; other profiles still count.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "demo/widget.go");
        write(
            dir.path(),
            GO_COVER,
            "mode: set\ndemo/widget.go:2.1,2.2 1 1\n",
        );
        write(
            dir.path(),
            GO_PROFILE,
            "mode: set\ndemo/widget.go:3.1,3.2 1 1\nbroken\n",
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Go),
            lines(&[2])
        );
    }

    #[test]
    fn mu_10_jacoco_marks_lines_with_covered_instructions_or_branches() {
        // MU-10: a JaCoCo line counts when ci > 0 or cb > 0; lines 3 and 5 are covered.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/Board.java");
        let report = r#"<?xml version="1.0"?>
<report name="demo">
  <package name="demo">
    <sourcefile name="Board.java">
      <line nr="3" mi="0" ci="2" mb="0" cb="0"/>
      <line nr="4" mi="4" ci="0" mb="1" cb="0"/>
      <line nr="5" mi="0" ci="0" mb="0" cb="1"/>
    </sourcefile>
  </package>
</report>
"#;
        write(dir.path(), JACOCO_REPORT, report);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Java),
            lines(&[3, 5])
        );
    }

    #[test]
    fn mu_10_jacoco_index_keys_by_package_name() {
        // MU-10: JaCoCo sources are keyed as `package/file`, and a Java lookup finds them.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/Board.java");
        let report = r#"<report name="demo"><package name="demo">
<sourcefile name="Board.java"><line nr="6" mi="0" ci="1" mb="0" cb="0"/></sourcefile>
</package></report>"#;
        write(dir.path(), JACOCO_REPORT, report);
        assert!(jacoco_index(dir.path()).contains_key("demo/Board.java"));
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Java),
            lines(&[6])
        );
    }

    #[test]
    fn mu_10_jacoco_doctype_is_removed_before_scanning() {
        // MU-10: a DOCTYPE declaration does not hide the report contents.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/Board.java");
        let report = r#"<?xml version="1.0"?>
<!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.0//EN" "report.dtd">
<report name="demo"><package name="demo"><sourcefile name="Board.java">
<line nr="3" mi="0" ci="2" mb="0" cb="0"/></sourcefile></package></report>"#;
        write(dir.path(), JACOCO_REPORT, report);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Java),
            lines(&[3])
        );
    }

    #[test]
    fn mu_10_clojure_prefers_cloverage_html_over_lcov() {
        // MU-10: a Cloverage form count for the file wins over the LCOV record for it.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/core.clj");
        let html = "<span title=\"1 out of 1 forms covered\">4&nbsp;</span>\n";
        write(dir.path(), "target/coverage/src/demo/core.clj.html", html);
        write(
            dir.path(),
            "target/coverage/clojure/lcov.info",
            "SF:src/demo/core.clj\nDA:9,1\nend_of_record\n",
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Clojure),
            lines(&[4])
        );
    }

    #[test]
    fn mu_10_clojure_falls_back_to_lcov_without_html() {
        // MU-10: without a Cloverage report for the file, Clojure uses its LCOV record.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/core.clj");
        write(
            dir.path(),
            PY_LCOV,
            "SF:src/demo/core.clj\nDA:9,1\nend_of_record\n",
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Clojure),
            lines(&[9])
        );
    }

    #[test]
    fn mu_16_unhit_lcov_lines_are_not_covered() {
        // MU-16: a line with zero hits is absent from the covered set, so its sites stay uncovered.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        write(
            dir.path(),
            PY_LCOV,
            "SF:src/demo/app.py\nDA:1,1\nDA:2,0\nDA:3,4\nend_of_record\n",
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[1, 3])
        );
    }

    #[test]
    fn mu_16_later_da_record_overrides_earlier_one() {
        // MU-16: the last DA record for a line decides whether it is covered.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        write(
            dir.path(),
            PY_LCOV,
            "SF:src/demo/app.py\nDA:1,1\nDA:1,0\nDA:3,1\nend_of_record\n",
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[3])
        );
    }

    #[test]
    fn mu_16_zero_hit_and_empty_form_spans_are_uncovered() {
        // MU-16: a Cloverage span with zero covered forms is not covered; a span with zero total
        // forms is ignored.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/core.clj");
        let html = concat!(
            "<span title=\"0 out of 2 forms covered\">3&nbsp;</span>\n",
            "<span title=\"2 out of 2 forms covered\">5&nbsp;</span>\n",
            "<span title=\"1 out of 0 forms covered\">7&nbsp;</span>\n",
        );
        write(dir.path(), "target/coverage/src/demo/core.clj.html", html);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Clojure),
            lines(&[5])
        );
    }

    #[test]
    fn mu_16_file_without_hit_lines_is_present_and_empty() {
        // MU-16: a report record with no DA lines still names the file, with nothing covered.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        write(dir.path(), PY_LCOV, "SF:src/demo/app.py\nend_of_record\n");
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[])
        );
    }

    #[test]
    fn mu_10_report_key_equal_in_length_to_source_matches() {
        // MU-10: a report key with the same path parts as the source, but no leading slash,
        // matches it: equal length is still a suffix.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        let key = absolute(&source).trim_start_matches('/').to_string();
        write(
            dir.path(),
            PY_LCOV,
            &format!("SF:{key}\nDA:1,1\nend_of_record\n"),
        );
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[1])
        );
    }

    #[test]
    fn mu_10_longer_report_key_outranks_shorter_suffix() {
        // MU-10: a key with extra leading parts (rank 0) beats a key one part short of the source
        // (rank 1), even though the short key has fewer missing parts.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/app.py");
        let abs = absolute(&source);
        let (_, short) = abs
            .trim_start_matches('/')
            .split_once('/')
            .expect("nested test path");
        let longer = format!("x/y{abs}");
        let report =
            format!("SF:{longer}\nDA:1,1\nend_of_record\nSF:{short}\nDA:2,1\nend_of_record\n");
        write(dir.path(), PY_LCOV, &report);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Python),
            lines(&[1])
        );
    }

    #[test]
    fn mu_10_empty_report_key_matches_nothing() {
        // MU-10: an empty report key has no path parts, so it is never a suffix of a source.
        let mut index: Keyed<LineSet> = BTreeMap::new();
        index.insert(String::new(), LineSet::new());
        assert!(lookup(&index, "/no/such/app.py").is_none());
    }

    #[test]
    fn mu_10_root_source_matches_no_report_key() {
        // MU-10: a source with no path parts ("/") has no suffix to match any report key.
        let mut index: Keyed<LineSet> = BTreeMap::new();
        index.insert("a/b.rs".to_string(), LineSet::new());
        assert!(lookup(&index, "/").is_none());
    }

    #[test]
    fn mu_10_go_malformed_segment_error_names_its_line() {
        // MU-10: the parse error reports the 1-based line number of the malformed segment.
        let error = parse_go_profile("mode: set\nbroken\n").expect_err("malformed segment");
        assert!(error.contains("on line 2:"), "{error}");
    }

    #[test]
    fn mu_16_jacoco_empty_counts_are_zero() {
        // MU-16: an empty or missing ci/cb count is 0, so that line has no covered instructions.
        let dir = TempDir::new().expect("temp dir");
        let source = write_source(dir.path(), "src/demo/Board.java");
        let report = r#"<report name="demo"><package name="demo"><sourcefile name="Board.java">
<line nr="3" mi="0" ci="" mb="0" cb=""/>
<line nr="4" mi="0" ci="1" mb="0" cb=""/>
</sourcefile></package></report>"#;
        write(dir.path(), JACOCO_REPORT, report);
        assert_eq!(
            covered_lines(dir.path(), &source, Language::Java),
            lines(&[4])
        );
    }
}
