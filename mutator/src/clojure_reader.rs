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

enum Token {
    Skip,
    Open(&'static str, u8, usize),
    Close(u8),
    Prefix(&'static str, usize, usize),
    Leaf(&'static str, usize),
}

enum Shape {
    Root,
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
}

impl<'a> Reader<'a> {
    fn new(src: &'a str) -> Self {
        let root = Frame {
            shape: Shape::Root,
            kind: "",
            start: 0,
            children: Vec::new(),
        };
        Self {
            src,
            pos: 0,
            stack: vec![root],
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
        while self.stack.len() > 1 {
            self.pop_into_parent(self.src.len());
        }
        self.stack.pop().map_or_else(Vec::new, |root| root.children)
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
    Some(Token::Prefix(kind, 1, 1 + token_len(body)))
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
    let mut escaped = false;
    for (index, byte) in rest.bytes().enumerate().skip(1) {
        match (escaped, byte) {
            (true, _) => escaped = false,
            (false, b'\\') => escaped = true,
            (false, b'"') => return index + 1,
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
    let len = token_len(rest).max(1);
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
}
