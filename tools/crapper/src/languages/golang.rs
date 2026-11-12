// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/golang.py

//! Go functions and cyclomatic complexity, following crap4go.
//!
//! Decision points are `if`, `for`, `range`, each `switch` / type-switch case,
//! each `select` clause, and `&&` / `||`. Coverage later uses Go's statement
//! profile. The namespace is the package import path; methods are namespaced by
//! their receiver type (`import/path.Widget`) and named `Run`, which is the join
//! uml-viewer makes between a class and its operations.

use std::fs;
use std::path::Path;

use tree_sitter::Node;

use crate::languages::ts::{
    absolute_path, binary_logic, child_of_type, complexity, descendants, end_line, node_text,
    parse, start_line, Grammar,
};
use crate::model::Function;

const DECISIONS: &[&str] = &[
    "if_statement",
    "for_statement",
    "expression_case",
    "type_case",
    "communication_case",
    "default_case",
];

fn is_decision(node: Node) -> bool {
    DECISIONS.contains(&node.kind()) || binary_logic(node)
}

fn package_clause<'d>(data: &'d [u8], root: Node) -> &'d str {
    descendants(root)
        .into_iter()
        .find(|node| node.kind() == "package_identifier")
        .map_or("", |node| node_text(data, node))
}

fn named_child_text<'d>(data: &'d [u8], node: Node, kind: &str) -> Option<&'d str> {
    child_of_type(node, &[kind]).map(|ident| node_text(data, ident))
}

/// Receiver type: the first type identifier in the method's first parameter list.
fn receiver_type<'d>(data: &'d [u8], method: Node) -> Option<&'d str> {
    let params = child_of_type(method, &["parameter_list"])?;
    descendants(params)
        .into_iter()
        .find(|node| node.kind() == "type_identifier")
        .map(|node| node_text(data, node))
}

fn function_entry(data: &[u8], node: Node, package_ns: &str, path: &str) -> Option<Function> {
    let (name, namespace) = match node.kind() {
        "function_declaration" => (
            named_child_text(data, node, "identifier")?,
            package_ns.to_string(),
        ),
        "method_declaration" => {
            let receiver = receiver_type(data, node)?;
            (
                named_child_text(data, node, "field_identifier")?,
                format!("{package_ns}.{receiver}"),
            )
        }
        _ => return None,
    };
    child_of_type(node, &["block"])?;
    Some(Function {
        name: name.to_string(),
        namespace,
        complexity: complexity(node, is_decision, |_| false),
        start_line: start_line(node),
        end_line: end_line(node),
        path: path.to_string(),
        language: "go".to_string(),
        jacoco_class: None,
    })
}

/// The `module` path of the first go.mod at or above `dir`, with that go.mod's directory.
fn go_module(start: &Path) -> Option<(&Path, String)> {
    let dir = start.ancestors().find(|dir| dir.join("go.mod").is_file())?;
    let module = module_path(&dir.join("go.mod"))?;
    Some((dir, module))
}

fn module_path(gomod: &Path) -> Option<String> {
    fs::read_to_string(gomod)
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("module ")
                .map(|rest| rest.trim().to_string())
        })
}

/// Import path of the package, or the package clause when no go.mod exists.
pub fn package_namespace(path: &str, package_name: &str) -> String {
    let dir = Path::new(path)
        .parent()
        .expect("an absolute source path has a parent directory");
    match go_module(dir) {
        Some((module_dir, module)) => {
            let relative = dir
                .strip_prefix(module_dir)
                .expect("the go.mod directory is an ancestor of the file");
            if relative.as_os_str().is_empty() {
                module
            } else {
                format!("{module}/{}", relative.display())
            }
        }
        None => package_name.to_string(),
    }
}

pub fn functions_in_source(source: &str, path: &str, project_root: &str) -> Vec<Function> {
    let (data, tree) = parse(source, Grammar::Go);
    let package = package_clause(&data, tree.root_node());
    let located = absolute_path(path, Some(project_root));
    let package_ns = package_namespace(&located, package);
    descendants(tree.root_node())
        .into_iter()
        .filter_map(|node| function_entry(&data, node, &package_ns, path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::functions_in_source;

    const BRANCHY: &str = "package sample\n\nfunc Branchy(x int, ch chan int) int {\n\tif x > 0 && x < 10 {\n\t\treturn 1\n\t}\n\tswitch x {\n\tcase 1, 2:\n\t\treturn 2\n\tdefault:\n\t\treturn 3\n\t}\n}\n";

    // CR-11
    #[test]
    fn cr_11_counts_the_same_decisions_as_crap4go() {
        let functions = functions_in_source(BRANCHY, "sample.go", "");
        let summary: Vec<(&str, &str, u32)> = functions
            .iter()
            .map(|fn_| (fn_.name.as_str(), fn_.namespace.as_str(), fn_.complexity))
            .collect();
        assert_eq!(summary, vec![("Branchy", "sample", 5)]);
    }

    // CR-8
    #[test]
    fn cr_8_methods_are_namespaced_by_receiver_type() {
        let source = "package sample\n\ntype Widget struct{}\n\nfunc (w *Widget) Run() {\n\tfor i := 0; i < 10; i++ {\n\t}\n\tfor _, item := range items {\n\t}\n}\n\nfunc Simple() int { return 1 }\n";
        let functions = functions_in_source(source, "widget.go", "");
        let summary: Vec<(&str, &str, u32)> = functions
            .iter()
            .map(|fn_| (fn_.namespace.as_str(), fn_.name.as_str(), fn_.complexity))
            .collect();
        assert_eq!(
            summary,
            vec![("sample.Widget", "Run", 3), ("sample", "Simple", 1)]
        );
    }

    // CR-8
    #[test]
    fn cr_8_import_path_comes_from_go_mod() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("go.mod"), "module github.com/acme/demo\n").unwrap();
        let dir = root.path().join("internal").join("game");
        std::fs::create_dir_all(&dir).unwrap();
        let source = "package game\n\ntype Board struct{}\n\nfunc (b *Board) Place() {\n\tif ready && ok {\n\t\treturn\n\t}\n}\n";
        std::fs::write(dir.join("board.go"), source).unwrap();

        let functions = functions_in_source(
            source,
            "internal/game/board.go",
            root.path().to_str().unwrap(),
        );
        let summary: Vec<(&str, &str, u32)> = functions
            .iter()
            .map(|fn_| (fn_.namespace.as_str(), fn_.name.as_str(), fn_.complexity))
            .collect();
        assert_eq!(
            summary,
            vec![("github.com/acme/demo/internal/game.Board", "Place", 3)]
        );
    }
}
