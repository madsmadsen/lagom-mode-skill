// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Port of src/mutator/functions.py.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::discover::language_of;
use crate::functions::functions_in_file;
use crate::model::{Function, Language, RawSite, Site};
use crate::sites::discover_raw;

/// A form's merged extent: first-seen name and privacy, min start and max end of its functions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub private: bool,
}

const LINE_BREAKS: [char; 10] = [
    '\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}', '\u{2029}',
];

/// Python `str.splitlines`: splits on every Python line break and drops the terminators.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices();
    while let Some((index, ch)) = chars.next() {
        if !LINE_BREAKS.contains(&ch) {
            continue;
        }
        lines.push(&text[start..index]);
        let mut next = index + ch.len_utf8();
        if ch == '\r' && text[next..].starts_with('\n') {
            next += 1;
            chars.next();
        }
        start = next;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Lines `start..=end` (1-based) as Python slicing `lines[start-1:end]` clamps them.
fn lines_between(source: &str, start: usize, end: usize) -> Vec<&str> {
    let lines = splitlines(source);
    let from = start.saturating_sub(1).min(lines.len());
    let to = end.clamp(from, lines.len());
    lines[from..to].to_vec()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// SHA-256 hex of the lines `start..=end` with trailing whitespace removed.
pub fn digest(source: &str, start: usize, end: usize) -> String {
    let chunk = lines_between(source, start, end)
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    sha256_hex(chunk.as_bytes())
}

/// The lines after leading whitespace and attributes, within the first six lines.
fn header(source: &str, start: usize, end: usize) -> String {
    let mut text = lines_between(source, start, end.min(start + 6))
        .join("\n")
        .trim_start()
        .to_string();
    while let Some(rest) = after_attribute(&text) {
        text = rest.trim_start().to_string();
    }
    text
}

fn after_attribute(text: &str) -> Option<&str> {
    if !text.starts_with("#[") {
        return None;
    }
    let close = text.find(']')?;
    Some(&text[close + 1..])
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// True when `word` occurs with word boundaries on both sides, like Python `\bword\b`.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + word.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

fn python_private(name: &str) -> bool {
    if name.starts_with("__") && name.ends_with("__") {
        return false;
    }
    name.starts_with('_')
}

fn typescript_private(name: &str, header: &str) -> bool {
    name.starts_with('#') || has_word(header, "private")
}

fn is_pub_item(header: &str) -> bool {
    header
        .strip_prefix("pub")
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// True when the Python mutator treats the function as private in its language.
pub fn is_private(language: Language, name: &str, source: &str, start: usize, end: usize) -> bool {
    let head = || header(source, start, end);
    match language {
        Language::Clojure => head().starts_with("(defn-"),
        Language::Java => has_word(&head(), "private"),
        Language::Go => name.chars().next().is_some_and(char::is_lowercase),
        Language::Python => python_private(name),
        Language::Rust => !is_pub_item(&head()),
        Language::TypeScript => typescript_private(name, &head()),
    }
}

/// The form id: `defn/name` when public, `defn-/name` when private.
pub fn form_id(name: &str, private: bool) -> String {
    let prefix = if private { "defn-" } else { "defn" };
    format!("{prefix}/{name}")
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        push_json_char(&mut out, ch);
    }
    out.push('"');
    out
}

/// Short escapes that Python `json.dumps` writes as a two-character sequence.
const SHORT_ESCAPES: [(char, &str); 7] = [
    ('"', "\\\""),
    ('\\', "\\\\"),
    ('\n', "\\n"),
    ('\r', "\\r"),
    ('\t', "\\t"),
    ('\u{8}', "\\b"),
    ('\u{c}', "\\f"),
];

/// Appends one character escaped as Python `json.dumps` with `ensure_ascii=True`.
fn push_json_char(out: &mut String, ch: char) {
    if let Some(&(_, escape)) = SHORT_ESCAPES.iter().find(|(short, _)| *short == ch) {
        out.push_str(escape);
    } else if (' '..='~').contains(&ch) {
        out.push(ch);
    } else {
        push_utf16_escapes(out, ch);
    }
}

/// Appends `\uXXXX` for each UTF-16 unit of `ch`; a non-BMP character gives a surrogate pair.
fn push_utf16_escapes(out: &mut String, ch: char) {
    let mut units = [0u16; 2];
    for unit in ch.encode_utf16(&mut units).iter() {
        out.push_str(&format!("\\u{unit:04x}"));
    }
}

/// Compact Python `json.dumps([file, namespace, form, start, end, original, mutant])`.
pub fn mutation_id(
    file: &str,
    namespace: &str,
    form: &str,
    start: usize,
    end: usize,
    original: &str,
    mutant: &str,
) -> String {
    format!(
        "[{},{},{},{start},{end},{},{}]",
        json_string(file),
        json_string(namespace),
        json_string(form),
        json_string(original),
        json_string(mutant)
    )
}

fn mutation_field(mutation: &str, index: usize) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(mutation).ok()?;
    let serde_json::Value::Array(items) = value else {
        return None;
    };
    items.get(index)?.as_str().map(str::to_owned)
}

/// Element 0 of a mutation id (the file), when it is a string.
pub fn mutation_file(mutation: &str) -> Option<String> {
    mutation_field(mutation, 0)
}

/// Element 1 of a mutation id (the namespace), when it is a string.
pub fn mutation_namespace(mutation: &str) -> Option<String> {
    mutation_field(mutation, 1)
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// The functions of one source file, as the crapper function scanner reports them.
pub fn project_functions(source: &str, path: &Path, root: &Path) -> Vec<Function> {
    let Some(language) = language_of(path) else {
        return Vec::new();
    };
    functions_in_file(language, source, &canonical(path), &canonical(root))
}

/// True when the function spans the line and, when byte offsets are known, the byte.
fn contains(function: &Function, line: usize, byte: Option<usize>) -> bool {
    if function.start_line > line || function.end_line < line {
        return false;
    }
    match (byte, function.start_byte, function.end_byte) {
        (Some(byte), Some(start), Some(end)) => start <= byte && byte < end,
        _ => true,
    }
}

fn tightness(function: &Function) -> usize {
    match (function.start_byte, function.end_byte) {
        (Some(start), Some(end)) if end >= start => end - start,
        _ => function
            .end_line
            .saturating_sub(function.start_line)
            .saturating_mul(1_000_000),
    }
}

/// The innermost function containing the line (and byte), if any.
fn owner_of(functions: &[Function], line: usize, byte: Option<usize>) -> Option<&Function> {
    functions
        .iter()
        .filter(|function| contains(function, line, byte))
        .min_by_key(|function| (tightness(function), function.start_line))
}

fn bind_site(
    source: &str,
    language: Language,
    functions: &[Function],
    file_key: &str,
    raw: RawSite,
) -> Option<Site> {
    let owner = owner_of(functions, raw.line, Some(raw.start))?;
    let private = is_private(
        language,
        &owner.name,
        source,
        owner.start_line,
        owner.end_line,
    );
    let form = form_id(&owner.name, private);
    let mutation_id = mutation_id(
        file_key,
        &owner.namespace,
        &form,
        raw.start,
        raw.end,
        &raw.original,
        &raw.mutant,
    );
    Some(Site {
        file: file_key.to_string(),
        namespace: owner.namespace.clone(),
        form_id: form,
        name: owner.name.clone(),
        line: raw.line,
        start: raw.start,
        end: raw.end,
        original: raw.original,
        mutant: raw.mutant,
        category: raw.category,
        mutation_id,
        anchor_line: raw.anchor_line,
    })
}

/// Mutation sites in one file, each bound to its innermost owning function.
pub fn sites_in_file(source: &str, path: &Path, root: &Path, file_key: &str) -> Vec<Site> {
    let Some(language) = language_of(path) else {
        return Vec::new();
    };
    let functions = project_functions(source, path, root);
    let file = path.to_string_lossy();
    discover_raw(source, language, &file)
        .into_iter()
        .filter_map(|raw| bind_site(source, language, &functions, file_key, raw))
        .collect()
}

/// `(namespace, form id)` of a function, and whether that form is private.
fn form_key(language: Language, source: &str, function: &Function) -> ((String, String), bool) {
    let private = is_private(
        language,
        &function.name,
        source,
        function.start_line,
        function.end_line,
    );
    (
        (function.namespace.clone(), form_id(&function.name, private)),
        private,
    )
}

/// One digest over the form's functions, each digested and ordered by (start, end).
fn joined_digest(source: &str, ranges: &mut [(usize, usize)]) -> String {
    ranges.sort_unstable();
    let joined: String = ranges
        .iter()
        .map(|&(start, end)| digest(source, start, end))
        .collect();
    sha256_hex(joined.as_bytes())
}

/// Map `(namespace, form id)` to the digest of the form's function text.
pub fn form_digests(source: &str, path: &Path, root: &Path) -> BTreeMap<(String, String), String> {
    let Some(language) = language_of(path) else {
        return BTreeMap::new();
    };
    let mut grouped: BTreeMap<(String, String), Vec<(usize, usize)>> = BTreeMap::new();
    for function in project_functions(source, path, root) {
        let (key, _) = form_key(language, source, &function);
        grouped
            .entry(key)
            .or_default()
            .push((function.start_line, function.end_line));
    }
    grouped
        .into_iter()
        .map(|(key, mut ranges)| (key, joined_digest(source, &mut ranges)))
        .collect()
}

fn merge_span(span: &mut Span, function: &Function) {
    span.start = span.start.min(function.start_line);
    span.end = span.end.max(function.end_line);
}

/// `(namespace, form id)` -> merged span, in first-seen order.
pub fn form_spans(source: &str, path: &Path, root: &Path) -> Vec<((String, String), Span)> {
    let Some(language) = language_of(path) else {
        return Vec::new();
    };
    let mut spans: Vec<((String, String), Span)> = Vec::new();
    let mut slots: HashMap<(String, String), usize> = HashMap::new();
    for function in project_functions(source, path, root) {
        let (key, private) = form_key(language, source, &function);
        let slot = slots.get(&key).copied();
        match slot {
            Some(slot) => merge_span(&mut spans[slot].1, &function),
            None => {
                slots.insert(key.clone(), spans.len());
                let span = Span {
                    start: function.start_line,
                    end: function.end_line,
                    name: function.name,
                    private,
                };
                spans.push((key, span));
            }
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn function(name: &str, namespace: &str, start: usize, end: usize) -> Function {
        Function {
            name: name.to_string(),
            namespace: namespace.to_string(),
            start_line: start,
            end_line: end,
            start_byte: None,
            end_byte: None,
        }
    }

    fn bytes_function(name: &str, start: usize, end: usize, bytes: (usize, usize)) -> Function {
        Function {
            start_byte: Some(bytes.0),
            end_byte: Some(bytes.1),
            ..function(name, "demo", start, end)
        }
    }

    fn write_source(root: &Path, relative: &str, source: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, source).expect("write");
        path
    }

    fn sites_of(relative: &str, source: &str) -> Vec<Site> {
        let root = TempDir::new().expect("tempdir");
        let path = write_source(root.path(), relative, source);
        sites_in_file(source, &path, root.path(), relative)
    }

    fn has(sites: &[Site], form: &str, original: &str, mutant: &str) -> bool {
        sites
            .iter()
            .any(|s| s.form_id == form && s.original == original && s.mutant == mutant)
    }

    fn pairs(sites: &[Site]) -> Vec<(&str, &str)> {
        sites
            .iter()
            .map(|s| (s.original.as_str(), s.mutant.as_str()))
            .collect()
    }

    // MU-19: splitlines breaks on \r\n, lone \r, \x0c and every other Python line break
    #[test]
    fn mu_19_splitlines_follows_python_line_breaks() {
        assert_eq!(splitlines("a\r\nb\x0cc"), vec!["a", "b", "c"]);
        assert_eq!(splitlines("p\rq\n"), vec!["p", "q"]);
        assert_eq!(
            splitlines("a\u{2028}b\u{85}c\x1cd\x0be"),
            vec!["a", "b", "c", "d", "e"]
        );
        assert_eq!(splitlines("x\n\n"), vec!["x", ""]);
        assert_eq!(splitlines("\n"), vec![""]);
        assert!(splitlines("").is_empty());
    }

    // MU-19: sha256_hex is the standard SHA-256 hex digest
    #[test]
    fn mu_19_sha256_hex_matches_the_standard_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // MU-19: digest ignores trailing whitespace and hashes the stripped chunk
    #[test]
    fn mu_19_digest_ignores_trailing_whitespace() {
        let clean = "def place(x):\n    return x > 0\n";
        let padded = "def place(x):   \n    return x > 0\t\r\n";
        assert_eq!(digest(clean, 1, 2), digest(padded, 1, 2));
        assert_eq!(
            digest(clean, 1, 2),
            sha256_hex(b"def place(x):\n    return x > 0")
        );
    }

    // MU-19: digest ranges clamp like Python slices and empty ranges hash the empty text
    #[test]
    fn mu_19_digest_clamps_ranges_like_python_slices() {
        assert_eq!(digest("a\nb", 2, 9), digest("a\nb", 2, 2));
        assert_eq!(digest("a", 5, 9), sha256_hex(b""));
        assert_eq!(digest("a\nb", 2, 1), sha256_hex(b""));
    }

    // MU-19: a form digest hashes the joined per-function digests and ignores trailing whitespace
    #[test]
    fn mu_19_form_digests_are_stable_under_trailing_whitespace() {
        let root = TempDir::new().expect("tempdir");
        let clean = "def place(x):\n    return x > 0\n";
        let padded = "def place(x):  \n    return x > 0  \n";
        let clean_path = write_source(root.path(), "src/demo.py", clean);
        let padded_path = write_source(root.path(), "src/padded.py", padded);
        let a = form_digests(clean, &clean_path, root.path());
        let b = form_digests(padded, &padded_path, root.path());
        assert_eq!(
            a.values().collect::<Vec<_>>(),
            b.values().collect::<Vec<_>>()
        );
        let expected = sha256_hex(digest(clean, 1, 2).as_bytes());
        assert_eq!(a.values().cloned().collect::<Vec<_>>(), vec![expected]);
    }

    // MU-19: form_spans merges same-form functions into the min start and max end, first-seen order
    #[test]
    fn mu_19_form_spans_merge_overloads_into_one_span() {
        let root = TempDir::new().expect("tempdir");
        let source = "package demo;\nclass Board {\n  int place(int x) {\n    return x;\n  }\n  \
                      int place(String s) {\n    return 0;\n  }\n}\n";
        let path = write_source(root.path(), "src/demo/Board.java", source);
        let spans = form_spans(source, &path, root.path());
        assert_eq!(spans.len(), 1);
        let ((namespace, form), span) = &spans[0];
        assert_eq!(namespace, "demo.Board");
        assert_eq!(form, "defn/place");
        assert_eq!((span.start, span.end, span.private), (3, 8, false));
    }

    // MU-9: Python privacy spares dunder names and marks single-underscore names private
    #[test]
    fn mu_9_python_privacy_spares_dunder_names() {
        let init = "def __init__(self):\n    pass\n";
        assert!(!is_private(Language::Python, "__init__", init, 1, 2));
        assert!(is_private(
            Language::Python,
            "_hide",
            "def _hide():\n    return 1\n",
            1,
            2
        ));
        assert!(is_private(
            Language::Python,
            "__x",
            "def __x():\n    return 1\n",
            1,
            2
        ));
    }

    // MU-9: Rust privacy needs a pub keyword followed by whitespace after any attributes
    #[test]
    fn mu_9_rust_privacy_needs_a_pub_keyword_after_attributes() {
        assert!(is_private(Language::Rust, "place", "fn place() {}\n", 1, 1));
        let attributed = "#[inline]\n#[must_use]\npub fn place() {}\n";
        assert!(!is_private(Language::Rust, "place", attributed, 1, 3));
        // The Python regex `pub\s` does not match `pub(crate)`, so it stays private.
        assert!(is_private(
            Language::Rust,
            "place",
            "pub(crate) fn place() {}\n",
            1,
            1
        ));
    }

    // MU-9: TypeScript privacy is a hash name or the private keyword in the header
    #[test]
    fn mu_9_typescript_privacy_uses_hash_names_and_the_private_keyword() {
        assert!(is_private(
            Language::TypeScript,
            "#place",
            "#place() {}\n",
            1,
            1
        ));
        assert!(is_private(
            Language::TypeScript,
            "place",
            "private place() {}\n",
            1,
            1
        ));
        assert!(!is_private(
            Language::TypeScript,
            "place",
            "function place() {}\n",
            1,
            1
        ));
    }

    // MU-9: Go privacy is an unexported first letter
    #[test]
    fn mu_9_go_privacy_is_an_unexported_name() {
        let hide = "func hide() bool { return false }\n";
        assert!(is_private(Language::Go, "hide", hide, 1, 1));
        assert!(!is_private(Language::Go, "Run", "func Run() {}\n", 1, 1));
        assert!(!is_private(
            Language::Go,
            "_hide",
            "func _hide() {}\n",
            1,
            1
        ));
    }

    // MU-9: Java privacy is the whole word private in the header
    #[test]
    fn mu_9_java_privacy_is_the_private_word() {
        let hide = "  private int hide() { return 1; }\n";
        assert!(is_private(Language::Java, "hide", hide, 1, 1));
        assert!(!is_private(
            Language::Java,
            "place",
            "  int privateers() { return 1; }\n",
            1,
            1
        ));
        assert!(!is_private(
            Language::Java,
            "place",
            "  public int place() { return 1; }\n",
            1,
            1
        ));
    }

    // MU-9: Clojure privacy is a defn- header
    #[test]
    fn mu_9_clojure_privacy_is_defn_dash() {
        let source = "(ns demo.core)\n(defn place [x]\n  x)\n(defn- hide []\n  false)\n";
        assert!(is_private(Language::Clojure, "hide", source, 4, 5));
        assert!(!is_private(Language::Clojure, "place", source, 2, 3));
    }

    // MU-9: form ids carry the defn- prefix for private forms
    #[test]
    fn mu_9_form_id_prefixes_private_forms() {
        assert_eq!(form_id("place", false), "defn/place");
        assert_eq!(form_id("hide", true), "defn-/hide");
    }

    // MU-9: the innermost function containing the line owns the site
    #[test]
    fn mu_9_owner_is_the_innermost_function_on_the_line() {
        let functions = [function("outer", "d", 1, 10), function("inner", "d", 8, 9)];
        assert_eq!(
            owner_of(&functions, 8, None).map(|f| f.name.as_str()),
            Some("inner")
        );
        assert_eq!(
            owner_of(&functions, 3, None).map(|f| f.name.as_str()),
            Some("outer")
        );
        assert!(owner_of(&functions, 11, None).is_none());
    }

    // MU-9: byte offsets decide ownership when crapper supplies them
    #[test]
    fn mu_9_owner_prefers_byte_ranges_when_present() {
        let functions = [
            bytes_function("outer", 1, 10, (0, 100)),
            bytes_function("inner", 8, 9, (50, 60)),
        ];
        assert_eq!(
            owner_of(&functions, 8, Some(55)).map(|f| f.name.as_str()),
            Some("inner")
        );
        assert_eq!(
            owner_of(&functions, 8, Some(10)).map(|f| f.name.as_str()),
            Some("outer")
        );
    }

    // MU-9: a file with no language binds no sites and reports no forms
    #[test]
    fn mu_9_unknown_extension_binds_nothing() {
        let root = TempDir::new().expect("tempdir");
        let path = write_source(root.path(), "notes.txt", "x > 0\n");
        assert!(sites_in_file("x > 0\n", &path, root.path(), "notes.txt").is_empty());
        assert!(form_digests("x > 0\n", &path, root.path()).is_empty());
        assert!(form_spans("x > 0\n", &path, root.path()).is_empty());
    }

    // MU-9: a Python form spans its function and is marked private from the leading underscore
    #[test]
    fn mu_9_form_spans_mark_python_private_forms() {
        let root = TempDir::new().expect("tempdir");
        let source = "def _hide():\n    return 1\n";
        let path = write_source(root.path(), "src/demo.py", source);
        let spans = form_spans(source, &path, root.path());
        assert_eq!(spans.len(), 1);
        assert!(spans[0].1.private);
        assert_eq!(spans[0].0.1, "defn-/_hide");
    }

    // MU-12: mutation ids are compact Python json.dumps arrays with ints plain
    #[test]
    fn mu_12_mutation_id_is_compact_python_json() {
        let id = mutation_id("src/a.py", "demo", "defn/place", 1, 2, ">", ">=");
        assert_eq!(id, r#"["src/a.py","demo","defn/place",1,2,">",">="]"#);
    }

    // MU-12: non-ASCII becomes lowercase \uXXXX and astral characters become surrogate pairs
    #[test]
    fn mu_12_mutation_id_escapes_non_ascii_as_python_does() {
        let id = mutation_id(
            "src/caf\u{e9}.py",
            "demo",
            "defn/x",
            1,
            2,
            "\u{1F600}",
            "\"",
        );
        assert_eq!(
            id,
            r#"["src/caf\u00e9.py","demo","defn/x",1,2,"\ud83d\ude00","\""]"#
        );
    }

    // MU-12: control characters, backslash, quote and DEL are escaped as Python json does
    #[test]
    fn mu_12_mutation_id_escapes_controls_and_del_like_python() {
        let original = "a\n\t\r\u{8}\u{c}\\\"\u{1}\u{7f}";
        let id = mutation_id("f", "n", "x", 0, 0, original, "");
        assert_eq!(id, r#"["f","n","x",0,0,"a\n\t\r\b\f\\\"\u0001\u007f",""]"#);
    }

    // MU-12: mutation_file and mutation_namespace read back what mutation_id wrote
    #[test]
    fn mu_12_mutation_file_and_namespace_round_trip() {
        let id = mutation_id("src/caf\u{e9}.py", "demo.core", "defn/x", 3, 4, ">", ">=");
        assert_eq!(mutation_file(&id).as_deref(), Some("src/caf\u{e9}.py"));
        assert_eq!(mutation_namespace(&id).as_deref(), Some("demo.core"));
    }

    // MU-12: mutation field readers return None for bad JSON, non-arrays and short arrays
    #[test]
    fn mu_12_mutation_fields_reject_bad_json() {
        assert_eq!(mutation_file("not-json"), None);
        assert_eq!(mutation_file("\"hello\""), None);
        assert_eq!(mutation_file("[1]"), None);
        assert_eq!(mutation_namespace("[\"only\"]"), None);
        let id = r#"["src/a.py","demo","defn/place",1,2,">",">="]"#;
        assert_eq!(mutation_file(id).as_deref(), Some("src/a.py"));
    }

    // MU-12: a site in a non-ASCII file keeps its real path through the mutation id
    #[test]
    fn mu_12_sites_in_a_non_ascii_file_round_trip_their_path() {
        let sites = sites_of("src/caf\u{e9}.py", "def place(x):\n    return x > 0\n");
        assert!(!sites.is_empty());
        assert!(
            sites
                .iter()
                .all(|s| mutation_file(&s.mutation_id).as_deref() == Some("src/caf\u{e9}.py"))
        );
        assert!(sites.iter().all(|s| s.mutation_id.contains(r"\u00e9")));
    }

    // MU-12: Clojure sites bind to public and private forms in the file's namespace
    #[test]
    fn mu_12_clojure_sites_bind_to_public_and_private_forms() {
        let source = "(ns demo.core)\n(defn place [x]\n  (if (> x 0)\n    (+ x 1)\n    0))\n\
                      (defn- hide []\n  false)\n";
        let sites = sites_of("src/demo/core.clj", source);
        assert!(has(&sites, "defn/place", "if", "if-not"));
        assert!(has(&sites, "defn/place", ">", ">="));
        assert!(has(&sites, "defn/place", "+", "-"));
        assert!(has(&sites, "defn/place", "0", "1"));
        assert!(has(&sites, "defn/place", "1", "0"));
        assert!(has(&sites, "defn-/hide", "false", "true"));
        assert!(sites.iter().all(|s| s.namespace == "demo.core"));
    }

    // MU-12: Clojure strings, comments and quoted forms bind no sites
    #[test]
    fn mu_12_clojure_ignores_strings_comments_and_quotes() {
        let source = "(ns demo.core)\n(defn place []\n  (comment (+ 1 0))\n  (quote (+ 1 0))\n  (str \"true + 1\"))\n";
        assert!(sites_of("src/demo/core.clj", source).is_empty());
    }

    // MU-12: Java sites bind to the enclosing class and a private method is marked private
    #[test]
    fn mu_12_java_sites_follow_mutate4java_and_mark_private_methods() {
        let source = "package demo;\nclass Board {\n  int place(int x) {\n    if (x > 0 && ready)\n      \
                      return x + 1;\n    return !ok ? -x : 0;\n  }\n  private int hide() { return 1; }\n}\n";
        let sites = sites_of("src/demo/Board.java", source);
        let public: Vec<&Site> = sites.iter().filter(|s| s.form_id == "defn/place").collect();
        let private: Vec<&Site> = sites.iter().filter(|s| s.form_id == "defn-/hide").collect();
        let public_pairs: Vec<(&str, &str)> = public
            .iter()
            .map(|s| (s.original.as_str(), s.mutant.as_str()))
            .collect();
        assert!(public_pairs.contains(&("+", "-")));
        assert!(public_pairs.contains(&(">", ">=")));
        assert!(public_pairs.contains(&("&&", "||")));
        assert!(public_pairs.contains(&("!", "")));
        assert!(public_pairs.contains(&("-", "")));
        assert!(public_pairs.contains(&("0", "1")));
        assert!(!public_pairs.contains(&("/", "*")));
        assert!(private.iter().all(|s| s.original == "1"));
        assert!(sites.iter().all(|s| s.namespace == "demo.Board"));
    }

    // MU-12: Java swaps division and multiplication both ways
    #[test]
    fn mu_12_java_swaps_division_both_ways() {
        let source =
            "package demo;\nclass Math {\n  int place(int a, int b) { return a * b + a / b; }\n}\n";
        let sites = sites_of("src/demo/Math.java", source);
        let found = pairs(&sites);
        assert!(found.contains(&("*", "/")));
        assert!(found.contains(&("/", "*")));
        assert!(found.contains(&("+", "-")));
    }

    // MU-12: Go free functions and methods bind to their own namespaces and forms
    #[test]
    fn mu_12_go_splits_receivers_and_binds_forms() {
        let source = "package demo\nfunc Run(a int, b int) int {\n    if a > 0 && b == 1 {\n        \
                      return a * b\n    }\n    return a / b\n}\ntype Widget struct{}\n\
                      func (w Widget) Run() bool { return true }\n";
        let sites = sites_of("demo.go", source);
        let free: Vec<Site> = sites
            .iter()
            .filter(|s| s.namespace == "demo")
            .cloned()
            .collect();
        let method: Vec<Site> = sites
            .iter()
            .filter(|s| s.namespace == "demo.Widget")
            .cloned()
            .collect();
        let free_pairs = pairs(&free);
        assert!(free_pairs.contains(&("*", "/")));
        assert!(!free_pairs.contains(&("/", "*")));
        assert!(free_pairs.contains(&("&&", "||")));
        assert!(free_pairs.contains(&("==", "!=")));
        assert!(free_pairs.contains(&("0", "1")));
        assert!(free.iter().all(|s| s.form_id == "defn/Run"));
        assert!(has(&method, "defn/Run", "true", "false"));
    }

    // MU-12: an unexported Go function binds to a private form
    #[test]
    fn mu_12_go_unexported_function_is_private() {
        let sites = sites_of(
            "demo.go",
            "package demo\nfunc hide() bool { return false }\n",
        );
        assert_eq!(
            sites.first().map(|s| s.form_id.as_str()),
            Some("defn-/hide")
        );
    }

    // MU-12: Python, TypeScript and Rust sites follow the Java decisions
    #[test]
    fn mu_12_python_typescript_and_rust_share_the_java_decisions() {
        let python = "def place(x):\n    if x > 0 and ready:\n        return x + 1\n    if not ok:\n        return 0\n";
        let typescript = "export function place(x: number) {\n  if (x > 0 && ready) return x + 1;\n  return true === false;\n}\n";
        let rust = "fn place(x: i32) -> i32 {\n    if x > 0 && ready { return x + 1; }\n    if !ok { return -x; }\n    0\n}\n";
        let py = sites_of("src/demo/app.py", python);
        let ts = sites_of("src/demo/app.ts", typescript);
        let rs = sites_of("src/lib.rs", rust);
        let py_pairs = pairs(&py);
        assert!(py_pairs.contains(&("and", "or")));
        assert!(py_pairs.contains(&("not", "")));
        assert!(!py_pairs.contains(&("True", "False")));
        assert!(py.iter().all(|s| s.namespace == "demo.app"));
        let ts_pairs = pairs(&ts);
        assert!(ts_pairs.contains(&("===", "!==")));
        assert!(ts_pairs.contains(&("&&", "||")));
        assert!(ts.iter().all(|s| s.namespace == "demo.app"));
        let rs_pairs = pairs(&rs);
        assert!(rs_pairs.contains(&("&&", "||")));
        assert!(rs_pairs.contains(&("!", "")));
        assert!(rs_pairs.contains(&("-", "")));
    }

    // MU-12: a Python method binds to its class namespace and a leading underscore is private
    #[test]
    fn mu_12_python_private_method_and_class_namespace() {
        let source = "class Board:\n    def place(self, x):\n        return x > 0\n    def _hide(self):\n        return False\n";
        let sites = sites_of("src/demo/board.py", source);
        assert!(
            sites
                .iter()
                .filter(|s| s.name == "place")
                .all(|s| s.form_id == "defn/place")
        );
        assert!(
            sites
                .iter()
                .filter(|s| s.name == "_hide")
                .all(|s| s.form_id == "defn-/_hide")
        );
        assert!(sites.iter().all(|s| s.namespace == "demo.board.Board"));
    }

    // MU-12: operators inside string literals are not sites
    #[test]
    fn mu_12_string_operators_are_not_sites() {
        let source = "def place():\n    return \"x > 0 and true\"\n";
        assert!(sites_of("src/demo/app.py", source).is_empty());
    }

    // MU-12: TypeScript mutates nullish coalescing and optional chaining, owned by the module
    #[test]
    fn mu_12_typescript_mutates_nullish_and_optional_chaining() {
        let source = "\nexport function place(a, b) {\n  return a ?? b?.c ?? b?.[0] ?? b?.();\n}\n";
        let sites = sites_of("src/demo/app.ts", source);
        assert_eq!(
            sites
                .iter()
                .filter(|s| s.original == "??" && s.mutant == "||")
                .count(),
            3
        );
        let chains: Vec<&str> = sites
            .iter()
            .filter(|s| s.original == "?.")
            .map(|s| s.mutant.as_str())
            .collect();
        assert!(chains.contains(&".") && chains.contains(&""));
        assert!(sites.iter().all(|s| s.namespace == "demo.app"));
    }

    // MU-12: a JavaScript route handler owns its nullish site, not the enclosing mount function
    #[test]
    fn mu_12_javascript_express_handler_owns_its_nullish_site() {
        let source = "\nexport function mount(app) {\n  app.get(\"/users\", (req, res) => {\n    return req.body ?? false;\n  });\n}\n";
        let sites = sites_of("src/demo/routes.js", source);
        assert!(sites.iter().all(|s| s.namespace == "demo.routes"));
        assert!(
            sites
                .iter()
                .any(|s| s.name == "GET /users" && s.original == "??" && s.mutant == "||")
        );
        assert!(sites.iter().all(|s| s.name != "mount"));
    }

    // MU-12: a quoted nullish operator is not a site
    #[test]
    fn mu_12_a_quoted_nullish_operator_is_not_a_site() {
        let source = "export function place() {\n  return \"a ?? b?.c\";\n}\n";
        assert!(sites_of("src/demo/app.ts", source).is_empty());
    }

    // MU-12: TypeScript types are not mutation sites
    #[test]
    fn mu_12_typescript_types_are_not_mutation_sites() {
        let source = "type Flag = { on: true };\ninterface Box { open: false }\n\
                      export function check(n: number): { ok: true } | { ok: false; used?: true } {\n\
                      \x20 const seen: Promise<{ done: false }> | null = null;\n\
                      \x20 return { ok: n > 0 };\n}\n";
        let sites = sites_of("src/demo/check.ts", source);
        let mut described: Vec<(usize, &str, &str)> = sites
            .iter()
            .map(|s| (s.line, s.original.as_str(), s.mutant.as_str()))
            .collect();
        described.sort();
        assert_eq!(described, vec![(5, "0", "1"), (5, ">", ">=")]);
    }

    // MU-12: a site remembers the line where its statement starts as its anchor
    #[test]
    fn mu_12_a_site_remembers_where_its_statement_starts() {
        let source = "export function save(db: Db, now: number) {\n  db\n    .prepare(\"x\")\n    \
                      .bind(now + 1)\n    .run();\n  return {\n    ok: true,\n  };\n}\n";
        let sites = sites_of("src/demo/save.ts", source);
        let anchored = |line: usize, original: &str, anchor: usize| {
            sites
                .iter()
                .any(|s| s.line == line && s.original == original && s.anchor_line == anchor)
        };
        assert!(anchored(4, "+", 2));
        assert!(anchored(4, "1", 2));
        assert!(anchored(7, "true", 6));
    }

    // MU-12: sites_in_file copies the file key into every site and its mutation id
    #[test]
    fn mu_12_sites_carry_the_file_key_into_their_mutation_ids() {
        let root = TempDir::new().expect("tempdir");
        let source = "def place(x):\n    return x > 0\n";
        let path = write_source(root.path(), "src/demo.py", source);
        let sites = sites_in_file(source, &path, root.path(), "src/demo.py");
        assert!(!sites.is_empty());
        assert!(sites.iter().all(|s| s.file == "src/demo.py"));
        assert!(
            sites
                .iter()
                .all(|s| mutation_file(&s.mutation_id).as_deref() == Some("src/demo.py"))
        );
    }

    #[test]
    fn mu_9_contains_byte_range_is_start_inclusive_end_exclusive() {
        let f = bytes_function("f", 1, 3, (10, 20));
        assert!(contains(&f, 2, Some(10)));
        assert!(contains(&f, 2, Some(19)));
        assert!(!contains(&f, 2, Some(20)));
        assert!(!contains(&f, 2, Some(9)));
    }

    #[test]
    fn mu_9_contains_line_range_is_inclusive_on_both_ends() {
        let f = function("f", "demo", 1, 3);
        assert!(!contains(&f, 0, None));
        assert!(contains(&f, 1, None));
        assert!(contains(&f, 3, None));
        assert!(!contains(&f, 4, None));
    }

    #[test]
    fn mu_9_tightness_zero_length_byte_span_is_zero() {
        assert_eq!(tightness(&bytes_function("f", 1, 4, (5, 5))), 0);
        assert_eq!(tightness(&bytes_function("f", 1, 4, (5, 9))), 4);
        assert_eq!(tightness(&bytes_function("f", 1, 4, (9, 5))), 3_000_000);
    }
}
