//! Port of crapper languages/rust.py without complexity (BSD Zero Clause License, Robert C. Martin).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Node;

use crate::model::Function;
use crate::ts::{
    absolute_path, child_of_type, descendants, end_line, node_text, parse, start_line,
};

const TYPE_KINDS: &[&str] = &["type_identifier", "generic_type", "scoped_type_identifier"];
static PACKAGE_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^\[package\](.*?)(?:^\[|\z)").expect("package block pattern is valid")
});
static PACKAGE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^name\s*=\s*"([^"]+)""#).expect("name pattern is valid"));

/// Functions and methods of a Rust source file, in source order (MU-9).
pub fn functions_in_source(source: &str, path: &str, project_root: Option<&str>) -> Vec<Function> {
    let Some(tree) = parse(source, "rust") else {
        return Vec::new();
    };
    let data = source.as_bytes();
    let located = absolute_path(path, project_root);
    let (crate_name, crate_root) = package_root(Path::new(&located));
    let modules = file_modules(Path::new(&located), &crate_name, crate_root.as_deref());
    descendants(tree.root_node())
        .into_iter()
        .filter_map(|node| record_function(data, node, &modules))
        .collect()
}

fn ancestors<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    std::iter::successors(node.parent(), |current| current.parent())
}

/// A function item with a body that is not nested directly in a block.
fn is_recordable(node: Node) -> bool {
    node.kind() == "function_item"
        && child_of_type(node, &["block"]).is_some()
        && !node.parent().is_some_and(|parent| parent.kind() == "block")
}

fn record_function(data: &[u8], node: Node, modules: &[String]) -> Option<Function> {
    if !is_recordable(node) {
        return None;
    }
    let mods = ancestor_mods(data, node);
    if mods.iter().any(|name| name.as_str() == "tests") {
        return None;
    }
    let ident = child_of_type(node, &["identifier"])?;
    let owner = impl_owner(data, node);
    Some(Function {
        name: node_text(data, ident).to_string(),
        namespace: namespace(modules, &mods, owner.as_deref()),
        start_line: start_line(node),
        end_line: end_line(node),
        start_byte: None,
        end_byte: None,
    })
}

/// Names of the enclosing `mod` items, outermost first.
fn ancestor_mods(data: &[u8], node: Node) -> Vec<String> {
    let mut names: Vec<String> = ancestors(node)
        .filter(|ancestor| ancestor.kind() == "mod_item")
        .filter_map(|module| child_of_type(module, &["identifier"]))
        .map(|ident| node_text(data, ident).to_string())
        .collect();
    names.reverse();
    names
}

fn impl_owner(data: &[u8], node: Node) -> Option<String> {
    let block = node
        .parent()
        .filter(|parent| parent.kind() == "declaration_list")?;
    let imp = block.parent().filter(|owner| owner.kind() == "impl_item")?;
    impl_type(data, imp)
}

/// The self type of an impl: the type after `for` in a trait impl, else the only type.
fn impl_type(data: &[u8], imp: Node) -> Option<String> {
    let mut cursor = imp.walk();
    let children: Vec<Node> = imp.children(&mut cursor).collect();
    let for_at = children.iter().position(|child| child.kind() == "for");
    let scope = match for_at {
        Some(index) => &children[index + 1..],
        None => &children[..],
    };
    let mut types = scope
        .iter()
        .filter(|child| TYPE_KINDS.contains(&child.kind()));
    let chosen = if for_at.is_some() {
        types.next()
    } else {
        types.next_back()
    };
    chosen.and_then(|child| first_type_name(data, *child))
}

fn first_type_name(data: &[u8], node: Node) -> Option<String> {
    descendants(node)
        .into_iter()
        .find(|item| item.kind() == "type_identifier")
        .map(|item| node_text(data, item).to_string())
}

fn namespace(modules: &[String], extra: &[String], type_name: Option<&str>) -> String {
    modules
        .iter()
        .chain(extra)
        .map(String::as_str)
        .chain(type_name)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("::")
}

/// The crate name and crate root (directory holding the `[package]` manifest) of a file.
fn package_root(file: &Path) -> (String, Option<PathBuf>) {
    resolve(file)
        .ancestors()
        .skip(1)
        .find_map(|dir| package_name(dir).map(|name| (name, dir.to_path_buf())))
        .map_or_else(
            || (String::from("crate"), None),
            |(name, dir)| (name, Some(dir)),
        )
}

fn package_name(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let block = PACKAGE_BLOCK.captures(&text)?;
    let body = block.get(1)?.as_str();
    let name = PACKAGE_NAME
        .captures(body)
        .and_then(|found| found.get(1))
        .map(|name| name.as_str().to_string())
        .or_else(|| {
            dir.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    Some(name.replace('-', "_"))
}

/// Canonical directory of the file joined with its file name; the path as given otherwise.
fn resolve(path: &Path) -> PathBuf {
    match (path.parent().map(fs::canonicalize), path.file_name()) {
        (Some(Ok(dir)), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

fn file_modules(path: &Path, crate_name: &str, crate_root: Option<&Path>) -> Vec<String> {
    let file = resolve(path);
    let Some(root) = crate_root else {
        return loose_modules(crate_name, &file);
    };
    let Ok(relative) = file.strip_prefix(root) else {
        return vec![crate_name.to_string()];
    };
    let relative = posix(relative);
    let relative = relative.strip_prefix("src/").unwrap_or(relative.as_str());
    match relative {
        "lib.rs" | "main.rs" => vec![crate_name.to_string()],
        _ => with_crate(crate_name, rs_parts(relative)),
    }
}

/// Module path for a file outside any crate root: the crate name, plus the file stem.
fn loose_modules(crate_name: &str, file: &Path) -> Vec<String> {
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if matches!(stem.as_str(), "lib" | "main") {
        vec![crate_name.to_string()]
    } else {
        vec![crate_name.to_string(), stem]
    }
}

fn posix(path: &Path) -> String {
    let parts: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    parts.join("/")
}

/// Path segments of a file under `src/`: `mod.rs` names its directory, `x.rs` names `x`.
fn rs_parts(relative: &str) -> Vec<&str> {
    let parts: Vec<&str> = relative.split('/').collect();
    let Some((last, init)) = parts.split_last() else {
        return Vec::new();
    };
    match *last {
        "mod.rs" => init.to_vec(),
        other => init
            .iter()
            .copied()
            .chain([other.strip_suffix(".rs").unwrap_or(other)])
            .collect(),
    }
}

fn with_crate(crate_name: &str, parts: Vec<&str>) -> Vec<String> {
    if parts.first() == Some(&"bin") && parts.len() >= 2 {
        return parts[1..].iter().map(|part| part.to_string()).collect();
    }
    std::iter::once(crate_name)
        .chain(parts)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{file_modules, functions_in_source};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

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

    /// A temporary crate whose `Cargo.toml` names the package `name`.
    fn crate_at(name: &str) -> TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let manifest = format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
        fs::write(dir.path().join("Cargo.toml"), manifest).expect("write manifest");
        dir
    }

    /// Absolute path of `relative` inside `dir`, with its parent directories created.
    fn make_parents(dir: &TempDir, relative: &str) -> String {
        let path = dir.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create dirs");
        }
        path.to_string_lossy().into_owned()
    }

    fn root_of(dir: &TempDir) -> &str {
        dir.path().to_str().expect("utf-8 temp path")
    }

    /// `namespace name` for every function, in order.
    fn listed(source: &str, path: &str, root: Option<&str>) -> Vec<String> {
        functions_in_source(source, path, root)
            .iter()
            .map(|f| format!("{} {}", f.namespace, f.name))
            .collect()
    }

    #[test]
    // MU-9: free fns use the crate namespace, methods the self type, trait impls the `for` type;
    // fns nested in a block and fns in `mod tests` are skipped
    fn mu_9_rust_free_fn_method_and_trait_impl_namespaces() {
        let dir = crate_at("demo-game");
        make_parents(&dir, "src/lib.rs");
        assert_eq!(
            listed(SOURCE, "src/lib.rs", Some(root_of(&dir))),
            [
                "demo_game choose",
                "demo_game::Sample run",
                "demo_game::Sample draw",
                "demo_game outer",
            ]
        );
    }

    #[test]
    // MU-9: a trait impl for a generic type takes the first type name after `for`
    fn mu_9_rust_impl_for_nested_generic_type_uses_first_type_name() {
        let dir = crate_at("demo");
        let path = make_parents(&dir, "src/lib.rs");
        let source = "impl Trait for Vec<crate::Sample> {\n    fn draw(&self) {}\n}\n";
        assert_eq!(
            listed(source, &path, Some(root_of(&dir))),
            ["demo::Vec draw"]
        );
    }

    #[test]
    // MU-9: src/bin files drop the bin segment; files outside the crate use the crate name
    fn mu_9_rust_bin_and_outside_files_module_paths() {
        let dir = crate_at("demo");
        let bin = make_parents(&dir, "src/bin/tool.rs");
        let bin_fns = functions_in_source("pub fn main_ok() {}\n", &bin, Some(root_of(&dir)));
        assert_eq!(bin_fns[0].namespace, "tool");
        let outside =
            functions_in_source("pub fn loose() {}\n", "/tmp/loose.rs", Some(root_of(&dir)));
        assert_eq!(outside[0].name, "loose");
        let bare = make_parents(&dir, "notes.rs");
        assert_eq!(
            listed("pub fn bare() {}\n", &bare, None),
            ["demo::notes bare"]
        );
    }

    #[test]
    // MU-9: a file outside the crate root gets only the crate name as its module path
    fn mu_9_rust_file_outside_crate_root_uses_crate_name() {
        assert_eq!(
            file_modules(
                Path::new("/tmp/loose.rs"),
                "demo",
                Some(Path::new("/projects/demo")),
            ),
            vec!["demo"]
        );
    }

    #[test]
    // MU-9: crate::module::file::f is the namespace of a free fn in a nested module
    fn mu_9_rust_nested_module_path_for_free_fn() {
        let dir = crate_at("demo");
        let path = make_parents(&dir, "src/game/board.rs");
        let source = "pub fn place() -> i32 { if ready { 1 } else { 0 } }\n";
        assert_eq!(
            listed(source, &path, Some(root_of(&dir))),
            ["demo::game::board place"]
        );
    }

    #[test]
    // MU-9: crate::module::file::Type is the namespace of a method in a nested module
    fn mu_9_rust_method_namespace_is_module_path_and_self_type() {
        let dir = crate_at("demo");
        let path = make_parents(&dir, "src/game/board.rs");
        let source = "impl Board {\n    pub fn place(&mut self) {}\n}\n";
        assert_eq!(
            listed(source, &path, Some(root_of(&dir))),
            ["demo::game::board::Board place"]
        );
    }

    #[test]
    // MU-9: start and end lines are 1-based spans; byte offsets are not set for Rust
    fn mu_9_rust_line_spans_without_byte_offsets() {
        let dir = crate_at("demo");
        let path = make_parents(&dir, "src/lib.rs");
        let source = "pub fn a() {\n    1\n}\n\nimpl S {\n    fn m(&self) {\n    }\n}\n";
        let found = functions_in_source(source, &path, Some(root_of(&dir)));
        let spans: Vec<_> = found
            .iter()
            .map(|f| (f.name.as_str(), f.start_line, f.end_line))
            .collect();
        assert_eq!(spans, vec![("a", 1, 3), ("m", 6, 7)]);
        assert!(
            found
                .iter()
                .all(|f| f.start_byte.is_none() && f.end_byte.is_none())
        );
    }
}
