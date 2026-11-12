// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/java.py

//! Java methods and cyclomatic complexity, following crap4java.
//!
//! Constructors, abstract methods, and methods inside anonymous or local classes
//! are ignored. Nested member types become their own namespace (`pkg.Outer.Inner`)
//! so uml-viewer can attach scores to that class. JaCoCo's binary name
//! (`pkg.Outer$Inner`) is kept for coverage lookup.

use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Node;

use crate::languages::ts::{
    binary_logic, child_of_type, complexity, descendants, end_line, node_text, parse, start_line,
    Grammar,
};
use crate::model::Function;

const NESTED_TYPES: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];
const DECISIONS: &[&str] = &[
    "if_statement",
    "for_statement",
    "enhanced_for_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "ternary_expression",
    "switch_label",
];
const EXECUTABLE: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "lambda_expression",
];

fn is_decision(node: Node) -> bool {
    DECISIONS.contains(&node.kind()) || binary_logic(node)
}

fn skipped(node: Node) -> bool {
    NESTED_TYPES.contains(&node.kind()) || node.kind() == "class_body"
}

fn inside_executable(node: Node) -> bool {
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .any(|ancestor| EXECUTABLE.contains(&ancestor.kind()))
}

fn package_name(source: &str) -> Option<&str> {
    static PACKAGE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^\s*package\s+([a-zA-Z_][\w.]*)\s*;").expect("valid package pattern")
    });
    PACKAGE
        .captures(source)
        .and_then(|caps| caps.get(1))
        .map(|name| name.as_str())
}

/// Names of the enclosing member types, outermost first.
fn type_chain<'d>(data: &'d [u8], method: Node) -> Vec<&'d str> {
    let mut names: Vec<&str> = std::iter::successors(method.parent(), |ancestor| ancestor.parent())
        .filter(|ancestor| NESTED_TYPES.contains(&ancestor.kind()))
        .filter_map(|ancestor| child_of_type(ancestor, &["identifier"]))
        .map(|ident| node_text(data, ident))
        .collect();
    names.reverse();
    names
}

/// `package.Outer<nested>Inner`. The package is optional.
fn qualify(package: Option<&str>, names: &[&str], nested: &str) -> String {
    let body = names.join(nested);
    match package {
        Some(pkg) => format!("{pkg}.{body}"),
        None => body,
    }
}

fn method_function(
    data: &[u8],
    method: Node,
    package: Option<&str>,
    path: &str,
) -> Option<Function> {
    child_of_type(method, &["block"])?;
    let ident = child_of_type(method, &["identifier"])?;
    let names = type_chain(data, method);
    if names.is_empty() {
        return None;
    }
    Some(Function {
        name: node_text(data, ident).to_string(),
        namespace: qualify(package, &names, "."),
        complexity: complexity(method, is_decision, skipped),
        start_line: start_line(method),
        end_line: end_line(method),
        path: path.to_string(),
        language: "java".to_string(),
        jacoco_class: Some(qualify(package, &names, "$")),
    })
}

pub fn functions_in_source(source: &str, path: &str, _project_root: &str) -> Vec<Function> {
    let (data, tree) = parse(source, Grammar::Java);
    let package = package_name(source);
    descendants(tree.root_node())
        .into_iter()
        .filter(|node| node.kind() == "method_declaration" && !inside_executable(*node))
        .filter_map(|method| method_function(&data, method, package, path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::functions_in_source;

    const SCORE: &str = r#"
class Sample {
    int score(boolean a, boolean b, int[] values) {
        for (int i = 0; i < values.length; i++) {
        }
        for (int value : values) {
        }
        while (a) {
            a = false;
        }
        do {
            b = false;
        } while (b);
        if (a && b || values.length > 0) {
        }
        try {
            return a ? 1 : 0;
        } catch (RuntimeException ex) {
            return 2;
        }
    }
}
"#;

    const NESTED: &str = r#"
class Sample {
    int nested(boolean a, boolean b, int[] values) {
        for (int i = 0; i < values.length; i++) {
            if (a) {
            }
        }
        for (int value : values) {
            if (b) {
            }
        }
        while (a) {
            if (b) {
            }
            a = false;
        }
        do {
            if (a) {
            }
            b = false;
        } while (b);
        try {
            return a ? (b ? 1 : 0) : 2;
        } catch (RuntimeException ex) {
            if (values.length > 0) {
                return values[0];
            }
            return 3;
        }
    }

    int switched(int value) {
        switch (value) {
            case 1:
                if (value > 0) {
                    return 1;
                }
                return 0;
            default:
                return 2;
        }
    }
}
"#;

    const ANONYMOUS: &str = r#"
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

    fn name_and_complexity(source: &str) -> Vec<(String, u32)> {
        functions_in_source(source, "Sample.java", "")
            .into_iter()
            .map(|method| (method.name, method.complexity))
            .collect()
    }

    // CR-11
    #[test]
    fn cr_11_counts_the_same_decisions_as_crap4java() {
        assert_eq!(name_and_complexity(SCORE), vec![("score".to_string(), 10)]);
        assert_eq!(
            name_and_complexity(NESTED),
            vec![("nested".to_string(), 13), ("switched".to_string(), 4)]
        );
    }

    // CR-11
    #[test]
    fn cr_11_anonymous_class_methods_are_not_entries_and_do_not_raise_complexity() {
        assert_eq!(
            name_and_complexity(ANONYMOUS),
            vec![("outer".to_string(), 1)]
        );
    }

    // CR-8
    #[test]
    fn cr_8_ignores_constructors_and_names_the_class_for_uml_viewer() {
        let source = r#"
    package demo.pkg;
    public class Board {
        Board() { if (true) {} }
        public void place(int x) { if (x > 0 && ready) return; }
        class Inner { void tick() { return; } }
    }
    "#;
        let methods = functions_in_source(source, "src/demo/pkg/Board.java", "");
        let summary: Vec<(&str, &str, u32)> = methods
            .iter()
            .map(|method| {
                (
                    method.namespace.as_str(),
                    method.name.as_str(),
                    method.complexity,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("demo.pkg.Board", "place", 3),
                ("demo.pkg.Board.Inner", "tick", 1),
            ]
        );
        assert_eq!(methods[0].jacoco_class.as_deref(), Some("demo.pkg.Board"));
        assert_eq!(
            methods[1].jacoco_class.as_deref(),
            Some("demo.pkg.Board$Inner")
        );
    }

    // CR-11
    #[test]
    fn cr_11_keywords_in_comments_and_strings_are_not_decisions() {
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
        assert_eq!(name_and_complexity(source), vec![("stable".to_string(), 1)]);
    }
}
