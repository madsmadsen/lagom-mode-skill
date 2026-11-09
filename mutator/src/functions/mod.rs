//! Port of crapper languages/__init__.py (BSD Zero Clause License, Robert C. Martin).

pub mod clojure;
pub mod golang;
pub mod java;
pub mod python;
pub mod rust;
pub mod typescript;

use tree_sitter::Node;

use crate::model::{Function, Language};
use crate::ts::{child_of_type, node_text};

/// Functions of `source` for `language`, read with `project_root` for path resolution.
pub fn functions_in_file(
    language: Language,
    source: &str,
    path: &str,
    project_root: &str,
) -> Vec<Function> {
    let root = Some(project_root);
    match language {
        Language::Clojure => clojure::functions_in_source(source, path, root),
        Language::Java => java::functions_in_source(source, path, root),
        Language::Go => golang::functions_in_source(source, path, root),
        Language::TypeScript => typescript::functions_in_source(source, path, root),
        Language::Rust => rust::functions_in_source(source, path, root),
        Language::Python => python::functions_in_source(source, path, root),
    }
}

/// Path after the first `src/` segment: `src/x/y.rs` and `a/src/x/y.rs` both give `x/y.rs`.
pub(crate) fn strip_src(relative: &str) -> &str {
    if let Some(rest) = relative.strip_prefix("src/") {
        return rest;
    }
    relative
        .split_once("/src/")
        .map_or(relative, |(_, rest)| rest)
}

/// Names of the ancestors of `node` whose kind is in `kinds`, outermost first.
/// Each name is the text of the first child whose kind is in `name_kinds`.
pub(crate) fn named_ancestors(
    data: &[u8],
    node: Node,
    kinds: &[&str],
    name_kinds: &[&str],
) -> Vec<String> {
    let mut names: Vec<String> = std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .filter(|ancestor| kinds.contains(&ancestor.kind()))
        .filter_map(|owner| child_of_type(owner, name_kinds))
        .map(|ident| node_text(data, ident).to_string())
        .collect();
    names.reverse();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MU-9: the Java reader is selected for Language::Java.
    #[test]
    fn mu_9_dispatches_java_source_to_java_reader() {
        let source = "class Sample { int run() { return 1; } }";
        let found = functions_in_file(Language::Java, source, "Sample.java", "/nonexistent-mu9");
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str()))
            .collect();
        assert_eq!(pairs, vec![("Sample", "run")]);
    }

    /// MU-9: the Go reader is selected for Language::Go and resolves the package name.
    #[test]
    fn mu_9_dispatches_go_source_to_go_reader() {
        let source = "package sample\n\nfunc Run() {}\n";
        let found = functions_in_file(Language::Go, source, "sample.go", "/nonexistent-mu9");
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str()))
            .collect();
        assert_eq!(pairs, vec![("sample", "Run")]);
    }

    // MU-10: strip_src keeps the path after the first src segment
    #[test]
    fn mu_10_strip_src_keeps_the_path_after_src() {
        assert_eq!(strip_src("src/demo/app.py"), "demo/app.py");
        assert_eq!(strip_src("pkg/src/demo/app.py"), "demo/app.py");
        assert_eq!(strip_src("demo/app.py"), "demo/app.py");
    }

    // MU-10: named_ancestors lists enclosing names outermost first
    #[test]
    fn mu_10_named_ancestors_lists_outermost_first() {
        let source = "class A:\n    class B:\n        def f(self):\n            pass\n";
        let tree = crate::ts::parse(source, "python").expect("python parses");
        let function = crate::ts::descendants(tree.root_node())
            .into_iter()
            .find(|node| node.kind() == "function_definition")
            .expect("function exists");
        assert_eq!(
            named_ancestors(
                source.as_bytes(),
                function,
                &["class_definition"],
                &["identifier"]
            ),
            ["A", "B"]
        );
    }
}
