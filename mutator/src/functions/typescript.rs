// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Port of crapper languages/typescript.py.

use std::collections::HashSet;

use tree_sitter::Node;

use super::{named_ancestors, strip_src};
use crate::model::Function;
use crate::ts::{child_of_type, descendants, end_line, node_text, parse, start_line};

type Found = Vec<(usize, Function)>;

const FUNCTION_TYPES: &[&str] = &[
    "function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
];
const ROUTE_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "all", "use",
];
const CALLBACKS: &[&str] = &["arrow_function", "function_expression"];
const JS_SUFFIXES: &[&str] = &[".jsx", ".mjs", ".cjs", ".js"];
const SCRIPT_SUFFIXES: &[&str] = &[".tsx", ".mts", ".cts", ".jsx", ".mjs", ".cjs", ".ts", ".js"];

/// A route callback waiting for its label, which is claimed in start-byte order.
struct Route<'t> {
    callback: Node<'t>,
    method: &'static str,
    path: Option<String>,
}

/// Functions, class methods, arrow declarators, and Express route callbacks, by start byte.
pub fn functions_in_source(source: &str, path: &str, project_root: Option<&str>) -> Vec<Function> {
    let Some(tree) = parse(source, grammar(path)) else {
        return Vec::new();
    };
    let data = source.as_bytes();
    let module = module_namespace(path, project_root);
    let mut found = Vec::new();
    let mut routes = Vec::new();
    for node in descendants(tree.root_node()) {
        visit(&mut found, &mut routes, data, &module, node);
    }
    append_routes(&mut found, routes, &module);
    found.sort_by_key(|(start, _)| *start);
    found.into_iter().map(|(_, function)| function).collect()
}

fn visit<'t>(
    found: &mut Found,
    routes: &mut Vec<Route<'t>>,
    data: &[u8],
    module: &str,
    node: Node<'t>,
) {
    match node.kind() {
        "function_declaration" => append_function(found, data, node, module),
        "method_definition" => append_method(found, data, node, module),
        "lexical_declaration" => append_arrows(found, data, node, module),
        "call_expression" => collect_routes(routes, data, node),
        _ => {}
    }
}

fn grammar(path: &str) -> &'static str {
    if path.ends_with(".tsx") {
        "tsx"
    } else if JS_SUFFIXES.iter().any(|suffix| path.ends_with(*suffix)) {
        "javascript"
    } else {
        "typescript"
    }
}

fn module_namespace(path: &str, project_root: Option<&str>) -> String {
    let relative = path.replace('\\', "/");
    let relative = strip_src(strip_root_prefix(&relative, project_root));
    strip_script_suffix(relative).replace('/', ".")
}

fn strip_root_prefix<'a>(relative: &'a str, project_root: Option<&str>) -> &'a str {
    let root = project_root.unwrap_or("").replace('\\', "/");
    let root = root.trim_end_matches('/');
    let under_root = match root {
        "" | "." => None,
        _ => relative.strip_prefix(&format!("{root}/")),
    };
    under_root.unwrap_or_else(|| relative.strip_prefix("./").unwrap_or(relative))
}

fn strip_script_suffix(relative: &str) -> &str {
    SCRIPT_SUFFIXES
        .iter()
        .find_map(|suffix| relative.strip_suffix(*suffix))
        .unwrap_or(relative)
}

fn child_nodes(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn inside_function(node: Node) -> bool {
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .any(|ancestor| FUNCTION_TYPES.contains(&ancestor.kind()))
}

fn has_body(node: Node) -> bool {
    child_of_type(node, &["statement_block"]).is_some()
}

fn push(found: &mut Found, node: Node, name: &str, namespace: &str) {
    found.push((
        node.start_byte(),
        Function {
            name: name.to_owned(),
            namespace: namespace.to_owned(),
            start_line: start_line(node),
            end_line: end_line(node),
            start_byte: Some(node.start_byte()),
            end_byte: Some(node.end_byte()),
        },
    ));
}

fn append_function(found: &mut Found, data: &[u8], node: Node, module: &str) {
    if inside_function(node) || !has_body(node) {
        return;
    }
    if let Some(ident) = child_of_type(node, &["identifier"]) {
        push(found, node, node_text(data, ident), module);
    }
}

fn append_method(found: &mut Found, data: &[u8], node: Node, module: &str) {
    if !has_body(node) {
        return;
    }
    let Some(ident) = child_of_type(node, &["property_identifier", "identifier"]) else {
        return;
    };
    let classes = named_ancestors(
        data,
        node,
        &["class_declaration", "abstract_class_declaration"],
        &["type_identifier", "identifier"],
    );
    if classes.is_empty() {
        return;
    }
    let namespace = format!("{module}.{}", classes.join("."));
    push(found, node, node_text(data, ident), &namespace);
}

fn arrow_name<'a>(data: &'a [u8], declarator: Node) -> Option<&'a str> {
    let ident = child_of_type(declarator, &["identifier"])?;
    child_of_type(declarator, &["arrow_function", "function_expression"])?;
    Some(node_text(data, ident))
}

fn append_arrows(found: &mut Found, data: &[u8], node: Node, module: &str) {
    if inside_function(node) {
        return;
    }
    for declarator in child_nodes(node)
        .into_iter()
        .filter(|child| child.kind() == "variable_declarator")
    {
        if let Some(name) = arrow_name(data, declarator) {
            push(found, declarator, name, module);
        }
    }
}

fn expression(node: Node) -> Option<Node> {
    let mut current = node;
    while current.kind() == "parenthesized_expression" {
        current = child_nodes(current)
            .into_iter()
            .find(|child| !matches!(child.kind(), "(" | ")"))?;
    }
    Some(current)
}

fn call_callee(call: Node) -> Option<Node> {
    child_nodes(call)
        .into_iter()
        .find(|child| child.kind() != "arguments")
}

fn property_name<'a>(data: &'a [u8], node: Option<Node>) -> Option<&'a str> {
    let member = node.filter(|node| node.kind() == "member_expression")?;
    child_of_type(member, &["property_identifier"]).map(|ident| node_text(data, ident))
}

fn route_method(data: &[u8], call: Node) -> Option<&'static str> {
    let lowered = property_name(data, call_callee(call))?.to_lowercase();
    ROUTE_METHODS
        .iter()
        .copied()
        .find(|method| *method == lowered)
}

fn literal_text(data: &[u8], node: Node) -> Option<String> {
    match node.kind() {
        "string" => Some(
            child_nodes(node)
                .into_iter()
                .filter(|child| child.kind() == "string_fragment")
                .map(|child| node_text(data, child))
                .collect(),
        ),
        "template_string" => node_text(data, node)
            .strip_prefix('`')?
            .strip_suffix('`')
            .map(str::to_owned),
        _ => None,
    }
}

fn string_argument(data: &[u8], call: Node) -> Option<String> {
    let arguments = child_of_type(call, &["arguments"])?;
    child_nodes(arguments)
        .into_iter()
        .filter_map(expression)
        .find_map(|expr| literal_text(data, expr))
}

/// Path from a preceding `.route("/path")` in a chained call.
fn path_from_route(data: &[u8], start: Option<Node>) -> Option<String> {
    let mut current = start?;
    loop {
        match current.kind() {
            "member_expression" => current = child_nodes(current).into_iter().next()?,
            "call_expression" => {
                let callee = call_callee(current);
                if property_name(data, callee) == Some("route") {
                    return string_argument(data, current);
                }
                current = callee?;
            }
            _ => return None,
        }
    }
}

fn callbacks(call: Node) -> Vec<Node> {
    let Some(arguments) = child_of_type(call, &["arguments"]) else {
        return Vec::new();
    };
    child_nodes(arguments)
        .into_iter()
        .filter_map(expression)
        .filter(|expr| CALLBACKS.contains(&expr.kind()))
        .collect()
}

fn collect_routes<'t>(routes: &mut Vec<Route<'t>>, data: &[u8], call: Node<'t>) {
    let Some(method) = route_method(data, call) else {
        return;
    };
    let found = callbacks(call);
    if found.is_empty() {
        return;
    }
    let path = string_argument(data, call)
        .filter(|route| !route.is_empty())
        .or_else(|| path_from_route(data, call_callee(call)));
    routes.extend(found.into_iter().map(|callback| Route {
        callback,
        method,
        path: path.clone(),
    }));
}

fn route_label(method: &str, path: Option<&str>) -> String {
    let label = method.to_uppercase();
    match path {
        Some(path) if !path.is_empty() => format!("{label} {path}"),
        _ => label,
    }
}

/// Returns `name`, or `name#N` for the lowest N >= 2 not yet used.
fn claim(used: &mut HashSet<String>, name: String) -> String {
    let mut chosen = name.clone();
    let mut number = 2;
    while used.contains(&chosen) {
        chosen = format!("{name}#{number}");
        number += 1;
    }
    used.insert(chosen.clone());
    chosen
}

fn append_routes(found: &mut Found, mut routes: Vec<Route>, module: &str) {
    routes.sort_by_key(|route| route.callback.start_byte());
    let mut used = HashSet::new();
    for route in &routes {
        let label = route_label(route.method, route.path.as_deref());
        let name = claim(&mut used, label);
        push(found, route.callback, &name, module);
    }
}

#[cfg(test)]
mod tests {
    use super::functions_in_source;
    use crate::model::Function;

    const BOX: &str = r#"
export function choose(x: number): number {
  if (x > 0 && x < 10) return 1;
  for (let i = 0; i < x; i++) {
    if (i === 2) return i;
  }
  try {
    return x;
  } catch (e) {
    return 0;
  }
  return x > 0 ? 1 : 0;
}

export class Box {
  open(flag?: boolean): number {
    return flag ? 1 : 0;
  }
}

const arrow = (n: number) => (n > 0 ? n : 0);

function outer() {
  function inner(n: number) {
    if (n) return 1;
    return 0;
  }
  return inner(1);
}
"#;

    const TSX: &str = r#"
export function View(ok: boolean, ready: boolean) {
  return ok && ready ? 1 : 0;
}
"#;

    const NULLISH: &str = r#"
export function choose(a, b, c) {
  return a ?? b?.c ?? c?.() ?? c?.[0];
}
export function text() {
  return "a ?? b?.c";
}
"#;

    const ROUTES: &str = r#"
export function mount(app) {
  const extra = [1].map((n) => (n ? 1 : 0));
  app.get("/users", (req, res) => {
    if (req.query.q) return 1;
    return 0;
  });
  app.post("/users", function (req, res) {
    return req.body ?? {};
  });
  app.use((req, res, next) => next());
  app.route("/items").get((req, res) => res.send(1)).post((req, res) => res.send(2));
  return extra;
}
app.get("/health", async (req, res) => res.send(req.query.ok ?? "no"));
app.get("/users", (req, res, next) => next(), (req, res) => res.send(req.id ?? 0));
app.delete(`/users/${id}`, ((req, res) => res.send(1)));
"#;

    const NESTED: &str = r#"
function outer() {
  const values = [1].map((n) => (n ? 1 : 0));
  return values;
}
"#;

    const JS: &str = r#"
export function choose(a, b, c) {
  return a ?? b?.c ?? c?.() ?? c?.[0];
}
app.get("/users", (req, res) => req.id ?? 0);
"#;

    fn entries(functions: &[Function]) -> Vec<(&str, &str)> {
        functions
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str()))
            .collect()
    }

    fn names(functions: &[Function]) -> Vec<&str> {
        functions.iter().map(|f| f.name.as_str()).collect()
    }

    fn lines(functions: &[Function]) -> Vec<(usize, usize)> {
        functions
            .iter()
            .map(|f| (f.start_line, f.end_line))
            .collect()
    }

    fn text_of<'a>(source: &'a str, function: &Function) -> &'a str {
        &source[function.start_byte.unwrap_or(0)..function.end_byte.unwrap_or(0)]
    }

    /// MU-9: functions, class methods (`module.Class`), and arrow declarators are separate
    /// entries with their line spans; nested functions stay inside their parent.
    #[test]
    fn mu_9_function_class_method_and_arrow_are_separate_entries() {
        let functions = functions_in_source(BOX, "src/demo/box.ts", Some("/proj"));
        assert_eq!(
            entries(&functions),
            [
                ("demo.box", "choose"),
                ("demo.box.Box", "open"),
                ("demo.box", "arrow"),
                ("demo.box", "outer"),
            ]
        );
        assert_eq!(lines(&functions), [(2, 13), (16, 18), (21, 21), (23, 29)]);
    }

    /// MU-9: byte spans cover the function, method, and arrow declarator nodes exactly.
    #[test]
    fn mu_9_byte_spans_cover_the_node_text() {
        let functions = functions_in_source(BOX, "src/demo/box.ts", Some("/proj"));
        let choose = text_of(BOX, &functions[0]);
        assert!(choose.starts_with("function choose(x: number): number {"));
        assert!(choose.ends_with("return x > 0 ? 1 : 0;\n}"));
        assert_eq!(
            text_of(BOX, &functions[1]),
            "open(flag?: boolean): number {\n    return flag ? 1 : 0;\n  }"
        );
        assert_eq!(
            text_of(BOX, &functions[2]),
            "arrow = (n: number) => (n > 0 ? n : 0)"
        );
    }

    /// MU-9: a .tsx file is parsed with the TSX grammar and keeps its module namespace.
    #[test]
    fn mu_9_tsx_file_is_parsed_and_namespaced() {
        let functions = functions_in_source(TSX, "src/ui/view.tsx", Some("/proj"));
        assert_eq!(entries(&functions), [("ui.view", "View")]);
        assert_eq!(lines(&functions), [(2, 4)]);
    }

    /// MU-9: nullish and optional-chain bodies are entries; string text is not code.
    #[test]
    fn mu_9_nullish_and_optional_chain_functions_are_found() {
        let functions = functions_in_source(NULLISH, "src/demo/box.ts", Some("/proj"));
        assert_eq!(names(&functions), ["choose", "text"]);
        assert_eq!(lines(&functions), [(2, 4), (5, 7)]);
    }

    /// MU-9: Express handlers are entries named by method and path; the parent keeps its span.
    #[test]
    fn mu_9_express_handlers_are_entries_and_leave_the_parent() {
        let functions = functions_in_source(ROUTES, "src/demo/routes.ts", Some("/proj"));
        assert_eq!(
            names(&functions),
            [
                "mount",
                "GET /users",
                "POST /users",
                "USE",
                "GET /items",
                "POST /items",
                "GET /health",
                "GET /users#2",
                "GET /users#3",
                "DELETE /users/${id}",
            ]
        );
        assert!(functions.iter().all(|f| f.namespace == "demo.routes"));
        assert_eq!(
            lines(&functions),
            [
                (2, 14),
                (4, 7),
                (8, 10),
                (11, 11),
                (12, 12),
                (12, 12),
                (15, 15),
                (16, 16),
                (16, 16),
                (17, 17),
            ]
        );
    }

    /// MU-9: a route callback span is the callback node, without the call or its parentheses.
    #[test]
    fn mu_9_route_callback_spans_exclude_the_call() {
        let functions = functions_in_source(ROUTES, "src/demo/routes.ts", Some("/proj"));
        assert_eq!(
            text_of(ROUTES, &functions[1]),
            "(req, res) => {\n    if (req.query.q) return 1;\n    return 0;\n  }"
        );
        assert_eq!(text_of(ROUTES, &functions[9]), "(req, res) => res.send(1)");
    }

    /// MU-9: a nested callback that is not a route stays inside its enclosing function.
    #[test]
    fn mu_9_nested_callback_that_is_not_a_route_stays_inside_its_function() {
        let functions = functions_in_source(NESTED, "src/demo/box.ts", Some("/proj"));
        assert_eq!(entries(&functions), [("demo.box", "outer")]);
        assert_eq!(lines(&functions), [(2, 5)]);
    }

    /// MU-9: .mjs and .js files use the JavaScript grammar with the same rules.
    #[test]
    fn mu_9_javascript_files_use_the_same_rules() {
        let mjs = functions_in_source(JS, "src/demo/app.mjs", Some("/proj"));
        assert_eq!(
            entries(&mjs),
            [("demo.app", "choose"), ("demo.app", "GET /users")]
        );
        assert_eq!(lines(&mjs), [(2, 4), (5, 5)]);
        let js = functions_in_source(JS, "src/demo/app.js", Some("/proj"));
        assert_eq!(names(&js), ["choose", "GET /users"]);
    }

    /// MU-9: a route with no string path is its bare method; repeats claim `#2`, `#3`.
    #[test]
    fn mu_9_route_without_path_is_bare_method_and_duplicates_claim_suffixes() {
        let source = r#"
app.get((req, res) => res.send(1));
app.get((req, res) => 1);
app.get((req, res) => 2);
"#;
        let functions = functions_in_source(source, "src/demo/bare.js", None);
        assert_eq!(names(&functions), ["GET", "GET#2", "GET#3"]);
    }

    /// MU-9: the module namespace strips the project root, the src/ dir, and the script suffix.
    #[test]
    fn mu_9_namespace_strips_root_src_and_script_suffix() {
        let source = "\nexport function f() {\n  return 1;\n}\n";
        let cases = [
            ("/proj/src/demo/box.ts", Some("/proj"), "demo.box"),
            ("lib/util.ts", None, "lib.util"),
            ("./src/a.tsx", None, "a"),
            ("src/x.mjs", None, "x"),
            ("src\\demo\\box.ts", None, "demo.box"),
            ("pkg/src/inner/m.cts", Some("."), "inner.m"),
        ];
        for (path, root, expected) in cases {
            let functions = functions_in_source(source, path, root);
            assert_eq!(entries(&functions), [(expected, "f")], "path {path}");
        }
    }

    /// MU-9: an object-literal method outside any class is not an entry.
    #[test]
    fn mu_9_object_methods_outside_a_class_are_not_entries() {
        let functions = functions_in_source(
            "\nconst o = { m() { return 1; } };\n",
            "src/demo/obj.ts",
            Some("/proj"),
        );
        assert!(functions.is_empty());
    }
}
