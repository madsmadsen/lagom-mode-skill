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
//! enclosing function unless a route call decorates it. A route handler is an entry named
//! by its route. A class method's namespace is `module.Class`.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::languages::ts::{
    ancestor_names, child_of_type, children, claim, complexity, descendants, end_line, node_text,
    parse, start_line, Grammar,
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

const ROUTE_VERBS: &[&str] = &[
    "get",
    "post",
    "put",
    "patch",
    "delete",
    "head",
    "options",
    "trace",
    "websocket",
];
const ROUTE_RULES: &[&str] = &["route", "api_route"];
const PATH_KEYWORDS: &[&str] = &["path", "rule"];

pub fn functions_in_source(source: &str, path: &str, project_root: &str) -> Vec<Function> {
    let (data, tree) = parse(source, Grammar::Python);
    let module = module_namespace(path, project_root);
    let mut used = HashSet::new();
    descendants(tree.root_node())
        .into_iter()
        .filter(|node| node.kind() == "function_definition")
        .filter_map(|node| {
            identity(&data, node, &module, &mut used).map(|id| record(&data, node, id, path))
        })
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

/// Text of a string's children, without the quote marks.
fn string_body(data: &[u8], node: Node) -> String {
    children(node)
        .into_iter()
        .filter(|child| !matches!(child.kind(), "string_start" | "string_end"))
        .map(|child| node_text(data, child))
        .collect()
}

/// Value of the first keyword argument whose name is in `names`.
fn keyword<'t>(data: &[u8], arguments: Node<'t>, names: &[&str]) -> Option<Node<'t>> {
    children(arguments)
        .into_iter()
        .find(|child| {
            child.kind() == "keyword_argument"
                && child
                    .child_by_field_name("name")
                    .is_some_and(|name| names.contains(&node_text(data, name)))
        })
        .and_then(|child| child.child_by_field_name("value"))
}

/// The argument that gives the URL path: the first positional one, else a `path` or `rule` keyword.
fn path_argument<'t>(data: &[u8], arguments: Node<'t>) -> Option<Node<'t>> {
    let first = children(arguments)
        .into_iter()
        .find(|child| child.is_named() && child.kind() != "comment");
    match first {
        Some(first) if first.kind() != "keyword_argument" => Some(first),
        _ => keyword(data, arguments, PATH_KEYWORDS),
    }
}

/// The URL path an argument names, or None when the argument is not a URL path.
fn route_path(data: &[u8], node: Node) -> Option<String> {
    match node.kind() {
        "identifier" | "attribute" => Some(node_text(data, node).to_string()),
        "string" => url_string(data, node),
        _ => None,
    }
}

/// A string is a URL path when it has an interpolation, is empty, or starts with `/`.
fn url_string(data: &[u8], node: Node) -> Option<String> {
    let path = string_body(data, node);
    let is_path = child_of_type(node, &["interpolation"]).is_some()
        || path.is_empty()
        || path.starts_with('/');
    is_path.then_some(path)
}

/// The upper-case HTTP verbs of a `methods` list, joined by `|`.
fn listed_verbs(data: &[u8], listed: Node) -> String {
    children(listed)
        .into_iter()
        .filter(|item| item.kind() == "string")
        .map(|item| string_body(data, item).to_uppercase())
        .collect::<Vec<_>>()
        .join("|")
}

/// The verbs a route call names. A `route` or `api_route` call without `methods` is GET.
fn route_verbs(data: &[u8], method: &str, arguments: Node) -> String {
    if !ROUTE_RULES.contains(&method) {
        return method.to_uppercase();
    }
    let listed = keyword(data, arguments, &["methods"])
        .map(|node| listed_verbs(data, node))
        .unwrap_or_default();
    if listed.is_empty() {
        "GET".to_string()
    } else {
        listed
    }
}

fn is_route_method(method: &str) -> bool {
    ROUTE_VERBS.contains(&method) || ROUTE_RULES.contains(&method)
}

/// The method name and arguments of a decorator `@<object>.<method>(...)` with a route method.
fn route_call<'a, 't>(data: &'a [u8], decorator: Node<'t>) -> Option<(&'a str, Node<'t>)> {
    let call = child_of_type(decorator, &["call"])?;
    let callee = call
        .child_by_field_name("function")
        .filter(|node| node.kind() == "attribute")?;
    let method = node_text(data, callee.child_by_field_name("attribute")?);
    let arguments = call.child_by_field_name("arguments")?;
    is_route_method(method).then_some((method, arguments))
}

/// `GET /users` for a decorator that is a route call with a URL path, else None.
fn route_label(data: &[u8], decorator: Node) -> Option<String> {
    let (method, arguments) = route_call(data, decorator)?;
    let path = path_argument(data, arguments).and_then(|node| route_path(data, node))?;
    let verbs = route_verbs(data, method, arguments);
    Some(if path.is_empty() {
        verbs
    } else {
        format!("{verbs} {path}")
    })
}

/// `GET /users` for a function that a route decorator marks as a route handler, else None.
fn route(data: &[u8], node: Node) -> Option<String> {
    let parent = node
        .parent()
        .filter(|parent| parent.kind() == "decorated_definition")?;
    children(parent)
        .into_iter()
        .filter(|child| child.kind() == "decorator")
        .find_map(|decorator| route_label(data, decorator))
}

/// True for a function that a route decorator makes a route handler.
fn is_route_handler(data: &[u8], node: Node) -> bool {
    node.kind() == "function_definition" && route(data, node).is_some()
}

/// Complexity of `node`, leaving out the route handlers and classes nested in it.
fn complexity_without_routes(data: &[u8], node: Node) -> u32 {
    complexity(node, is_decision, |current| {
        current.id() != node.id() && (is_route_handler(data, current) || skip_nested_class(current))
    })
}

/// The module, then each enclosing class name, joined by `.`.
fn namespace_of(data: &[u8], node: Node, module: &str) -> String {
    let classes = ancestor_names(data, node, "class_definition");
    std::iter::once(module)
        .chain(classes.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(".")
}

/// The name and namespace of a function entry, or None when the function is not an entry.
/// A nested function is an entry only when a route call decorates it, and then its name is the route.
fn identity(
    data: &[u8],
    node: Node,
    module: &str,
    used: &mut HashSet<String>,
) -> Option<(String, String)> {
    if is_nested_function(node) {
        let label = route(data, node)?;
        return Some((claim(used, label), module.to_string()));
    }
    let ident = child_of_type(node, &["identifier"])?;
    Some((
        node_text(data, ident).to_string(),
        namespace_of(data, node, module),
    ))
}

fn record(data: &[u8], node: Node, (name, namespace): (String, String), path: &str) -> Function {
    Function {
        name,
        namespace,
        complexity: complexity_without_routes(data, node),
        start_line: start_line(node),
        end_line: end_line(node),
        path: path.to_string(),
        language: "python".to_string(),
        jacoco_class: None,
    }
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

    // CR-22, AC-31: route handlers in a factory are their own entries, named by route, and their decisions leave the factory.
    #[test]
    fn cr_22_ac_31_route_handlers_in_a_factory_are_their_own_entries() {
        let source = r#"
def build_router(config):
    router = APIRouter()
    if config.debug:
        router.debug = True

    @router.get("/users/{user_id}", response_model=User)
    async def user(user_id: str):
        if not user_id:
            return None
        return user_id

    @router.post("/users", include_in_schema=config.debug or config.docs)
    @requires("admin")
    async def add_user(body):
        return body if body else None

    @router.api_route(path="/sync", methods=["GET", "post"])
    def sync():
        return 1

    @app.route("/legacy")
    def legacy():
        return 1

    @router.get("/users/{user_id}")
    async def user_again(user_id: str):
        return user_id

    @router.get("/users/{user_id}")
    async def user_third(user_id: str):
        return user_id

    @router.delete(PATHS.user)
    def remove():
        return 1

    @router.get(f"{PREFIX}/items")
    def items():
        return 1

    @router.get("")
    def root():
        return 1

    def helper(x):
        return 1 if x else 0

    return router
"#;
        let functions = functions_in_source(source, "src/demo/api.py", "/proj");
        assert_eq!(
            summary(&functions),
            [
                ("demo.api", "build_router", 4),
                ("demo.api", "GET /users/{user_id}", 2),
                ("demo.api", "POST /users", 2),
                ("demo.api", "GET|POST /sync", 1),
                ("demo.api", "GET /legacy", 1),
                ("demo.api", "GET /users/{user_id}#2", 1),
                ("demo.api", "GET /users/{user_id}#3", 1),
                ("demo.api", "DELETE PATHS.user", 1),
                ("demo.api", "GET {PREFIX}/items", 1),
                ("demo.api", "GET", 1),
            ]
        );
        // CR-22 item 9: the handler lines are still inside the lines of the factory.
        assert_eq!((functions[1].start_line, functions[1].end_line), (8, 11));
        assert!(functions[0].start_line < 8 && functions[0].end_line > 11);
    }

    // CR-22, AC-31: top-level handlers and methods keep their names, and look-alikes stay nested.
    #[test]
    fn cr_22_ac_31_top_level_handlers_keep_their_names_and_lookalikes_stay_nested() {
        let source = r#"
@app.get("/health")
def health():
    return 1

class Views:
    @router.get("/items")
    def items(self):
        return 1

def decorate(fn):
    @functools.wraps(fn)
    def wrapper(*args):
        return fn(*args) if args else None

    @mock.patch("pkg.mod.attr")
    def patched(mocked):
        return 1 if mocked else 0

    @router.get()
    def unrouted(flag):
        return 1 if flag else 0

    @router.get(build_path())
    def computed(flag):
        return 1 if flag else 0

    @cache
    @retry(3)
    def cached(flag):
        return 1 if flag else 0

    @router.route("/many", methods=METHODS)
    def many():
        return 1

    return wrapper
"#;
        let functions = functions_in_source(source, "src/demo/api.py", "/proj");
        assert_eq!(
            summary(&functions),
            [
                ("demo.api", "health", 1),
                ("demo.api.Views", "items", 1),
                ("demo.api", "decorate", 6),
                ("demo.api", "GET /many", 1),
            ]
        );
    }

    // CR-22 item 4: two handlers with the label GET /users give GET /users and GET /users#2.
    #[test]
    fn cr_22_ac_31_a_repeated_route_label_gets_a_suffix() {
        let source = r#"
def build(router):
    @router.get("/users")
    def first():
        return 1

    @router.get("/users")
    def second():
        return 1
"#;
        let functions = functions_in_source(source, "src/demo/api.py", "/proj");
        assert_eq!(
            summary(&functions),
            [
                ("demo.api", "build", 1),
                ("demo.api", "GET /users", 1),
                ("demo.api", "GET /users#2", 1),
            ]
        );
    }
}
