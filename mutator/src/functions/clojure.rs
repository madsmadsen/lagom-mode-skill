//! Port of crapper languages/clojure.py without complexity (BSD Zero Clause License, Robert C. Martin).

use std::sync::OnceLock;

use regex::Regex;

use crate::model::Function;

/// Characters that end a metadata or defn name (Python `_NAME_STOP`).
const NAME_STOP: &[char] = &[' ', '\t', '\n', '\r', '(', ')', '[', ']', '{', '}', '"'];
/// Characters that end an atom or cannot start a name.
const DELIMITERS: &[char] = &['(', ')', '[', ']', '{', '}', '"', ';'];
/// Characters that end a character literal.
const CHAR_DELIMITERS: &[char] = &['(', ')', '[', ']', '{', '}', '"', ';', ','];
/// Suffixes removed from a path to get a namespace; longest first.
const SOURCE_EXTENSIONS: &[&str] = &[".cljc", ".cljs", ".clj", ".bb"];

/// One top-level `defn` or `defn-` form: its name and 1-based line span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormSpan {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Functions declared in a Clojure source file, in source order.
pub fn functions_in_source(source: &str, path: &str, project_root: Option<&str>) -> Vec<Function> {
    let namespace =
        declared_namespace(source).unwrap_or_else(|| namespace_from_path(path, project_root));
    extract_functions(source)
        .into_iter()
        .map(|form| Function {
            name: form.name,
            namespace: namespace.clone(),
            start_line: form.start_line,
            end_line: form.end_line,
            start_byte: None,
            end_byte: None,
        })
        .collect()
}

/// Top-level `defn` and `defn-` forms, found by a reader-aware depth scan.
pub fn extract_functions(source: &str) -> Vec<FormSpan> {
    let src: Vec<char> = source.chars().collect();
    let mut scan = FormScan::default();
    let mut index = 0;
    let mut line = 1;
    while index < src.len() {
        if let Some((end, added)) = skip_reader(&src, index) {
            index = end;
            line += added;
            continue;
        }
        match src[index] {
            '(' => scan.open(index, line),
            ')' => scan.close(&src, index, line),
            '\n' => line += 1,
            _ => {}
        }
        index += 1;
    }
    scan.forms
}

/// Namespace from the first `ns`, `in-ns`, or `in-ns (quote ...)` form on blanked text.
pub fn declared_namespace(source: &str) -> Option<String> {
    let chars: Vec<char> = source.chars().collect();
    let clean = without_strings_and_comments(&chars);
    namespace_patterns()
        .iter()
        .find_map(|pattern| pattern.captures(&clean)?.get(1))
        .map(|name| name.as_str().to_owned())
}

/// Namespace derived from a source path relative to an optional project root.
pub fn namespace_from_path(path: &str, source_root: Option<&str>) -> String {
    let relative = path.replace('\\', "/");
    let body = strip_src(strip_root(&relative, source_root));
    strip_extension(body).replace('/', ".").replace('_', "-")
}

/// Python `str.isspace`, which also counts U+001C..U+001F.
fn is_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Whether `src` contains `pattern` starting at `index`.
fn at(src: &[char], index: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, ch)| src.get(index + offset) == Some(&ch))
}

/// Index of the first char at or after `index` that fails `keep`, or the length.
fn run_while(src: &[char], index: usize, keep: impl Fn(char) -> bool) -> usize {
    let rest = src.get(index..).unwrap_or(&[]);
    index + rest.iter().take_while(|&&ch| keep(ch)).count()
}

fn skip_space(src: &[char], index: usize) -> usize {
    run_while(src, index, is_space)
}

/// Index of the newline that ends the comment starting at `index`, or the length.
fn comment_stop(src: &[char], index: usize) -> usize {
    run_while(src, index, |ch| ch != '\n')
}

fn closer_of(opener: char) -> char {
    match opener {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

/// Index just after the string opened by the `"` at `open`, or the length.
fn string_end(src: &[char], open: usize) -> usize {
    let mut escaped = false;
    for (offset, &ch) in src.iter().enumerate().skip(open + 1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return offset + 1;
        }
    }
    src.len()
}

/// Index just after the character literal whose backslash is at `backslash`.
fn char_end(src: &[char], backslash: usize) -> usize {
    let first = backslash + 1;
    if first >= src.len() {
        return src.len();
    }
    run_while(src, first + 1, |ch| {
        !is_space(ch) && !CHAR_DELIMITERS.contains(&ch)
    })
}

/// End of the string, character literal, or comment at `index`; None for code.
fn literal_end(src: &[char], index: usize) -> Option<usize> {
    match src[index] {
        '"' => Some(string_end(src, index)),
        '\\' => Some(char_end(src, index)),
        ';' => Some(comment_stop(src, index)),
        _ => None,
    }
}

/// Depth change of one step in a balanced scan, and the index after it.
fn balance_step(src: &[char], index: usize, opener: char, closer: char) -> (usize, isize) {
    if let Some(next) = literal_end(src, index) {
        return (next, 0);
    }
    let ch = src[index];
    let delta = if ch == opener {
        1
    } else if ch == closer {
        -1
    } else {
        0
    };
    (index + 1, delta)
}

/// Index just after the bracket that closes the one opened at `open`.
fn balanced_end(src: &[char], open: usize) -> usize {
    let opener = src[open];
    let closer = closer_of(opener);
    let mut depth: isize = 0;
    let mut index = open;
    while index < src.len() {
        let (next, delta) = balance_step(src, index, opener, closer);
        index = next;
        depth += delta;
        if depth == 0 {
            break;
        }
    }
    index
}

/// Index of the first char at or after `index` that is not whitespace or a comment.
fn skip_ignorable(src: &[char], mut index: usize) -> usize {
    loop {
        let next = skip_space_and_comment(src, index);
        if next == index {
            return index;
        }
        index = next;
    }
}

fn skip_space_and_comment(src: &[char], index: usize) -> usize {
    let after = skip_space(src, index);
    if src.get(after) == Some(&';') {
        comment_stop(src, after)
    } else {
        after
    }
}

/// Index just after the form at or after `index`, skipping leading whitespace and comments.
fn form_end(src: &[char], index: usize) -> usize {
    let start = skip_ignorable(src, index);
    if start >= src.len() {
        return start;
    }
    one_form_end(src, start)
}

fn one_form_end(src: &[char], index: usize) -> usize {
    match src[index] {
        '(' | '[' | '{' => balanced_end(src, index),
        '"' => string_end(src, index),
        '\\' => char_end(src, index),
        _ if at(src, index, "#_") => form_end(src, index + 2),
        _ => run_while(src, index, |ch| !is_space(ch) && !DELIMITERS.contains(&ch)),
    }
}

/// End of a string, comment, character literal, or `#_` form at `index`, with its newline count.
fn skip_reader(src: &[char], index: usize) -> Option<(usize, usize)> {
    match src[index] {
        ';' => Some(span_with_lines(
            src,
            index,
            (comment_stop(src, index) + 1).min(src.len()),
        )),
        '"' => Some(span_with_lines(src, index, string_end(src, index))),
        '\\' => Some((char_end(src, index), 0)),
        '#' if at(src, index, "#_") => Some(span_with_lines(src, index, form_end(src, index + 2))),
        _ => None,
    }
}

fn span_with_lines(src: &[char], start: usize, end: usize) -> (usize, usize) {
    let lines = src[start..end].iter().filter(|&&ch| ch == '\n').count();
    (end, lines)
}

/// Index of the masked end for a string, comment, character literal, or `#_` form.
fn masked_end(src: &[char], index: usize) -> Option<usize> {
    if at(src, index, "#_") {
        Some(form_end(src, index + 2))
    } else {
        literal_end(src, index)
    }
}

fn blank(ch: char) -> char {
    if ch == '\n' { '\n' } else { ' ' }
}

/// Source with strings, comments, character literals, and `#_` forms blanked to spaces.
fn without_strings_and_comments(src: &[char]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut index = 0;
    while index < src.len() {
        match masked_end(src, index) {
            Some(end) => {
                out.extend(src[index..end].iter().map(|&ch| blank(ch)));
                index = end;
            }
            None => {
                out.push(src[index]);
                index += 1;
            }
        }
    }
    out
}

fn namespace_patterns() -> &'static [Regex; 3] {
    static PATTERNS: OnceLock<[Regex; 3]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"\(\s*ns\s+([A-Za-z0-9*+!_?.\-/]+)",
            r"\(\s*in-ns\s+'([A-Za-z0-9*+!_?.\-/]+)\s*\)",
            r"\(\s*in-ns\s+\(quote\s+([A-Za-z0-9*+!_?.\-/]+)\)\s*\)",
        ]
        .map(|pattern| Regex::new(pattern).expect("static namespace pattern is valid"))
    })
}

fn strip_root<'a>(relative: &'a str, source_root: Option<&str>) -> &'a str {
    let root = source_root.unwrap_or("").replace('\\', "/");
    let root = root.trim_end_matches('/');
    if root.is_empty() || root == "." {
        return relative;
    }
    relative
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(relative)
}

fn strip_src(relative: &str) -> &str {
    if let Some(rest) = relative.strip_prefix("src/") {
        rest
    } else if let Some((_, rest)) = relative.split_once("/src/") {
        rest
    } else {
        relative
    }
}

fn strip_extension(relative: &str) -> &str {
    SOURCE_EXTENSIONS
        .iter()
        .find_map(|ext| relative.strip_suffix(ext))
        .unwrap_or(relative)
}

/// Whether `index` in `form` starts the `defn` or `defn-` keyword; returns the index after it.
fn defn_keyword_end(form: &[char], index: usize) -> Option<usize> {
    ["defn-", "defn"].into_iter().find_map(|keyword| {
        let after = index + keyword.len();
        (at(form, index, keyword) && keyword_ends(form, after)).then_some(after)
    })
}

fn keyword_ends(form: &[char], after: usize) -> bool {
    after >= form.len() || NAME_STOP.contains(&form[after])
}

fn skip_metadata(form: &[char], caret: usize) -> usize {
    let index = caret + 1;
    if form.get(index) == Some(&'{') {
        return balanced_end(form, index);
    }
    run_while(form, index, |ch| !NAME_STOP.contains(&ch))
}

fn skip_leading_metadata(form: &[char], mut index: usize) -> usize {
    loop {
        index = skip_space(form, index);
        if form.get(index) != Some(&'^') {
            return index;
        }
        index = skip_metadata(form, index);
    }
}

fn read_name(form: &[char], index: usize) -> Option<String> {
    if index >= form.len() || DELIMITERS.contains(&form[index]) {
        return None;
    }
    let end = run_while(form, index, |ch| !NAME_STOP.contains(&ch));
    Some(form[index..end].iter().collect())
}

/// Name of a `(defn name ...)` or `(defn- name ...)` form, after leading metadata.
fn defn_name(form: &[char]) -> Option<String> {
    if form.first() != Some(&'(') {
        return None;
    }
    let keyword = defn_keyword_end(form, skip_space(form, 1))?;
    read_name(form, skip_leading_metadata(form, keyword))
}

fn defn_form(text: &[char], start_line: usize, end_line: usize) -> Option<FormSpan> {
    defn_name(text).map(|name| FormSpan {
        name,
        start_line,
        end_line,
    })
}

#[derive(Default)]
struct FormScan {
    depth: usize,
    /// Start index and start line of the open top-level form.
    form: Option<(usize, usize)>,
    forms: Vec<FormSpan>,
}

impl FormScan {
    fn open(&mut self, index: usize, line: usize) {
        if self.depth == 0 {
            self.form = Some((index, line));
        }
        self.depth += 1;
    }

    fn close(&mut self, src: &[char], index: usize, line: usize) {
        if self.depth == 1
            && let Some((start, start_line)) = self.form.take()
            && let Some(form) = defn_form(&src[start..=index], start_line, line)
        {
            self.forms.push(form);
        }
        self.depth = self.depth.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str) -> Vec<String> {
        extract_functions(source)
            .into_iter()
            .map(|form| form.name)
            .collect()
    }

    fn spans(source: &str) -> Vec<String> {
        extract_functions(source)
            .into_iter()
            .map(|form| format!("{}:{}-{}", form.name, form.start_line, form.end_line))
            .collect()
    }

    // MU-9: defn and defn- names, start/end lines, and the `(ns ...)` namespace.
    #[test]
    fn mu_9_extracts_defn_names_lines_and_namespace() {
        let source =
            "(ns demo.core)\n\n(defn choose [x]\n  (if x 1 0))\n\n(defn- hidden [y]\n  y)\n";
        let found = functions_in_source(source, "src/demo/core.clj", Some("/proj"));
        assert_eq!(
            found
                .iter()
                .map(|f| format!("{}:{}-{}", f.name, f.start_line, f.end_line))
                .collect::<Vec<_>>(),
            ["choose:3-4", "hidden:6-7"]
        );
        assert!(found.iter().all(|f| f.namespace == "demo.core"));
        assert!(
            found
                .iter()
                .all(|f| f.start_byte.is_none() && f.end_byte.is_none())
        );
    }

    // MU-9: a top-level def is not a function and closes the previous defn span.
    #[test]
    fn mu_9_top_level_def_does_not_attach_to_previous_function() {
        let source = "(defn alpha [x]\n  (if x 1 0))\n\n(def rules\n  [{:pred (fn [v]\n            (if v (when (pos? v) true) false))}])\n\n(defn omega [y]\n  y)";
        assert_eq!(names(source), ["alpha", "omega"]);
    }

    // MU-9: namespace falls back to the path when no ns form is declared.
    #[test]
    fn mu_9_namespace_falls_back_to_path() {
        assert_eq!(
            namespace_from_path("src/demo/core_util.clj", None),
            "demo.core-util"
        );
        assert_eq!(
            namespace_from_path("/proj/src/demo/core.cljc", Some("/proj")),
            "demo.core"
        );
        assert_eq!(
            namespace_from_path("/proj/src/demo/core.clj", Some("/proj/")),
            "demo.core"
        );
        assert_eq!(
            namespace_from_path("pkg/src/demo/core.bb", None),
            "demo.core"
        );
        assert_eq!(
            namespace_from_path("src\\demo\\core.cljs", None),
            "demo.core"
        );
        let found = functions_in_source(
            "(defn foo [] 1)\n",
            "src/demo/core_extra.clj",
            Some("/proj"),
        );
        assert_eq!(found[0].namespace, "demo.core-extra");
    }

    // MU-9: `(ns ...)` text inside a comment or string is not a namespace declaration.
    #[test]
    fn mu_9_ns_in_comment_or_string_is_ignored() {
        let source = ";(ns demo.wrong)\n\"(ns also.wrong)\"\n(ns demo.core)\n";
        assert_eq!(declared_namespace(source), Some("demo.core".to_string()));
        assert_eq!(declared_namespace("(defn x [] 1)"), None);
    }

    // MU-9: `in-ns` with a quoted name or a `(quote ...)` form declares the namespace.
    #[test]
    fn mu_9_in_ns_declares_namespace() {
        assert_eq!(
            declared_namespace("(in-ns 'demo.tools)\n(defn x [] 1)"),
            Some("demo.tools".to_string())
        );
        assert_eq!(
            declared_namespace("(in-ns (quote demo.q))"),
            Some("demo.q".to_string())
        );
        let found = functions_in_source("(in-ns 'demo.tools)\n(defn x [] 1)", "src/a.clj", None);
        assert_eq!(found[0].namespace, "demo.tools");
    }

    // MU-9: `defn-` declares a private name and `^:private` metadata is skipped before the name.
    #[test]
    fn mu_9_private_defn_and_metadata_names() {
        assert_eq!(
            spans("(defn- hidden [y] y)\n(defn ^:private hidden-one [] 1)\n"),
            ["hidden:1-1", "hidden-one:2-2"]
        );
        assert_eq!(spans("(defn ^{:private true} kept [] 1)"), ["kept:1-1"]);
    }

    // MU-9: parentheses inside strings do not change the form's extent.
    #[test]
    fn mu_9_strings_with_parens_stay_inside_form() {
        assert_eq!(
            spans("(defn parens [] \"(((\")\n(defn after [] 1)\n"),
            ["parens:1-1", "after:2-2"]
        );
    }

    // MU-9: a `;` comment hides parentheses and extends to the end of its line only.
    #[test]
    fn mu_9_semicolon_comments_hide_parens() {
        assert_eq!(
            spans("(defn commented [] ; ) not closed\n  1)\n(defn after [] 1)\n"),
            ["commented:1-2", "after:3-3"]
        );
    }

    // MU-9: strings with escaped quotes and a `\space` literal keep the form whole.
    #[test]
    fn mu_9_strings_comments_and_character_literals_stay_inside_one_form() {
        let source = "(defn foo [x]\n  \"say \\\"hi\\\"\n  (if x 1)\"\n  \\space\n  ;; (defn hidden [])\n  x)\n";
        assert_eq!(names(source), ["foo"]);
    }

    // MU-9: the character literal `\(` does not open a nested form.
    #[test]
    fn mu_9_character_literal_paren_does_not_open_form() {
        let source = "(defn open-paren [] \\()\n(defn after [] 1)\n";
        assert_eq!(spans(source), ["open-paren:1-1", "after:2-2"]);
    }

    // MU-9: the character literal `\;` is not a comment, and `\"` is not a string.
    #[test]
    fn mu_9_character_literals_do_not_start_comments_or_strings() {
        assert_eq!(spans("(def x #{\\\"})\n(defn foo [] 1)"), ["foo:2-2"]);
        assert_eq!(
            spans("(defn semi [] \\; 1)\n(defn b [] 2)"),
            ["semi:1-1", "b:2-2"]
        );
    }

    // MU-9: `#_` discarded forms, including nested and multi-line ones, produce no functions.
    #[test]
    fn mu_9_discards_and_metadata_cover_each_reader_form() {
        let source = r##"
#_
; gone
(defn ghost [] (if x 1 0))
#_ "string"
#_ \x
#_ symbol
#_ #_(defn inner [] (if x 1 0))
#_(defn boxed []
  "str"
  \;
  ; comment
  (if x 1 0))
(defn ^{:doc "hi" :private true} kept [] 1)
(defn ^{:flag \; :private true} also [] 1)
(defn ^{:private true ; note
  } third [] 1)
( defn spaced [] 1)
(defn)
(defn ^)
"##;
        assert_eq!(names(source), ["kept", "also", "third", "spaced"]);
        assert!(extract_functions("(defn ^{:private kept [] 1)").is_empty());
    }

    // MU-9: a discarded form keeps line counting right for the next defn.
    #[test]
    fn mu_9_discarded_multiline_form_counts_its_lines() {
        assert_eq!(spans("#_(a\n b)\n(defn k [] 1)\n"), ["k:3-3"]);
        assert_eq!(
            spans("#_(defn dropped [] 1)\n(defn kept [] 1)\n"),
            ["kept:2-2"]
        );
    }

    // MU-9: a `#_` before a defn does not swallow the defn after it.
    #[test]
    fn mu_9_reader_keeps_real_namespace_and_functions() {
        let source = r#";; (ns demo.wrong)
(ns demo.core)
#_(defn dropped [x]
  (if x 1 0))
(defn ^:private kept [x]
  #_(if x 1 0)
  1)
(defn ^{:private true} also [] 1)
(defn quoted []
  \"
  (if x 1 0))
(defn semi []
  \;
  (if true 1 2))
"#;
        assert_eq!(
            spans(source),
            ["kept:5-7", "also:8-8", "quoted:9-11", "semi:12-14"]
        );
        let found = functions_in_source(source, "src/demo/core.clj", Some("/proj"));
        assert!(found.iter().all(|f| f.namespace == "demo.core"));
    }
}
