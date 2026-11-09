//! Port of crapper languages/golang.py (BSD Zero Clause License, Robert C. Martin).
//!
//! Functions are namespaced by their package import path. Methods are namespaced by their
//! receiver type (`import/path.Widget`). The import path comes from the nearest `go.mod`.

use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use crate::model::Function;
use crate::ts::{self, child_of_type, end_line, node_text, start_line};

/// Functions with a body in a Go source file.
pub fn functions_in_source(source: &str, path: &str, project_root: Option<&str>) -> Vec<Function> {
    let Some(tree) = ts::parse(source, "go") else {
        return Vec::new();
    };
    let data = source.as_bytes();
    let root = tree.root_node();
    let absolute = ts::absolute_path(path, project_root);
    let package_ns = package_namespace(&absolute, &package_clause(data, root));
    ts::descendants(root)
        .into_iter()
        .filter_map(|node| go_function(data, node, &package_ns))
        .collect()
}

fn package_clause(data: &[u8], root: Node<'_>) -> String {
    ts::descendants(root)
        .into_iter()
        .find(|node| node.kind() == "package_identifier")
        .map(|node| node_text(data, node).to_string())
        .unwrap_or_default()
}

fn go_function(data: &[u8], node: Node<'_>, package_ns: &str) -> Option<Function> {
    child_of_type(node, &["block"])?;
    let (name, namespace) = entry_identity(data, node, package_ns)?;
    Some(Function {
        name,
        namespace,
        start_line: start_line(node),
        end_line: end_line(node),
        start_byte: None,
        end_byte: None,
    })
}

fn entry_identity(data: &[u8], node: Node<'_>, package_ns: &str) -> Option<(String, String)> {
    match node.kind() {
        "function_declaration" => {
            let name = non_empty_child_text(data, node, "identifier")?;
            Some((name, package_ns.to_string()))
        }
        "method_declaration" => {
            let name = non_empty_child_text(data, node, "field_identifier")?;
            let receiver = receiver_type(data, node)?;
            Some((name, format!("{package_ns}.{receiver}")))
        }
        _ => None,
    }
}

fn non_empty_child_text(data: &[u8], node: Node<'_>, kind: &str) -> Option<String> {
    let child = child_of_type(node, &[kind])?;
    owned_nonempty(node_text(data, child))
}

fn receiver_type(data: &[u8], method: Node<'_>) -> Option<String> {
    let params = child_of_type(method, &["parameter_list"])?;
    let ident = ts::descendants(params)
        .into_iter()
        .find(|node| node.kind() == "type_identifier")?;
    owned_nonempty(node_text(data, ident))
}

fn owned_nonempty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

/// Import path of the package containing `path`, or `package_name` when no `go.mod` applies.
fn package_namespace(path: &str, package_name: &str) -> String {
    let dir = source_dir(path);
    let Some((module, module_dir)) = go_module(&dir) else {
        return package_name.to_string();
    };
    match dir.strip_prefix(module_dir) {
        Ok(relative) => import_path(&module, relative),
        Err(_) => package_name.to_string(),
    }
}

fn source_dir(path: &str) -> PathBuf {
    let file = Path::new(path);
    let resolved = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    resolved
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

/// Module path and directory of the nearest `go.mod` at or above `dir`.
fn go_module(dir: &Path) -> Option<(String, PathBuf)> {
    let root = dir
        .ancestors()
        .find(|candidate| candidate.join("go.mod").is_file())?;
    let text = fs::read_to_string(root.join("go.mod")).ok()?;
    let module = text
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("module "))?;
    Some((module.trim().to_string(), root.to_path_buf()))
}

fn import_path(module: &str, relative: &Path) -> String {
    let parts: Vec<&str> = relative
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect();
    if parts.is_empty() {
        module.to_string()
    } else {
        format!("{module}/{}", parts.join("/"))
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

    /// MU-9: a package-level function is namespaced by its package and has line bounds.
    #[test]
    fn mu_9_go_function_namespace_and_lines() {
        let source = r#"package sample

func Branchy(x int, ch chan int) int {
	if x > 0 && x < 10 {
		return 1
	}
	switch x {
	case 1, 2:
		return 2
	default:
		return 3
	}
}
"#;
        let found = functions_in_source(source, "sample.go", None);
        assert_eq!(pairs(&found), vec![("sample", "Branchy")]);
        assert_eq!((found[0].start_line, found[0].end_line), (3, 13));
        assert_eq!((found[0].start_byte, found[0].end_byte), (None, None));
    }

    /// MU-9: methods are namespaced by receiver type; functions by package.
    #[test]
    fn mu_9_go_methods_are_namespaced_by_receiver_type() {
        let source = r#"package sample

type Widget struct{}

func (w *Widget) Run() {
	for i := 0; i < 10; i++ {
	}
	for _, item := range items {
	}
}

func Simple() int { return 1 }
"#;
        let found = functions_in_source(source, "widget.go", None);
        assert_eq!(
            pairs(&found),
            vec![("sample.Widget", "Run"), ("sample", "Simple")]
        );
        assert_eq!((found[0].start_line, found[0].end_line), (5, 10));
        assert_eq!((found[1].start_line, found[1].end_line), (12, 12));
    }

    /// MU-9: declarations without a body (interface methods, externs) are not entries.
    #[test]
    fn mu_9_go_skips_declarations_without_body() {
        let source = "package sample\n\ntype Runner interface {\n\tRun()\n}\n\nfunc Extern() int\n";
        let found = functions_in_source(source, "runner.go", None);
        assert_eq!(pairs(&found), Vec::<(&str, &str)>::new());
    }

    /// MU-9: the import path comes from go.mod plus the file's directory.
    #[test]
    fn mu_9_go_import_path_comes_from_go_mod() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("go.mod"), "module github.com/acme/demo\n").expect("go.mod");
        let dir = tmp.path().join("internal").join("game");
        fs::create_dir_all(&dir).expect("dirs");
        let source = r#"package game

type Board struct{}

func (b *Board) Place() {
	if ready && ok {
		return
	}
}
"#;
        fs::write(dir.join("board.go"), source).expect("source");
        let root = tmp.path().to_str().expect("utf-8 tempdir");
        let found = functions_in_source(source, "internal/game/board.go", Some(root));
        assert_eq!(
            pairs(&found),
            vec![("github.com/acme/demo/internal/game.Board", "Place")]
        );
        assert_eq!((found[0].start_line, found[0].end_line), (5, 9));
    }

    /// MU-9: a go.mod at the root maps the root package to the bare module path.
    #[test]
    fn mu_9_go_root_package_uses_bare_module_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("go.mod"), "module github.com/acme/demo\n").expect("go.mod");
        let source = "package main\n\nfunc (m *Main) Start() {}\n";
        let root = tmp.path().to_str().expect("utf-8 tempdir");
        let found = functions_in_source(source, "main.go", Some(root));
        assert_eq!(pairs(&found), vec![("github.com/acme/demo.Main", "Start")]);
    }

    /// MU-9: without a go.mod the namespace is the package clause name.
    #[test]
    fn mu_9_go_without_go_mod_uses_package_clause() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = "package util\n\nfunc Helper() {}\n";
        let path = tmp.path().join("util.go");
        let found = functions_in_source(source, path.to_str().expect("utf-8 path"), None);
        assert_eq!(pairs(&found), vec![("util", "Helper")]);
    }
}
