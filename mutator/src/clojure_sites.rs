//! Port of the Clojure branch of mutator/src/mutator/sites.py (BSD Zero Clause License, Robert C. Martin).

use crate::clojure_reader::{CNode, read};
use crate::model::RawSite;

/// Head symbol, mutant and category: the `_CLOJURE_HEAD` table.
const CLOJURE_HEAD: &[(&str, &str, &str)] = &[
    ("+", "-", "arithmetic"),
    ("-", "+", "arithmetic"),
    ("*", "/", "arithmetic"),
    ("/", "*", "arithmetic"),
    ("inc", "dec", "arithmetic"),
    ("dec", "inc", "arithmetic"),
    (">", ">=", "comparison"),
    (">=", ">", "comparison"),
    ("<", "<=", "comparison"),
    ("<=", "<", "comparison"),
    ("=", "not=", "equality"),
    ("not=", "=", "equality"),
    ("if", "if-not", "conditional"),
    ("if-not", "if", "conditional"),
    ("when", "when-not", "conditional"),
    ("when-not", "when", "conditional"),
    ("and", "or", "logical"),
    ("or", "and", "logical"),
    ("double", "int", "coercion"),
    ("int", "double", "coercion"),
    ("first", "second", "seq"),
    ("second", "first", "seq"),
    ("filter", "remove", "seq"),
    ("remove", "filter", "seq"),
    ("take", "drop", "seq"),
    ("drop", "take", "seq"),
    ("rest", "next", "seq"),
    ("next", "rest", "seq"),
    ("every?", "some", "seq"),
    ("some", "every?", "seq"),
    ("min", "max", "numeric"),
    ("max", "min", "numeric"),
    ("pos?", "neg?", "predicate"),
    ("neg?", "pos?", "predicate"),
    ("even?", "odd?", "predicate"),
    ("odd?", "even?", "predicate"),
    ("nil?", "some?", "predicate"),
    ("some?", "nil?", "predicate"),
];

/// Node kinds whose subtrees are never mutated.
const SKIP_MARKERS: &[&str] = &["comment", "quot", "string", "str_lit", "char_lit"];

#[derive(Clone, Copy)]
struct Visit<'a> {
    node: &'a CNode,
    head: bool,
}

/// Mutation sites in a Clojure source, sorted by (line, start, end).
pub fn discover(source: &str) -> Vec<RawSite> {
    let forms = read(source);
    let newlines: Vec<usize> = source.match_indices('\n').map(|(index, _)| index).collect();
    let mut stack: Vec<Visit> = Vec::new();
    push_children(&mut stack, &forms, None);
    let mut found = Vec::new();
    while let Some(visit) = stack.pop() {
        if is_skipped(visit.node, source) {
            continue;
        }
        found.extend(site_for(visit, source, &newlines));
        push_children(&mut stack, &visit.node.children, head_index(visit.node));
    }
    found.sort_by_key(|site| (site.line, site.start, site.end));
    found
}

/// Pushes children in reverse so that the stack pops them in source order.
fn push_children<'a>(stack: &mut Vec<Visit<'a>>, nodes: &'a [CNode], head: Option<usize>) {
    stack.extend(nodes.iter().enumerate().rev().map(|(index, node)| Visit {
        node,
        head: Some(index) == head,
    }));
}

/// Index of the first symbol child of a list, which is the head of the form.
fn head_index(node: &CNode) -> Option<usize> {
    if node.kind == "list_lit" {
        node.children
            .iter()
            .position(|child| child.kind == "sym_lit")
    } else {
        None
    }
}

fn is_skipped(node: &CNode, source: &str) -> bool {
    SKIP_MARKERS
        .iter()
        .any(|&marker| node.kind.contains(marker))
        || is_quoting_list(node, source)
}

/// A list headed by `quote` or `comment` is data or commented out, so it is not mutated.
fn is_quoting_list(node: &CNode, source: &str) -> bool {
    node.kind == "list_lit"
        && node
            .children
            .iter()
            .find(|child| child.kind == "sym_lit")
            .is_some_and(|head| matches!(text_of(head, source), "comment" | "quote"))
}

fn text_of<'s>(node: &CNode, source: &'s str) -> &'s str {
    source.get(node.start..node.end).unwrap_or("")
}

fn site_for(visit: Visit, source: &str, newlines: &[usize]) -> Option<RawSite> {
    let node = visit.node;
    let text = text_of(node, source);
    let (mutant, category) = mutation(node.kind, visit.head, text)?;
    let line = line_of(newlines, node.start);
    Some(RawSite {
        line,
        anchor_line: line,
        start: node.start,
        end: node.end,
        original: text.to_string(),
        mutant: mutant.to_string(),
        category: category.to_string(),
    })
}

fn mutation(kind: &str, head: bool, text: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "sym_lit" if head => head_mutation(text),
        "bool_lit" => boolean_mutation(text),
        "num_lit" => constant_mutation(text),
        _ => None,
    }
}

fn head_mutation(text: &str) -> Option<(&'static str, &'static str)> {
    CLOJURE_HEAD
        .iter()
        .find(|(name, _, _)| *name == text)
        .map(|&(_, mutant, category)| (mutant, category))
}

fn boolean_mutation(text: &str) -> Option<(&'static str, &'static str)> {
    match text {
        "true" => Some(("false", "boolean")),
        "false" => Some(("true", "boolean")),
        _ => None,
    }
}

fn constant_mutation(text: &str) -> Option<(&'static str, &'static str)> {
    match text {
        "0" => Some(("1", "constant")),
        "1" => Some(("0", "constant")),
        _ => None,
    }
}

/// 1-based line of `offset`, given the byte offsets of every newline.
fn line_of(newlines: &[usize], offset: usize) -> usize {
    newlines.partition_point(|&newline| newline < offset) + 1
}

#[cfg(test)]
mod tests {
    use super::discover;

    fn described(source: &str) -> Vec<String> {
        discover(source)
            .iter()
            .map(|site| format!("{}->{}", site.original, site.mutant))
            .collect()
    }

    // MU-12: a head symbol is replaced by its table partner
    #[test]
    fn mu_12_head_plus_is_mutated_to_minus() {
        assert_eq!(described("(+ a b)"), ["+->-"]);
    }

    // MU-12: if swaps to if-not and the literals flip
    #[test]
    fn mu_12_if_and_literals_are_mutated() {
        assert_eq!(described("(if x 1 0)"), ["if->if-not", "1->0", "0->1"]);
    }

    // MU-12: inc swaps to dec
    #[test]
    fn mu_12_inc_is_mutated_to_dec() {
        assert_eq!(described("(inc x)"), ["inc->dec"]);
    }

    // MU-12: categories come from the head table and the literal rules
    #[test]
    fn mu_12_categories_follow_the_rules() {
        let sites = discover("(< a b) (f true 0)");
        let categories: Vec<&str> = sites.iter().map(|site| site.category.as_str()).collect();
        assert_eq!(categories, ["comparison", "boolean", "constant"]);
    }

    // MU-12: only a head symbol is mutated; a symbol elsewhere is not
    #[test]
    fn mu_12_non_head_symbols_are_not_mutated() {
        assert_eq!(described("(+ + 1)"), ["+->-", "1->0"]);
        assert!(described("[inc]").is_empty());
    }

    // MU-12: true and false swap
    #[test]
    fn mu_12_true_and_false_swap() {
        assert_eq!(described("(f true false)"), ["true->false", "false->true"]);
    }

    // MU-12: the head of an anonymous function is not a list head, so it is not mutated
    #[test]
    fn mu_12_anon_fn_head_is_not_mutated() {
        assert_eq!(described("#(+ % 1)"), ["1->0"]);
    }

    // MU-12: only the exact values 0 and 1 are constants
    #[test]
    fn mu_12_only_zero_and_one_are_constants() {
        assert_eq!(described("(f 0 1 10)"), ["0->1", "1->0"]);
    }

    // MU-12: sites carry 1-based lines, anchor on the same line, and are sorted
    #[test]
    fn mu_12_sites_have_one_based_lines_and_are_sorted() {
        let sites = discover("(+ a b)\n(inc 0)");
        let lines: Vec<usize> = sites.iter().map(|site| site.line).collect();
        let anchors: Vec<usize> = sites.iter().map(|site| site.anchor_line).collect();
        let originals: Vec<&str> = sites.iter().map(|site| site.original.as_str()).collect();
        assert_eq!(lines, [1, 2, 2]);
        assert_eq!(anchors, [1, 2, 2]);
        assert_eq!(originals, ["+", "inc", "0"]);
    }

    // MU-12: site offsets are UTF-8 byte offsets
    #[test]
    fn mu_12_site_offsets_are_byte_offsets() {
        let sites = discover("\"é\" (+ 1 2)");
        assert_eq!((sites[0].start, sites[0].end), (6, 7));
    }

    // MU-12: a deep chain of inc forms is mutated without overflowing the stack
    #[test]
    fn mu_12_deep_nesting_is_discovered_iteratively() {
        let source = format!("{}1{}", "(inc ".repeat(1200), ")".repeat(1200));
        let sites = discover(&source);
        assert_eq!(sites.len(), 1201);
        assert_eq!(sites.last().map(|site| site.original.as_str()), Some("1"));
    }

    // MU-13: a quoted list is data, not code, so it is not mutated
    #[test]
    fn mu_13_quoted_list_is_not_mutated() {
        assert!(described("'(+ 1 2)").is_empty());
        assert!(described("(quote (+ 1 2))").is_empty());
    }

    // MU-13: a comment form is not mutated
    #[test]
    fn mu_13_comment_form_is_not_mutated() {
        assert!(described("(comment (+ 1 2))").is_empty());
    }

    // MU-13: string contents are not mutated
    #[test]
    fn mu_13_string_contents_are_not_mutated() {
        assert!(described("(str \"true + 1\")").is_empty());
    }

    // MU-13: line comments are not mutated
    #[test]
    fn mu_13_line_comments_are_not_mutated() {
        assert!(described("; (+ 1 2)\n(f)").is_empty());
    }

    // MU-13: a syntax-quoted form is not mutated
    #[test]
    fn mu_13_syntax_quoted_form_is_not_mutated() {
        assert!(described("`(+ 1 2)").is_empty());
    }

    // MU-13: a symbol with metadata is never a head that matches the table
    #[test]
    fn mu_13_metadata_symbol_is_never_a_head() {
        assert_eq!(described("(^:x + 1)"), ["1->0"]);
    }
}
