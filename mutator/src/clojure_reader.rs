//! Port of the tree-sitter-clojure node shapes that mutator/src/mutator/sites.py walks (BSD Zero Clause License, Robert C. Martin).

/// A Clojure form: its tree-sitter-clojure kind, its byte span, and its child forms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CNode {
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub children: Vec<CNode>,
}

/// Top-level forms of `source`. Never panics; unbalanced input is closed at the end of the source.
pub fn read(source: &str) -> Vec<CNode> {
    let mut reader = Reader::new(source);
    while reader.pos < source.len() {
        reader.step();
    }
    reader.into_forms()
}

/// Bracketed forms: opening text, node kind, closing byte.
const OPENERS: &[(&str, &str, u8)] = &[
    ("(", "list_lit", b')'),
    ("[", "vec_lit", b']'),
    ("{", "map_lit", b'}'),
    ("#(", "anon_fn_lit", b')'),
    ("#{", "set_lit", b'}'),
    ("#?@(", "read_cond_lit", b')'),
    ("#?(", "read_cond_lit", b')'),
];

/// Forms that wrap the following forms: prefix text, node kind, forms they wrap.
const PREFIXES: &[(&str, &str, usize)] = &[
    ("'", "quoting_lit", 1),
    ("`", "syn_quoting_lit", 1),
    ("~@", "unquote_splicing_lit", 1),
    ("~", "unquoting_lit", 1),
    ("@", "derefing_lit", 1),
    ("^", "meta_lit", 2),
    ("#'", "var_quoting_lit", 1),
    ("#_", "dis_expr", 1),
];

/// The wrapped-form count of a prefix that wraps exactly one form.
const ONE_FORM: usize = 1;

enum Token {
    Skip,
    Open(&'static str, u8, usize),
    Close(u8),
    Prefix(&'static str, usize, usize),
    Leaf(&'static str, usize),
}

enum Shape {
    Group(u8),
    Prefix { need: usize, token_end: usize },
}

/// An open form on the explicit stack, waiting for its children or its closer.
struct Frame {
    shape: Shape,
    kind: &'static str,
    start: usize,
    children: Vec<CNode>,
}

impl Frame {
    fn is_complete(&self) -> bool {
        matches!(self.shape, Shape::Prefix { need, .. } if self.children.len() >= need)
    }

    fn merged_symbol(&self) -> Option<CNode> {
        match self.children.as_slice() {
            [_, target] if self.kind == "meta_lit" && target.kind == "sym_lit" => Some(CNode {
                kind: "sym_lit",
                start: self.start,
                end: target.end,
                children: Vec::new(),
            }),
            _ => None,
        }
    }

    fn finish(self, group_end: usize) -> CNode {
        if let Some(symbol) = self.merged_symbol() {
            return symbol;
        }
        let end = match self.shape {
            Shape::Prefix { token_end, .. } => self.children.last().map_or(token_end, |c| c.end),
            _ => group_end,
        };
        CNode {
            kind: self.kind,
            start: self.start,
            end,
            children: self.children,
        }
    }
}

struct Reader<'a> {
    src: &'a str,
    pos: usize,
    stack: Vec<Frame>,
    forms: Vec<CNode>,
}

impl<'a> Reader<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            stack: Vec::new(),
            forms: Vec::new(),
        }
    }

    fn step(&mut self) {
        match lex(&self.src[self.pos..]) {
            Token::Skip => self.pos += 1,
            Token::Open(kind, closer, len) => self.open(kind, Shape::Group(closer), len),
            Token::Close(closer) => self.close(closer),
            Token::Prefix(kind, need, len) => {
                self.open(
                    kind,
                    Shape::Prefix {
                        need,
                        token_end: self.pos + len,
                    },
                    len,
                );
            }
            Token::Leaf(kind, len) => self.leaf(kind, len),
        }
    }

    fn open(&mut self, kind: &'static str, shape: Shape, len: usize) {
        self.stack.push(Frame {
            shape,
            kind,
            start: self.pos,
            children: Vec::new(),
        });
        self.pos += len;
    }

    fn leaf(&mut self, kind: &'static str, len: usize) {
        let node = CNode {
            kind,
            start: self.pos,
            end: self.pos + len,
            children: Vec::new(),
        };
        self.pos += len;
        self.attach(node);
    }

    fn close(&mut self, closer: u8) {
        self.pos += 1;
        while self.top_is_prefix() {
            self.pop_into_parent(self.pos);
        }
        if self.top_closer() == Some(closer) {
            self.pop_into_parent(self.pos);
        }
    }

    /// Adds `node` to the top frame, then closes every prefix frame that is now complete.
    fn attach(&mut self, mut node: CNode) {
        while let Some(top) = self.stack.last_mut() {
            top.children.push(node);
            if !top.is_complete() {
                return;
            }
            let Some(frame) = self.stack.pop() else {
                return;
            };
            node = frame.finish(self.pos);
        }
        self.forms.push(node);
    }

    fn pop_into_parent(&mut self, end: usize) {
        if let Some(frame) = self.stack.pop() {
            self.attach(frame.finish(end));
        }
    }

    fn top_is_prefix(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(Frame {
                shape: Shape::Prefix { .. },
                ..
            })
        )
    }

    fn top_closer(&self) -> Option<u8> {
        match self.stack.last()?.shape {
            Shape::Group(closer) => Some(closer),
            _ => None,
        }
    }

    /// Closes the forms still open at the end of the source.
    fn into_forms(mut self) -> Vec<CNode> {
        while !self.stack.is_empty() {
            self.pop_into_parent(self.src.len());
        }
        self.forms
    }
}

fn lex(rest: &str) -> Token {
    skip(rest)
        .or_else(|| open(rest))
        .or_else(|| close(rest))
        .or_else(|| prefix(rest))
        .unwrap_or_else(|| leaf(rest))
}

fn skip(rest: &str) -> Option<Token> {
    rest.starts_with(|c: char| c == ',' || c.is_ascii_whitespace())
        .then_some(Token::Skip)
}

fn open(rest: &str) -> Option<Token> {
    OPENERS
        .iter()
        .find(|(text, _, _)| rest.starts_with(*text))
        .map(|&(text, kind, closer)| Token::Open(kind, closer, text.len()))
}

fn close(rest: &str) -> Option<Token> {
    let byte = rest.bytes().next()?;
    matches!(byte, b')' | b']' | b'}').then_some(Token::Close(byte))
}

fn prefix(rest: &str) -> Option<Token> {
    PREFIXES
        .iter()
        .find(|(text, _, _)| rest.starts_with(*text))
        .map(|&(text, kind, need)| Token::Prefix(kind, need, text.len()))
        .or_else(|| hash_prefix(rest))
}

/// `#:ns{...}` is a namespace map; `#inst "..."` is a tagged literal. The tag is not a child.
fn hash_prefix(rest: &str) -> Option<Token> {
    let body = rest.strip_prefix('#')?;
    let kind = match body.bytes().next()? {
        b':' => "ns_map_lit",
        byte if byte.is_ascii_alphabetic() => "tagged_or_ctor_lit",
        _ => return None,
    };
    Some(Token::Prefix(kind, ONE_FORM, 1 + token_len(body)))
}

fn leaf(rest: &str) -> Token {
    let (kind, len) = leaf_shape(rest);
    Token::Leaf(kind, len)
}

fn leaf_shape(rest: &str) -> (&'static str, usize) {
    match rest.bytes().next() {
        Some(b';') => ("comment", rest.find('\n').unwrap_or(rest.len())),
        Some(b'"') => ("str_lit", quoted_len(rest)),
        Some(b'\\') => ("char_lit", char_len(rest)),
        Some(b'#') => hash_leaf(rest),
        _ => atom(rest),
    }
}

fn hash_leaf(rest: &str) -> (&'static str, usize) {
    match rest.as_bytes().get(1).copied() {
        Some(b'"') => ("regex_lit", 1 + quoted_len(&rest[1..])),
        Some(b'#') => ("sym_val_lit", 2 + token_len(&rest[2..])),
        _ => ("other", 1),
    }
}

/// Length of a string literal that starts at `rest[0]`, honouring backslash escapes.
fn quoted_len(rest: &str) -> usize {
    let mut bytes = rest.bytes().enumerate().skip(1);
    while let Some((index, byte)) = bytes.next() {
        match byte {
            b'\\' => {
                bytes.next();
            }
            b'"' => return index + 1,
            _ => {}
        }
    }
    rest.len()
}

/// Length of a character literal: a backslash, one character, then any token characters.
fn char_len(rest: &str) -> usize {
    let body = &rest[1..];
    let first = body.chars().next().map_or(0, char::len_utf8);
    1 + first + token_len(&body[first..])
}

fn atom(rest: &str) -> (&'static str, usize) {
    let len = token_len(rest);
    (atom_kind(rest.get(..len).unwrap_or("")), len)
}

fn atom_kind(text: &str) -> &'static str {
    match text {
        "nil" => "nil_lit",
        "true" | "false" => "bool_lit",
        _ if text.starts_with(':') => "kwd_lit",
        _ if is_number(text) => "num_lit",
        _ => "sym_lit",
    }
}

fn is_number(text: &str) -> bool {
    text.strip_prefix(|c: char| c == '+' || c == '-')
        .unwrap_or(text)
        .starts_with(|c: char| c.is_ascii_digit())
}

fn token_len(rest: &str) -> usize {
    rest.find(is_delimiter).unwrap_or(rest.len())
}

fn is_delimiter(c: char) -> bool {
    c.is_ascii_whitespace()
        || matches!(
            c,
            ',' | '"' | ';' | '@' | '^' | '`' | '~' | '(' | ')' | '[' | ']' | '{' | '}' | '\\'
        )
}

#[cfg(test)]
mod tests {
    use super::{CNode, read};

    fn kinds(nodes: &[CNode]) -> Vec<&'static str> {
        nodes.iter().map(|node| node.kind).collect()
    }

    // MU-8: lists keep their child forms with byte spans
    #[test]
    fn mu_8_list_children_have_kinds_and_byte_spans() {
        let forms = read("(+ a 1)");
        assert_eq!(forms.len(), 1);
        let list = &forms[0];
        assert_eq!((list.kind, list.start, list.end), ("list_lit", 0, 7));
        assert_eq!(kinds(&list.children), ["sym_lit", "sym_lit", "num_lit"]);
        let spans: Vec<(usize, usize)> = list.children.iter().map(|c| (c.start, c.end)).collect();
        assert_eq!(spans, [(1, 2), (3, 4), (5, 6)]);
    }

    // MU-8: quote and syntax-quote wrap the form that follows them
    #[test]
    fn mu_8_quote_and_syntax_quote_wrap_the_next_form() {
        let forms = read("'(a) `b");
        assert_eq!(kinds(&forms), ["quoting_lit", "syn_quoting_lit"]);
        assert_eq!(kinds(&forms[0].children), ["list_lit"]);
        assert_eq!(kinds(&forms[1].children), ["sym_lit"]);
        assert_eq!((forms[1].start, forms[1].end), (5, 7));
    }

    // MU-8: metadata before a symbol makes one sym_lit that includes the metadata
    #[test]
    fn mu_8_metadata_and_symbol_form_one_sym_lit() {
        let forms = read("^:private foo");
        assert_eq!(kinds(&forms), ["sym_lit"]);
        assert_eq!((forms[0].start, forms[0].end), (0, 13));
        assert!(forms[0].children.is_empty());
    }

    // MU-8: string and character literals hold no child forms
    #[test]
    fn mu_8_string_and_char_literals_have_no_children() {
        let forms = read("\"(+ 1)\" \\(");
        assert_eq!(kinds(&forms), ["str_lit", "char_lit"]);
        assert!(forms.iter().all(|form| form.children.is_empty()));
    }

    // MU-8: anonymous functions, regexes and line comments get their own kinds
    #[test]
    fn mu_8_anon_fn_regex_and_comment_kinds() {
        let forms = read("#(+ % 1) #\"a+\" ; hi\n");
        assert_eq!(kinds(&forms), ["anon_fn_lit", "regex_lit", "comment"]);
        assert_eq!(kinds(&forms[0].children), ["sym_lit", "sym_lit", "num_lit"]);
    }

    // MU-8: atoms are classified by their text
    #[test]
    fn mu_8_atoms_are_classified_by_text() {
        let forms = read("nil true :k 1 -2 ##Inf x");
        assert_eq!(
            kinds(&forms),
            [
                "nil_lit",
                "bool_lit",
                "kwd_lit",
                "num_lit",
                "num_lit",
                "sym_val_lit",
                "sym_lit"
            ]
        );
    }

    // MU-8: a 1,200-deep form is read without overflowing the stack
    #[test]
    fn mu_8_deep_nesting_is_read_iteratively() {
        let source = format!("{}1{}", "(".repeat(1200), ")".repeat(1200));
        let forms = read(&source);
        assert_eq!(forms.len(), 1);
        let mut depth = 0;
        let mut node = &forms[0];
        while let Some(child) = node.children.first() {
            depth += 1;
            node = child;
        }
        assert_eq!(depth, 1200);
        assert_eq!(node.kind, "num_lit");
    }

    // MU-8: unbalanced input never panics; open forms end at the source end
    #[test]
    fn mu_8_unbalanced_input_is_tolerated() {
        let open = read("(+ 1");
        assert_eq!(
            (open[0].kind, open[0].start, open[0].end),
            ("list_lit", 0, 4)
        );
        assert_eq!(kinds(&read("'")), ["quoting_lit"]);
        assert!(read(")]}").is_empty());
    }

    // MU-8: offsets are UTF-8 byte offsets
    #[test]
    fn mu_8_offsets_are_utf8_byte_offsets() {
        let forms = read("\"é\" (é 1)");
        assert_eq!((forms[1].start, forms[1].end), (5, 11));
        let number = &forms[1].children[1];
        assert_eq!((number.kind, number.start, number.end), ("num_lit", 9, 10));
    }

    // MU-8: a comment runs to the newline, or to the source end when unterminated
    #[test]
    fn mu_8_comment_spans_end_at_newline_or_source_end() {
        let forms = read("; hi\nx");
        assert_eq!(kinds(&forms), ["comment", "sym_lit"]);
        assert_eq!((forms[0].start, forms[0].end), (0, 4));
        assert_eq!((forms[1].start, forms[1].end), (5, 6));
        let open = read("; hi");
        assert_eq!(
            (open[0].kind, open[0].start, open[0].end),
            ("comment", 0, 4)
        );
    }

    // MU-8: a string ends at its first unescaped quote; an escape skips the next byte
    #[test]
    fn mu_8_string_spans_honour_escapes_and_empty_strings() {
        let empty = read("\"\" x");
        assert_eq!(kinds(&empty), ["str_lit", "sym_lit"]);
        assert_eq!((empty[0].start, empty[0].end), (0, 2));
        assert_eq!((empty[1].start, empty[1].end), (3, 4));

        let escaped_quote = read("\"a\\\"b\" c");
        assert_eq!(kinds(&escaped_quote), ["str_lit", "sym_lit"]);
        assert_eq!((escaped_quote[0].start, escaped_quote[0].end), (0, 6));
        assert_eq!((escaped_quote[1].start, escaped_quote[1].end), (7, 8));

        let escaped_backslash = read("\"a\\\\\" b");
        assert_eq!(kinds(&escaped_backslash), ["str_lit", "sym_lit"]);
        assert_eq!(
            (escaped_backslash[0].start, escaped_backslash[0].end),
            (0, 5)
        );
        assert_eq!(
            (escaped_backslash[1].start, escaped_backslash[1].end),
            (6, 7)
        );

        let open = read("\"abc");
        assert_eq!(
            (open[0].kind, open[0].start, open[0].end),
            ("str_lit", 0, 4)
        );
    }

    // MU-8: a character literal is a backslash, one character, then token characters
    #[test]
    fn mu_8_char_literal_spans() {
        let forms = read("\\a b");
        assert_eq!(kinds(&forms), ["char_lit", "sym_lit"]);
        assert_eq!((forms[0].start, forms[0].end), (0, 2));
        assert_eq!((forms[1].start, forms[1].end), (3, 4));

        let named = read("\\space x");
        assert_eq!(
            (named[0].kind, named[0].start, named[0].end),
            ("char_lit", 0, 6)
        );
        assert_eq!((named[1].start, named[1].end), (7, 8));

        let multibyte = read("\\é x");
        assert_eq!((multibyte[0].start, multibyte[0].end), (0, 3));
        assert_eq!((multibyte[1].start, multibyte[1].end), (4, 5));

        let lone = read("\\");
        assert_eq!(
            (lone[0].kind, lone[0].start, lone[0].end),
            ("char_lit", 0, 1)
        );
    }

    // MU-8: regex, symbolic-value and lone-hash leaves end where their text ends
    #[test]
    fn mu_8_hash_leaf_spans() {
        let regex = read("#\"a+\" x");
        assert_eq!(kinds(&regex), ["regex_lit", "sym_lit"]);
        assert_eq!((regex[0].start, regex[0].end), (0, 5));
        assert_eq!((regex[1].start, regex[1].end), (6, 7));
        let open_regex = read("#\"ab");
        assert_eq!(
            (open_regex[0].kind, open_regex[0].start, open_regex[0].end),
            ("regex_lit", 0, 4)
        );
        let sym_val = read("##Inf");
        assert_eq!(
            (sym_val[0].kind, sym_val[0].start, sym_val[0].end),
            ("sym_val_lit", 0, 5)
        );
        let other = read("#1");
        assert_eq!(kinds(&other), ["other", "num_lit"]);
        assert_eq!((other[0].start, other[0].end), (0, 1));
        assert_eq!((other[1].start, other[1].end), (1, 2));
        let lone = read("#");
        assert_eq!((lone[0].kind, lone[0].start, lone[0].end), ("other", 0, 1));
    }

    // MU-8: #_ wraps the next form; unterminated, it ends after the marker
    #[test]
    fn mu_8_discard_spans() {
        let forms = read("#_ foo bar");
        assert_eq!(kinds(&forms), ["dis_expr", "sym_lit"]);
        assert_eq!((forms[0].start, forms[0].end), (0, 6));
        assert_eq!(kinds(&forms[0].children), ["sym_lit"]);
        assert_eq!(
            (forms[0].children[0].start, forms[0].children[0].end),
            (3, 6)
        );
        assert_eq!((forms[1].start, forms[1].end), (7, 10));
        let open = read("#_");
        assert_eq!(
            (open[0].kind, open[0].start, open[0].end),
            ("dis_expr", 0, 2)
        );
        assert!(open[0].children.is_empty());
    }

    // MU-8: #? and #?@ hold their branches; unterminated, they end at the source end
    #[test]
    fn mu_8_read_conditional_spans() {
        let forms = read("#?(:clj 1)");
        assert_eq!(
            (forms[0].kind, forms[0].start, forms[0].end),
            ("read_cond_lit", 0, 10)
        );
        assert_eq!(kinds(&forms[0].children), ["kwd_lit", "num_lit"]);
        assert_eq!(
            (forms[0].children[0].start, forms[0].children[0].end),
            (3, 7)
        );
        assert_eq!(
            (forms[0].children[1].start, forms[0].children[1].end),
            (8, 9)
        );
        let splice = read("#?@(1)");
        assert_eq!(
            (splice[0].kind, splice[0].start, splice[0].end),
            ("read_cond_lit", 0, 6)
        );
        assert_eq!(
            (splice[0].children[0].start, splice[0].children[0].end),
            (4, 5)
        );
        let open = read("#?(1");
        assert_eq!(
            (open[0].kind, open[0].start, open[0].end),
            ("read_cond_lit", 0, 4)
        );
        assert_eq!((open[0].children[0].start, open[0].children[0].end), (3, 4));
    }

    // MU-8: #:ns{} wraps a map; a tagged literal's tag is not a child
    #[test]
    fn mu_8_namespace_map_and_tagged_literal_spans() {
        let ns = read("#:ns{:a 1}");
        assert_eq!((ns[0].kind, ns[0].start, ns[0].end), ("ns_map_lit", 0, 10));
        assert_eq!(kinds(&ns[0].children), ["map_lit"]);
        let map = &ns[0].children[0];
        assert_eq!((map.start, map.end), (4, 10));
        assert_eq!(kinds(&map.children), ["kwd_lit", "num_lit"]);
        assert_eq!((map.children[0].start, map.children[0].end), (5, 7));
        assert_eq!((map.children[1].start, map.children[1].end), (8, 9));

        let tagged = read("#inst \"2020\"");
        assert_eq!(
            (tagged[0].kind, tagged[0].start, tagged[0].end),
            ("tagged_or_ctor_lit", 0, 12)
        );
        assert_eq!(kinds(&tagged[0].children), ["str_lit"]);
        assert_eq!(
            (tagged[0].children[0].start, tagged[0].children[0].end),
            (6, 12)
        );

        let bare = read("#inst");
        assert_eq!(
            (bare[0].kind, bare[0].start, bare[0].end),
            ("tagged_or_ctor_lit", 0, 5)
        );
        assert!(bare[0].children.is_empty());
    }

    // MU-8: metadata keeps its value children; a symbol target merges into one sym_lit
    #[test]
    fn mu_8_metadata_spans() {
        let pair = read("^{:a 1} [x]");
        assert_eq!(
            (pair[0].kind, pair[0].start, pair[0].end),
            ("meta_lit", 0, 11)
        );
        assert_eq!(kinds(&pair[0].children), ["map_lit", "vec_lit"]);
        assert_eq!((pair[0].children[0].start, pair[0].children[0].end), (1, 7));
        assert_eq!(
            (pair[0].children[1].start, pair[0].children[1].end),
            (8, 11)
        );

        let open = read("^foo");
        assert_eq!(
            (open[0].kind, open[0].start, open[0].end),
            ("meta_lit", 0, 4)
        );
        assert_eq!(kinds(&open[0].children), ["sym_lit"]);

        let bare = read("^");
        assert_eq!(
            (bare[0].kind, bare[0].start, bare[0].end),
            ("meta_lit", 0, 1)
        );
        assert!(bare[0].children.is_empty());
    }

    // MU-8: each reader prefix wraps the one form after it, and its span stops there
    #[test]
    fn mu_8_prefix_family_spans() {
        let forms = read("~@a ~b @c #'d");
        assert_eq!(
            kinds(&forms),
            [
                "unquote_splicing_lit",
                "unquoting_lit",
                "derefing_lit",
                "var_quoting_lit"
            ]
        );
        let spans: Vec<(usize, usize)> = forms.iter().map(|f| (f.start, f.end)).collect();
        assert_eq!(spans, [(0, 3), (4, 6), (7, 9), (10, 13)]);
        let inner: Vec<(usize, usize)> = forms
            .iter()
            .map(|f| (f.children[0].start, f.children[0].end))
            .collect();
        assert_eq!(inner, [(2, 3), (5, 6), (8, 9), (12, 13)]);
    }

    // MU-8: a prefix popped by a stray closer ends after its own marker
    #[test]
    fn mu_8_prefix_closed_by_stray_closer() {
        let forms = read("')");
        assert_eq!(forms.len(), 1);
        assert_eq!(
            (forms[0].kind, forms[0].start, forms[0].end),
            ("quoting_lit", 0, 1)
        );
    }

    // MU-8: a mismatched closer is dropped; lists and vectors still open at the end close there
    #[test]
    fn mu_8_mismatched_closers_are_dropped_and_open_forms_close_at_end() {
        let dropped = read("(a]b)");
        assert_eq!(
            (dropped[0].kind, dropped[0].start, dropped[0].end),
            ("list_lit", 0, 5)
        );
        assert_eq!(kinds(&dropped[0].children), ["sym_lit", "sym_lit"]);
        assert_eq!(
            (dropped[0].children[0].start, dropped[0].children[0].end),
            (1, 2)
        );
        assert_eq!(
            (dropped[0].children[1].start, dropped[0].children[1].end),
            (3, 4)
        );

        let lone = read("(]");
        assert_eq!(
            (lone[0].kind, lone[0].start, lone[0].end),
            ("list_lit", 0, 2)
        );
        assert!(lone[0].children.is_empty());

        let open_string = read("(\"a");
        assert_eq!(
            (
                open_string[0].kind,
                open_string[0].start,
                open_string[0].end
            ),
            ("list_lit", 0, 3)
        );
        assert_eq!(
            (
                open_string[0].children[0].start,
                open_string[0].children[0].end
            ),
            (1, 3)
        );

        let open_vec = read("[1 {2");
        assert_eq!(
            (open_vec[0].kind, open_vec[0].start, open_vec[0].end),
            ("vec_lit", 0, 5)
        );
        assert_eq!(kinds(&open_vec[0].children), ["num_lit", "map_lit"]);
        assert_eq!(
            (open_vec[0].children[1].start, open_vec[0].children[1].end),
            (3, 5)
        );
    }

    // MU-8: a comment inside a list is a child that ends at its newline
    #[test]
    fn mu_8_comment_inside_list_is_a_child() {
        let forms = read("(a ; c\n)");
        assert_eq!(
            (forms[0].kind, forms[0].start, forms[0].end),
            ("list_lit", 0, 8)
        );
        assert_eq!(kinds(&forms[0].children), ["sym_lit", "comment"]);
        assert_eq!(
            (forms[0].children[1].start, forms[0].children[1].end),
            (3, 6)
        );
    }

    // MU-8: vectors, maps and sets report their own kinds and spans
    #[test]
    fn mu_8_collection_spans() {
        let forms = read("[1 2] {:a 1} #{3}");
        assert_eq!(kinds(&forms), ["vec_lit", "map_lit", "set_lit"]);
        let spans: Vec<(usize, usize)> = forms.iter().map(|f| (f.start, f.end)).collect();
        assert_eq!(spans, [(0, 5), (6, 12), (13, 17)]);
        assert_eq!(
            (forms[0].children[1].start, forms[0].children[1].end),
            (3, 4)
        );
        assert_eq!(
            (forms[1].children[0].start, forms[1].children[0].end),
            (7, 9)
        );
        assert_eq!(
            (forms[2].children[0].start, forms[2].children[0].end),
            (15, 16)
        );
    }
}
