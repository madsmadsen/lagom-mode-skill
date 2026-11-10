//! Port of mutator/src/mutator/sites.py (BSD Zero Clause License, Robert C. Martin).

use tree_sitter::Node;

use crate::clojure_sites;
use crate::model::{Language, RawSite};
use crate::ts;

const BINARY: &[(&str, &str)] = &[
    ("+", "-"),
    ("-", "+"),
    ("*", "/"),
    ("/", "*"),
    (">", ">="),
    (">=", ">"),
    ("<", "<="),
    ("<=", "<"),
    ("==", "!="),
    ("!=", "=="),
    ("===", "!=="),
    ("!==", "==="),
    ("&&", "||"),
    ("||", "&&"),
    ("??", "||"),
    ("and", "or"),
    ("or", "and"),
];

const BINARY_PARENTS: &[&str] = &[
    "binary_expression",
    "binary_operator",
    "comparison_operator",
    "boolean_operator",
];

const UNARY_PARENTS: &[&str] = &["unary_expression", "unary_operator", "not_operator"];

const BOOLEANS: &[(&str, &str)] = &[
    ("true", "false"),
    ("false", "true"),
    ("True", "False"),
    ("False", "True"),
];

const NUMBERS: &[&str] = &[
    "decimal_integer_literal",
    "integer_literal",
    "int_literal",
    "integer",
    "number",
    "num_lit",
];

// A literal in a TypeScript type, such as `{ ok: true }` in a return type, never runs.
const TYPE_NODES: &[&str] = &[
    "type_annotation",
    "type_alias_declaration",
    "interface_declaration",
    "type_arguments",
    "type_parameters",
    "literal_type",
];

/// A node together with its ancestors, parent last, so no tree walk is needed per node.
#[derive(Clone, Copy)]
struct At<'a, 't> {
    node: Node<'t>,
    ancestors: &'a [Node<'t>],
}

impl<'t> At<'_, 't> {
    fn parent(self) -> Option<Node<'t>> {
        self.ancestors.last().copied()
    }

    fn grandparent(self) -> Option<Node<'t>> {
        self.ancestors.iter().rev().nth(1).copied()
    }

    fn parent_kind(self) -> &'t str {
        self.parent().map_or("", |parent| parent.kind())
    }
}

/// Mutation sites in source order. Offsets are UTF-8 byte offsets.
pub fn discover_raw(source: &str, language: Language, path: &str) -> Vec<RawSite> {
    let mut found = match language {
        Language::Clojure => clojure_sites::discover(source),
        _ => discover_tree(source, language, path),
    };
    found.sort_by_key(|site| (site.line, site.start, site.end));
    found
}

/// Splices `mutant` over the bytes `start..end` of `source`.
pub fn apply_site(source: &str, start: usize, end: usize, mutant: &str) -> String {
    let data = source.as_bytes();
    let head = start.min(data.len());
    let tail = end.clamp(head, data.len());
    let bytes = [&data[..head], mutant.as_bytes(), &data[tail..]].concat();
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

fn discover_tree(source: &str, language: Language, path: &str) -> Vec<RawSite> {
    let Some(tree) = ts::parse(source, grammar(language, path)) else {
        return Vec::new();
    };
    let data = source.as_bytes();
    let mut found = Vec::new();
    let mut ancestors: Vec<Node> = Vec::new();
    let mut stack: Vec<(Node, usize)> = vec![(tree.root_node(), 0)];
    while let Some((node, depth)) = stack.pop() {
        ancestors.truncate(depth);
        visit_node(
            At {
                node,
                ancestors: &ancestors,
            },
            data,
            language,
            &mut found,
        );
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        ancestors.push(node);
    }
    found
}

fn grammar(language: Language, path: &str) -> &'static str {
    match language {
        Language::TypeScript => typescript_grammar(path),
        _ => language.name(),
    }
}

fn typescript_grammar(path: &str) -> &'static str {
    if path.ends_with(".tsx") {
        "tsx"
    } else if [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|ext| path.ends_with(*ext))
    {
        "javascript"
    } else {
        "typescript"
    }
}

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(from, _)| *from == key)
        .map(|(_, to)| *to)
}

fn category(token: &str) -> &'static str {
    match token {
        ">" | ">=" | "<" | "<=" => "comparison",
        "==" | "!=" | "===" | "!==" => "equality",
        "&&" | "||" | "??" | "and" | "or" => "logical",
        _ => "arithmetic",
    }
}

fn binary_mutant(language: Language, token: &str) -> Option<&'static str> {
    if language == Language::Go && token == "/" {
        return None;
    }
    if token == "??" && language != Language::TypeScript {
        return None;
    }
    lookup(BINARY, token)
}

fn flip01(text: &str) -> &'static str {
    if text == "0" { "1" } else { "0" }
}

fn optional_mutant(at: At) -> Option<&'static str> {
    let host = if at.parent_kind() == "optional_chain" {
        at.grandparent()?
    } else {
        at.parent()?
    };
    match host.kind() {
        "member_expression" => Some("."),
        "call_expression" | "subscript_expression" => Some(""),
        _ => None,
    }
}

fn is_statement(kind: &str) -> bool {
    kind.ends_with("_statement") || kind.ends_with("_declaration")
}

fn statement_line(at: At) -> usize {
    at.ancestors
        .iter()
        .rev()
        .find(|ancestor| is_statement(ancestor.kind()))
        .copied()
        .map_or_else(|| ts::start_line(at.node), ts::start_line)
}

fn in_type(at: At) -> bool {
    at.ancestors
        .iter()
        .any(|ancestor| TYPE_NODES.contains(&ancestor.kind()))
}

fn add(found: &mut Vec<RawSite>, at: At, original: &str, mutant: &str, category: &str) {
    if mutant == original {
        return;
    }
    found.push(RawSite {
        line: ts::start_line(at.node),
        anchor_line: statement_line(at),
        start: at.node.start_byte(),
        end: at.node.end_byte(),
        original: original.to_owned(),
        mutant: mutant.to_owned(),
        category: category.to_owned(),
    });
}

fn visit_binary(at: At, token: &str, language: Language, found: &mut Vec<RawSite>) {
    if !BINARY_PARENTS.contains(&at.parent_kind()) {
        return;
    }
    if let Some(mutant) = binary_mutant(language, token) {
        add(found, at, token, mutant, category(token));
    }
}

fn visit_optional(at: At, token: &str, language: Language, found: &mut Vec<RawSite>) {
    if language != Language::TypeScript || token != "?." {
        return;
    }
    if let Some(mutant) = optional_mutant(at) {
        add(found, at, token, mutant, "optional");
    }
}

fn visit_not(at: At, token: &str, found: &mut Vec<RawSite>) {
    let unary_host = UNARY_PARENTS.contains(&at.parent_kind());
    if matches!(token, "!" | "not") && unary_host {
        add(found, at, token, "", "unary");
    }
}

fn visit_negation(at: At, token: &str, found: &mut Vec<RawSite>) {
    if token == "-" && UNARY_PARENTS.contains(&at.parent_kind()) {
        add(found, at, token, "", "unary");
    }
}

fn visit_boolean_token(at: At, token: &str, found: &mut Vec<RawSite>) {
    if !matches!(at.node.kind(), "true" | "false") {
        return;
    }
    if let Some(mutant) = lookup(BOOLEANS, token) {
        add(found, at, token, mutant, "boolean");
    }
}

fn visit_constant(at: At, token: &str, found: &mut Vec<RawSite>) {
    if NUMBERS.contains(&at.node.kind()) && matches!(token, "0" | "1") {
        add(found, at, token, flip01(token), "constant");
    }
}

// A literal sits inside the same expression as an operator, so these checks are
// independent. `0` is a child of `a > 0`, not an operator.
fn visit_leaf(at: At, data: &[u8], language: Language, found: &mut Vec<RawSite>) {
    let token = ts::node_text(data, at.node);
    visit_binary(at, token, language, found);
    visit_optional(at, token, language, found);
    visit_not(at, token, found);
    visit_negation(at, token, found);
    visit_boolean_token(at, token, found);
    visit_constant(at, token, found);
}

fn visit_node(at: At, data: &[u8], language: Language, found: &mut Vec<RawSite>) {
    if language == Language::TypeScript && in_type(at) {
        return;
    }
    if at.node.child_count() == 0 {
        visit_leaf(at, data, language, found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(sites: &[RawSite], original: &str, mutant: &str) -> bool {
        sites
            .iter()
            .any(|site| site.original == original && site.mutant == mutant)
    }

    fn pairs(sites: &[RawSite]) -> Vec<(&str, &str)> {
        sites
            .iter()
            .map(|site| (site.original.as_str(), site.mutant.as_str()))
            .collect()
    }

    fn apply_one(
        source: &str,
        language: Language,
        path: &str,
        original: &str,
        mutant: &str,
    ) -> String {
        discover_raw(source, language, path)
            .iter()
            .find(|site| site.original == original && site.mutant == mutant)
            .map_or_else(
                || source.to_owned(),
                |site| apply_site(source, site.start, site.end, &site.mutant),
            )
    }

    /// MU-12: the Java row of the README table: booleans, comparisons, equality, arithmetic,
    /// logic, delete `!` and unary `-`, and swap `0`/`1`.
    #[test]
    fn mu_12_java_row_is_applied() {
        let source = r#"class Board {
  boolean place(int x, boolean ok) {
    if (x > 0 && ok == true || x < 1 && x <= 2 && x != 3) return !ok;
    int y = -x * 2 / 4 + 1;
    return x >= 0 && ok || x - 1 == 0 && false;
  }
}
"#;
        let found = discover_raw(source, Language::Java, "Board.java");
        for (original, mutant) in [
            (">", ">="),
            (">=", ">"),
            ("<", "<="),
            ("<=", "<"),
            ("==", "!="),
            ("!=", "=="),
            ("true", "false"),
            ("false", "true"),
            ("&&", "||"),
            ("||", "&&"),
            ("+", "-"),
            ("-", "+"),
            ("*", "/"),
            ("/", "*"),
            ("!", ""),
            ("-", ""),
            ("0", "1"),
            ("1", "0"),
        ] {
            assert!(
                has(&found, original, mutant),
                "missing {original} -> {mutant}"
            );
        }
    }

    /// MU-12: the Go row of the README table: `*` to `/` only, comparisons, `==`/`!=`,
    /// `true`/`false`, `&&`/`||`, and `0`/`1`.
    #[test]
    fn mu_12_go_row_is_applied_and_slash_is_not_mutated() {
        let source = "package demo\nfunc Run(a int, b int) bool {\n\tif a > 0 && b == 1 || a+b < 2 && a != 3 {\n\t\treturn a*b >= a-1 && true\n\t}\n\treturn a/b <= 0 || false\n}\n";
        let found = discover_raw(source, Language::Go, "demo.go");
        for (original, mutant) in [
            ("+", "-"),
            ("-", "+"),
            ("*", "/"),
            (">", ">="),
            ("<", "<="),
            ("==", "!="),
            ("!=", "=="),
            ("&&", "||"),
            ("||", "&&"),
            ("true", "false"),
            ("false", "true"),
            ("0", "1"),
            ("1", "0"),
        ] {
            assert!(
                has(&found, original, mutant),
                "missing {original} -> {mutant}"
            );
        }
        assert!(
            !found.iter().any(|site| site.original == "/"),
            "Go `/` must not mutate"
        );
    }

    /// MU-12: Go `*` becomes `/` in the program text, and Go `/` is left alone.
    #[test]
    fn mu_12_go_star_becomes_slash_in_the_program() {
        let source = "package demo\nfunc f(a int, b int) int { return a / b * 2 }\n";
        assert_eq!(
            apply_one(source, Language::Go, "demo.go", "*", "/"),
            "package demo\nfunc f(a int, b int) int { return a / b / 2 }\n"
        );
    }

    /// MU-12: the TypeScript row: the Java set, `===`/`!==`, `??` to `||`, `?.` to `.`, and a
    /// call or index drops `?.`.
    #[test]
    fn mu_12_typescript_row_is_applied() {
        let source = r#"export function place(a: number, b: any, ok: boolean) {
  if (a > 0 && b === 1 || b !== 2) return a ?? b?.c;
  if (!ok) return -a + 0;
  return a?.() ?? b?.[0];
}
"#;
        let found = discover_raw(source, Language::TypeScript, "app.ts");
        for (original, mutant) in [
            (">", ">="),
            ("&&", "||"),
            ("||", "&&"),
            ("===", "!=="),
            ("!==", "==="),
            ("??", "||"),
            ("?.", "."),
            ("?.", ""),
            ("!", ""),
            ("-", ""),
            ("+", "-"),
            ("0", "1"),
            ("1", "0"),
        ] {
            assert!(
                has(&found, original, mutant),
                "missing {original} -> {mutant}"
            );
        }
    }

    /// MU-12: `a?.b` becomes `a.b` in the program text.
    #[test]
    fn mu_12_typescript_optional_member_drops_question_dot() {
        assert_eq!(
            apply_one(
                "export function f(a) { return a?.b; }\n",
                Language::TypeScript,
                "f.ts",
                "?.",
                "."
            ),
            "export function f(a) { return a.b; }\n"
        );
    }

    /// MU-12: `a?.()` becomes `a()` in the program text.
    #[test]
    fn mu_12_typescript_optional_call_drops_question_dot() {
        assert_eq!(
            apply_one(
                "export function f(a) { return a?.(); }\n",
                Language::TypeScript,
                "f.ts",
                "?.",
                ""
            ),
            "export function f(a) { return a(); }\n"
        );
    }

    /// MU-12: `??` becomes `||` in the program text.
    #[test]
    fn mu_12_typescript_nullish_becomes_or() {
        assert_eq!(
            apply_one(
                "export function f(a, b) { return a ?? b; }\n",
                Language::TypeScript,
                "f.ts",
                "??",
                "||"
            ),
            "export function f(a, b) { return a || b; }\n"
        );
    }

    /// MU-12: `.js` files use the TypeScript rules, including `??` and `?.`.
    #[test]
    fn mu_12_javascript_files_use_typescript_rules() {
        let found = discover_raw(
            "function f(a) { return a?.b ?? 1; }\n",
            Language::TypeScript,
            "app.js",
        );
        assert!(has(&found, "?.", "."));
        assert!(has(&found, "??", "||"));
    }

    /// MU-12: the Rust row: the Java set, with `&&`/`||`, `!` and unary `-` deleted.
    #[test]
    fn mu_12_rust_row_is_applied() {
        let source = "fn place(x: i32, ok: bool) -> i32 {\n    if x > 0 && ready || !ok { return -x + 1; }\n    0\n}\n";
        let found = discover_raw(source, Language::Rust, "src/lib.rs");
        for (original, mutant) in [
            (">", ">="),
            ("&&", "||"),
            ("||", "&&"),
            ("!", ""),
            ("-", ""),
            ("+", "-"),
            ("1", "0"),
            ("0", "1"),
        ] {
            assert!(
                has(&found, original, mutant),
                "missing {original} -> {mutant}"
            );
        }
    }

    /// MU-12: Rust `!ok` and unary `-x` are deleted in the program text.
    #[test]
    fn mu_12_rust_bang_and_unary_minus_are_deleted() {
        let source = "fn f(ok: bool, x: i32) -> i32 { if !ok { return -x; } 0 }\n";
        assert_eq!(
            apply_one(source, Language::Rust, "src/lib.rs", "!", ""),
            "fn f(ok: bool, x: i32) -> i32 { if ok { return -x; } 0 }\n"
        );
        assert_eq!(
            apply_one(source, Language::Rust, "src/lib.rs", "-", ""),
            "fn f(ok: bool, x: i32) -> i32 { if !ok { return x; } 0 }\n"
        );
    }

    /// MU-12: the Python row: the Java set spelled `and`/`or`, `not`, and `True`/`False`.
    #[test]
    fn mu_12_python_row_is_applied() {
        let source = "def place(x, ready):\n    if x > 0 and ready or not ready:\n        return x - 1 == 0 and True\n    return False\n";
        let found = discover_raw(source, Language::Python, "src/app.py");
        for (original, mutant) in [
            ("and", "or"),
            ("or", "and"),
            ("not", ""),
            (">", ">="),
            ("-", "+"),
            ("==", "!="),
            ("1", "0"),
            ("0", "1"),
            ("True", "False"),
            ("False", "True"),
        ] {
            assert!(
                has(&found, original, mutant),
                "missing {original} -> {mutant}"
            );
        }
    }

    /// MU-12: Python `and`, `not`, and `True` are rewritten in the program text. Deleting
    /// `not` leaves its surrounding spaces behind.
    #[test]
    fn mu_12_python_and_not_and_true_are_rewritten() {
        assert_eq!(
            apply_one(
                "def f(a, b):\n    return a and b\n",
                Language::Python,
                "a.py",
                "and",
                "or"
            ),
            "def f(a, b):\n    return a or b\n"
        );
        assert_eq!(
            apply_one(
                "def f(a):\n    return not a\n",
                Language::Python,
                "a.py",
                "not",
                ""
            ),
            "def f(a):\n    return  a\n"
        );
        assert_eq!(
            apply_one(
                "def f():\n    return True\n",
                Language::Python,
                "a.py",
                "True",
                "False"
            ),
            "def f():\n    return False\n"
        );
    }

    /// MU-12: Python `not in` is not a site. The original reports no site for it, so the
    /// program text is left as written.
    #[test]
    fn mu_12_python_not_in_has_no_sites() {
        assert!(
            discover_raw(
                "def f(x, items):\n    return x not in items\n",
                Language::Python,
                "a.py"
            )
            .is_empty()
        );
    }

    /// MU-12: `0` and `1` swap in the program text.
    #[test]
    fn mu_12_constants_swap_zero_and_one() {
        assert_eq!(
            apply_one(
                "class A { int f() { return 0; } }\n",
                Language::Java,
                "A.java",
                "0",
                "1"
            ),
            "class A { int f() { return 1; } }\n"
        );
        assert_eq!(
            apply_one(
                "class A { int f() { return 1; } }\n",
                Language::Java,
                "A.java",
                "1",
                "0"
            ),
            "class A { int f() { return 0; } }\n"
        );
    }

    /// MU-12: a unary minus yields only its sign as a site. The operand `a` is not a site.
    #[test]
    fn mu_12_unary_minus_site_is_only_the_sign() {
        let rust = discover_raw(
            "fn f(a: i32) -> i32 {\n    -a\n}\n",
            Language::Rust,
            "src/lib.rs",
        );
        assert_eq!(pairs(&rust), vec![("-", "")]);
        let python = discover_raw("def f(a):\n    return -a\n", Language::Python, "a.py");
        assert_eq!(pairs(&python), vec![("-", "")]);
        let typescript = discover_raw(
            "function f(a: number) { return -a; }\n",
            Language::TypeScript,
            "a.ts",
        );
        assert_eq!(pairs(&typescript), vec![("-", "")]);
    }

    /// MU-12: number literals other than `0` and `1` are not constant sites. Only `+` mutates.
    #[test]
    fn mu_12_numbers_other_than_zero_and_one_are_not_sites() {
        let found = discover_raw(
            "fn f() -> i32 {\n    let x = 7;\n    x + 2\n}\n",
            Language::Rust,
            "src/lib.rs",
        );
        assert_eq!(pairs(&found), vec![("+", "-")]);
        assert_eq!(
            found
                .iter()
                .filter(|site| site.category == "constant")
                .count(),
            0
        );
    }

    /// MU-12: string contents spelled `0` or `1` are not constant sites.
    #[test]
    fn mu_12_string_contents_are_not_constant_sites() {
        let found = discover_raw(
            "fn f() {\n    let s = \"01\";\n    let t = \"0\";\n    let u = \"1\";\n}\n",
            Language::Rust,
            "src/lib.rs",
        );
        assert_eq!(
            found
                .iter()
                .filter(|site| site.category == "constant")
                .count(),
            0
        );
    }

    /// MU-12: `0` and `1` in numeric literals are constant sites that swap. This is the control.
    #[test]
    fn mu_12_zero_and_one_are_constant_sites() {
        let found = discover_raw(
            "fn f() -> i32 {\n    let z = 0;\n    let o = 1;\n    z + o\n}\n",
            Language::Rust,
            "src/lib.rs",
        );
        let constants: Vec<RawSite> = found
            .into_iter()
            .filter(|site| site.category == "constant")
            .collect();
        assert_eq!(pairs(&constants), vec![("0", "1"), ("1", "0")]);
    }

    /// MU-12: offsets are UTF-8 byte offsets, so a multi-byte character before a site is counted.
    #[test]
    fn mu_12_offsets_are_utf8_bytes() {
        let source = "name = 'é'\nok = a > 0\n";
        assert_eq!(
            apply_one(source, Language::Python, "a.py", ">", ">="),
            "name = 'é'\nok = a >= 0\n"
        );
    }

    /// MU-12: sites come back sorted by (line, start, end).
    #[test]
    fn mu_12_sites_are_sorted_by_line_then_offset() {
        let found = discover_raw(
            "def f(a):\n    return a - 1 > 0\n",
            Language::Python,
            "a.py",
        );
        assert!(!found.is_empty());
        assert!(found.windows(2).all(|pair| {
            (pair[0].line, pair[0].start, pair[0].end) <= (pair[1].line, pair[1].start, pair[1].end)
        }));
    }

    /// MU-12: a 1,200-term expression is walked without quadratic tree walks.
    #[test]
    fn mu_12_deep_expression_is_walked() {
        let expr = vec!["1"; 1200].join(" + ");
        let found = discover_raw(
            &format!("def f():\n    return {expr}\n"),
            Language::Python,
            "a.py",
        );
        assert!(has(&found, "1", "0"));
    }

    /// MU-12: the grammar follows the file extension for TypeScript and is the language name
    /// otherwise.
    #[test]
    fn mu_12_grammar_follows_extension() {
        assert_eq!(grammar(Language::TypeScript, "src/app.tsx"), "tsx");
        assert_eq!(grammar(Language::TypeScript, "src/app.ts"), "typescript");
        for path in ["src/app.js", "src/app.jsx", "src/app.mjs", "src/app.cjs"] {
            assert_eq!(grammar(Language::TypeScript, path), "javascript");
        }
        assert_eq!(grammar(Language::Python, "src/app.py"), "python");
        assert_eq!(grammar(Language::Go, "demo.go"), "go");
    }

    /// MU-12: a site remembers the first line of the statement that holds it.
    #[test]
    fn mu_12_anchor_line_is_the_enclosing_statement_start() {
        let source = r#"export function save(db: Db, now: number) {
  db
    .prepare("x")
    .bind(now + 1)
    .run();
  return {
    ok: true,
  };
}
"#;
        let found = discover_raw(source, Language::TypeScript, "src/demo/save.ts");
        let anchors: Vec<(usize, &str, usize)> = found
            .iter()
            .map(|site| (site.line, site.original.as_str(), site.anchor_line))
            .collect();
        assert!(anchors.contains(&(4, "+", 2)));
        assert!(anchors.contains(&(4, "1", 2)));
        assert!(anchors.contains(&(7, "true", 6)));
    }

    /// MU-13: tokens inside strings and comments are not sites in any language.
    #[test]
    fn mu_13_string_and_comment_tokens_are_not_sites() {
        let cases = [
            (
                Language::Java,
                "Board.java",
                "class A {\n  // a + 1 == 0 !ok\n  String s = \"x > 0 && true\";\n}\n",
            ),
            (
                Language::Python,
                "app.py",
                "def place():\n    # a > 1 and not True\n    return \"x > 0 and true\"\n",
            ),
            (
                Language::TypeScript,
                "app.ts",
                "export function place() {\n  // a ?? b?.c\n  return \"a ?? b?.c\";\n}\n",
            ),
            (
                Language::Go,
                "demo.go",
                "package demo\nfunc place() string {\n\t// a + 1\n\treturn `x == 0 && true`\n}\n",
            ),
            (
                Language::Rust,
                "src/lib.rs",
                "fn place() -> &'static str {\n    // !ok && -1\n    \"x > 0 || !ok\"\n}\n",
            ),
        ];
        for (language, path, source) in cases {
            assert!(
                discover_raw(source, language, path).is_empty(),
                "{path} has a site"
            );
        }
    }

    /// MU-13: literals in TypeScript types are not sites; only the `>` and `0` in the body are.
    #[test]
    fn mu_13_typescript_type_literals_are_not_sites() {
        let source = r#"type Flag = { on: true };
interface Box { open: false }
export function check(n: number): { ok: true } | { ok: false; used?: true } {
  const seen: Promise<{ done: false }> | null = null;
  return { ok: n > 0 };
}
"#;
        let found = discover_raw(source, Language::TypeScript, "src/demo/check.ts");
        let described: Vec<(usize, &str, &str)> = found
            .iter()
            .map(|site| (site.line, site.original.as_str(), site.mutant.as_str()))
            .collect();
        assert_eq!(described, vec![(5, ">", ">="), (5, "0", "1")]);
    }
}
