//! Port of crapper languages/java.py (BSD Zero Clause License, Robert C. Martin).
//!
//! Constructors, abstract methods, and methods inside anonymous or local classes are ignored.
//! Nested member types become their own namespace (`pkg.Outer.Inner`).

use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Node;

use crate::model::Function;
use crate::ts::{self, child_of_type, end_line, node_text, start_line};

static PACKAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*package\s+([a-zA-Z_][\w.]*)\s*;").expect("valid package pattern")
});

const NESTED_TYPES: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];

const EXECUTABLE: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "lambda_expression",
];

/// Java methods with a body, outside any method, constructor, or lambda.
pub fn functions_in_source(
    source: &str,
    _path: &str,
    _project_root: Option<&str>,
) -> Vec<Function> {
    let Some(tree) = ts::parse(source, "java") else {
        return Vec::new();
    };
    let data = source.as_bytes();
    let package = package_name(source);
    method_nodes(tree.root_node())
        .into_iter()
        .filter_map(|method| method_function(data, method, package.as_deref()))
        .collect()
}

fn package_name(source: &str) -> Option<String> {
    PACKAGE
        .captures(source)
        .and_then(|found| found.get(1))
        .map(|name| name.as_str().to_string())
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Pre-order method declarations, not descending into the methods themselves.
fn method_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "method_declaration" && !inside_executable(node) {
            found.push(node);
        } else {
            stack.extend(children(node).into_iter().rev());
        }
    }
    found
}

fn inside_executable(node: Node<'_>) -> bool {
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .any(|ancestor| EXECUTABLE.contains(&ancestor.kind()))
}

fn method_function(data: &[u8], node: Node<'_>, package: Option<&str>) -> Option<Function> {
    child_of_type(node, &["block"])?;
    let ident = child_of_type(node, &["identifier"])?;
    let names = type_chain(data, node);
    if names.is_empty() {
        return None;
    }
    Some(Function {
        name: node_text(data, ident).to_string(),
        namespace: qualify(package, &names),
        start_line: start_line(node),
        end_line: end_line(node),
        start_byte: None,
        end_byte: None,
    })
}

/// Names of the enclosing member types, outermost first.
fn type_chain(data: &[u8], node: Node<'_>) -> Vec<String> {
    let mut names: Vec<String> = std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .filter(|ancestor| NESTED_TYPES.contains(&ancestor.kind()))
        .filter_map(|ancestor| child_of_type(ancestor, &["identifier"]))
        .map(|ident| node_text(data, ident).to_string())
        .collect();
    names.reverse();
    names
}

fn qualify(package: Option<&str>, names: &[String]) -> String {
    let body = names.join(".");
    match package {
        Some(package) if body.is_empty() => package.to_string(),
        Some(package) => format!("{package}.{body}"),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(functions: &[Function]) -> Vec<(&str, &str)> {
        functions
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str()))
            .collect()
    }

    /// MU-9: a single method is named and namespaced by its class.
    #[test]
    fn mu_9_java_names_method_by_class() {
        let source = r#"
class Sample {
    int score(boolean a, boolean b, int[] values) {
        for (int i = 0; i < values.length; i++) {
        }
        return 1;
    }
}
"#;
        let found = functions_in_source(source, "Sample.java", None);
        assert_eq!(pairs(&found), vec![("Sample", "score")]);
        assert_eq!((found[0].start_line, found[0].end_line), (3, 7));
        assert_eq!((found[0].start_byte, found[0].end_byte), (None, None));
    }

    /// MU-9: methods in nested classes are listed in document order.
    #[test]
    fn mu_9_java_lists_nested_class_methods_in_order() {
        let source = r#"
class Sample {
    int nested(boolean a) {
        if (a) {
        }
        return 3;
    }

    int switched(int value) {
        switch (value) {
            case 1:
                return 0;
            default:
                return 2;
        }
    }
}
"#;
        let found = functions_in_source(source, "Sample.java", None);
        assert_eq!(
            pairs(&found),
            vec![("Sample", "nested"), ("Sample", "switched")]
        );
    }

    /// MU-9: methods inside anonymous classes are not entries.
    #[test]
    fn mu_9_java_ignores_anonymous_class_methods() {
        let source = r#"
class Sample {
    int outer() {
        Runnable runnable = new Runnable() {
            @Override
            public void run() {
                if (true) {
                }
            }
        };
        return 1;
    }
}
"#;
        let found = functions_in_source(source, "Sample.java", None);
        assert_eq!(pairs(&found), vec![("Sample", "outer")]);
        assert_eq!((found[0].start_line, found[0].end_line), (3, 12));
    }

    /// MU-9: constructors are ignored and nested classes get `pkg.Outer.Inner` namespaces.
    #[test]
    fn mu_9_java_ignores_constructors_and_names_nested_class() {
        let source = r#"
    package demo.pkg;
    public class Board {
        Board() { if (true) {} }
        public void place(int x) { if (x > 0 && ready) return; }
        class Inner { void tick() { return; } }
    }
    "#;
        let found = functions_in_source(source, "src/demo/pkg/Board.java", None);
        assert_eq!(
            pairs(&found),
            vec![
                ("demo.pkg.Board", "place"),
                ("demo.pkg.Board.Inner", "tick"),
            ]
        );
        assert_eq!((found[0].start_line, found[0].end_line), (5, 5));
        assert_eq!((found[1].start_line, found[1].end_line), (6, 6));
    }

    /// MU-9: abstract methods have no body and are ignored; interface defaults are kept.
    #[test]
    fn mu_9_java_ignores_abstract_methods_keeps_interface_defaults() {
        let source = r#"
abstract class Base { abstract void run(); }
interface Shape { double area(); default String label() { return "x"; } }
"#;
        let found = functions_in_source(source, "Shape.java", None);
        assert_eq!(pairs(&found), vec![("Shape", "label")]);
    }

    /// MU-9: keywords inside strings and comments are not counted as entries or names.
    #[test]
    fn mu_9_java_keywords_in_comments_and_strings_are_not_entries() {
        let source = r#"
class Sample {
    int stable() {
        String text = "if && || ? case default catch";
        // if && || ? case default catch
        /* if && || ? case default catch */
        return 1;
    }
}
"#;
        let found = functions_in_source(source, "Sample.java", None);
        assert_eq!(pairs(&found), vec![("Sample", "stable")]);
    }

    /// MU-9: a package declaration prefixes the class namespace.
    #[test]
    fn mu_9_java_uses_package_prefix() {
        let source = "package a.b;\nclass C { void m() {} }\n";
        let found = functions_in_source(source, "C.java", None);
        assert_eq!(pairs(&found), vec![("a.b.C", "m")]);
    }
}
