// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/typescript.py

//! TypeScript, TSX, and JavaScript functions, methods, and complexity.
//!
//! Top-level functions, class methods, top-level arrow functions and function
//! expressions in `var`, `let`, or `const`, methods of top-level object literals,
//! class fields that hold arrow functions, and unnamed `export default` functions
//! are entries. An inline Express route callback is its own entry, named like
//! `GET /users`. Other nested callbacks stay inside the enclosing function.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::languages::ts::{
    binary_logic, child_of_type, claim, complexity, descendants, end_line, node_text, parse,
    start_line, Grammar,
};
use crate::model::Function;

const DECISION_TYPES: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "ternary_expression",
    "switch_case",
    "switch_default",
];
const SKIP_TYPES: &[&str] = &[
    "class_declaration",
    "abstract_class_declaration",
    "interface_declaration",
];
const CLASS_TYPES: &[&str] = &["class_declaration", "abstract_class_declaration"];
const DECLARATION_TYPES: &[&str] = &["lexical_declaration", "variable_declaration"];
const DEFAULT_FUNCTION_TYPES: &[&str] = &[
    "function_declaration",
    "function_expression",
    "generator_function",
    "generator_function_declaration",
];
const FUNCTION_TYPES: &[&str] = &[
    "function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
];
const CALLBACK_TYPES: &[&str] = &["arrow_function", "function_expression"];
const ROUTE_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "all", "use",
];
const JS_SUFFIXES: &[&str] = &[".jsx", ".mjs", ".cjs", ".js"];
const SCRIPT_SUFFIXES: &[&str] = &[".tsx", ".mts", ".cts", ".jsx", ".mjs", ".cjs", ".ts", ".js"];

struct Route<'t> {
    start: usize,
    callback: Node<'t>,
    method: &'static str,
    path: Option<String>,
}

/// Functions, methods, and route callbacks of one TypeScript, TSX, or
/// JavaScript source file, in source order.
pub fn functions_in_source(source: &str, path: &str, project_root: &str) -> Vec<Function> {
    let (data, tree) = parse(source, grammar(path));
    let module = module_namespace(path, project_root);
    let mut found = Vec::new();
    let mut routes = Vec::new();
    for node in descendants(tree.root_node()) {
        match node.kind() {
            "function_declaration" => append_function(&mut found, &data, node, &module, path),
            "method_definition" => append_method(&mut found, &data, node, &module, path),
            "lexical_declaration" | "variable_declaration" => {
                append_arrows(&mut found, &data, node, &module, path)
            }
            "public_field_definition" | "field_definition" => {
                append_field(&mut found, &data, node, &module, path)
            }
            "export_statement" => append_default_export(&mut found, &data, node, &module, path),
            "call_expression" => collect_routes(&mut routes, &data, node),
            _ => {}
        }
    }
    append_routes(&mut found, routes, &data, &module, path);
    found.sort_by_key(|(start, _)| *start);
    found.into_iter().map(|(_, function)| function).collect()
}

fn grammar(path: &str) -> Grammar {
    if path.ends_with(".tsx") {
        Grammar::Tsx
    } else if JS_SUFFIXES.iter().any(|suffix| path.ends_with(suffix)) {
        Grammar::JavaScript
    } else {
        Grammar::TypeScript
    }
}

fn module_namespace(path: &str, project_root: &str) -> String {
    let relative = path.replace('\\', "/");
    let relative = strip_root_prefix(&relative, project_root);
    let relative = strip_src_dir(relative);
    strip_script_suffix(relative).replace('/', ".")
}

fn strip_root_prefix<'a>(relative: &'a str, project_root: &str) -> &'a str {
    let root = project_root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    if !root.is_empty() && root != "." {
        if let Some(rest) = relative
            .strip_prefix(root)
            .and_then(|r| r.strip_prefix('/'))
        {
            return rest;
        }
    }
    relative.strip_prefix("./").unwrap_or(relative)
}

fn strip_src_dir(relative: &str) -> &str {
    if let Some(rest) = relative.strip_prefix("src/") {
        return rest;
    }
    relative
        .split_once("/src/")
        .map_or(relative, |(_, rest)| rest)
}

fn strip_script_suffix(relative: &str) -> &str {
    SCRIPT_SUFFIXES
        .iter()
        .find_map(|suffix| relative.strip_suffix(suffix))
        .unwrap_or(relative)
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    std::iter::successors(node.parent(), |parent| parent.parent())
}

fn inside_function(node: Node<'_>) -> bool {
    ancestors(node).any(|ancestor| FUNCTION_TYPES.contains(&ancestor.kind()))
}

fn has_body(node: Node<'_>) -> bool {
    child_of_type(node, &["statement_block"]).is_some()
}

fn class_names<'a>(data: &'a [u8], node: Node<'_>) -> Vec<&'a str> {
    let mut names: Vec<&str> = ancestors(node)
        .filter(|ancestor| CLASS_TYPES.contains(&ancestor.kind()))
        .filter_map(|class| child_of_type(class, &["type_identifier", "identifier"]))
        .map(|ident| node_text(data, ident))
        .collect();
    names.reverse();
    names
}

fn expression(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(node);
    while let Some(parenthesized) = current {
        if parenthesized.kind() != "parenthesized_expression" {
            break;
        }
        current = children(parenthesized)
            .into_iter()
            .find(|child| !matches!(child.kind(), "(" | ")"));
    }
    current
}

fn call_callee(call: Node<'_>) -> Option<Node<'_>> {
    children(call)
        .into_iter()
        .find(|child| child.kind() != "arguments")
}

fn property_name<'a>(data: &'a [u8], node: Option<Node<'_>>) -> Option<&'a str> {
    let member = node.filter(|member| member.kind() == "member_expression")?;
    let ident = child_of_type(member, &["property_identifier"])?;
    Some(node_text(data, ident))
}

fn route_method(data: &[u8], call: Node<'_>) -> Option<&'static str> {
    let name = property_name(data, call_callee(call))?.to_lowercase();
    ROUTE_METHODS.iter().copied().find(|method| *method == name)
}

fn literal_text(data: &[u8], node: Node<'_>) -> Option<String> {
    match node.kind() {
        "string" => Some(
            children(node)
                .into_iter()
                .filter(|child| child.kind() == "string_fragment")
                .map(|child| node_text(data, child))
                .collect(),
        ),
        "template_string" => template_body(node_text(data, node)).map(str::to_string),
        _ => None,
    }
}

fn template_body(text: &str) -> Option<&str> {
    if text.len() >= 2 && text.starts_with('`') && text.ends_with('`') {
        Some(&text[1..text.len() - 1])
    } else {
        None
    }
}

fn string_argument(data: &[u8], call: Node<'_>) -> Option<String> {
    let arguments = child_of_type(call, &["arguments"])?;
    children(arguments)
        .into_iter()
        .filter_map(expression)
        .find_map(|expr| literal_text(data, expr))
}

/// Path from a preceding `.route("/path")` in a chained call.
fn path_from_route(data: &[u8], node: Option<Node<'_>>) -> Option<String> {
    let mut current = node;
    while let Some(link) = current {
        match link.kind() {
            "member_expression" => current = children(link).into_iter().next(),
            "call_expression" => {
                let callee = call_callee(link);
                if property_name(data, callee) == Some("route") {
                    return string_argument(data, link);
                }
                current = callee;
            }
            _ => return None,
        }
    }
    None
}

fn callbacks(call: Node<'_>) -> Vec<Node<'_>> {
    let Some(arguments) = child_of_type(call, &["arguments"]) else {
        return Vec::new();
    };
    children(arguments)
        .into_iter()
        .filter_map(expression)
        .filter(|expr| CALLBACK_TYPES.contains(&expr.kind()))
        .collect()
}

fn is_route_callback(data: &[u8], node: Node<'_>) -> bool {
    if !CALLBACK_TYPES.contains(&node.kind()) {
        return false;
    }
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() != "parenthesized_expression" {
            break;
        }
        current = parent.parent();
    }
    let Some(arguments) = current.filter(|parent| parent.kind() == "arguments") else {
        return false;
    };
    let Some(call) = arguments
        .parent()
        .filter(|parent| parent.kind() == "call_expression")
    else {
        return false;
    };
    route_method(data, call).is_some()
}

fn is_decision(node: Node<'_>) -> bool {
    let kind = node.kind();
    if DECISION_TYPES.contains(&kind) || binary_logic(node) {
        return true;
    }
    if kind == "binary_expression" && child_of_type(node, &["??"]).is_some() {
        return true;
    }
    // `a?.b` and `a?.[0]` wrap `?.` in optional_chain: count the wrapper once.
    // A bare `?.` (as in TypeScript `a?.()`) counts only when nothing wraps it.
    if kind == "optional_chain" {
        return true;
    }
    kind == "?."
        && !node
            .parent()
            .is_some_and(|parent| parent.kind() == "optional_chain")
}

fn complexity_of(data: &[u8], node: Node<'_>) -> u32 {
    let root = node.id();
    complexity(node, is_decision, |current| {
        current.id() != root
            && (SKIP_TYPES.contains(&current.kind()) || is_route_callback(data, current))
    })
}

fn entry(
    node: Node<'_>,
    name: &str,
    namespace: &str,
    complexity: u32,
    path: &str,
) -> (usize, Function) {
    (
        node.start_byte(),
        Function {
            name: name.to_string(),
            namespace: namespace.to_string(),
            complexity,
            start_line: start_line(node),
            end_line: end_line(node),
            path: path.to_string(),
            language: "typescript".to_string(),
            jacoco_class: None,
        },
    )
}

fn append_function(
    found: &mut Vec<(usize, Function)>,
    data: &[u8],
    node: Node<'_>,
    module: &str,
    path: &str,
) {
    if inside_function(node) || !has_body(node) {
        return;
    }
    let Some(ident) = child_of_type(node, &["identifier"]) else {
        return;
    };
    let complexity = complexity_of(data, node);
    found.push(entry(
        node,
        node_text(data, ident),
        module,
        complexity,
        path,
    ));
}

fn append_method(
    found: &mut Vec<(usize, Function)>,
    data: &[u8],
    node: Node<'_>,
    module: &str,
    path: &str,
) {
    if !has_body(node) {
        return;
    }
    let Some(ident) = child_of_type(node, &["property_identifier", "identifier"]) else {
        return;
    };
    let Some(namespace) = member_namespace(data, node, module) else {
        return;
    };
    push_member(found, data, node, ident, &namespace, node, path);
}

fn append_arrows(
    found: &mut Vec<(usize, Function)>,
    data: &[u8],
    node: Node<'_>,
    module: &str,
    path: &str,
) {
    if inside_function(node) {
        return;
    }
    for declarator in children(node) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        let ident = child_of_type(declarator, &["identifier"]);
        let value = child_of_type(declarator, &["arrow_function", "function_expression"]);
        let (Some(ident), Some(value)) = (ident, value) else {
            continue;
        };
        let complexity = complexity_of(data, value);
        found.push(entry(
            declarator,
            node_text(data, ident),
            module,
            complexity,
            path,
        ));
    }
}

fn push_member(
    found: &mut Vec<(usize, Function)>,
    data: &[u8],
    node: Node<'_>,
    name: Node<'_>,
    namespace: &str,
    body: Node<'_>,
    path: &str,
) {
    let complexity = complexity_of(data, body);
    found.push(entry(
        node,
        node_text(data, name),
        namespace,
        complexity,
        path,
    ));
}

fn append_field(
    found: &mut Vec<(usize, Function)>,
    data: &[u8],
    node: Node<'_>,
    module: &str,
    path: &str,
) {
    if inside_function(node) {
        return;
    }
    let Some(value) = child_of_type(node, CALLBACK_TYPES) else {
        return;
    };
    let Some(ident) = child_of_type(node, &["property_identifier"]) else {
        return;
    };
    let Some(namespace) = class_namespace(data, node, module) else {
        return;
    };
    push_member(found, data, node, ident, &namespace, value, path);
}

fn append_default_export(
    found: &mut Vec<(usize, Function)>,
    data: &[u8],
    node: Node<'_>,
    module: &str,
    path: &str,
) {
    if child_of_type(node, &["default"]).is_none() {
        return;
    }
    let Some(function) = child_of_type(node, DEFAULT_FUNCTION_TYPES) else {
        return;
    };
    if child_of_type(function, &["identifier"]).is_some() || !has_body(function) {
        return;
    }
    let complexity = complexity_of(data, function);
    found.push(entry(function, "default", module, complexity, path));
}

fn class_namespace(data: &[u8], node: Node<'_>, module: &str) -> Option<String> {
    let classes = class_names(data, node);
    (!classes.is_empty()).then(|| format!("{module}.{}", classes.join(".")))
}

fn member_namespace(data: &[u8], node: Node<'_>, module: &str) -> Option<String> {
    class_namespace(data, node, module).or_else(|| object_namespace(data, node, module))
}

/// The namespace of a method in a top-level object literal, such as `shapes.store`.
fn object_namespace(data: &[u8], method: Node<'_>, module: &str) -> Option<String> {
    let object = method.parent().filter(|parent| parent.kind() == "object")?;
    let declarator = object
        .parent()
        .filter(|parent| parent.kind() == "variable_declarator")?;
    let declaration = declarator
        .parent()
        .filter(|parent| DECLARATION_TYPES.contains(&parent.kind()))?;
    if inside_function(declaration) {
        return None;
    }
    let variable = child_of_type(declarator, &["identifier"])?;
    Some(format!("{module}.{}", node_text(data, variable)))
}

fn collect_routes<'t>(routes: &mut Vec<Route<'t>>, data: &[u8], call: Node<'t>) {
    let Some(method) = route_method(data, call) else {
        return;
    };
    let callbacks = callbacks(call);
    if callbacks.is_empty() {
        return;
    }
    let path = string_argument(data, call)
        .filter(|path| !path.is_empty())
        .or_else(|| path_from_route(data, call_callee(call)));
    routes.extend(callbacks.into_iter().map(|callback| Route {
        start: callback.start_byte(),
        callback,
        method,
        path: path.clone(),
    }));
}

fn route_label(method: &str, path: Option<&str>) -> String {
    let label = method.to_uppercase();
    match path.filter(|path| !path.is_empty()) {
        Some(path) => format!("{label} {path}"),
        None => label,
    }
}

fn append_routes(
    found: &mut Vec<(usize, Function)>,
    mut routes: Vec<Route<'_>>,
    data: &[u8],
    module: &str,
    path: &str,
) {
    routes.sort_by_key(|route| route.start);
    let mut used = HashSet::new();
    for route in routes {
        let label = claim(&mut used, route_label(route.method, route.path.as_deref()));
        let complexity = complexity_of(data, route.callback);
        found.push(entry(route.callback, &label, module, complexity, path));
    }
}

#[cfg(test)]
mod tests {
    use super::{functions_in_source, strip_root_prefix, template_body};
    use crate::model::Function;

    fn located(functions: &[Function]) -> Vec<(&str, &str, u32)> {
        functions
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str(), f.complexity))
            .collect()
    }

    // CR-8, CR-11
    #[test]
    fn cr_8_cr_11_function_class_method_and_arrow_are_separate_entries() {
        let source = r#"
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
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert_eq!(
            located(&functions),
            vec![
                ("demo.box", "choose", 7),
                ("demo.box.Box", "open", 2),
                ("demo.box", "arrow", 2),
                ("demo.box", "outer", 2),
            ]
        );
    }

    // CR-9, CR-11
    #[test]
    fn cr_9_cr_11_tsx_grammar_counts_jsx_logic() {
        let source = r#"
export function View(ok: boolean, ready: boolean) {
  return ok && ready ? 1 : 0;
}
"#;
        let functions = functions_in_source(source, "src/ui/view.tsx", "/proj");
        assert_eq!(located(&functions), vec![("ui.view", "View", 3)]);
    }

    // CR-11
    #[test]
    fn cr_11_nullish_and_optional_chains_are_branches() {
        let source = r#"
export function choose(a, b, c) {
  return a ?? b?.c ?? c?.() ?? c?.[0];
}
export function text() {
  return "a ?? b?.c";
}
"#;
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert_eq!(
            located(&functions),
            vec![("demo.box", "choose", 7), ("demo.box", "text", 1)]
        );
    }

    // CR-8, CR-11
    #[test]
    fn cr_8_cr_11_express_handlers_are_entries_and_leave_the_parent() {
        let source = r##"
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
"##;
        let functions = functions_in_source(source, "src/demo/routes.ts", "/proj");
        assert_eq!(
            located(&functions),
            vec![
                ("demo.routes", "mount", 2),
                ("demo.routes", "GET /users", 2),
                ("demo.routes", "POST /users", 2),
                ("demo.routes", "USE", 1),
                ("demo.routes", "GET /items", 1),
                ("demo.routes", "POST /items", 1),
                ("demo.routes", "GET /health", 2),
                ("demo.routes", "GET /users#2", 1),
                ("demo.routes", "GET /users#3", 2),
                ("demo.routes", "DELETE /users/${id}", 1),
            ]
        );
    }

    // CR-11
    #[test]
    fn cr_11_a_nested_callback_that_is_not_a_route_stays_inside_its_function() {
        let source = r#"
function outer() {
  const values = [1].map((n) => (n ? 1 : 0));
  return values;
}
"#;
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert_eq!(located(&functions), vec![("demo.box", "outer", 2)]);
    }

    // CR-9, CR-8
    #[test]
    fn cr_9_cr_8_javascript_files_use_the_same_rules() {
        let source = r#"
export function choose(a, b, c) {
  return a ?? b?.c ?? c?.() ?? c?.[0];
}
app.get("/users", (req, res) => req.id ?? 0);
"#;
        let functions = functions_in_source(source, "src/demo/app.mjs", "/proj");
        assert_eq!(
            located(&functions),
            vec![("demo.app", "choose", 7), ("demo.app", "GET /users", 2)]
        );
        let js = functions_in_source(source, "src/demo/app.js", "/proj");
        assert_eq!(
            located(&js),
            vec![("demo.app", "choose", 7), ("demo.app", "GET /users", 2)]
        );
    }

    // CR-8: strip_root_prefix removes a project root only when it is a real prefix followed by "/"; "" and "." never strip.
    #[test]
    fn cr_8_strip_root_prefix_strips_only_a_real_root() {
        assert_eq!(strip_root_prefix("/x/y.ts", ""), "/x/y.ts");
        assert_eq!(strip_root_prefix("./x/y.ts", ""), "x/y.ts");
        assert_eq!(strip_root_prefix("./x/y.ts", "."), "x/y.ts");
        assert_eq!(strip_root_prefix("src/a.ts", "src"), "a.ts");
        assert_eq!(strip_root_prefix("src/a.ts", "src/"), "a.ts");
        assert_eq!(strip_root_prefix("src/a.ts", "src\\"), "a.ts");
        assert_eq!(strip_root_prefix("srcx/a.ts", "src"), "srcx/a.ts");
        assert_eq!(strip_root_prefix("other/a.ts", "src"), "other/a.ts");
    }

    // CR-11: a template literal body needs at least two characters, and both ends must be backticks.
    #[test]
    fn cr_11_template_body_needs_both_backticks_and_two_characters() {
        assert_eq!(template_body("``"), Some(""));
        assert_eq!(template_body("`/users`"), Some("/users"));
        assert_eq!(template_body("`"), None);
        assert_eq!(template_body("`a"), None);
        assert_eq!(template_body("a`"), None);
        assert_eq!(template_body("a"), None);
    }

    // CR-11: a callback bound to a name is not a route, so its branches stay in the enclosing function.
    #[test]
    fn cr_11_a_callback_bound_to_a_name_is_not_a_route() {
        let source = r#"
function outer(a) {
  const pick = (x) => { if (x) return 1; return 0; };
  return pick(a);
}
"#;
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert_eq!(located(&functions), vec![("demo.box", "outer", 2)]);
    }

    // CR-11: a callback passed to a non-route call, or to `new`, stays inside the enclosing function.
    #[test]
    fn cr_11_a_callback_passed_to_a_non_route_call_stays_inside() {
        let source = r#"
function mount(app) {
  app.listen(3000, () => { if (app) return 1; return 0; });
  const w = new Widget(() => { if (app) return 1; return 0; });
  return w;
}
"#;
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert_eq!(located(&functions), vec![("demo.box", "mount", 3)]);
    }

    const SHAPES_TS: &str = r#"
export function plain(x: number): number {
  if (x > 0) { return 1 }
  return 0
}

export var viaVar = (x: number): number => {
  if (x > 0) { return 1 }
  return 0
}

export const store = {
  objectMethod(x: number): number {
    if (x > 0) { return 1 }
    if (x < 0) { return -1 }
    return 0
  },
}

export class Box {
  method(x: number): number {
    if (x > 0) { return 1 }
    return 0
  }
  field = (x: number): number => {
    if (x > 0) { return 1 }
    if (x < 0) { return -1 }
    return 0
  }
}

export var legacy = function (x: number): number {
  if (x > 0) { return 1 }
  return 0
}

export default function (x: number): number {
  if (x > 0) { return 1 }
  return 0
}
"#;

    const SHAPES_JS: &str = r#"
export function plain(x) {
  if (x > 0) { return 1 }
  return 0
}

export var viaVar = (x) => {
  if (x > 0) { return 1 }
  return 0
}

export const store = {
  objectMethod(x) {
    if (x > 0) { return 1 }
    if (x < 0) { return -1 }
    return 0
  },
}

export class Box {
  method(x) {
    if (x > 0) { return 1 }
    return 0
  }
  field = (x) => {
    if (x > 0) { return 1 }
    if (x < 0) { return -1 }
    return 0
  }
}

export var legacy = function (x) {
  if (x > 0) { return 1 }
  return 0
}

export default function (x) {
  if (x > 0) { return 1 }
  return 0
}
"#;

    fn shapes_entries() -> Vec<(&'static str, &'static str, u32)> {
        vec![
            ("shapes", "plain", 2),
            ("shapes", "viaVar", 2),
            ("shapes.store", "objectMethod", 3),
            ("shapes.Box", "method", 2),
            ("shapes.Box", "field", 3),
            ("shapes", "legacy", 2),
            ("shapes", "default", 2),
        ]
    }

    // CR-21, AC-30: the file of issue 20 gives seven entries in TypeScript.
    #[test]
    fn cr_21_ac_30_issue_20_shapes_file_gives_seven_entries_in_typescript() {
        let functions = functions_in_source(SHAPES_TS, "src/shapes.ts", "/proj");
        assert_eq!(located(&functions), shapes_entries());
    }

    // CR-21, AC-30: the same file without types gives the same seven entries in JavaScript.
    #[test]
    fn cr_21_ac_30_issue_20_shapes_file_gives_seven_entries_in_javascript() {
        let functions = functions_in_source(SHAPES_JS, "src/shapes.js", "/proj");
        assert_eq!(located(&functions), shapes_entries());
    }

    // CR-21: a var, an object method, a class field, or a default export inside a function is not its own entry.
    #[test]
    fn cr_21_shapes_inside_a_function_stay_inside_it() {
        let source = r#"
function outer(a) {
  var inner = (x) => { if (x) return 1; return 0; };
  const holder = { member(y) { if (y) return 1; return 0; } };
  class Local { field = (z) => { if (z) return 1; return 0; } }
  return inner(a) + holder.member(a);
}
"#;
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert_eq!(located(&functions), vec![("demo.box", "outer", 3)]);
    }

    // CR-21: a var declaration holding no function, and a field holding no function, make no entry.
    #[test]
    fn cr_21_a_var_or_field_that_holds_no_function_makes_no_entry() {
        let source = "var n = 1;\nclass A { size = 3; static label = 'a'; }\n";
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert!(functions.is_empty(), "{functions:?}");
    }

    // CR-21: let and const object literals name their methods the same way as var.
    #[test]
    fn cr_21_let_and_const_object_literals_hold_methods() {
        let source =
            "let a = { one() { if (1) return 1; return 0; } };\nvar b = { two() { return 1; } };\n";
        let functions = functions_in_source(source, "src/demo/box.ts", "/proj");
        assert_eq!(
            located(&functions),
            vec![("demo.box.a", "one", 2), ("demo.box.b", "two", 1)]
        );
    }
}
