// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/treesitter.py
// license: MIT

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use tree_sitter::{Language, Node, Parser, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grammar {
    Java,
    Go,
    Python,
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
}

impl Grammar {
    fn language(self) -> Language {
        match self {
            Grammar::Java => tree_sitter_java::LANGUAGE.into(),
            Grammar::Go => tree_sitter_go::LANGUAGE.into(),
            Grammar::Python => tree_sitter_python::LANGUAGE.into(),
            Grammar::Rust => tree_sitter_rust::LANGUAGE.into(),
            Grammar::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Grammar::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Grammar::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        }
    }
}

/// Parses `source` with a fresh parser. Returns the UTF-8 bytes the tree indexes.
pub fn parse(source: &str, grammar: Grammar) -> (Vec<u8>, Tree) {
    let data = source.as_bytes().to_vec();
    let mut parser = Parser::new();
    parser
        .set_language(&grammar.language())
        .expect("bundled grammar matches the tree-sitter ABI");
    let tree = parser
        .parse(&data, None)
        .expect("parser has a language and is not cancelled");
    (data, tree)
}

pub fn node_text<'a>(data: &'a [u8], node: Node) -> &'a str {
    std::str::from_utf8(&data[node.byte_range()])
        .expect("tree-sitter nodes sit on character boundaries of valid UTF-8 source")
}

/// 1-based line where the node starts.
pub fn start_line(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

/// 1-based last line of the node. A node that ends at column 0 ends on the row before.
pub fn end_line(node: Node) -> u32 {
    let end = node.end_position();
    if end.column == 0 {
        return start_line(node).max(end.row as u32);
    }
    end.row as u32 + 1
}

pub fn children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    let kids: Vec<Node> = node.children(&mut cursor).collect();
    kids
}

pub fn child_of_type<'t>(node: Node<'t>, types: &[&str]) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .find(|child| types.contains(&child.kind()));
    found
}

/// Pre-order walk: the node first, then each subtree in source order.
pub fn descendants<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut found = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        found.push(current);
        stack.extend(children(current).into_iter().rev());
    }
    found
}

/// Names of the ancestors of `node` with the given `kind`, outermost first. Each name is the
/// text of the ancestor's `identifier` child; ancestors without one are skipped.
pub fn ancestor_names(data: &[u8], node: Node, kind: &str) -> Vec<String> {
    let mut names: Vec<String> = std::iter::successors(node.parent(), |n| n.parent())
        .filter(|n| n.kind() == kind)
        .filter_map(|n| {
            child_of_type(n, &["identifier"]).map(|ident| node_text(data, ident).to_string())
        })
        .collect();
    names.reverse();
    names
}

/// Absolute path for a file. A relative path is joined to `project_root` when one is given.
/// Relative paths are resolved like Python's `Path.resolve()`.
pub fn absolute_path(path: &str, project_root: Option<&str>) -> String {
    let file_path = Path::new(path);
    if file_path.is_absolute() {
        return file_path
            .components()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned();
    }
    let joined = match project_root.filter(|root| !root.is_empty()) {
        Some(root) => Path::new(root).join(file_path),
        None => file_path.to_path_buf(),
    };
    resolve(&joined).to_string_lossy().into_owned()
}

/// Python's `Path.resolve()`: absolute, lexically normalized, and symlinks resolved on the
/// longest existing prefix. The file need not exist.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut current = lexically_normal(&absolute);
    let mut missing: Vec<OsString> = Vec::new();
    loop {
        if let Ok(real) = fs::canonicalize(&current) {
            return missing.iter().rev().fold(real, |acc, name| acc.join(name));
        }
        let Some(name) = current.file_name().map(OsString::from) else {
            return current;
        };
        missing.push(name);
        current.pop();
    }
}

fn lexically_normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// McCabe complexity: 1 plus each decision node in the subtree, skipping subtrees `skip` accepts.
/// The walk uses an explicit stack, so deep generated code cannot overflow the call stack.
pub fn complexity<'t>(
    node: Node<'t>,
    is_decision: impl Fn(Node<'t>) -> bool,
    skip: impl Fn(Node<'t>) -> bool,
) -> u32 {
    let mut score: u32 = 1;
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if skip(current) {
            continue;
        }
        if is_decision(current) {
            score += 1;
        }
        stack.extend(children(current).into_iter().rev());
    }
    score
}

/// True for a `binary_expression` whose operator is `&&` or `||`.
pub fn binary_logic(node: Node) -> bool {
    node.kind() == "binary_expression"
        && children(node)
            .iter()
            .any(|child| matches!(child.kind(), "&&" | "||"))
}

/// `name`, or `name#2`, `name#3`, ... for the first one not in `used`. The result joins `used`.
pub fn claim(used: &mut HashSet<String>, name: String) -> String {
    let mut candidate = name.clone();
    let mut suffix = 2;
    while used.contains(&candidate) {
        candidate = format!("{name}#{suffix}");
        suffix += 1;
    }
    used.insert(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    // CR-9
    #[test]
    fn cr_9_end_line_of_a_trailing_newline_stays_on_the_last_source_line() {
        let (_data, tree) = parse("def choose():\n    return 1\n", Grammar::Python);
        assert_eq!(end_line(tree.root_node()), 2);
        assert!(child_of_type(tree.root_node(), &["not_a_node"]).is_none());
    }

    // CR-9
    #[test]
    fn cr_9_end_line_of_a_node_that_ends_mid_line_is_its_last_row_plus_one() {
        let source = "class A {\n  void f() {\n  }\n}\n";
        let (data, tree) = parse(source, Grammar::Java);
        let method = descendants(tree.root_node())
            .into_iter()
            .find(|node| node.kind() == "method_declaration")
            .expect("the source declares a method");
        assert_eq!((start_line(method), end_line(method)), (2, 3));
        assert_eq!(node_text(&data, method), "void f() {\n  }");
    }

    // CR-11
    #[test]
    fn cr_11_binary_logic_is_true_for_and_or_or_and_false_for_arithmetic() {
        let source = "class A { boolean f(boolean a, boolean b, int c) { boolean x = a && b; int y = c + 1; return x || b; } }\n";
        let (_data, tree) = parse(source, Grammar::Java);
        let logic: Vec<bool> = descendants(tree.root_node())
            .into_iter()
            .filter(|node| node.kind() == "binary_expression")
            .map(binary_logic)
            .collect();
        assert_eq!(logic, vec![true, false, true]);
    }

    // CR-11
    #[test]
    fn cr_11_complexity_skips_a_subtree_the_skip_predicate_accepts() {
        let source = "class A { void f(boolean a) { if (a) {} } }\n";
        let (_data, tree) = parse(source, Grammar::Java);
        let is_if = |node: Node| node.kind() == "if_statement";
        assert_eq!(complexity(tree.root_node(), is_if, |_| false), 2);
        assert_eq!(
            complexity(tree.root_node(), is_if, |node| node.kind() == "class_body"),
            1
        );
    }

    // CR-9
    #[test]
    fn cr_9_descendants_lists_the_node_first_then_children_in_source_order() {
        let (_data, tree) = parse("a = 1\n", Grammar::Python);
        let kinds: Vec<&str> = descendants(tree.root_node())
            .iter()
            .map(|node| node.kind())
            .collect();
        assert_eq!(
            kinds,
            [
                "module",
                "expression_statement",
                "assignment",
                "identifier",
                "=",
                "integer"
            ]
        );
    }

    // CR-8
    #[test]
    fn cr_8_relative_path_resolves_under_the_project_root_without_dot_segments() {
        let root = tempfile::tempdir().unwrap();
        let expected = fs::canonicalize(root.path())
            .unwrap()
            .join("pkg")
            .join("a.go");
        let got = absolute_path("pkg/../pkg/a.go", Some(root.path().to_str().unwrap()));
        assert_eq!(got, expected.to_str().unwrap());
    }

    // CR-22
    #[test]
    fn cr_22_claim_gives_numbered_suffixes() {
        let mut used = HashSet::new();
        assert_eq!(claim(&mut used, "GET /a".to_string()), "GET /a");
        assert_eq!(claim(&mut used, "GET /a".to_string()), "GET /a#2");
        assert_eq!(claim(&mut used, "GET /a".to_string()), "GET /a#3");
        assert_eq!(claim(&mut used, "GET /b".to_string()), "GET /b");
    }
}
