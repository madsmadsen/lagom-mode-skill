// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/rust.py

//! Rust functions and cyclomatic complexity.
//!
//! Decision points are `if` / `if let`, `for`, `while` / `while let`, `loop`,
//! each `match` arm, `?`, and `&&` / `||`. Free functions are namespaced by the
//! module path (`crate::foo::bar`). Methods are namespaced by the self type
//! (`crate::foo::Widget`) so uml-viewer can join them to that type. Test code is not
//! scored (CR-25): a function in `mod tests` or in a `#[cfg(test)]` module, and a
//! function with `#[test]` or `#[cfg(test)]`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Node;

use crate::languages::ts::{
    absolute_path, ancestor_names, binary_logic, child_of_type, complexity, descendants, end_line,
    node_text, parse, resolve, start_line, Grammar,
};
use crate::model::Function;

const DECISIONS: &[&str] = &[
    "if_expression",
    "for_expression",
    "while_expression",
    "loop_expression",
    "match_arm",
    "try_expression",
];

static PACKAGE_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?ms)^\[package\].*?(?:^\[|\z)").expect("valid regex"));
static PACKAGE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^name\s*=\s*"([^"]+)""#).expect("valid regex"));

pub fn functions_in_source(source: &str, path: &str, project_root: &str) -> Vec<Function> {
    let (data, tree) = parse(source, Grammar::Rust);
    let root = (!project_root.is_empty()).then_some(project_root);
    let located = PathBuf::from(absolute_path(path, root));
    let (crate_name, crate_root) = crate_name(&located);
    let modules = file_modules(&located, &crate_name, crate_root.as_deref());
    descendants(tree.root_node())
        .into_iter()
        .filter_map(|node| record_function(&data, node, &modules, path))
        .collect()
}

fn is_decision(node: Node) -> bool {
    DECISIONS.contains(&node.kind()) || binary_logic(node)
}

fn no_skip(_node: Node) -> bool {
    false
}

fn has_block(node: Node) -> bool {
    child_of_type(node, &["block"]).is_some()
}

fn lossy(name: Option<&OsStr>) -> String {
    name.map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn first_type_name(data: &[u8], node: Node) -> Option<String> {
    descendants(node)
        .into_iter()
        .find(|item| item.kind() == "type_identifier")
        .map(|item| node_text(data, item).to_string())
}

fn impl_type(data: &[u8], imp: Node) -> Option<String> {
    let mut chosen = None;
    let mut cursor = imp.walk();
    for child in imp.children(&mut cursor) {
        match child.kind() {
            "for" => chosen = None,
            "type_identifier" | "generic_type" | "scoped_type_identifier" => {
                chosen = first_type_name(data, child);
            }
            _ => {}
        }
    }
    chosen
}

fn impl_owner(data: &[u8], node: Node) -> Option<String> {
    let parent = node.parent().filter(|p| p.kind() == "declaration_list")?;
    let owner = parent.parent().filter(|o| o.kind() == "impl_item")?;
    impl_type(data, owner)
}

fn package_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let block = PACKAGE_BLOCK.find(&text)?;
    let name = PACKAGE_NAME
        .captures(block.as_str())
        .map_or_else(|| lossy(dir.file_name()), |m| m[1].to_string());
    Some(name.replace('-', "_"))
}

fn crate_name(path: &Path) -> (String, Option<PathBuf>) {
    let mut current = resolve(path)
        .parent()
        .unwrap_or(Path::new("/"))
        .to_path_buf();
    loop {
        if let Some(name) = package_name(&current) {
            return (name, Some(current));
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return ("crate".to_string(), None),
        }
    }
}

fn rs_parts(relative: &str) -> Vec<String> {
    let mut parts: Vec<String> = relative.split('/').map(String::from).collect();
    if parts.last().is_some_and(|last| last.as_str() == "mod.rs") {
        parts.pop();
    } else if let Some(last) = parts.last_mut() {
        let stem = last.strip_suffix(".rs").map(str::to_string);
        if let Some(stem) = stem {
            *last = stem;
        }
    }
    parts
}

fn with_crate(crate_name: &str, mut parts: Vec<String>) -> Vec<String> {
    if parts.len() >= 2 && parts[0] == "bin" {
        parts.remove(0);
        return parts;
    }
    let mut modules = vec![crate_name.to_string()];
    modules.extend(parts);
    modules
}

fn file_modules(path: &Path, crate_name: &str, crate_root: Option<&Path>) -> Vec<String> {
    let file = resolve(path);
    let Some(root) = crate_root else {
        let stem = lossy(file.file_stem());
        return if stem == "lib" || stem == "main" {
            vec![crate_name.to_string()]
        } else {
            vec![crate_name.to_string(), stem]
        };
    };
    let Ok(relative) = file.strip_prefix(root) else {
        return vec![crate_name.to_string()];
    };
    let text = relative.to_string_lossy();
    let rel = text.strip_prefix("src/").unwrap_or(&text);
    if rel == "lib.rs" || rel == "main.rs" {
        return vec![crate_name.to_string()];
    }
    with_crate(crate_name, rs_parts(rel))
}

fn namespace(modules: &[String], mods: &[String], type_name: Option<&str>) -> String {
    modules
        .iter()
        .map(String::as_str)
        .chain(mods.iter().map(String::as_str))
        .chain(type_name)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("::")
}

/// True when the function is test code (CR-25): it sits in a module named `tests` or in a
/// `#[cfg(test)]` module, or it has the attribute `#[test]` or `#[cfg(test)]` itself.
fn is_test_code(data: &[u8], node: Node, mods: &[String]) -> bool {
    mods.iter().any(|name| name == "tests")
        || has_attribute(data, node, &["#[test]", "#[cfg(test)]"])
        || std::iter::successors(node.parent(), |n| n.parent())
            .filter(|n| n.kind() == "mod_item")
            .any(|module| has_attribute(data, module, &["#[cfg(test)]"]))
}

/// True for a line or block comment node, the two kinds that carry Rust comments.
fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

/// True when an attribute of `item` reads as one of `wanted`. The attributes of an item are
/// the `attribute_item` siblings right before it; comments between them do not count.
/// Spaces inside the attribute do not matter.
fn has_attribute(data: &[u8], item: Node, wanted: &[&str]) -> bool {
    std::iter::successors(item.prev_sibling(), |n| n.prev_sibling())
        .take_while(|n| n.kind() == "attribute_item" || is_comment(*n))
        .filter(|n| n.kind() == "attribute_item")
        .any(|attr| {
            wanted.iter().any(|text| {
                node_text(data, attr)
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .eq(text.chars())
            })
        })
}

fn record_function(data: &[u8], node: Node, modules: &[String], path: &str) -> Option<Function> {
    if node.kind() != "function_item" || !has_block(node) {
        return None;
    }
    if node.parent().is_some_and(|p| p.kind() == "block") {
        return None;
    }
    let mods = ancestor_names(data, node, "mod_item");
    if is_test_code(data, node, &mods) {
        return None;
    }
    let ident = child_of_type(node, &["identifier"])?;
    Some(Function {
        name: node_text(data, ident).to_string(),
        namespace: namespace(modules, &mods, impl_owner(data, node).as_deref()),
        complexity: complexity(node, is_decision, no_skip),
        start_line: start_line(node),
        end_line: end_line(node),
        path: path.to_string(),
        language: "rust".to_string(),
        jacoco_class: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{tempdir, TempDir};

    const SOURCE: &str = r#"
pub fn choose(x: i32) -> i32 {
    if x > 0 && x < 10 {
        return 1;
    }
    match x {
        1 => 1,
        2 => 2,
        _ => 0,
    }
}

impl Sample {
    pub fn run(&self) -> i32 {
        if self.ok { 1 } else { 0 }
    }
}

impl Trait for Sample {
    fn draw(&mut self) {}
}

mod tests {
    fn hidden() {
        if true {}
    }
}

fn outer() {
    fn inner() {
        if true {}
    }
}
"#;

    fn crate_named(name: &str) -> TempDir {
        let dir = tempdir().expect("tempdir");
        let manifest = format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
        fs::write(dir.path().join("Cargo.toml"), manifest).expect("write Cargo.toml");
        dir
    }

    fn functions_in(dir: &TempDir, rel: &str, source: &str) -> Vec<Function> {
        functions_in_source(source, rel, dir.path().to_str().expect("utf-8 temp path"))
    }

    fn summary(functions: &[Function]) -> Vec<(&str, &str, u32)> {
        functions
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str(), f.complexity))
            .collect()
    }

    // CR-8 CR-11: free functions and methods are namespaced; mod tests is skipped; nested fns count in the outer fn.
    #[test]
    fn cr_8_cr_11_functions_methods_and_skipped_tests() {
        let dir = crate_named("demo-game");
        let functions = functions_in(&dir, "src/lib.rs", SOURCE);
        assert_eq!(
            summary(&functions),
            [
                ("demo_game", "choose", 6),
                ("demo_game::Sample", "run", 2),
                ("demo_game::Sample", "draw", 1),
                ("demo_game", "outer", 2),
            ]
        );
    }

    fn names_in(source: &str) -> Vec<String> {
        let dir = crate_named("demo");
        functions_in(&dir, "src/lib.rs", source)
            .into_iter()
            .map(|f| f.name)
            .collect()
    }

    // CR-25 unit a: functions in a `#[cfg(test)]` module are test code, whatever the module is named.
    #[test]
    fn cr_25_functions_in_a_cfg_test_module_are_skipped() {
        let source = "fn keep() {}
#[cfg(test)]
mod unit_tests {
    #[test]
    fn b() { if true {} }
    fn helper() {}
    mod inner { fn deep() {} }
}
";
        assert_eq!(names_in(source), ["keep"]);
    }

    // CR-25 unit b: a function with `#[test]` is test code, even in a plain module.
    #[test]
    fn cr_25_a_test_function_in_a_plain_module_is_skipped() {
        let source = "mod plain {\n    fn keep() {}\n    #[test]\n    fn check() { if true {} }\n    #[test]\n    #[ignore]\n    fn slow() {}\n}\n";
        assert_eq!(names_in(source), ["keep"]);
    }

    // CR-25 unit c: a function with `#[cfg(test)]` is test code, spaces in the attribute do not matter.
    #[test]
    fn cr_25_a_cfg_test_helper_function_is_skipped() {
        let source = "fn keep() {}\n#[cfg(test)]\nfn helper() {}\n#[cfg( test )]\nfn spaced() {}\n#[allow(dead_code)]\n#[cfg(test)]\nfn second() {}\n";
        assert_eq!(names_in(source), ["keep"]);
    }

    // CR-25 unit d: `#[cfg(not(test))]`, `#[cfg(all(test, unix))]` and `#[tokio::test]` are not `#[cfg(test)]` or `#[test]`.
    #[test]
    fn cr_25_other_test_looking_attributes_stay_production_code() {
        let source = "#[cfg(not(test))]\nmod prod {\n    fn a() {}\n}\n#[cfg(all(test, unix))]\nmod both {\n    fn b() {}\n}\n#[tokio::test]\nfn c() {}\n#[cfg(not(test))]\nfn d() {}\n#[cfg(test)]\nstruct Marker;\nfn e() {}\n";
        assert_eq!(names_in(source), ["a", "b", "c", "d", "e"]);
    }

    // CR-25 unit e: `mod tests` with no attribute is still skipped, and a function next to it is still reported.
    #[test]
    fn cr_25_a_module_named_tests_is_skipped_and_its_neighbor_is_reported() {
        let source = "fn keep() {}\nmod tests {\n    fn hidden() {}\n}\nfn neighbor() {}\n";
        assert_eq!(names_in(source), ["keep", "neighbor"]);
    }

    // CR-25 unit f: a comment between an attribute and the function does not hide the attribute.
    #[test]
    fn cr_25_a_comment_between_the_attribute_and_the_function_is_skipped() {
        let source = "fn keep() {}\n#[test]\n// a note\nfn check() { if true {} }\n#[cfg(test)]\n/* a block note */\nfn helper() {}\n";
        assert_eq!(names_in(source), ["keep"]);
    }

    // CR-8: impl for a generic self type takes the first type name after `for`.
    #[test]
    fn cr_8_impl_for_a_generic_type_uses_its_base_name() {
        let dir = crate_named("demo");
        let source = "impl Trait for Vec<crate::Sample> {\n    fn draw(&self) {}\n}\n";
        let functions = functions_in(&dir, "src/lib.rs", source);
        assert_eq!(summary(&functions), [("demo::Vec", "draw", 1)]);
    }

    // CR-8: mod.rs maps to its directory, src/bin/<name>.rs maps to <name> alone.
    #[test]
    fn cr_8_module_paths_outside_the_usual_layout() {
        let dir = crate_named("demo");
        let open = functions_in(&dir, "src/game/mod.rs", "pub fn open() {}\n");
        assert_eq!(summary(&open), [("demo::game", "open", 1)]);
        let tool = functions_in(&dir, "src/bin/tool.rs", "pub fn main_ok() {}\n");
        assert_eq!(summary(&tool), [("tool", "main_ok", 1)]);
        let bare = functions_in_source("pub fn bare() {}\n", "notes.rs", "");
        assert_eq!(bare[0].name, "bare");
    }

    // CR-8: a file outside the crate root is namespaced by the crate name only.
    #[test]
    fn cr_8_a_file_outside_the_crate_root_uses_the_crate_name() {
        let modules = file_modules(
            Path::new("/tmp/loose.rs"),
            "demo",
            Some(Path::new("/projects/demo")),
        );
        assert_eq!(modules, vec!["demo".to_string()]);
    }

    // CR-8 CR-11: the module path comes from the file location; complexity counts the `if`.
    #[test]
    fn cr_8_cr_11_nested_module_path_from_file_layout() {
        let dir = crate_named("demo");
        let source = "pub fn place() -> i32 { if ready { 1 } else { 0 } }\n";
        let functions = functions_in(&dir, "src/game/board.rs", source);
        assert_eq!(summary(&functions), [("demo::game::board", "place", 2)]);
    }

    // CR-11: for, while, loop, `?`, and `||` each add one decision point.
    #[test]
    fn cr_11_loops_try_and_logic_operators_each_count() {
        let dir = crate_named("demo");
        let source = "pub fn spin(v: &[i32]) -> Result<i32, String> {\n    for _ in v {}\n    while v.is_empty() {}\n    loop { break; }\n    let n = v.first().ok_or(String::new())?;\n    if *n > 0 || *n < -5 { return Ok(1); }\n    Ok(0)\n}\n";
        let functions = functions_in(&dir, "src/lib.rs", source);
        assert_eq!(summary(&functions), [("demo", "spin", 7)]);
    }

    // CR-9: tree-sitter positions are 1-based and cover the whole item; language and path are recorded.
    #[test]
    fn cr_9_items_carry_tree_sitter_line_positions() {
        let dir = crate_named("demo");
        let functions = functions_in(&dir, "src/lib.rs", "pub fn a() {\n}\n\nfn b() {}\n");
        let lines: Vec<_> = functions
            .iter()
            .map(|f| (f.name.as_str(), f.start_line, f.end_line))
            .collect();
        assert_eq!(lines, [("a", 1, 2), ("b", 4, 4)]);
        assert_eq!(functions[0].language, "rust");
        assert_eq!(functions[0].path, "src/lib.rs");
    }

    // CR-8: without a crate root, lib and main name the crate alone; any other stem adds itself.
    #[test]
    fn cr_8_stem_without_crate_root_names_lib_and_main_as_the_crate() {
        let path = |name: &str| Path::new("/nowhere").join(name);
        assert_eq!(
            file_modules(&path("lib.rs"), "demo", None),
            vec!["demo".to_string()]
        );
        assert_eq!(
            file_modules(&path("main.rs"), "demo", None),
            vec!["demo".to_string()]
        );
        assert_eq!(
            file_modules(&path("tool.rs"), "demo", None),
            vec!["demo".to_string(), "tool".to_string()]
        );
    }

    // CR-8: under a crate root, src/lib.rs and src/main.rs name the crate, src/a.rs and src/a/mod.rs name a, src/bin/main.rs names main.
    #[test]
    fn cr_8_crate_root_files_map_to_their_module_paths() {
        let dir = crate_named("demo");
        let namespace = |rel: &str| {
            functions_in(&dir, rel, "pub fn f() {}\n")[0]
                .namespace
                .clone()
        };
        assert_eq!(namespace("src/lib.rs"), "demo");
        assert_eq!(namespace("src/main.rs"), "demo");
        assert_eq!(namespace("src/a.rs"), "demo::a");
        assert_eq!(namespace("src/a/mod.rs"), "demo::a");
        assert_eq!(namespace("src/bin/main.rs"), "main");
    }
}
