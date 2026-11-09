// Derived from dryer by Robert C. Martin (https://github.com/unclebob/dryer), BSD Zero Clause License.
// Ports src/dryer/extract.py: picks the forms each language compares and turns them into entries.

use tree_sitter::Node;

use crate::astnorm;
use crate::clojure;
use crate::model::{Entry, Language};
use crate::shape::{self, Interner};
use crate::treesitter::{self, child_of_type, descendants, end_line, node_text, start_line};

const TS_FUNCTION: &[&str] = &[
    "function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
];
const DEFINITION_PARENTS: &[&str] = &[
    "variable_declarator",
    "public_field_definition",
    "property_definition",
];
const JAVA_NESTED: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "lambda_expression",
];

fn typescript_grammar(path: &str) -> &'static str {
    if path.ends_with(".tsx") {
        "tsx"
    } else {
        "typescript"
    }
}

fn grammar_for(language: Language, path: &str) -> &'static str {
    match language {
        Language::Java => "java",
        Language::Go => "go",
        Language::Python => "python",
        Language::Rust => "rust",
        Language::TypeScript => typescript_grammar(path),
        // Clojure is read by clojure::read_source and never reaches the tree-sitter path.
        Language::Clojure => "clojure",
    }
}

fn has(node: Node, types: &[&str]) -> bool {
    child_of_type(node, types).is_some()
}

fn inside(node: Node, types: &[&str]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if types.contains(&ancestor.kind()) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

/// A function inside another function. A class in between keeps the method.
fn python_nested(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "class_definition" => return false,
            "function_definition" => return true,
            _ => {}
        }
        current = ancestor.parent();
    }
    false
}

fn is_tests_module(data: &[u8], node: Node) -> bool {
    node.kind() == "mod_item"
        && child_of_type(node, &["identifier"])
            .is_some_and(|ident| node_text(data, ident) == "tests")
}

fn rust_in_tests(data: &[u8], node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if is_tests_module(data, ancestor) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

/// Preorder walk that stops at each method declaration, so methods nested in methods are never reached.
fn java_nodes(root: Node) -> Vec<Node> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "method_declaration" {
            if has(node, &["block"]) && !inside(node, JAVA_NESTED) {
                found.push(node);
            }
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    found
}

fn go_nodes(root: Node) -> Vec<Node> {
    descendants(root)
        .into_iter()
        .filter(|node| {
            matches!(node.kind(), "function_declaration" | "method_declaration")
                && has(*node, &["block"])
        })
        .collect()
}

fn python_nodes(root: Node) -> Vec<Node> {
    descendants(root)
        .into_iter()
        .filter(|node| node.kind() == "function_definition" && !python_nested(*node))
        .collect()
}

fn keep_typescript_function(node: Node) -> bool {
    has(node, &["statement_block"]) && !inside(node, TS_FUNCTION)
}

/// The declaration that names an arrow or function expression, when it is not nested in a function.
fn typescript_parent(node: Node) -> Option<Node> {
    let parent = node.parent()?;
    if !DEFINITION_PARENTS.contains(&parent.kind()) || inside(parent, TS_FUNCTION) {
        return None;
    }
    Some(parent)
}

fn typescript_nodes(root: Node) -> Vec<Node> {
    let mut found = Vec::new();
    for node in descendants(root) {
        match node.kind() {
            "function_declaration" | "method_definition" => {
                if keep_typescript_function(node) {
                    found.push(node);
                }
            }
            "arrow_function" | "function_expression" => {
                if let Some(parent) = typescript_parent(node) {
                    found.push(parent);
                }
            }
            _ => {}
        }
    }
    found
}

fn rust_node_kept(data: &[u8], node: Node) -> bool {
    node.kind() == "function_item"
        && has(node, &["block"])
        && !node.parent().is_some_and(|parent| parent.kind() == "block")
        && !rust_in_tests(data, node)
}

fn rust_nodes<'a>(data: &[u8], root: Node<'a>) -> Vec<Node<'a>> {
    descendants(root)
        .into_iter()
        .filter(|node| rust_node_kept(data, *node))
        .collect()
}

fn pick_nodes<'a>(language: Language, data: &[u8], root: Node<'a>) -> Vec<Node<'a>> {
    match language {
        Language::Java => java_nodes(root),
        Language::Go => go_nodes(root),
        Language::Python => python_nodes(root),
        Language::TypeScript => typescript_nodes(root),
        Language::Rust => rust_nodes(data, root),
        Language::Clojure => Vec::new(),
    }
}

fn tree_entry(
    language: Language,
    data: &[u8],
    node: Node,
    file: &str,
    interner: &mut Interner,
) -> Option<Entry> {
    let normalized = astnorm::normalize(node, data)?;
    Some(Entry {
        language,
        file: file.to_string(),
        start_line: start_line(node),
        end_line: end_line(node),
        nodes: shape::node_count(&normalized),
        fingerprints: shape::fingerprints(&normalized, interner),
    })
}

fn tree_entries(
    language: Language,
    path: &str,
    source: &str,
    file: &str,
    interner: &mut Interner,
) -> Vec<Entry> {
    let data = source.as_bytes();
    let tree = treesitter::parse(grammar_for(language, path), data);
    let mut entries = Vec::new();
    for node in pick_nodes(language, data, tree.root_node()) {
        entries.extend(tree_entry(language, data, node, file, interner));
    }
    entries
}

fn clojure_entry(form: &clojure::Form, file: &str, interner: &mut Interner) -> Entry {
    let normalized = clojure::normalize(form);
    Entry {
        language: Language::Clojure,
        file: file.to_string(),
        start_line: form.line(),
        end_line: clojure::max_line(form),
        nodes: shape::node_count(&normalized),
        fingerprints: shape::fingerprints(&normalized, interner),
    }
}

fn clojure_entries(
    source: &str,
    file: &str,
    interner: &mut Interner,
) -> (Vec<Entry>, Option<String>) {
    let (forms, warning) = clojure::read_source(source);
    let mut entries = Vec::new();
    for form in forms.iter().filter(|form| clojure::is_candidate_form(form)) {
        entries.push(clojure_entry(form, file, interner));
    }
    (entries, warning)
}

pub fn entries_in_source(
    language: Language,
    source: &str,
    path: &str,
    file: &str,
    interner: &mut Interner,
) -> (Vec<Entry>, Option<String>) {
    match language {
        Language::Clojure => clojure_entries(source, file, interner),
        _ => (tree_entries(language, path, source, file, interner), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(language: Language, path: &str, source: &str) -> Vec<(usize, usize)> {
        let mut interner = Interner::new();
        let (entries, warning) = entries_in_source(language, source, path, path, &mut interner);
        assert_eq!(warning, None);
        entries
            .iter()
            .map(|entry| (entry.start_line, entry.end_line))
            .collect()
    }

    // DR-11: Clojure candidates are every top-level form except ns.
    #[test]
    fn dr11_clojure_compares_every_top_level_form_except_ns() {
        let source = r#"(ns demo)
(defn alpha [x]
  #?(:cljs (inc x) :clj (dec x)))
(println "hi")
"#;
        assert_eq!(
            spans(Language::Clojure, "a.clj", source),
            vec![(2, 3), (4, 4)]
        );
    }

    // DR-11: reader-conditional branches other than :clj are dropped from the structure.
    #[test]
    fn dr11_clojure_drops_reader_conditional_branches_other_than_clj() {
        let mut interner = Interner::new();
        let conditional = "(defn alpha [x]\n  #?(:cljs (inc x) :clj (dec x)))\n";
        let plain = "(defn alpha [x]\n  (dec x))\n";
        let (with_conditional, warning) = entries_in_source(
            Language::Clojure,
            conditional,
            "a.cljc",
            "a.cljc",
            &mut interner,
        );
        let (without, _) =
            entries_in_source(Language::Clojure, plain, "a.cljc", "a.cljc", &mut interner);
        assert_eq!(warning, None);
        assert_eq!(with_conditional[0].nodes, without[0].nodes);
        assert_eq!(with_conditional[0].fingerprints, without[0].fingerprints);
    }

    // DR-11: Java candidates are methods with a body; abstract methods, constructors,
    // and methods of an anonymous class inside a method are left out.
    #[test]
    fn dr11_java_compares_methods_with_a_body_only() {
        let source = r#"abstract class Left {
  abstract int declared(int[] xs);
  Left() { }
  int alpha(int[] xs) {
    Runnable r = new Runnable() {
      public void run() { }
    };
    return xs.length;
  }
  int beta(int[] ys) {
    return ys.length;
  }
}
"#;
        assert_eq!(
            spans(Language::Java, "a.java", source),
            vec![(4, 9), (10, 12)]
        );
    }

    // DR-11: Go candidates are functions and methods with a body; a function literal stays inside.
    #[test]
    fn dr11_go_compares_functions_and_methods_with_a_body_only() {
        let source = r#"package demo

func declared(xs []int)

func alpha(xs []int) int {
    f := func() int { return 1 }
    return f() + len(xs)
}

func (b *Board) beta() int {
    return 2
}
"#;
        assert_eq!(spans(Language::Go, "a.go", source), vec![(5, 8), (10, 12)]);
    }

    // DR-11: TypeScript candidates are functions, methods, and top-level arrow functions;
    // callbacks nested inside another function are excluded.
    #[test]
    fn dr11_typescript_compares_functions_methods_and_top_level_arrows() {
        let source = r#"export function alpha(xs: number[]): number[] {
  return xs;
}
export class Box {
  run(ys: number[]): number[] {
    return ys;
  }
}
export const beta = (zs: number[]): number[] => {
  return zs;
};
xs.map((item: number) => item + 1);
export function outer(ws: number[]): number[] {
  const cb = (vs: number[]) => vs;
  return ws.map(cb);
}
"#;
        assert_eq!(
            spans(Language::TypeScript, "a.ts", source),
            vec![(1, 3), (5, 7), (9, 11), (13, 16)]
        );
    }

    // DR-5, DR-8: a .tsx path selects the tsx grammar, so JSX parses inside a component arrow.
    #[test]
    fn dr5_dr8_tsx_path_selects_the_tsx_grammar() {
        let source =
            "export const View = (xs: number[]) => {\n  return <div>{xs.length}</div>;\n};\n";
        let mut interner = Interner::new();
        let (entries, warning) = entries_in_source(
            Language::TypeScript,
            source,
            "ui/view.tsx",
            "ui/view.tsx",
            &mut interner,
        );
        assert_eq!(warning, None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].language, Language::TypeScript);
        assert_eq!(entries[0].file, "ui/view.tsx");
        assert_eq!((entries[0].start_line, entries[0].end_line), (1, 3));
    }

    // DR-11: Rust candidates are functions and methods with a body; functions in mod tests,
    // closures, and functions nested in a function body are excluded.
    #[test]
    fn dr11_rust_compares_fns_and_methods_with_a_body() {
        let source = r#"struct Board;
impl Board {
    fn alpha(&self, xs: Vec<i32>) -> usize {
        let f = |x: i32| { x + 1 };
        fn helper() -> usize { 1 }
        xs.len() + f(0) as usize + helper()
    }
}
fn beta(xs: Vec<i32>) -> usize {
    xs.len()
}
mod tests {
    fn gamma(xs: Vec<i32>) -> usize {
        xs.len()
    }
}
"#;
        assert_eq!(spans(Language::Rust, "a.rs", source), vec![(3, 7), (9, 11)]);
    }

    // DR-11: Python candidates are functions and methods; nested functions are excluded.
    #[test]
    fn dr11_python_compares_functions_and_methods_not_nested_ones() {
        let source = r#"def alpha(xs):
    def inner(ys):
        return ys
    return inner(xs)
class Board:
    def beta(self, xs):
        return xs
def outer(xs):
    class Panel:
        def gamma(self, ys):
            return ys
    return Panel
"#;
        assert_eq!(
            spans(Language::Python, "a.py", source),
            vec![(1, 4), (6, 7), (8, 12), (10, 11)]
        );
    }

    // DR-11: a method of a class inside a function is kept, because the class ends the nesting.
    #[test]
    fn dr11_python_keeps_a_method_of_a_class_inside_a_function() {
        let source = "def outer(xs):\n    class Panel:\n        def gamma(self, ys):\n            return ys\n    return Panel\n";
        assert_eq!(
            spans(Language::Python, "a.py", source),
            vec![(1, 5), (3, 4)]
        );
    }

    // DR-8: the end line of a function is its last occupied row.
    #[test]
    fn dr8_python_end_line_uses_the_last_occupied_row() {
        assert_eq!(
            spans(Language::Python, "a.py", "def alpha(xs):\n    return xs\n"),
            vec![(1, 2)]
        );
    }
}
