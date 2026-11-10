// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/python.py
// license: MIT

//! Python functions, methods, and cyclomatic complexity.
//!
//! Decision points follow the same structural rule as the other languages:
//! `if`, `elif`, `for`, `while`, `except`, each `match` case, a comprehension
//! filter, a conditional expression, and each `and` / `or`. Module-level
//! functions and class methods are entries. A nested function stays inside the
//! enclosing function. A class method's namespace is `module.Class`.

use tree_sitter::Node;

use crate::languages::ts::{
    ancestor_names, child_of_type, complexity, descendants, end_line, node_text, parse, start_line,
    Grammar,
};
use crate::model::Function;

const DECISIONS: &[&str] = &[
    "if_statement",
    "elif_clause",
    "for_statement",
    "while_statement",
    "except_clause",
    "conditional_expression",
    "case_clause",
    "if_clause",
];

pub fn functions_in_source(source: &str, path: &str, project_root: &str) -> Vec<Function> {
    let (data, tree) = parse(source, Grammar::Python);
    let module = module_namespace(path, project_root);
    descendants(tree.root_node())
        .into_iter()
        .filter(|node| node.kind() == "function_definition" && !is_nested_function(*node))
        .filter_map(|node| record(&data, node, &module, path))
        .collect()
}

fn is_decision(node: Node) -> bool {
    DECISIONS.contains(&node.kind()) || node.kind() == "boolean_operator"
}

fn skip_nested_class(node: Node) -> bool {
    node.kind() == "class_definition"
}

fn strip_root(relative: &str, source_root: &str) -> String {
    let root = source_root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    if !root.is_empty() && root != "." {
        if let Some(rest) = relative.strip_prefix(format!("{root}/").as_str()) {
            return rest.to_string();
        }
    }
    relative.strip_prefix("./").unwrap_or(relative).to_string()
}

fn strip_src(relative: &str) -> &str {
    if let Some(rest) = relative.strip_prefix("src/") {
        return rest;
    }
    match relative.split_once("/src/") {
        Some((_, rest)) => rest,
        None => relative,
    }
}

fn strip_module_suffix(relative: &str) -> &str {
    let relative = relative.strip_suffix(".py").unwrap_or(relative);
    if let Some(rest) = relative.strip_suffix("/__init__") {
        return rest;
    }
    if relative == "__init__" {
        return "";
    }
    relative
}

fn module_namespace(path: &str, source_root: &str) -> String {
    let normalized = path.replace('\\', "/");
    let rooted = strip_root(&normalized, source_root);
    let relative = strip_module_suffix(strip_src(&rooted));
    if relative.is_empty() {
        "__init__".to_string()
    } else {
        relative.replace('/', ".")
    }
}

/// A function inside another function, unless it is a method of a class.
fn is_nested_function(node: Node) -> bool {
    std::iter::successors(node.parent(), |n| n.parent())
        .find(|ancestor| matches!(ancestor.kind(), "class_definition" | "function_definition"))
        .is_some_and(|ancestor| ancestor.kind() == "function_definition")
}

fn record(data: &[u8], node: Node, module: &str, path: &str) -> Option<Function> {
    let ident = child_of_type(node, &["identifier"])?;
    let classes = ancestor_names(data, node, "class_definition");
    let namespace = std::iter::once(module)
        .chain(classes.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(".");
    Some(Function {
        name: node_text(data, ident).to_string(),
        namespace,
        complexity: complexity(node, is_decision, skip_nested_class),
        start_line: start_line(node),
        end_line: end_line(node),
        path: path.to_string(),
        language: "python".to_string(),
        jacoco_class: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(functions: &[Function]) -> Vec<(&str, &str, u32)> {
        functions
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str(), f.complexity))
            .collect()
    }

    // CR-8 CR-11: module functions, methods, nested functions stay inside their outer function, async defs.
    #[test]
    fn cr_8_cr_11_function_method_and_nested_function() {
        let source = r#"
def choose(x, ready):
    if x > 0 and x < 10:
        return 1
    elif x == 0 or ready:
        return 0
    for i in xs:
        if i == 2:
            return i
    while ready:
        break
    try:
        return x
    except ValueError:
        return 0
    except KeyError:
        return -1
    match x:
        case 1:
            return 1
        case 2 | 3:
            return 2
        case _:
            return 0
    return x if x > 0 else 0

class Box:
    def open(self, flag=False):
        return 1 if flag else 0

    @staticmethod
    def shut(self):
        xs = [n for n in range(3) if n]
        return xs

def outer():
    def inner(n):
        if n:
            return 1
        return 0
    return inner(1)

async def load():
    if True:
        return 1
"#;
        let functions = functions_in_source(source, "src/demo/box.py", "/proj");
        assert_eq!(
            summary(&functions),
            [
                ("demo.box", "choose", 14),
                ("demo.box.Box", "open", 2),
                ("demo.box.Box", "shut", 2),
                ("demo.box", "outer", 2),
                ("demo.box", "load", 2),
            ]
        );
    }

    // CR-8 CR-11: a class nested in a class or in a function gives its methods the full class path.
    #[test]
    fn cr_8_cr_11_nested_class_method_is_its_own_entry() {
        let source = r#"
class Outer:
    class Inner:
        def tick(self, ready, ok):
            if ready and ok:
                return 1
            return 0

def factory():
    class Hidden:
        def secret(self):
            if True:
                return 1
            return 0
    return Hidden()
"#;
        let functions = functions_in_source(source, "src/demo/sample.py", "/proj");
        assert_eq!(
            summary(&functions),
            [
                ("demo.sample.Outer.Inner", "tick", 3),
                ("demo.sample", "factory", 1),
                ("demo.sample.Hidden", "secret", 2),
            ]
        );
    }

    // CR-8: the namespace strips the project root, the src directory, the .py suffix, and __init__.
    #[test]
    fn cr_8_namespace_strips_roots_src_and_init() {
        let source = "def f():\n    return 1\n";
        let namespace =
            |path: &str, root: &str| functions_in_source(source, path, root)[0].namespace.clone();
        assert_eq!(namespace("/proj/src/demo/app.py", "/proj"), "demo.app");
        assert_eq!(namespace("./src/demo/app.py", "."), "demo.app");
        assert_eq!(namespace("pkg/src/demo/__init__.py", ""), "demo");
        assert_eq!(namespace("__init__.py", ""), "__init__");
    }

    // CR-11: docstrings, comments, and string literals add no decisions.
    #[test]
    fn cr_11_strings_and_comments_do_not_add_decisions() {
        let source = r#"
def literal():
    """if and or for while except"""
    text = "if x and y"
    # if x: return 1
    return text
"#;
        let functions = functions_in_source(source, "src/demo/__init__.py", "/proj");
        assert_eq!(summary(&functions), [("demo", "literal", 1)]);
    }

    // CR-9: tree-sitter positions are 1-based and cover the whole definition; language and path are recorded.
    #[test]
    fn cr_9_items_carry_tree_sitter_line_positions() {
        let source =
            "def a():\n    return 1\n\n\ndef b():\n    if x:\n        return 1\n    return 0\n";
        let functions = functions_in_source(source, "src/demo/lines.py", "/proj");
        let lines: Vec<_> = functions
            .iter()
            .map(|f| (f.name.as_str(), f.start_line, f.end_line))
            .collect();
        assert_eq!(lines, [("a", 1, 2), ("b", 5, 8)]);
        assert_eq!(functions[0].language, "python");
        assert_eq!(functions[0].path, "src/demo/lines.py");
    }

    // CR-8: strip_root removes a source root only when it is a real prefix; "" and "." never strip.
    #[test]
    fn cr_8_strip_root_removes_only_a_root_that_prefixes_the_path() {
        assert_eq!(strip_root("lib/pkg/mod.py", "lib"), "pkg/mod.py");
        assert_eq!(strip_root("src/pkg/mod.py", "src/"), "pkg/mod.py");
        assert_eq!(strip_root("src/pkg/mod.py", "src\\"), "pkg/mod.py");
        assert_eq!(strip_root("other/pkg/mod.py", "lib"), "other/pkg/mod.py");
        assert_eq!(strip_root("lib/pkg/mod.py", "."), "lib/pkg/mod.py");
        assert_eq!(strip_root("/x/y.py", ""), "/x/y.py");
        assert_eq!(strip_root("./x/y.py", ""), "x/y.py");
        assert_eq!(strip_root("./src/pkg/mod.py", "."), "src/pkg/mod.py");
    }

    // CR-8: module names follow each source_root value: empty, ".", "src", "src/", backslash, and a non-prefix root.
    #[test]
    fn cr_8_module_names_follow_source_root_values() {
        let source = "def f():\n    return 1\n";
        let namespace =
            |path: &str, root: &str| functions_in_source(source, path, root)[0].namespace.clone();
        assert_eq!(namespace("/x/y.py", ""), ".x.y");
        assert_eq!(namespace("x.py", ""), "x");
        assert_eq!(namespace("./src/pkg/mod.py", "."), "pkg.mod");
        assert_eq!(namespace("src/pkg/mod.py", "src"), "pkg.mod");
        assert_eq!(namespace("src/pkg/mod.py", "src/"), "pkg.mod");
        assert_eq!(namespace("src/pkg/mod.py", "src\\"), "pkg.mod");
        assert_eq!(namespace("lib\\pkg\\mod.py", "lib\\"), "pkg.mod");
        assert_eq!(namespace("lib/pkg/mod.py", "lib"), "pkg.mod");
        assert_eq!(namespace("./lib/pkg/mod.py", "lib"), "lib.pkg.mod");
        assert_eq!(namespace("other/pkg/mod.py", "lib"), "other.pkg.mod");
    }
}
