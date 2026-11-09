//! Port of crapper languages/treesitter.py (BSD Zero Clause License, Robert C. Martin).

use crate::fs_util::resolve;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

/// Parses `source` with the named grammar; None for an unknown grammar or a failed parse.
pub fn parse(source: &str, grammar: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&grammar_language(grammar)?).ok()?;
    parser.parse(source, None)
}

/// Source text of `node`; empty when the bytes are not UTF-8.
pub fn node_text<'a>(data: &'a [u8], node: Node<'_>) -> &'a str {
    node.utf8_text(data).unwrap_or("")
}

/// One-based line where `node` starts.
pub fn start_line(node: Node<'_>) -> usize {
    node.start_position().row + 1
}

/// One-based line where `node` ends; a node ending at column 0 ends on its row.
pub fn end_line(node: Node<'_>) -> usize {
    let end = node.end_position();
    if end.column == 0 {
        start_line(node).max(end.row)
    } else {
        end.row + 1
    }
}

/// First direct child, named or not, whose type is one of `types`.
pub fn child_of_type<'t>(node: Node<'t>, types: &[&str]) -> Option<Node<'t>> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .find(|child| types.contains(&child.kind()))
}

/// `node` and all its descendants in pre-order, without recursion.
pub fn descendants(node: Node<'_>) -> Vec<Node<'_>> {
    let mut found = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        found.push(current);
        stack.extend(
            (0..current.child_count())
                .rev()
                .filter_map(|index| current.child(index)),
        );
    }
    found
}

/// Absolute form of `path`, relative to `project_root` when given; symlinks resolved where they exist.
pub fn absolute_path(path: &str, project_root: Option<&str>) -> String {
    let file_path = Path::new(path);
    if file_path.is_absolute() {
        return path.to_string();
    }
    let joined = match project_root.filter(|root| !root.is_empty()) {
        Some(root) => Path::new(root).join(file_path),
        None => file_path.to_path_buf(),
    };
    let absolute = std::path::absolute(&joined).unwrap_or(joined);
    resolve(&absolute).display().to_string()
}

fn grammar_language(grammar: &str) -> Option<tree_sitter::Language> {
    Some(match grammar {
        "java" => tree_sitter_java::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const SNIPPETS: &[(&str, &str)] = &[
        ("java", "class A { int f() { return 1; } }\n"),
        ("go", "package main\n\nfunc f() int { return 1 }\n"),
        ("typescript", "const n: number = 1;\n"),
        ("tsx", "const el = <div />;\n"),
        ("javascript", "const n = 1;\n"),
        ("rust", "fn f() -> i32 { 1 }\n"),
        ("python", "def f():\n    return 1\n"),
    ];

    fn find_kind<'t>(root: Node<'t>, kind: &str) -> Option<Node<'t>> {
        descendants(root)
            .into_iter()
            .find(|node| node.kind() == kind)
    }

    fn reference_preorder(node: Node<'_>, out: &mut Vec<usize>) {
        out.push(node.id());
        for index in 0..node.child_count() {
            if let Some(child) = node.child(index) {
                reference_preorder(child, out);
            }
        }
    }

    #[test]
    // MU-8: each supported grammar parses a tiny snippet without errors.
    fn mu_8_parse_accepts_each_grammar_without_errors() {
        for (grammar, source) in SNIPPETS {
            let tree = parse(source, grammar).expect("grammar is known");
            assert!(!tree.root_node().has_error(), "{grammar}");
        }
    }

    #[test]
    // MU-8: an unknown grammar name yields no tree.
    fn mu_8_parse_unknown_grammar_is_none() {
        assert!(parse("x", "cobol").is_none());
        assert!(parse("x", "").is_none());
    }

    #[test]
    // MU-8: descendants lists the node and then its subtree in pre-order.
    fn mu_8_descendants_is_pre_order() {
        let tree = parse("a + b;\n", "javascript").expect("javascript is known");
        let root = tree.root_node();
        let got: Vec<usize> = descendants(root)
            .into_iter()
            .map(|node| node.id())
            .collect();
        let mut want = Vec::new();
        reference_preorder(root, &mut want);
        assert_eq!(got, want);
        assert_eq!(got.first(), Some(&root.id()));
    }

    #[test]
    // MU-8: end_line is the end row plus one when the end column is not zero.
    fn mu_8_end_line_uses_row_plus_one_when_column_nonzero() {
        let tree = parse("\n\nx = 1\n", "python").expect("python is known");
        let assignment = find_kind(tree.root_node(), "assignment").expect("assignment node");
        assert_eq!(start_line(assignment), 3);
        assert_eq!(end_line(assignment), 3);
    }

    #[test]
    // MU-8: a node ending at column zero ends at max(start line, end row).
    fn mu_8_end_line_col_zero_uses_max_of_start_and_row() {
        let tree = parse("def f():\n    return 1\n", "python").expect("python is known");
        let root = tree.root_node();
        assert_eq!(root.end_position().column, 0);
        assert_eq!(start_line(root), 1);
        assert_eq!(end_line(root), 2);
    }

    #[test]
    // MU-8: child_of_type matches anonymous tokens as well as named children.
    fn mu_8_child_of_type_finds_unnamed_child() {
        let tree = parse("a + b;\n", "javascript").expect("javascript is known");
        let binary = find_kind(tree.root_node(), "binary_expression").expect("binary node");
        assert_eq!(
            child_of_type(binary, &["+"]).map(|child| child.kind()),
            Some("+")
        );
        assert_eq!(
            child_of_type(binary, &["identifier"]).map(|c| c.kind()),
            Some("identifier")
        );
        assert!(child_of_type(binary, &["-"]).is_none());
    }

    #[test]
    // MU-8: node_text slices the source bytes under the node.
    fn mu_8_node_text_slices_source_bytes() {
        let source = "value = 1\n";
        let tree = parse(source, "python").expect("python is known");
        let identifier = find_kind(tree.root_node(), "identifier").expect("identifier node");
        assert_eq!(node_text(source.as_bytes(), identifier), "value");
    }

    #[test]
    // MU-8: an absolute path is returned unchanged.
    fn mu_8_absolute_path_keeps_absolute_input() {
        assert_eq!(
            absolute_path("/abs/src/a.py", Some("/root")),
            "/abs/src/a.py"
        );
    }

    #[test]
    // MU-8: a relative path is joined to the project root and resolved.
    fn mu_8_absolute_path_joins_relative_path_to_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path()).expect("canonical root");
        let root_text = root.display().to_string();
        let got = absolute_path("src/missing.py", Some(&root_text));
        assert_eq!(got, root.join("src/missing.py").display().to_string());
    }

    #[test]
    // MU-8: a relative path without a project root resolves against the working directory.
    fn mu_8_absolute_path_without_root_uses_working_directory() {
        let got = absolute_path("some/where.py", None);
        assert!(Path::new(&got).is_absolute());
        assert!(got.ends_with("some/where.py"));
    }
}
