// Derived from dryer by Robert C. Martin (https://github.com/unclebob/dryer), BSD Zero Clause License.
//! Port of `dryer/astnorm.py`: normalize a tree-sitter subtree the way dry4clj
//! normalizes a Clojure form.
//!
//! The name in call position stays: `filter`, `Println`, `map`, an operator, the
//! type constructed by `new`. Local names, field names, type names in argument
//! position, and literals become generic markers.

use tree_sitter::Node;

use crate::model::Norm;
use crate::treesitter::{kind_static, node_text};

const SKIP: &[&str] = &[
    "comment",
    "line_comment",
    "block_comment",
    "documentation_comment",
    "doc_comment",
];

const IDENTIFIERS: &[&str] = &[
    "identifier",
    "type_identifier",
    "field_identifier",
    "property_identifier",
    "shorthand_property_identifier",
    "shorthand_property_identifier_pattern",
    "package_identifier",
];

const LITERALS: &[&str] = &[
    "string",
    "string_literal",
    "interpreted_string_literal",
    "raw_string_literal",
    "char_literal",
    "rune_literal",
    "character_literal",
    "number",
    "integer",
    "float",
    "decimal_integer_literal",
    "hex_integer_literal",
    "octal_integer_literal",
    "binary_integer_literal",
    "decimal_floating_point_literal",
    "hex_floating_point_literal",
    "int_literal",
    "float_literal",
    "imaginary_literal",
    "integer_literal",
    "true",
    "false",
    "null",
    "nil",
    "none",
    "undefined",
    "null_literal",
    "boolean_literal",
    "template_string",
    "regex",
    "regex_literal",
];

const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "==", "!=", "<", ">", "<=", ">=", "===", "!==", "&&", "||", "&",
    "|", "^", "<<", ">>", ">>>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=",
    ">>>=", "!", "~", "++", "--", "and", "or", "not", "??", "?.", "=", ":=", "=>", "?",
];

const CALLS: &[&str] = &[
    "call",
    "call_expression",
    "method_invocation",
    "object_creation_expression",
    "new_expression",
];

const ATTRIBUTES: &[&str] = &[
    "attribute",
    "member_expression",
    "selector_expression",
    "field_access",
    "field_expression",
];

const SCOPED: &[&str] = &["scoped_identifier", "scoped_type_identifier"];

const ARG_LISTS: &[&str] = &["argument_list", "arguments"];
const TYPE_ARGS: &[&str] = &["type_arguments", "type_parameters"];

/// Which rule a node is normalized under. Python modes: "norm", "callee", "path".
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Norm,
    Callee,
    Path,
}

/// Outcome of the leaf rules. `Pending` means the node needs its children assembled.
enum Leaf {
    Done(Option<Norm>),
    Pending,
}

/// Normalized tree for `node`, or `None` when the node is only punctuation or a comment.
pub fn normalize(node: Node, data: &[u8]) -> Option<Norm> {
    value(node, Mode::Norm, false, data)
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    let kids: Vec<Node<'_>> = node.children(&mut cursor).collect();
    kids
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    let kids: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    kids
}

fn push_some(out: &mut Vec<Norm>, item: Option<Norm>) {
    if let Some(item) = item {
        out.push(item);
    }
}

fn symbol_named(node: Node, data: &[u8]) -> Norm {
    Norm::List(vec![Norm::Kw("symbol"), Norm::Str(node_text(data, node))])
}

fn is_path(node: Node) -> bool {
    SCOPED.contains(&node.kind()) || node.kind() == "generic_type"
}

fn value(node: Node, mode: Mode, head: bool, data: &[u8]) -> Option<Norm> {
    match try_leaf(node, mode, head, data) {
        Leaf::Done(result) => result,
        Leaf::Pending => assemble(node, mode, head, data),
    }
}

fn try_leaf(node: Node, mode: Mode, head: bool, data: &[u8]) -> Leaf {
    match mode {
        Mode::Callee => callee_leaf(node, data),
        Mode::Path => Leaf::Pending,
        Mode::Norm => norm_leaf(node, head, data),
    }
}

fn callee_leaf(node: Node, data: &[u8]) -> Leaf {
    if IDENTIFIERS.contains(&node.kind()) {
        Leaf::Done(Some(symbol_named(node, data)))
    } else {
        Leaf::Pending
    }
}

fn norm_leaf(node: Node, head: bool, data: &[u8]) -> Leaf {
    let kind = node.kind();
    if SKIP.contains(&kind) {
        return Leaf::Done(None);
    }
    if OPERATORS.contains(&kind) {
        return Leaf::Done(Some(Norm::List(vec![
            Norm::Kw("symbol"),
            Norm::Str(kind.to_string()),
        ])));
    }
    if !node.is_named() {
        return Leaf::Done(None);
    }
    named_leaf(node, head, data)
}

fn named_leaf(node: Node, head: bool, data: &[u8]) -> Leaf {
    if IDENTIFIERS.contains(&node.kind()) {
        let result = if head {
            symbol_named(node, data)
        } else {
            Norm::Kw("symbol")
        };
        return Leaf::Done(Some(result));
    }
    if is_literal(node) {
        Leaf::Done(Some(Norm::Kw("literal")))
    } else {
        Leaf::Pending
    }
}

fn is_literal(node: Node) -> bool {
    let kind = node.kind();
    if !LITERALS.contains(&kind) {
        return false;
    }
    !(kind == "string"
        && children(node)
            .iter()
            .any(|child| child.kind() == "interpolation"))
}

fn assemble(node: Node, mode: Mode, head: bool, data: &[u8]) -> Option<Norm> {
    match mode {
        Mode::Callee => callee_value(node, data),
        Mode::Path => Some(assemble_path(node, data)),
        Mode::Norm => assemble_norm(node, head, data),
    }
}

fn assemble_norm(node: Node, head: bool, data: &[u8]) -> Option<Norm> {
    let kind = node.kind();
    if kind == "parenthesized_expression" {
        if let [only] = named_children(node).as_slice() {
            return value(*only, Mode::Norm, head, data);
        }
    }
    if CALLS.contains(&kind) {
        return Some(assemble_call(node, data));
    }
    if ATTRIBUTES.contains(&kind) {
        return Some(assemble_attr(node, head, data));
    }
    Some(assemble_children(node, data))
}

fn callee_value(node: Node, data: &[u8]) -> Option<Norm> {
    if ATTRIBUTES.contains(&node.kind()) {
        return Some(assemble_attr(node, true, data));
    }
    if is_path(node) {
        return Some(assemble_path(node, data));
    }
    value(node, Mode::Norm, true, data)
}

/// Keeps the name at the head of a call, e.g. `[:symbol "filter"]`.
fn name_value(name: Node, head: bool, data: &[u8]) -> Option<Norm> {
    if IDENTIFIERS.contains(&name.kind()) {
        return Some(if head {
            symbol_named(name, data)
        } else {
            Norm::Kw("symbol")
        });
    }
    if head {
        value(name, Mode::Callee, false, data)
    } else {
        value(name, Mode::Norm, false, data)
    }
}

fn named_before_args(node: Node<'_>) -> (Vec<Node<'_>>, Option<Node<'_>>) {
    let mut incoming = Vec::new();
    for child in named_children(node) {
        if ARG_LISTS.contains(&child.kind()) {
            return (incoming, Some(child));
        }
        incoming.push(child);
    }
    (incoming, None)
}

/// Splits nodes into (non-type-argument nodes, type-argument nodes), keeping order.
fn split_type_args(nodes: Vec<Node<'_>>) -> (Vec<Node<'_>>, Vec<Node<'_>>) {
    nodes
        .into_iter()
        .partition(|child| !TYPE_ARGS.contains(&child.kind()))
}

fn assemble_call(node: Node, data: &[u8]) -> Norm {
    let (incoming, args) = named_before_args(node);
    let (rest, type_args) = split_type_args(incoming);
    let mut out = vec![Norm::Kw(kind_static(node))];
    if let Some((callee, receivers)) = rest.split_last() {
        for child in receivers {
            push_some(&mut out, value(*child, Mode::Norm, false, data));
        }
        push_some(&mut out, value(*callee, Mode::Callee, false, data));
    }
    for child in type_args {
        push_some(&mut out, value(child, Mode::Norm, false, data));
    }
    if let Some(args) = args {
        push_some(&mut out, value(args, Mode::Norm, false, data));
    }
    Norm::List(out)
}

fn assemble_attr(node: Node, head: bool, data: &[u8]) -> Norm {
    let kind = kind_static(node);
    let named = named_children(node);
    let Some((name, objects)) = named.split_last() else {
        return Norm::List(vec![Norm::Kw(kind)]);
    };
    let mut parts = vec![Norm::Kw(kind)];
    for object in objects {
        push_some(&mut parts, value(*object, Mode::Norm, false, data));
    }
    push_some(&mut parts, name_value(*name, head, data));
    Norm::List(parts)
}

/// A callee path keeps each name in the path. Type arguments stay structural.
fn assemble_path(node: Node, data: &[u8]) -> Norm {
    let mut parts = vec![Norm::Kw(kind_static(node))];
    for child in named_children(node) {
        if IDENTIFIERS.contains(&child.kind()) {
            parts.push(symbol_named(child, data));
        } else if is_path(child) {
            push_some(&mut parts, value(child, Mode::Path, false, data));
        } else {
            push_some(&mut parts, value(child, Mode::Norm, false, data));
        }
    }
    Norm::List(parts)
}

fn assemble_children(node: Node, data: &[u8]) -> Norm {
    let mut parts = vec![Norm::Kw(kind_static(node))];
    for child in children(node) {
        push_some(&mut parts, value(child, Mode::Norm, false, data));
    }
    Norm::List(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Norm;
    use crate::treesitter::{descendants, parse};

    const JAVA_LEFT: &str = "class Left {\n  int alpha(int[] xs) {\n    int[] ys = filter(xs, odd);\n    return map(ys, inc);\n  }\n}\n";
    const JAVA_RIGHT: &str = "class Right {\n  int beta(int[] items) {\n    int[] kept = filter(items, even);\n    return map(kept, dec);\n  }\n}\n";

    /// Normalizes the first node of `kind` in `source`.
    fn first_norm(grammar: &str, kind: &str, source: &str) -> Option<Norm> {
        let data = source.as_bytes();
        let tree = parse(grammar, data);
        let node = descendants(tree.root_node())
            .into_iter()
            .find(|node| node.kind() == kind)
            .expect("node of kind");
        normalize(node, data)
    }

    fn sym(name: &str) -> Norm {
        Norm::List(vec![Norm::Kw("symbol"), Norm::Str(name.to_string())])
    }

    /// True when `needle` appears as a subtree anywhere in `norm`.
    fn contains_subtree(norm: &Norm, needle: &Norm) -> bool {
        if norm == needle {
            return true;
        }
        match norm {
            Norm::List(items) => items.iter().any(|item| contains_subtree(item, needle)),
            _ => false,
        }
    }

    // DR-9: two Java methods that differ only in local names normalize identically.
    #[test]
    fn dr9_java_renamed_locals_normalize_identically() {
        let left = first_norm("java", "method_declaration", JAVA_LEFT);
        let right = first_norm("java", "method_declaration", JAVA_RIGHT);
        assert!(matches!(left, Some(Norm::List(_))));
        assert_eq!(left, right);
    }

    // DR-9: call names stay and locals become the symbol marker.
    #[test]
    fn dr9_call_names_stay_and_locals_become_symbol() {
        let norm = first_norm("java", "method_declaration", JAVA_LEFT).expect("norm");
        assert!(contains_subtree(&norm, &sym("filter")));
        assert!(contains_subtree(&norm, &sym("map")));
        assert!(contains_subtree(&norm, &Norm::Kw("symbol")));
        assert!(!contains_subtree(&norm, &sym("xs")));
    }

    // DR-9: literals of any kind collapse to the literal marker.
    #[test]
    fn dr9_literals_collapse_to_one_marker() {
        let left = first_norm(
            "java",
            "method_declaration",
            "class A { int alpha(int n) { return n + 1; } }",
        );
        let right = first_norm(
            "java",
            "method_declaration",
            "class B { int beta(int m) { return m + 42; } }",
        );
        assert_eq!(left, right);
        let norm = left.expect("norm");
        assert!(contains_subtree(&norm, &Norm::Kw("literal")));
    }

    // DR-9: operator tokens are kept as symbols.
    #[test]
    fn dr9_operator_tokens_are_kept() {
        let norm = first_norm(
            "java",
            "method_declaration",
            "class A { int alpha(int n) { return n + 1; } }",
        )
        .expect("norm");
        assert!(contains_subtree(&norm, &sym("+")));
    }

    // DR-9: comments are dropped from the normalized tree.
    #[test]
    fn dr9_comments_are_dropped() {
        let with_comments = first_norm(
            "java",
            "method_declaration",
            "class A { int alpha(int n) { // note\n return n; /* c */ } }",
        );
        let without = first_norm(
            "java",
            "method_declaration",
            "class A { int alpha(int n) { return n; } }",
        );
        assert_eq!(with_comments, without);
    }

    // DR-9: a Python callee in parentheses keeps its name, as the bare call does.
    #[test]
    fn dr9_parenthesized_callee_keeps_name() {
        let norm = first_norm(
            "python",
            "function_definition",
            "def alpha(xs):\n    return (len)(xs)\n",
        )
        .expect("norm");
        assert!(contains_subtree(&norm, &sym("len")));
    }

    // DR-9: a member name outside callee position is not kept.
    #[test]
    fn dr9_member_name_outside_callee_is_not_kept() {
        let norm = first_norm(
            "python",
            "function_definition",
            "def alpha(xs):\n    return xs.total\n",
        )
        .expect("norm");
        assert!(!contains_subtree(&norm, &sym("total")));
        assert!(contains_subtree(&norm, &Norm::Kw("symbol")));
    }

    // DR-9: a Python string and a number both normalize to the literal marker.
    #[test]
    fn dr9_string_and_number_share_literal_marker() {
        let string = first_norm(
            "python",
            "function_definition",
            "def alpha(xs):\n    return \"xs\"\n",
        )
        .expect("norm");
        let number = first_norm(
            "python",
            "function_definition",
            "def beta(items):\n    return 1\n",
        )
        .expect("norm");
        assert!(contains_subtree(&string, &Norm::Kw("literal")));
        assert!(contains_subtree(&number, &Norm::Kw("literal")));
    }

    // DR-9: Go, Rust, Python, and TypeScript samples each normalize to a list.
    #[test]
    fn dr9_each_language_sample_normalizes_to_a_list() {
        let go = first_norm(
            "go",
            "function_declaration",
            "package demo\n\nfunc alpha(xs []int) []int {\n\tys := filter(xs, odd)\n\treturn mapFn(ys, inc)\n}\n",
        );
        let rust = first_norm(
            "rust",
            "function_item",
            "pub fn alpha(xs: Vec<i32>) -> Vec<i32> {\n    let ys = filter(xs, odd);\n    map(ys, inc)\n}\n",
        );
        let python = first_norm(
            "python",
            "function_definition",
            "def alpha(self, xs):\n    ys = filter(xs, odd)\n    return map(ys, inc)\n",
        );
        let typescript = first_norm(
            "typescript",
            "function_declaration",
            "export function alpha(xs: number[]): number[] {\n  const ys = filter(xs, odd);\n  return map(ys, inc);\n}\n",
        );
        for norm in [go, rust, python, typescript] {
            assert!(matches!(norm, Some(Norm::List(_))));
        }
    }

    // DR-9: a 1,200-deep nested expression normalizes without overflowing the stack.
    #[test]
    fn dr9_deep_expression_normalizes_without_overflow() {
        let handle = std::thread::Builder::new()
            .stack_size(512 << 20)
            .spawn(|| {
                let expr = vec!["1"; 1200].join(" + ");
                let source = format!("def place():\n    return {expr}\n");
                assert!(first_norm("python", "function_definition", &source).is_some());
            })
            .expect("spawn test thread");
        handle.join().expect("deep expression test");
    }

    /// Compares the normalized tree against output from the original Python dryer.
    fn assert_golden(
        grammar: &str,
        kind: &str,
        source: &str,
        expected_pr: &str,
        expected_count: usize,
    ) {
        let norm = first_norm(grammar, kind, source).expect("norm");
        assert_eq!(crate::shape::pr(&norm), expected_pr);
        assert_eq!(crate::shape::node_count(&norm), expected_count);
    }

    // DR-9: golden from the original Python dryer for b.py sum_cost.
    #[test]
    fn dr9_golden_py_sum_cost() {
        assert_golden(
            "python",
            "function_definition",
            "def sum_cost(entries, rate):\n    running = 0\n    for entry in entries:\n        running += entry.cost * entry.count\n    final = running + running * rate\n    return round(final, 2)\n",
            r#"[:function_definition :symbol [:parameters :symbol :symbol] [:block [:assignment :symbol [:symbol "="] :literal] [:for_statement :symbol :symbol [:block [:augmented_assignment :symbol [:symbol "+="] [:binary_operator [:attribute :symbol :symbol] [:symbol "*"] [:attribute :symbol :symbol]]]]] [:assignment :symbol [:symbol "="] [:binary_operator :symbol [:symbol "+"] [:binary_operator :symbol [:symbol "*"] :symbol]]] [:return_statement [:call [:symbol "round"] [:argument_list :symbol :literal]]]]]"#,
            71,
        );
    }

    // DR-9: golden from the original Python dryer, method chain and f-string sample.
    #[test]
    fn dr9_golden_py_chain() {
        assert_golden(
            "python",
            "function_definition",
            "def chain(items, key):\n    # note\n    label = f\"item {key!r}\"\n    picked = sorted(items, key=lambda x: x.cost)[0:2]\n    ok = items.filter(key).first() and not None\n    return picked if ok else (len)(items)\n",
            r#"[:function_definition :symbol [:parameters :symbol :symbol] [:block [:assignment :symbol [:symbol "="] [:string [:string_start] [:string_content] [:interpolation :symbol [:type_conversion]] [:string_end]]] [:assignment :symbol [:symbol "="] [:subscript [:call [:symbol "sorted"] [:argument_list :symbol [:keyword_argument :symbol [:symbol "="] [:lambda [:lambda_parameters :symbol] [:attribute :symbol :symbol]]]]] [:slice :literal :literal]]] [:assignment :symbol [:symbol "="] [:boolean_operator [:call [:attribute [:call [:attribute :symbol [:symbol "filter"]] [:argument_list :symbol]] [:symbol "first"]] [:argument_list]] [:symbol "and"] [:not_operator [:symbol "not"] :literal]]] [:return_statement [:conditional_expression :symbol :symbol [:call [:symbol "len"] [:argument_list :symbol]]]]]]"#,
            114,
        );
    }

    // DR-9: golden from the original Python dryer, scoped attribute call and power operator.
    #[test]
    fn dr9_golden_py_build() {
        assert_golden(
            "python",
            "function_definition",
            "def build(pkg, value):\n    result = pkg.make.Thing(value)  # trailing\n    result.items.append(3.5)\n    total = result.count ** 2\n    return total >= 10 or total == 0\n",
            r#"[:function_definition :symbol [:parameters :symbol :symbol] [:block [:assignment :symbol [:symbol "="] [:call [:attribute [:attribute :symbol :symbol] [:symbol "Thing"]] [:argument_list :symbol]]] [:call [:attribute [:attribute :symbol :symbol] [:symbol "append"]] [:argument_list :literal]] [:assignment :symbol [:symbol "="] [:binary_operator [:attribute :symbol :symbol] [:symbol "**"] :literal]] [:return_statement [:boolean_operator [:comparison_operator :symbol [:symbol ">="] :literal] [:symbol "or"] [:comparison_operator :symbol [:symbol "=="] :literal]]]]]"#,
            80,
        );
    }

    // DR-9: golden from the original Python dryer, Java chains, generics, and lambda.
    #[test]
    fn dr9_golden_java_run() {
        assert_golden(
            "java",
            "method_declaration",
            "class A {\n  int run(List<String> xs) {\n    int n = xs.stream().filter(s -> s.isEmpty()).count();\n    Map<String, Integer> m = new HashMap<String, Integer>(n);\n    return Collections.<String>emptyList().size() + n;\n  }\n}\n",
            r#"[:method_declaration [:integral_type] :symbol [:formal_parameters [:formal_parameter [:generic_type :symbol [:type_arguments [:symbol "<"] :symbol [:symbol ">"]]] :symbol]] [:block [:local_variable_declaration [:integral_type] [:variable_declarator :symbol [:symbol "="] [:method_invocation [:method_invocation [:method_invocation :symbol [:symbol "stream"] [:argument_list]] [:symbol "filter"] [:argument_list [:lambda_expression :symbol [:method_invocation :symbol [:symbol "isEmpty"] [:argument_list]]]]] [:symbol "count"] [:argument_list]]]] [:local_variable_declaration [:generic_type :symbol [:type_arguments [:symbol "<"] :symbol :symbol [:symbol ">"]]] [:variable_declarator :symbol [:symbol "="] [:object_creation_expression [:generic_type [:symbol "HashMap"] [:type_arguments [:symbol "<"] :symbol :symbol [:symbol ">"]]] [:argument_list :symbol]]]] [:return_statement [:binary_expression [:method_invocation [:method_invocation :symbol [:symbol "emptyList"] [:type_arguments [:symbol "<"] :symbol [:symbol ">"]] [:argument_list]] [:symbol "size"] [:argument_list]] [:symbol "+"] :symbol]]]]"#,
            140,
        );
    }

    // DR-9: golden from the original Python dryer, Java operators and ternary with comments.
    #[test]
    fn dr9_golden_java_check() {
        assert_golden(
            "java",
            "method_declaration",
            "class B {\n  boolean check(int a, String s) {\n    // leading comment\n    boolean ok = a > 0 && s != null || a == 7; /* block */\n    int b = a << 2 ^ 3;\n    return ok ? true : false;\n  }\n}\n",
            r#"[:method_declaration [:boolean_type] :symbol [:formal_parameters [:formal_parameter [:integral_type] :symbol] [:formal_parameter :symbol :symbol]] [:block [:local_variable_declaration [:boolean_type] [:variable_declarator :symbol [:symbol "="] [:binary_expression [:binary_expression [:binary_expression :symbol [:symbol ">"] :literal] [:symbol "&&"] [:binary_expression :symbol [:symbol "!="] :literal]] [:symbol "||"] [:binary_expression :symbol [:symbol "=="] :literal]]]] [:local_variable_declaration [:integral_type] [:variable_declarator :symbol [:symbol "="] [:binary_expression [:binary_expression :symbol [:symbol "<<"] :literal] [:symbol "^"] :literal]]] [:return_statement [:ternary_expression :symbol [:symbol "?"] :literal :literal]]]]"#,
            92,
        );
    }

    // DR-9: golden from the original Python dryer, Java switch and field access.
    #[test]
    fn dr9_golden_java_pick() {
        assert_golden(
            "java",
            "method_declaration",
            "class C {\n  int pick(Node node, int k) {\n    switch (k) {\n      case 1: return node.left.value;\n      default: return this.size;\n    }\n  }\n}\n",
            r#"[:method_declaration [:integral_type] :symbol [:formal_parameters [:formal_parameter :symbol :symbol] [:formal_parameter [:integral_type] :symbol]] [:block [:switch_expression :symbol [:switch_block [:switch_block_statement_group [:switch_label :literal] [:return_statement [:field_access [:field_access :symbol :symbol] :symbol]]] [:switch_block_statement_group [:switch_label] [:return_statement [:field_access [:this] :symbol]]]]]]]"#,
            48,
        );
    }

    // DR-9: golden from the original Python dryer, Go range loop and slices.
    #[test]
    fn dr9_golden_go_process() {
        assert_golden(
            "go",
            "function_declaration",
            "package demo\n\nfunc process(xs []int, rate float64) (int, error) {\n\ttotal := 0\n\tfor _, x := range xs {\n\t\ttotal += x * 2\n\t}\n\treturn total, nil\n}\n",
            r#"[:function_declaration :symbol [:parameter_list [:parameter_declaration :symbol [:slice_type :symbol]] [:parameter_declaration :symbol :symbol]] [:parameter_list [:parameter_declaration :symbol] [:parameter_declaration :symbol]] [:block [:statement_list [:short_var_declaration [:expression_list :symbol] [:symbol ":="] [:expression_list :literal]] [:for_statement [:range_clause [:expression_list :symbol :symbol] [:symbol ":="] :symbol] [:block [:statement_list [:assignment_statement [:expression_list :symbol] [:symbol "+="] [:expression_list [:binary_expression :symbol [:symbol "*"] :literal]]]]]] [:return_statement [:expression_list :symbol :literal]]]]]"#,
            77,
        );
    }

    // DR-9: golden from the original Python dryer, Go selectors, generics, and operators.
    #[test]
    fn dr9_golden_go_build() {
        assert_golden(
            "go",
            "function_declaration",
            "package demo\n\nfunc build(cfg Config, n int) string {\n\t// comment\n\tname := strings.ToUpper(cfg.Name)\n\titems := pkg.Make[int](n)\n\tok := n > 0 && !cfg.Disabled || n == 7\n\treturn name + \"x\" + fmt.Sprint(ok, 3.5, 'a')\n}\n",
            r#"[:function_declaration :symbol [:parameter_list [:parameter_declaration :symbol :symbol] [:parameter_declaration :symbol :symbol]] :symbol [:block [:statement_list [:short_var_declaration [:expression_list :symbol] [:symbol ":="] [:expression_list [:call_expression [:selector_expression :symbol [:symbol "ToUpper"]] [:argument_list [:selector_expression :symbol :symbol]]]]] [:short_var_declaration [:expression_list :symbol] [:symbol ":="] [:expression_list [:type_conversion_expression [:generic_type [:qualified_type :symbol :symbol] [:type_arguments [:type_elem :symbol]]] :symbol]]] [:short_var_declaration [:expression_list :symbol] [:symbol ":="] [:expression_list [:binary_expression [:binary_expression [:binary_expression :symbol [:symbol ">"] :literal] [:symbol "&&"] [:unary_expression [:symbol "!"] [:selector_expression :symbol :symbol]]] [:symbol "||"] [:binary_expression :symbol [:symbol "=="] :literal]]]] [:return_statement [:expression_list [:binary_expression [:binary_expression :symbol [:symbol "+"] :literal] [:symbol "+"] [:call_expression [:selector_expression :symbol [:symbol "Sprint"]] [:argument_list :symbol :literal :literal]]]]]]]]"#,
            138,
        );
    }

    // DR-9: golden from the original Python dryer, Go switch and func literal.
    #[test]
    fn dr9_golden_go_route() {
        assert_golden(
            "go",
            "function_declaration",
            "package demo\n\nfunc route(code int) string {\n\tswitch code {\n\tcase 200:\n\t\treturn `ok`\n\tdefault:\n\t\tf := func(s string) string { return s + \"!\" }\n\t\treturn f(\"no\")\n\t}\n}\n",
            r#"[:function_declaration :symbol [:parameter_list [:parameter_declaration :symbol :symbol]] :symbol [:block [:statement_list [:expression_switch_statement :symbol [:expression_case [:expression_list :literal] [:statement_list [:return_statement [:expression_list :literal]]]] [:default_case [:statement_list [:short_var_declaration [:expression_list :symbol] [:symbol ":="] [:expression_list [:func_literal [:parameter_list [:parameter_declaration :symbol :symbol]] :symbol [:block [:statement_list [:return_statement [:expression_list [:binary_expression :symbol [:symbol "+"] :literal]]]]]]]] [:return_statement [:expression_list [:call_expression [:symbol "f"] [:argument_list :literal]]]]]]]]]]"#,
            79,
        );
    }

    // DR-9: golden from the original Python dryer, Rust turbofish, closures, and casts.
    #[test]
    fn dr9_golden_rs_tally() {
        assert_golden(
            "rust",
            "function_item",
            "fn tally(xs: Vec<i32>) -> i32 {\n    let total: i32 = xs.iter().map(|x| x * 2).sum::<i32>();\n    let v = Vec::<u8>::with_capacity(4);\n    std::cmp::max(total, 0) + v.len() as i32\n}\n",
            r#"[:function_item :symbol [:parameters [:parameter :symbol [:generic_type :symbol [:type_arguments [:symbol "<"] [:primitive_type] [:symbol ">"]]]]] [:primitive_type] [:block [:let_declaration :symbol [:primitive_type] [:symbol "="] [:call_expression [:generic_function [:field_expression [:call_expression [:field_expression [:call_expression [:field_expression :symbol [:symbol "iter"]] [:arguments]] [:symbol "map"]] [:arguments [:closure_expression [:closure_parameters [:symbol "|"] :symbol [:symbol "|"]] [:binary_expression :symbol [:symbol "*"] :literal]]]] :symbol] [:type_arguments [:symbol "<"] [:primitive_type] [:symbol ">"]]] [:arguments]]] [:let_declaration :symbol [:symbol "="] [:call_expression [:scoped_identifier [:generic_type [:symbol "Vec"] [:type_arguments [:symbol "<"] [:primitive_type] [:symbol ">"]]] [:symbol "with_capacity"]] [:arguments :literal]]] [:binary_expression [:call_expression [:scoped_identifier [:scoped_identifier [:symbol "std"] [:symbol "cmp"]] [:symbol "max"]] [:arguments :symbol :literal]] [:symbol "+"] [:type_cast_expression [:call_expression [:field_expression :symbol [:symbol "len"]] [:arguments]] [:primitive_type]]]]]"#,
            158,
        );
    }

    // DR-9: golden from the original Python dryer, Rust try, match, and comments.
    #[test]
    fn dr9_golden_rs_parse() {
        assert_golden(
            "rust",
            "function_item",
            "fn parse(s: &str) -> Result<u32, String> {\n    // comment\n    let n = s.parse::<u32>().map_err(|e| e.to_string())?;\n    let ok = n > 3 && !s.is_empty() || n == 9;\n    match n {\n        0 => Err(String::from(\"zero\")),\n        _ => Ok(n + 1),\n    }\n}\n",
            r#"[:function_item :symbol [:parameters [:parameter :symbol [:reference_type [:symbol "&"] [:primitive_type]]]] [:generic_type :symbol [:type_arguments [:symbol "<"] [:primitive_type] :symbol [:symbol ">"]]] [:block [:let_declaration :symbol [:symbol "="] [:try_expression [:call_expression [:field_expression [:call_expression [:generic_function [:field_expression :symbol :symbol] [:type_arguments [:symbol "<"] [:primitive_type] [:symbol ">"]]] [:arguments]] [:symbol "map_err"]] [:arguments [:closure_expression [:closure_parameters [:symbol "|"] :symbol [:symbol "|"]] [:call_expression [:field_expression :symbol [:symbol "to_string"]] [:arguments]]]]] [:symbol "?"]]] [:let_declaration :symbol [:symbol "="] [:binary_expression [:binary_expression [:binary_expression :symbol [:symbol ">"] :literal] [:symbol "&&"] [:unary_expression [:symbol "!"] [:call_expression [:field_expression :symbol [:symbol "is_empty"]] [:arguments]]]] [:symbol "||"] [:binary_expression :symbol [:symbol "=="] :literal]]] [:expression_statement [:match_expression :symbol [:match_block [:match_arm [:match_pattern :literal] [:symbol "=>"] [:call_expression [:symbol "Err"] [:arguments [:call_expression [:scoped_identifier [:symbol "String"] [:symbol "from"]] [:arguments :literal]]]]] [:match_arm [:match_pattern] [:symbol "=>"] [:call_expression [:symbol "Ok"] [:arguments [:binary_expression :symbol [:symbol "+"] :literal]]]]]]]]]"#,
            193,
        );
    }

    // DR-9: golden from the original Python dryer, Rust struct literal and scoped call.
    #[test]
    fn dr9_golden_rs_make() {
        assert_golden(
            "rust",
            "function_item",
            "fn make(p: Point) -> Point {\n    let flag = true;\n    let name = \"pt\";\n    if let Some(v) = p.inner.value {\n        return Point { x: v, y: 2.5 };\n    }\n    Point::new(p.x, flag, name)\n}\n",
            r#"[:function_item :symbol [:parameters [:parameter :symbol :symbol]] :symbol [:block [:let_declaration :symbol [:symbol "="] :literal] [:let_declaration :symbol [:symbol "="] :literal] [:expression_statement [:if_expression [:let_condition [:tuple_struct_pattern :symbol :symbol] [:symbol "="] [:field_expression [:field_expression :symbol :symbol] :symbol]] [:block [:expression_statement [:return_expression [:struct_expression :symbol [:field_initializer_list [:field_initializer :symbol :symbol] [:field_initializer :symbol :literal]]]]]]]] [:call_expression [:scoped_identifier [:symbol "Point"] [:symbol "new"]] [:arguments [:field_expression :symbol :symbol] :symbol :symbol]]]]"#,
            83,
        );
    }

    // DR-9: golden from the original Python dryer, TypeScript optional chain and generics.
    #[test]
    fn dr9_golden_ts_load() {
        assert_golden(
            "typescript",
            "function_declaration",
            "function load(api: Api, id: string): string {\n  const user = api.users.find<User>(id)?.name ?? \"anon\";\n  const label = `user ${user}`;\n  return (new Map<string, number>(api.entries)).get(label) ?? 0;\n}\n",
            r#"[:function_declaration :symbol [:formal_parameters [:required_parameter :symbol [:type_annotation :symbol]] [:required_parameter :symbol [:type_annotation [:predefined_type]]]] [:type_annotation [:predefined_type]] [:statement_block [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:binary_expression [:member_expression [:call_expression [:member_expression [:member_expression :symbol :symbol] [:symbol "find"]] [:type_arguments [:symbol "<"] :symbol [:symbol ">"]] [:arguments :symbol]] [:optional_chain [:symbol "?."]] :symbol] [:symbol "??"] :literal]]] [:lexical_declaration [:variable_declarator :symbol [:symbol "="] :literal]] [:return_statement [:binary_expression [:call_expression [:member_expression [:new_expression [:symbol "Map"] [:type_arguments [:symbol "<"] [:predefined_type] [:predefined_type] [:symbol ">"]] [:arguments [:member_expression :symbol :symbol]]] [:symbol "get"]] [:arguments :symbol]] [:symbol "??"] :literal]]]]"#,
            119,
        );
    }

    // DR-9: golden from the original Python dryer, TypeScript arrows, ternary, and comments.
    #[test]
    fn dr9_golden_ts_check() {
        assert_golden(
            "typescript",
            "function_declaration",
            "function check(a: number, b: boolean): boolean {\n  // leading comment\n  const ok = a > 0 && b || a === 7;\n  const pick = ok ? (x: number) => x + 1 : (x: number) => x - 1;\n  return !ok /* tail */ && (a !== 2);\n}\n",
            r#"[:function_declaration :symbol [:formal_parameters [:required_parameter :symbol [:type_annotation [:predefined_type]]] [:required_parameter :symbol [:type_annotation [:predefined_type]]]] [:type_annotation [:predefined_type]] [:statement_block [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:binary_expression [:binary_expression [:binary_expression :symbol [:symbol ">"] :literal] [:symbol "&&"] :symbol] [:symbol "||"] [:binary_expression :symbol [:symbol "==="] :literal]]]] [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:ternary_expression :symbol [:symbol "?"] [:arrow_function [:formal_parameters [:required_parameter :symbol [:type_annotation [:predefined_type]]]] [:symbol "=>"] [:binary_expression :symbol [:symbol "+"] :literal]] [:arrow_function [:formal_parameters [:required_parameter :symbol [:type_annotation [:predefined_type]]]] [:symbol "=>"] [:binary_expression :symbol [:symbol "-"] :literal]]]]] [:return_statement [:binary_expression [:unary_expression [:symbol "!"] :symbol] [:symbol "&&"] [:binary_expression :symbol [:symbol "!=="] :literal]]]]]"#,
            134,
        );
    }

    // DR-9: golden from the original Python dryer, TypeScript for-of and object literal.
    #[test]
    fn dr9_golden_ts_run() {
        assert_golden(
            "typescript",
            "function_declaration",
            "function run(items: Item[], pkg: Pkg): number {\n  let total = 0;\n  for (const item of items) {\n    total += (pkg.price)(item.qty);\n  }\n  const shape = { total, count: items.length, flag: true, label: 'x' };\n  return Math.max(shape.total, 0) + pkg.util.scale(1.5);\n}\n",
            r#"[:function_declaration :symbol [:formal_parameters [:required_parameter :symbol [:type_annotation [:array_type :symbol]]] [:required_parameter :symbol [:type_annotation :symbol]]] [:type_annotation [:predefined_type]] [:statement_block [:lexical_declaration [:variable_declarator :symbol [:symbol "="] :literal]] [:for_in_statement :symbol :symbol [:statement_block [:expression_statement [:augmented_assignment_expression :symbol [:symbol "+="] [:call_expression [:member_expression :symbol [:symbol "price"]] [:arguments [:member_expression :symbol :symbol]]]]]]] [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:object :symbol [:pair :symbol [:member_expression :symbol :symbol]] [:pair :symbol :literal] [:pair :symbol :literal]]]] [:return_statement [:binary_expression [:call_expression [:member_expression :symbol [:symbol "max"]] [:arguments [:member_expression :symbol :symbol] :literal]] [:symbol "+"] [:call_expression [:member_expression [:member_expression :symbol :symbol] [:symbol "scale"]] [:arguments :literal]]]]]]"#,
            124,
        );
    }

    // DR-9: golden from the original Python dryer, TSX JSX element and nullish.
    #[test]
    fn dr9_golden_tsx_view() {
        assert_golden(
            "tsx",
            "function_declaration",
            "function View(props: Props): JSX.Element {\n  const title = props.title ?? \"untitled\";\n  return <div className=\"box\">{title.toUpperCase()}</div>;\n}\n",
            r#"[:function_declaration :symbol [:formal_parameters [:required_parameter :symbol [:type_annotation :symbol]]] [:type_annotation [:nested_type_identifier :symbol :symbol]] [:statement_block [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:binary_expression [:member_expression :symbol :symbol] [:symbol "??"] :literal]]] [:return_statement [:jsx_element [:jsx_opening_element [:symbol "<"] :symbol [:jsx_attribute :symbol [:symbol "="] :literal] [:symbol ">"]] [:jsx_expression [:call_expression [:member_expression :symbol [:symbol "toUpperCase"]] [:arguments]]] [:jsx_closing_element :symbol [:symbol ">"]]]]]]"#,
            75,
        );
    }

    // DR-9: golden from the original Python dryer, TSX map with arrow JSX.
    #[test]
    fn dr9_golden_tsx_list() {
        assert_golden(
            "tsx",
            "function_declaration",
            "function List(props: Props) {\n  const rows = props.items.map((item) => <li key={item.id}>{item.name}</li>);\n  return <ul>{rows}</ul>;\n}\n",
            r#"[:function_declaration :symbol [:formal_parameters [:required_parameter :symbol [:type_annotation :symbol]]] [:statement_block [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:call_expression [:member_expression [:member_expression :symbol :symbol] [:symbol "map"]] [:arguments [:arrow_function [:formal_parameters [:required_parameter :symbol]] [:symbol "=>"] [:jsx_element [:jsx_opening_element [:symbol "<"] :symbol [:jsx_attribute :symbol [:symbol "="] [:jsx_expression [:member_expression :symbol :symbol]]] [:symbol ">"]] [:jsx_expression [:member_expression :symbol :symbol]] [:jsx_closing_element :symbol [:symbol ">"]]]]]]]] [:return_statement [:jsx_element [:jsx_opening_element [:symbol "<"] :symbol [:symbol ">"]] [:jsx_expression :symbol] [:jsx_closing_element :symbol [:symbol ">"]]]]]]"#,
            101,
        );
    }

    // DR-9: golden from the original Python dryer, TSX generic hook call and ternary JSX.
    #[test]
    fn dr9_golden_tsx_box() {
        assert_golden(
            "tsx",
            "function_declaration",
            "function Box(props: Props) {\n  // comment\n  const count = useCount<number>(props.start) || 0;\n  const on = props?.enabled && count > 2;\n  return on ? <span>{count}</span> : null;\n}\n",
            r#"[:function_declaration :symbol [:formal_parameters [:required_parameter :symbol [:type_annotation :symbol]]] [:statement_block [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:binary_expression [:call_expression [:symbol "useCount"] [:type_arguments [:symbol "<"] [:predefined_type] [:symbol ">"]] [:arguments [:member_expression :symbol :symbol]]] [:symbol "||"] :literal]]] [:lexical_declaration [:variable_declarator :symbol [:symbol "="] [:binary_expression [:member_expression :symbol [:optional_chain [:symbol "?."]] :symbol] [:symbol "&&"] [:binary_expression :symbol [:symbol ">"] :literal]]]] [:return_statement [:ternary_expression :symbol [:symbol "?"] [:jsx_element [:jsx_opening_element [:symbol "<"] :symbol [:symbol ">"]] [:jsx_expression :symbol] [:jsx_closing_element :symbol [:symbol ">"]]] :literal]]]]"#,
            106,
        );
    }
}
