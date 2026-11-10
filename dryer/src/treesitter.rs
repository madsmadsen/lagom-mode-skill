// Derived from dryer by Robert C. Martin (https://github.com/unclebob/dryer), BSD Zero Clause License.
//! Port of `dryer/treesitter.py`: parser setup and small node helpers.

use std::cell::RefCell;
use std::collections::HashMap;

use tree_sitter::{Language, Node, Parser, Tree};

fn language_for(grammar: &str) -> Option<Language> {
    match grammar {
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        _ => None,
    }
}

/// Parse `data` with one of the bundled grammars.
/// A grammar that fails to load is a build bug, so this panics.
pub fn parse(grammar: &str, data: &[u8]) -> Tree {
    let language = language_for(grammar).expect("bundled grammar");
    let mut parser = Parser::new();
    parser.set_language(&language).expect("bundled grammar");
    parser.parse(data, None).expect("bundled grammar")
}

pub fn node_text(data: &[u8], node: Node) -> String {
    String::from_utf8_lossy(&data[node.start_byte()..node.end_byte()]).into_owned()
}

pub fn start_line(node: Node) -> usize {
    node.start_position().row + 1
}

pub fn end_line(node: Node) -> usize {
    let end = node.end_position();
    if end.column == 0 {
        start_line(node).max(end.row)
    } else {
        end.row + 1
    }
}

/// Preorder traversal, root first, children in document order.
pub fn descendants(root: Node<'_>) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(current) = stack.pop() {
        out.push(current);
        let mut cursor = current.walk();
        let kids: Vec<Node<'_>> = current.children(&mut cursor).collect();
        stack.extend(kids.into_iter().rev());
    }
    out
}

/// First direct child (named or anonymous) whose kind is one of `types`.
pub fn child_of_type<'a>(node: Node<'a>, types: &[&str]) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .find(|child| types.contains(&child.kind()));
    found
}

thread_local! {
    static KINDS: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
}

/// `'static` copy of a node kind. Kinds are a finite grammar set, so the leak is bounded.
pub fn kind_static(node: Node) -> &'static str {
    let kind = node.kind();
    KINDS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(found) = cache.get(kind) {
            return *found;
        }
        let leaked: &'static str = Box::leak(kind.to_string().into_boxed_str());
        cache.insert(kind.to_string(), leaked);
        leaked
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // DR-8: the bundled grammars parse their own language without error nodes.
    #[test]
    fn dr8_parse_produces_a_root_for_each_grammar() {
        let samples = [
            ("java", "class A { int f() { return 1; } }"),
            ("go", "package demo\nfunc f() int { return 1 }\n"),
            ("python", "def f():\n    return 1\n"),
            ("rust", "fn f() -> i32 { 1 }\n"),
            ("typescript", "function f(): number { return 1; }\n"),
            ("tsx", "const f = () => <div />;\n"),
        ];
        for (grammar, source) in samples {
            let tree = parse(grammar, source.as_bytes());
            assert!(!tree.root_node().has_error(), "{grammar}");
        }
    }

    // DR-14: the javascript grammar parses JSX without error nodes.
    #[test]
    fn dr14_parse_reads_javascript_with_jsx() {
        let tree = parse(
            "javascript",
            b"const f = (xs) => <div>{xs.length}</div>;\nfunction g(a) { return a + 1; }\n",
        );
        assert!(!tree.root_node().has_error());
    }

    // DR-8: a Python function reports 1-based start and end lines.
    #[test]
    fn dr8_line_helpers_are_one_based_and_end_on_last_row() {
        let source = "def f():\n    return 1\n";
        let tree = parse("python", source.as_bytes());
        let function = descendants(tree.root_node())
            .into_iter()
            .find(|node| node.kind() == "function_definition")
            .expect("function");
        assert_eq!(start_line(function), 1);
        assert_eq!(end_line(function), 2);
    }

    // DR-9: node_text returns the exact byte slice of the node.
    #[test]
    fn dr9_node_text_is_the_node_bytes() {
        let source = "x = foo(1)\n";
        let tree = parse("python", source.as_bytes());
        let call = descendants(tree.root_node())
            .into_iter()
            .find(|node| node.kind() == "call")
            .expect("call");
        assert_eq!(node_text(source.as_bytes(), call), "foo(1)");
    }

    // DR-8: descendants visits the root first and then children in order.
    #[test]
    fn dr8_descendants_is_preorder_with_root_first() {
        let tree = parse("python", b"x = 1\n");
        let nodes = descendants(tree.root_node());
        assert_eq!(nodes[0].id(), tree.root_node().id());
        assert_eq!(nodes.len(), count_all(tree.root_node()));
    }

    fn count_all(node: Node<'_>) -> usize {
        let mut cursor = node.walk();
        1 + node.children(&mut cursor).map(count_all).sum::<usize>()
    }

    // DR-8: child_of_type finds a direct child, not a grandchild.
    #[test]
    fn dr8_child_of_type_checks_direct_children_only() {
        let tree = parse("python", b"def f(x):\n    return x\n");
        let function = descendants(tree.root_node())
            .into_iter()
            .find(|node| node.kind() == "function_definition")
            .expect("function");
        assert!(child_of_type(function, &["parameters"]).is_some());
        assert!(child_of_type(function, &["identifier"]).is_some());
        assert!(child_of_type(function, &["return_statement"]).is_none());
    }

    // DR-9: kind_static returns equal strs for two different nodes of the same kind.
    #[test]
    fn dr9_kind_static_interns_equal_kinds() {
        let tree = parse("python", b"x = 1\ny = 2\n");
        let assignments: Vec<Node<'_>> = descendants(tree.root_node())
            .into_iter()
            .filter(|node| node.kind() == "assignment")
            .collect();
        assert_eq!(assignments.len(), 2);
        assert_eq!(kind_static(assignments[0]), kind_static(assignments[1]));
        assert_eq!(kind_static(assignments[0]), "assignment");
    }
}
