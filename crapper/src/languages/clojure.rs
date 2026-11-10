// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/clojure.py
// license: MIT

//! Clojure cyclomatic complexity, ported from crap4clj.
//!
//! Decision points are the same forms crap4clj counts: `if` / `when` and their
//! variants, `and`, `or`, `loop`, `catch`, and each clause of `cond`, `condp`,
//! `case`, `cond->`, `cond->>`, `some->`, and `some->>`. Clojure has no
//! tree-sitter grammar here, so this module reads the forms itself.

use std::sync::LazyLock;

use regex::Regex;

use crate::model::Function;

macro_rules! static_regex {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pattern).expect("static pattern is valid"));
    };
}

static_regex!(
    DECISION,
    r"\((if-not|if-let|if-some|when-not|when-let|when-some|when-first|if|when|and|or|loop|catch)[\s\)]"
);
static_regex!(
    COND,
    r"\((some->>|some->|cond->>|cond->|cond|condp|case)[\s\)]"
);
static_regex!(NS, r"\(\s*ns\s+([A-Za-z0-9*+!_?.\-/]+)");
static_regex!(IN_NS, r"\(\s*in-ns\s+'([A-Za-z0-9*+!_?.\-/]+)\s*\)");
static_regex!(
    IN_NS_QUOTE,
    r"\(\s*in-ns\s+\(quote\s+([A-Za-z0-9*+!_?.\-/]+)\)\s*\)"
);

/// Characters that end a string, character literal, or atom.
const FORM_DELIMITERS: &[char] = &['(', ')', '[', ']', '{', '}', '"', ';'];
/// Characters that end a character literal: the delimiters plus `,`.
const CHAR_DELIMITERS: &[char] = &['(', ')', '[', ']', '{', '}', '"', ';', ','];
/// Characters that end a name read after `defn`.
const NAME_STOPS: &[char] = &[' ', '\t', '\n', '\r', '(', ')', '[', ']', '{', '}', '"'];
/// Source file suffixes removed when a namespace is taken from a path.
const SOURCE_SUFFIXES: &[&str] = &[".clj", ".cljs", ".cljc", ".bb"];

/// A top-level `defn`-style form and its complexity.
#[derive(Debug, PartialEq)]
struct Found {
    name: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
}

fn chars_of(text: &str) -> Vec<char> {
    text.chars().collect()
}

/// First index at or after `index` whose character fails `keep`, or the end of `source`.
fn skip_while(source: &[char], index: usize, keep: impl Fn(char) -> bool) -> usize {
    let start = index.min(source.len());
    source[start..]
        .iter()
        .position(|&ch| !keep(ch))
        .map_or(source.len(), |offset| start + offset)
}

fn starts_with_at(source: &[char], index: usize, prefix: &str) -> bool {
    prefix
        .chars()
        .enumerate()
        .all(|(offset, ch)| source.get(index + offset) == Some(&ch))
}

fn newlines(chars: &[char]) -> u32 {
    chars.iter().filter(|&&ch| ch == '\n').count() as u32
}

fn line_end(source: &[char], index: usize) -> usize {
    skip_while(source, index, |ch| ch != '\n')
}

fn skip_space(source: &[char], index: usize) -> usize {
    skip_while(source, index, char::is_whitespace)
}

fn skip_to_name_stop(source: &[char], index: usize) -> usize {
    skip_while(source, index, |ch| !NAME_STOPS.contains(&ch))
}

/// Index just after a string whose opening quote is at `start`.
fn string_end(source: &[char], start: usize) -> usize {
    let mut escaped = false;
    for (offset, &ch) in source[start + 1..].iter().enumerate() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return start + 2 + offset;
        }
    }
    source.len()
}

fn char_literal_end(source: &[char], index: usize) -> usize {
    skip_while(source, index + 2, |ch| {
        !(ch.is_whitespace() || CHAR_DELIMITERS.contains(&ch))
    })
}

/// Index after a string, character literal, or comment that starts at `index`.
fn skip_reader_token(source: &[char], index: usize) -> Option<usize> {
    match source[index] {
        '"' => Some(string_end(source, index)),
        '\\' => Some(char_literal_end(source, index)),
        ';' => Some(line_end(source, index)),
        _ => None,
    }
}

fn closer_of(opener: char) -> char {
    match opener {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

/// Index just after the bracket that closes the one at `start`.
fn balanced_end(source: &[char], start: usize) -> usize {
    let opener = source[start];
    let closer = closer_of(opener);
    let mut depth = 0u32;
    let mut index = start;
    while index < source.len() {
        if let Some(next) = skip_reader_token(source, index) {
            index = next;
            continue;
        }
        let ch = source[index];
        index += 1;
        if ch == opener {
            depth += 1;
        } else if ch == closer {
            depth -= 1;
            if depth == 0 {
                return index;
            }
        }
    }
    index
}

fn skip_space_and_comment(source: &[char], index: usize) -> usize {
    let after_space = skip_space(source, index);
    if source.get(after_space) == Some(&';') {
        line_end(source, after_space)
    } else {
        after_space
    }
}

fn skip_ignorable(source: &[char], mut index: usize) -> usize {
    loop {
        let next = skip_space_and_comment(source, index);
        if next == index {
            return index;
        }
        index = next;
    }
}

fn end_of_one_form(source: &[char], index: usize) -> usize {
    match source[index] {
        '(' | '[' | '{' => balanced_end(source, index),
        '"' => string_end(source, index),
        '\\' => char_literal_end(source, index),
        '#' if starts_with_at(source, index, "#_") => form_end(source, index + 2),
        _ => skip_while(source, index, |ch| {
            !ch.is_whitespace() && !FORM_DELIMITERS.contains(&ch)
        }),
    }
}

/// Index just after the next form. `index` may sit on whitespace before it.
fn form_end(source: &[char], index: usize) -> usize {
    let start = skip_ignorable(source, index);
    if start >= source.len() {
        start
    } else {
        end_of_one_form(source, start)
    }
}

fn blank_span(start: usize, end: usize, out: &mut String) {
    out.extend(std::iter::repeat_n(' ', end - start));
}

/// Copies or blanks the token at `index` into `out`; returns the index after it.
fn blank_or_copy(source: &[char], index: usize, out: &mut String) -> usize {
    if starts_with_at(source, index, "#_") {
        let end = form_end(source, index + 2);
        blank_span(index, end, out);
        return end;
    }
    let end = match source[index] {
        '"' => string_end(source, index),
        '\\' => char_literal_end(source, index),
        ';' => line_end(source, index),
        ch => {
            out.push(ch);
            return index + 1;
        }
    };
    blank_span(index, end, out);
    end
}

/// Blanks strings, comments, character literals, and `#_` discarded forms.
///
/// Character literals are blanked before `"` or `;` can be read, so `\"` does
/// not open a string and `\;` does not open a comment.
fn without_strings_and_comments(source: &[char]) -> String {
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        index = blank_or_copy(source, index, &mut out);
    }
    out
}

/// Tracks the bracket depth of a form body while counting its top-level forms.
#[derive(Default)]
struct FormCounter {
    depth: u32,
    in_form: bool,
    forms: i64,
}

impl FormCounter {
    fn new() -> Self {
        Self {
            depth: 1,
            ..Self::default()
        }
    }

    /// A form starts at this character unless a form is already in progress at top level.
    fn begin_form(&mut self) {
        if self.depth == 1 && !self.in_form {
            self.forms += 1;
        }
        self.in_form = true;
    }

    fn open(&mut self) {
        self.begin_form();
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth -= 1;
    }

    fn atom(&mut self) {
        self.begin_form();
    }

    fn space(&mut self) {
        self.in_form = self.depth != 1;
    }
}

fn count_top_level_forms(text: &[char], start: usize) -> i64 {
    let mut counter = FormCounter::new();
    for &ch in &text[start..] {
        if counter.depth == 0 {
            break;
        }
        match ch {
            '(' | '[' | '{' => counter.open(),
            ')' | ']' | '}' => counter.close(),
            _ if ch.is_whitespace() => counter.space(),
            _ => counter.atom(),
        }
    }
    counter.forms
}

/// Forms that come before the clauses of a `form_type` (the test and the expression).
fn form_skip(form_type: &str) -> i64 {
    match form_type {
        "condp" => 2,
        "case" | "cond->" | "cond->>" | "some->" | "some->>" => 1,
        _ => 0,
    }
}

/// Clauses in `remaining` forms. `case` also counts a trailing default form.
fn pairwise_clause_count(form_type: &str, remaining: i64) -> i64 {
    let base = remaining.div_euclid(2);
    if form_type == "case" && remaining.rem_euclid(2) == 1 {
        base + 1
    } else {
        base
    }
}

/// Decisions added by one `cond`-style form whose name starts at `match_start`.
fn count_clauses(text: &[char], form_type: &str, match_start: usize) -> i64 {
    let body_start = skip_while(text, match_start, |ch| !ch.is_whitespace() && ch != ')');
    let remaining = count_top_level_forms(text, body_start) - form_skip(form_type);
    match form_type {
        "some->" | "some->>" => remaining,
        _ => pairwise_clause_count(form_type, remaining),
    }
}

fn count_cond_decisions(clean: &str) -> i64 {
    let chars = chars_of(clean);
    COND.captures_iter(clean)
        .map(|caps| {
            let start = clean[..caps.get_match().start()].chars().count();
            count_clauses(&chars, &caps[1], start)
        })
        .sum()
}

/// Cyclomatic complexity of one form. Can be negative for a `condp` or `case`
/// with no clauses, as in the original.
fn cyclomatic_complexity(fn_text: &str) -> i64 {
    let clean = without_strings_and_comments(&chars_of(fn_text));
    let simple = DECISION.find_iter(&clean).count() as i64;
    1 + simple + count_cond_decisions(&clean)
}

/// The model stores complexity as `u32`, so a negative count becomes 0.
fn to_u32(complexity: i64) -> u32 {
    u32::try_from(complexity.max(0)).unwrap_or(u32::MAX)
}

/// Index just after the `(` / `"` / `\` / `#_` reader token at `index`, with the line count after it.
fn skip_reader(source: &[char], index: usize, line: u32) -> Option<(usize, u32)> {
    let end = match source[index] {
        '\\' => return Some((char_literal_end(source, index), line)),
        ';' => line_end(source, index),
        '"' => string_end(source, index),
        '#' if starts_with_at(source, index, "#_") => form_end(source, index + 2),
        _ => return None,
    };
    Some((end, line + newlines(&source[index..end])))
}

fn skip_metadata(form: &[char], caret: usize) -> usize {
    let index = caret + 1;
    if form.get(index) == Some(&'{') {
        return balanced_end(form, index);
    }
    skip_to_name_stop(form, index)
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

fn keyword_ends(form: &[char], after: usize) -> bool {
    form.get(after).is_none_or(|ch| NAME_STOPS.contains(ch))
}

fn defn_keyword_end(form: &[char], index: usize) -> Option<usize> {
    ["defn-", "defn"].into_iter().find_map(|candidate| {
        let after = index + candidate.len();
        (starts_with_at(form, index, candidate) && keyword_ends(form, after)).then_some(after)
    })
}

fn read_name(form: &[char], index: usize) -> Option<String> {
    match form.get(index) {
        None => None,
        Some(ch) if FORM_DELIMITERS.contains(ch) => None,
        Some(_) => Some(form[index..skip_to_name_stop(form, index)].iter().collect()),
    }
}

/// The name of a form such as `(defn name ...)`, `(defn- name ...)`, or `(defn ^meta name ...)`.
fn defn_name(form: &[char]) -> Option<String> {
    if form.first() != Some(&'(') {
        return None;
    }
    let keyword_at = defn_keyword_end(form, skip_space(form, 1))?;
    read_name(form, skip_leading_metadata(form, keyword_at))
}

/// Scans the source for top-level forms, tracking parenthesis depth.
#[derive(Default)]
struct TopLevelScan {
    depth: u32,
    open: Option<(usize, u32)>,
    found: Vec<Found>,
}

impl TopLevelScan {
    fn open_paren(&mut self, index: usize, line: u32) {
        if self.depth == 0 {
            self.open = Some((index, line));
        }
        self.depth += 1;
    }

    fn close_paren(&mut self, source: &[char], index: usize, line: u32) {
        if self.depth == 1 {
            if let Some((start, start_line)) = self.open.take() {
                self.remember(&source[start..=index], start_line, line);
            }
        }
        self.depth = self.depth.saturating_sub(1);
    }

    fn remember(&mut self, form: &[char], start_line: u32, end_line: u32) {
        let Some(name) = defn_name(form) else {
            return;
        };
        let text: String = form.iter().collect();
        self.found.push(Found {
            name,
            start_line,
            end_line,
            complexity: to_u32(cyclomatic_complexity(&text)),
        });
    }
}

fn extract_top_level_defns(source: &[char]) -> Vec<Found> {
    let mut scan = TopLevelScan::default();
    let mut index = 0;
    let mut line = 1;
    while index < source.len() {
        if let Some((next, next_line)) = skip_reader(source, index, line) {
            index = next;
            line = next_line;
            continue;
        }
        match source[index] {
            '(' => scan.open_paren(index, line),
            ')' => scan.close_paren(source, index, line),
            '\n' => line += 1,
            _ => {}
        }
        index += 1;
    }
    scan.found
}

fn extract_functions(source: &str) -> Vec<Found> {
    extract_top_level_defns(&chars_of(source))
}

fn declared_namespace(source: &str) -> Option<String> {
    let clean = without_strings_and_comments(&chars_of(source));
    [&NS, &IN_NS, &IN_NS_QUOTE]
        .into_iter()
        .find_map(|re| re.captures(&clean).map(|caps| caps[1].to_string()))
}

/// Removes `source_root` from the start of `relative`. A root of `""` or `"."` is no root.
fn strip_source_root<'a>(relative: &'a str, source_root: &str) -> &'a str {
    let root = source_root.trim_end_matches('/');
    if root.is_empty() || root == "." {
        return relative;
    }
    relative
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(relative)
}

fn strip_src_dir(relative: &str) -> &str {
    if let Some(rest) = relative.strip_prefix("src/") {
        rest
    } else if let Some((_, rest)) = relative.split_once("/src/") {
        rest
    } else {
        relative
    }
}

fn strip_clojure_suffix(relative: &str) -> &str {
    SOURCE_SUFFIXES
        .iter()
        .copied()
        .find_map(|suffix| relative.strip_suffix(suffix))
        .unwrap_or(relative)
}

fn namespace_from_path(path: &str, source_root: &str) -> String {
    let relative = path.replace('\\', "/");
    let root = source_root.replace('\\', "/");
    let relative = strip_source_root(&relative, &root);
    let relative = strip_clojure_suffix(strip_src_dir(relative));
    relative.replace('/', ".").replace('_', "-")
}

/// Functions in one Clojure source file. The namespace is the declared `ns`, or else the path.
pub fn functions_in_source(source: &str, path: &str, project_root: &str) -> Vec<Function> {
    let namespace =
        declared_namespace(source).unwrap_or_else(|| namespace_from_path(path, project_root));
    extract_functions(source)
        .into_iter()
        .map(|found| Function {
            name: found.name,
            namespace: namespace.clone(),
            complexity: found.complexity,
            start_line: found.start_line,
            end_line: found.end_line,
            path: path.to_string(),
            language: "clojure".to_string(),
            jacoco_class: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(found: &[Found]) -> Vec<&str> {
        found.iter().map(|item| item.name.as_str()).collect()
    }

    // CR-11
    #[test]
    fn cr_11_base_and_branch_forms() {
        assert_eq!(cyclomatic_complexity("(defn foo [])"), 1);
        assert_eq!(cyclomatic_complexity("(defn foo [x]\n  (+ x 1))"), 1);
        assert_eq!(cyclomatic_complexity("(defn foo [x]\n  (if x 1 0))"), 2);
        assert_eq!(cyclomatic_complexity("(defn foo [x]\n  (if-not x 1 0))"), 2);
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (if-let [y x] y 0))"),
            2
        );
        assert_eq!(cyclomatic_complexity("(defn foo [x]\n  (when x 1))"), 2);
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (when-first [y x] y))"),
            2
        );
        assert_eq!(cyclomatic_complexity("(defn foo [x y]\n  (and x y))"), 2);
        assert_eq!(cyclomatic_complexity("(defn foo [x y]\n  (or x y))"), 2);
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (loop [i 0] (recur (inc i))))"),
            2
        );
    }

    // CR-11
    #[test]
    fn cr_11_cond_case_and_threading() {
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (cond\n    (= x 1) :one\n    :else :other))"),
            3
        );
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (condp = x\n    1 :one\n    2 :two))"),
            3
        );
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (case x\n    1 :one\n    :other))"),
            3
        );
        assert_eq!(
            cyclomatic_complexity(
                "(defn foo [x]\n  (cond-> x\n    (pos? x) inc\n    (even? x) (* 2)))"
            ),
            3
        );
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (some-> x inc dec))"),
            3
        );
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (some->> x (map inc) (filter pos?)))"),
            3
        );
    }

    // CR-11: an odd number of `case` forms leaves a default, which counts as a clause.
    #[test]
    fn cr_11_case_with_default_counts_the_default() {
        assert_eq!(
            cyclomatic_complexity("(defn f [x] (case x 1 :a 2 :b :default))"),
            4
        );
    }

    // CR-11
    #[test]
    fn cr_11_ignores_decisions_in_strings_and_comments() {
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  \"if when cond and or\"\n  x)"),
            1
        );
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  \"(if true 1 0)\"\n  x)"),
            1
        );
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  ;; if when cond\n  x)"),
            1
        );
        assert_eq!(
            cyclomatic_complexity("(defn foo [x]\n  (if x :yes :no) ; and or when cond)"),
            2
        );
    }

    // CR-11
    #[test]
    fn cr_11_map_literals_inside_cond_are_not_extra_clauses() {
        let source = r#"(defn foo [cell]
  (let [contents (:contents cell)]
    (cond
      (pred-a? contents)
      {:type :army :mode :awake :owner (:owner contents) :aboard true}

      (pred-b? contents)
      {:type :fighter :mode :awake :owner (:owner contents) :fuel 20 :from-carrier true}

      (pred-c? contents) contents

      (pred-d? cell)
      {:type :fighter :mode :awake :owner :player :fuel 20 :from-airport true}

      :else nil)))"#;
        assert_eq!(cyclomatic_complexity(source), 6);
    }

    // CR-11: keyword calls such as `(:if-let m)` are not decision forms.
    #[test]
    fn cr_11_keyword_calls_are_not_decisions() {
        assert_eq!(cyclomatic_complexity("(defn f [m]\n  (:if-let m))"), 1);
        assert_eq!(cyclomatic_complexity("(defn f [m] (:cond m))"), 1);
        assert_eq!(cyclomatic_complexity("(defn f [m] (if (:and m) 1 0))"), 2);
    }

    // CR-11: a reader conditional keeps every branch, so both branches count.
    #[test]
    fn cr_11_reader_conditional_counts_both_branches() {
        assert_eq!(
            cyclomatic_complexity("(defn f [x]\n  #?(:clj (if x 1 0) :cljs (when x 1)))"),
            3
        );
        assert_eq!(
            cyclomatic_complexity("(defn f [x]\n  #?@(:clj [(and x 1)] :cljs []))"),
            2
        );
    }

    // CR-11: a syntax-quoted decision is still a decision.
    #[test]
    fn cr_11_syntax_quote_keeps_decisions() {
        assert_eq!(cyclomatic_complexity("(defn f [x]\n  `(if ~x 1 0))"), 2);
        assert_eq!(cyclomatic_complexity("(defn f [x]\n  `(when x# 1))"), 2);
    }

    // CR-8, CR-9
    #[test]
    fn cr_8_extracts_defn_names_lines_and_namespace() {
        let source =
            "(ns demo.core)\n\n(defn choose [x]\n  (if x 1 0))\n\n(defn- hidden [y]\n  y)\n";
        let found = functions_in_source(source, "src/demo/core.clj", "/proj");
        let summary: Vec<(&str, u32, u32)> = found
            .iter()
            .map(|item| (item.name.as_str(), item.complexity, item.start_line))
            .collect();
        assert_eq!(summary, vec![("choose", 2, 3), ("hidden", 1, 6)]);
        assert!(found.iter().all(|item| item.namespace == "demo.core"));
    }

    // CR-9
    #[test]
    fn cr_9_top_level_def_does_not_attach_to_the_previous_function() {
        let source = concat!(
            "(defn alpha [x]\n",
            "  (if x 1 0))\n\n",
            "(def rules\n",
            "  [{:pred (fn [v]\n",
            "            (if v (when (pos? v) true) false))}])\n\n",
            "(defn omega [y]\n",
            "  y)"
        );
        let functions = extract_functions(source);
        let found: Vec<(&str, u32)> = functions
            .iter()
            .map(|item| (item.name.as_str(), item.complexity))
            .collect();
        assert_eq!(found, vec![("alpha", 2), ("omega", 1)]);
    }

    // CR-8
    #[test]
    fn cr_8_namespace_falls_back_to_the_path() {
        assert_eq!(
            namespace_from_path("src/demo/core_extra.clj", ""),
            "demo.core-extra"
        );
        assert_eq!(
            namespace_from_path("/proj/src/demo/core.cljc", "/proj"),
            "demo.core"
        );
        assert_eq!(namespace_from_path("pkg/src/demo/core.bb", ""), "demo.core");
        let found = functions_in_source("(defn foo [] 1)\n", "src/demo/core_extra.clj", "/proj");
        assert_eq!(found[0].namespace, "demo.core-extra");
    }

    // CR-9
    #[test]
    fn cr_9_strings_comments_and_character_literals_stay_inside_one_form() {
        let source = "(defn foo [x]\n  \"say \\\"hi\\\"\n  (if x 1)\"\n  \\space\n  ;; (defn hidden [])\n  x)\n";
        assert_eq!(names(&extract_functions(source)), vec!["foo"]);
    }

    // CR-9
    #[test]
    fn cr_9_discards_and_metadata_cover_each_reader_form() {
        let source = r#"
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
"#;
        assert_eq!(
            names(&extract_functions(source)),
            vec!["kept", "also", "third", "spaced"]
        );
        assert!(extract_functions("(defn ^{:private kept [] 1)").is_empty());
    }

    // CR-9
    #[test]
    fn cr_9_character_literals_do_not_swallow_the_next_defn() {
        let source = "(def x #{\\\"})\n(defn foo [] 1)";
        assert_eq!(names(&extract_functions(source)), vec!["foo"]);
    }

    // CR-8, CR-9, CR-11
    #[test]
    fn cr_8_reader_keeps_the_real_namespace_name_and_decisions() {
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
        let found = functions_in_source(source, "src/demo/core.clj", "/proj");
        let summary: Vec<(&str, u32)> = found
            .iter()
            .map(|item| (item.name.as_str(), item.complexity))
            .collect();
        assert_eq!(
            summary,
            vec![("kept", 1), ("also", 1), ("quoted", 2), ("semi", 2)]
        );
        assert!(found.iter().all(|item| item.namespace == "demo.core"));
        assert_eq!(cyclomatic_complexity(r"(defn semi [] \; (if true 1 2))"), 2);
        assert_eq!(
            cyclomatic_complexity(r#"(defn quoted [] \" (if x 1 0))"#),
            2
        );
        assert_eq!(
            extract_functions("#_(defn dropped [] (if x 1 0))\n(defn kept [] 1)\n"),
            vec![Found {
                name: "kept".to_string(),
                start_line: 2,
                end_line: 2,
                complexity: 1,
            }]
        );
    }

    // CR-9: a `defn` inside a reader-conditional branch is not a top-level form (upstream behavior).
    #[test]
    fn cr_9_reader_conditional_branch_defn_is_not_extracted() {
        let source = "#?(:clj (defn a [] (if 1 2 3)) :cljs (defn a [] 1))\n";
        assert!(extract_functions(source).is_empty());
    }

    // CR-9: a syntax-quoted `defn` is still a top-level form (upstream behavior).
    #[test]
    fn cr_9_syntax_quoted_defn_is_extracted() {
        let source = "`(defn g [] 1)\n(defn f [] 1)";
        let functions = extract_functions(source);
        let found: Vec<(&str, u32, u32)> = functions
            .iter()
            .map(|item| (item.name.as_str(), item.start_line, item.complexity))
            .collect();
        assert_eq!(found, vec![("g", 1, 1), ("f", 2, 1)]);
    }

    // CR-9: a keyword call at top level is not a `defn` name.
    #[test]
    fn cr_9_keyword_call_at_top_level_is_not_a_defn() {
        assert!(extract_functions("(:defn m)\n").is_empty());
    }

    // CR-9: an empty string closes at its second quote, so the following `if` is seen.
    #[test]
    fn cr_9_empty_string_closes_at_its_second_quote() {
        assert_eq!(cyclomatic_complexity("(defn f [] \"\" (if x 1 0))"), 2);
    }

    // CR-9: a string closes at its own closing quote, not at its opening quote or first character.
    #[test]
    fn cr_9_string_closes_at_its_closing_quote() {
        assert_eq!(cyclomatic_complexity("(defn f [] \"abc\" (if x 1 0))"), 2);
    }

    // CR-9: escaped quotes and parentheses inside a string neither end it nor add decisions.
    #[test]
    fn cr_9_escaped_quote_and_parens_inside_string_are_blanked() {
        let source = "(defn f [x]\n  \"(if x 1 0) \\\" (when\"\n  (if x 1 0))";
        assert_eq!(cyclomatic_complexity(source), 2);
    }

    // CR-9: a string with no closing quote runs to the end of the text.
    #[test]
    fn cr_9_unterminated_string_blanks_to_end_of_text() {
        assert_eq!(cyclomatic_complexity("(defn f [] \"(if x 1 0)"), 1);
    }

    // CR-9: a `#_` discard directly followed by another `#_` leaves the next defn extracted.
    #[test]
    fn cr_9_adjacent_nested_discards_keep_the_next_defn() {
        assert_eq!(
            extract_functions("#_#_(a) (defn f [] 1)"),
            vec![Found {
                name: "f".to_string(),
                start_line: 1,
                end_line: 1,
                complexity: 1,
            }]
        );
    }

    // CR-9: a discarded atom ends at a delimiter, so a `#_` inside it is not read as a discard.
    #[test]
    fn cr_9_discarded_atom_stops_at_delimiter() {
        assert_eq!(
            extract_functions("#_ a#_(defn gone [] 1)\n(defn kept [] 1)\n"),
            vec![
                Found {
                    name: "gone".to_string(),
                    start_line: 1,
                    end_line: 1,
                    complexity: 1,
                },
                Found {
                    name: "kept".to_string(),
                    start_line: 2,
                    end_line: 2,
                    complexity: 1,
                },
            ]
        );
    }

    // CR-9: a `#_` at the very end of the text discards nothing and does not panic.
    #[test]
    fn cr_9_discard_at_end_of_text_is_empty() {
        assert_eq!(
            extract_functions("(defn f [] 1)\n#_"),
            vec![Found {
                name: "f".to_string(),
                start_line: 1,
                end_line: 1,
                complexity: 1,
            }]
        );
    }

    // CR-9: a multi-line discard advances the line count of the defn after it.
    #[test]
    fn cr_9_multiline_discard_keeps_later_line_numbers() {
        assert_eq!(
            extract_functions("(defn a [] 1)\n#_ (x\n y)\n(defn b [] 1)\n"),
            vec![
                Found {
                    name: "a".to_string(),
                    start_line: 1,
                    end_line: 1,
                    complexity: 1,
                },
                Found {
                    name: "b".to_string(),
                    start_line: 4,
                    end_line: 4,
                    complexity: 1,
                },
            ]
        );
    }

    // CR-9: a multi-line string before a defn keeps that defn's line numbers.
    #[test]
    fn cr_9_multiline_string_keeps_later_line_numbers() {
        assert_eq!(
            extract_functions("(defn a [] \"x\n\ny\")\n(defn b [] 1)\n"),
            vec![
                Found {
                    name: "a".to_string(),
                    start_line: 1,
                    end_line: 3,
                    complexity: 1,
                },
                Found {
                    name: "b".to_string(),
                    start_line: 4,
                    end_line: 4,
                    complexity: 1,
                },
            ]
        );
    }

    // CR-11: a top-level `(if ...)` at the start of the text is counted.
    #[test]
    fn cr_11_decision_at_start_of_text_is_counted() {
        assert_eq!(cyclomatic_complexity("(if x 1 0)"), 2);
    }

    // CR-11: a nested form that closes does not end the top-level clause it sits in.
    #[test]
    fn cr_11_nested_form_does_not_start_a_new_clause() {
        assert_eq!(cyclomatic_complexity("(cond (a)b)"), 1);
    }

    // CR-11: a `cond` with an odd number of forms has one clause per complete pair.
    #[test]
    fn cr_11_odd_cond_counts_only_complete_pairs() {
        assert_eq!(
            cyclomatic_complexity("(defn f [x] (cond (a x) 1 (b x)))"),
            2
        );
    }

    // CR-11: a `case` with an even number of clauses has no trailing default.
    #[test]
    fn cr_11_even_case_has_no_default_clause() {
        assert_eq!(cyclomatic_complexity("(defn f [x] (case x 1 :a 2 :b))"), 3);
    }

    // CR-11: a `condp` with no clauses has negative clauses, clamped to complexity 0.
    #[test]
    fn cr_11_negative_clause_count_clamps_to_zero() {
        assert_eq!(
            extract_functions("(defn f [x] (condp =))"),
            vec![Found {
                name: "f".to_string(),
                start_line: 1,
                end_line: 1,
                complexity: 0,
            }]
        );
    }

    // CR-8: an empty source root keeps a leading slash in the path.
    #[test]
    fn cr_8_empty_source_root_does_not_strip_path() {
        assert_eq!(
            namespace_from_path("/proj/demo/core.clj", ""),
            ".proj.demo.core"
        );
    }

    // CR-8: a `.` source root keeps the leading `./` in the path.
    #[test]
    fn cr_8_dot_source_root_does_not_strip_path() {
        assert_eq!(
            namespace_from_path("./proj/demo/core.clj", "."),
            "..proj.demo.core"
        );
    }

    // CR-8: a non-empty source root is stripped from the path.
    #[test]
    fn cr_8_named_source_root_is_stripped() {
        assert_eq!(
            namespace_from_path("/proj/demo/core.clj", "/proj"),
            "demo.core"
        );
    }
}
