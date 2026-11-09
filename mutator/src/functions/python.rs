//! Port of crapper languages/python.py without complexity (BSD Zero Clause License, Robert C. Martin).

use tree_sitter::Node;

use super::{named_ancestors, strip_src};
use crate::model::Function;
use crate::ts::{child_of_type, descendants, end_line, node_text, parse, start_line};

/// Functions and methods of a Python source file, in source order (MU-9).
pub fn functions_in_source(source: &str, path: &str, source_root: Option<&str>) -> Vec<Function> {
    let Some(tree) = parse(source, "python") else {
        return Vec::new();
    };
    let data = source.as_bytes();
    let module = module_namespace(path, source_root);
    descendants(tree.root_node())
        .into_iter()
        .filter(|node| node.kind() == "function_definition" && !is_nested_function(*node))
        .filter_map(|node| function_entry(data, node, &module))
        .collect()
}

fn function_entry(data: &[u8], node: Node, module: &str) -> Option<Function> {
    let ident = child_of_type(node, &["identifier"])?;
    let classes = named_ancestors(data, node, &["class_definition"], &["identifier"]);
    let namespace = std::iter::once(module)
        .chain(classes.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(".");
    Some(Function {
        name: node_text(data, ident).to_string(),
        namespace,
        start_line: start_line(node),
        end_line: end_line(node),
        start_byte: None,
        end_byte: None,
    })
}

fn ancestors<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    std::iter::successors(node.parent(), |current| current.parent())
}

/// A function inside another function, unless it is a method of a class.
fn is_nested_function(node: Node) -> bool {
    ancestors(node)
        .find(|ancestor| matches!(ancestor.kind(), "class_definition" | "function_definition"))
        .is_some_and(|ancestor| ancestor.kind() == "function_definition")
}

fn module_namespace(path: &str, source_root: Option<&str>) -> String {
    let normalized = path.replace('\\', "/");
    let relative = strip_module_suffix(strip_src(strip_root(&normalized, source_root)));
    if relative.is_empty() {
        "__init__".to_string()
    } else {
        relative.replace('/', ".")
    }
}

fn strip_root<'a>(relative: &'a str, source_root: Option<&str>) -> &'a str {
    let root = source_root.unwrap_or("").replace('\\', "/");
    let root = root.trim_end_matches('/');
    if !root.is_empty()
        && root != "."
        && let Some(rest) = relative
            .strip_prefix(root)
            .and_then(|r| r.strip_prefix('/'))
    {
        return rest;
    }
    relative.strip_prefix("./").unwrap_or(relative)
}

fn strip_module_suffix(relative: &str) -> &str {
    let relative = relative.strip_suffix(".py").unwrap_or(relative);
    if let Some(rest) = relative.strip_suffix("/__init__") {
        return rest;
    }
    if relative == "__init__" { "" } else { relative }
}

#[cfg(test)]
mod tests {
    use super::functions_in_source;

    /// `namespace name` for every function, in order.
    fn listed(source: &str, path: &str, root: Option<&str>) -> Vec<String> {
        functions_in_source(source, path, root)
            .iter()
            .map(|f| format!("{} {}", f.namespace, f.name))
            .collect()
    }

    #[test]
    // MU-9: module functions, class methods, nested functions stay inside, async defs
    fn mu_9_python_method_namespace_and_nested_function_stays_inside() {
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
        assert_eq!(
            listed(source, "src/demo/box.py", Some("/proj")),
            [
                "demo.box choose",
                "demo.box.Box open",
                "demo.box.Box shut",
                "demo.box outer",
                "demo.box load",
            ]
        );
    }

    #[test]
    // MU-9: a class nested in a class gets the full dotted namespace; a class in a function is kept
    fn mu_9_python_nested_class_method_is_its_own_entry() {
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
        assert_eq!(
            listed(source, "src/demo/sample.py", Some("/proj")),
            [
                "demo.sample.Outer.Inner tick",
                "demo.sample factory",
                "demo.sample.Hidden secret",
            ]
        );
    }

    #[test]
    // MU-9: namespace strips the project root, a leading ./, and src/; __init__ maps to its package
    fn mu_9_python_namespace_strips_roots_src_and_init() {
        let source = "def f():\n    return 1\n";
        assert_eq!(
            listed(source, "/proj/src/demo/app.py", Some("/proj")),
            ["demo.app f"]
        );
        assert_eq!(
            listed(source, "./src/demo/app.py", Some(".")),
            ["demo.app f"]
        );
        assert_eq!(listed(source, "pkg/src/demo/__init__.py", None), ["demo f"]);
        assert_eq!(listed(source, "__init__.py", None), ["__init__ f"]);
    }

    #[test]
    // MU-9: strings and comments are not functions; a package __init__ names its package
    fn mu_9_python_strings_and_comments_add_no_functions() {
        let source = r#"
def literal():
    """if and or for while except"""
    text = "if x and y"
    # if x: return 1
    return text
"#;
        assert_eq!(
            listed(source, "src/demo/__init__.py", Some("/proj")),
            ["demo literal"]
        );
    }

    #[test]
    // MU-9: a class __init__ method is a method entry named __init__ in its class namespace
    fn mu_9_python_init_method_is_a_method_of_its_class() {
        let source = "class Box:\n    def __init__(self, n):\n        self.n = n\n";
        assert_eq!(
            listed(source, "src/demo/box.py", Some("/proj")),
            ["demo.box.Box __init__"]
        );
    }

    #[test]
    // MU-9: start and end lines are 1-based spans; byte offsets are not set for Python
    fn mu_9_python_line_spans_without_byte_offsets() {
        let source = "def a():\n    return 1\n\nclass C:\n    def m(self):\n        return 2\n";
        let found = functions_in_source(source, "src/demo/spans.py", Some("/proj"));
        let spans: Vec<_> = found
            .iter()
            .map(|f| (f.name.as_str(), f.start_line, f.end_line))
            .collect();
        assert_eq!(spans, vec![("a", 1, 2), ("m", 5, 6)]);
        assert!(
            found
                .iter()
                .all(|f| f.start_byte.is_none() && f.end_byte.is_none())
        );
    }
}
