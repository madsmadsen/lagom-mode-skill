// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Port of `src/dryer/clojure_read.py`: reads Clojure source into forms
//! and normalizes them the way dry4clj does.
//!
//! Names in argument position, keywords, and literals become generic markers.
//! The symbol at the head of a list keeps its spelling, so `filter` and `map`
//! stay distinct while the locals passed to them do not.
//!
//! Reader conditionals keep the `:clj` branch, which is what dry4clj selects.
//! Syntax-quote is kept as a `syntax-quote` list instead of being expanded.

use std::sync::LazyLock;

use regex::Regex;

use crate::model::Norm;

const NUMBER_PATTERN: &str = r"^[+-]?(?:\d+/\d+|\d+\.\d+(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?|\d+(?:[eE][+-]?\d+)?|0[xX][0-9A-Fa-f]+)[MN]?$";

/// Collection kinds the reader produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollKind {
    List,
    Vector,
    Map,
    Set,
}

impl CollKind {
    fn name(self) -> &'static str {
        match self {
            CollKind::List => "list",
            CollKind::Vector => "vector",
            CollKind::Map => "map",
            CollKind::Set => "set",
        }
    }
}

/// A form read from source. `Coll` maps use `pairs`; every other kind uses `items`.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Sym {
        name: String,
        line: usize,
    },
    Kw {
        name: String,
        line: usize,
    },
    Lit {
        line: usize,
    },
    Coll {
        kind: CollKind,
        line: usize,
        items: Vec<Form>,
        pairs: Vec<(Form, Form)>,
    },
}

impl Form {
    pub fn line(&self) -> usize {
        match self {
            Form::Sym { line, .. }
            | Form::Kw { line, .. }
            | Form::Lit { line }
            | Form::Coll { line, .. } => *line,
        }
    }
}

#[derive(Debug)]
struct ReadError {
    line: usize,
    message: String,
}

fn err(line: usize, message: &str) -> ReadError {
    ReadError {
        line,
        message: message.to_string(),
    }
}

fn is_ws(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | ',')
}

fn is_term(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '"'
            | ';'
            | '\''
            | '@'
            | '^'
            | '`'
            | '~'
            | '\\'
            | ','
            | '#'
            | ':'
    )
}

fn is_delim(ch: char) -> bool {
    is_ws(ch) || is_term(ch)
}

/// An open collection waiting for its closing delimiter.
#[derive(Debug)]
struct CollFrame {
    end: char,
    kind: CollKind,
    line: usize,
    items: Vec<Form>,
    fn_star: bool,
}

impl CollFrame {
    fn new(end: char, kind: CollKind, line: usize, fn_star: bool) -> Self {
        Self {
            end,
            kind,
            line,
            items: Vec::new(),
            fn_star,
        }
    }

    fn accept(&mut self, produced: Produced) {
        match produced {
            Produced::Form(form) => self.items.push(form),
            Produced::Splice(forms) => self.items.extend(forms),
            Produced::Nil | Produced::Need => {}
        }
    }
}

/// Explicit parser stack frame (replaces Python recursion).
#[derive(Debug)]
enum Frame {
    Coll(CollFrame),
    Wrap { name: &'static str, line: usize },
    Discard,
    Meta,
    Lit { line: usize },
    Cond { splicing: bool },
}

/// Result of one parser step. `Need` means "read another token";
/// `Nil` is a discarded form; `Splice` is a spliced reader-conditional body.
#[derive(Debug)]
enum Produced {
    Need,
    Nil,
    Form(Form),
    Splice(Vec<Form>),
}

struct Reader {
    chars: Vec<char>,
    pos: usize,
    line: usize,
}

impl Reader {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    /// Consumes one character, counting newlines. `None` at end of input.
    fn get(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
        }
        Some(ch)
    }

    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(ch) if is_ws(ch) => {
                    self.get();
                }
                Some(';') => {
                    while self.peek().is_some_and(|ch| ch != '\n') {
                        self.get();
                    }
                }
                _ => break,
            }
        }
    }

    fn skip_shebang(&mut self) {
        if self.chars.starts_with(&['#', '!']) {
            while self.peek().is_some_and(|ch| ch != '\n') {
                self.get();
            }
        }
    }

    fn read_token(&mut self) -> Result<String, ReadError> {
        let start = self.pos;
        let line = self.line;
        while self.peek().is_some_and(|ch| !is_delim(ch)) {
            self.get();
        }
        if self.pos == start {
            return Err(err(line, "expected a symbol"));
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn read_string(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        self.get();
        while let Some(ch) = self.get() {
            match ch {
                '\\' => {
                    if self.eof() {
                        break;
                    }
                    self.get();
                }
                '"' => return Ok(Form::Lit { line }),
                _ => {}
            }
        }
        Err(err(line, "unterminated string"))
    }

    fn read_char(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        self.get();
        if self.eof() {
            return Err(err(line, "incomplete character"));
        }
        self.get();
        while self.peek().is_some_and(|ch| !is_delim(ch)) {
            self.get();
        }
        Ok(Form::Lit { line })
    }

    fn read_keyword(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        self.get();
        if self.peek() == Some(':') {
            self.get();
        }
        match self.peek() {
            None => Ok(Form::Kw {
                name: String::new(),
                line,
            }),
            Some(ch) if is_delim(ch) => Ok(Form::Kw {
                name: String::new(),
                line,
            }),
            Some(_) => Ok(Form::Kw {
                name: self.read_token()?,
                line,
            }),
        }
    }

    /// One form. `Nil` and `Splice` are real results.
    fn read_form(&mut self) -> Result<Produced, ReadError> {
        let mut stack = Vec::new();
        let mut produced = Produced::Need;
        loop {
            produced = match produced {
                Produced::Need => self.next(&mut stack)?,
                other if stack.is_empty() => return Ok(other),
                other => self.deliver(&mut stack, other)?,
            };
        }
    }

    fn next(&mut self, stack: &mut Vec<Frame>) -> Result<Produced, ReadError> {
        self.skip_ws();
        if let Some(closed) = self.close_if_at_end(stack)? {
            return Ok(closed);
        }
        let Some(ch) = self.peek() else {
            return Err(err(self.line, "unexpected end of file"));
        };
        self.open(stack, ch)
    }

    /// Closes the innermost collection when its closing delimiter is next.
    fn close_if_at_end(&mut self, stack: &mut Vec<Frame>) -> Result<Option<Produced>, ReadError> {
        let Some(Frame::Coll(coll)) = stack.last() else {
            return Ok(None);
        };
        if self.eof() {
            return Err(err(
                coll.line,
                &format!("unterminated {}", coll.kind.name()),
            ));
        }
        if self.peek() != Some(coll.end) {
            return Ok(None);
        }
        self.get();
        let Some(Frame::Coll(coll)) = stack.pop() else {
            return Ok(None);
        };
        Ok(Some(Produced::Form(close_coll(coll)?)))
    }

    fn deliver(&self, stack: &mut Vec<Frame>, produced: Produced) -> Result<Produced, ReadError> {
        let Some(frame) = stack.pop() else {
            return Ok(produced);
        };
        self.finish_frame(stack, frame, produced)
    }

    fn finish_frame(
        &self,
        stack: &mut Vec<Frame>,
        frame: Frame,
        produced: Produced,
    ) -> Result<Produced, ReadError> {
        match frame {
            Frame::Coll(mut coll) => {
                coll.accept(produced);
                stack.push(Frame::Coll(coll));
                Ok(Produced::Need)
            }
            Frame::Wrap { name, line } => Ok(Produced::Form(wrap_form(name, line, produced))),
            Frame::Discard => Ok(Produced::Nil),
            Frame::Meta => Ok(Produced::Need),
            Frame::Lit { line } => Ok(Produced::Form(Form::Lit { line })),
            Frame::Cond { splicing } => self.finish_cond(splicing, produced),
        }
    }

    fn finish_cond(&self, splicing: bool, produced: Produced) -> Result<Produced, ReadError> {
        let Produced::Form(Form::Coll {
            kind: CollKind::List,
            items,
            ..
        }) = produced
        else {
            return Err(err(self.line, "reader conditional body must be a list"));
        };
        let Some(chosen) = chosen_branch(items) else {
            return Ok(Produced::Nil);
        };
        if !splicing {
            return Ok(Produced::Form(chosen));
        }
        splice(chosen)
            .map(Produced::Splice)
            .ok_or_else(|| err(self.line, "splicing reader conditional needs a collection"))
    }

    fn open(&mut self, stack: &mut Vec<Frame>, ch: char) -> Result<Produced, ReadError> {
        let line = self.line;
        match ch {
            '(' | '[' | '{' => Ok(self.open_collection(stack, ch, line)),
            '"' => self.read_string().map(Produced::Form),
            '\\' => self.read_char().map(Produced::Form),
            ':' => self.read_keyword().map(Produced::Form),
            '\'' | '@' | '`' | '~' | '^' => {
                self.open_wrapper(stack, ch, line);
                Ok(Produced::Need)
            }
            '#' => self.dispatch(stack, line),
            _ => self.open_token(line).map(Produced::Form),
        }
    }

    fn open_collection(&mut self, stack: &mut Vec<Frame>, ch: char, line: usize) -> Produced {
        let (end, kind) = match ch {
            '(' => (')', CollKind::List),
            '[' => (']', CollKind::Vector),
            _ => ('}', CollKind::Map),
        };
        self.open_coll(stack, end, kind, line, false)
    }

    /// Consumes the opening delimiter and pushes a collection frame.
    fn open_coll(
        &mut self,
        stack: &mut Vec<Frame>,
        end: char,
        kind: CollKind,
        line: usize,
        fn_star: bool,
    ) -> Produced {
        self.get();
        stack.push(Frame::Coll(CollFrame::new(end, kind, line, fn_star)));
        Produced::Need
    }

    fn open_wrapper(&mut self, stack: &mut Vec<Frame>, ch: char, line: usize) {
        self.get();
        let frame = match ch {
            '\'' => Frame::Wrap {
                name: "quote",
                line,
            },
            '@' => Frame::Wrap {
                name: "deref",
                line,
            },
            '`' => Frame::Wrap {
                name: "syntax-quote",
                line,
            },
            '~' => self.unquote_frame(line),
            _ => Frame::Meta,
        };
        stack.push(frame);
    }

    fn unquote_frame(&mut self, line: usize) -> Frame {
        if self.peek() == Some('@') {
            self.get();
            Frame::Wrap {
                name: "unquote-splicing",
                line,
            }
        } else {
            Frame::Wrap {
                name: "unquote",
                line,
            }
        }
    }

    fn open_token(&mut self, line: usize) -> Result<Form, ReadError> {
        let token = self.read_token()?;
        if is_number(&token) {
            Ok(Form::Lit { line })
        } else {
            Ok(Form::Sym { name: token, line })
        }
    }

    fn dispatch(&mut self, stack: &mut Vec<Frame>, line: usize) -> Result<Produced, ReadError> {
        self.get();
        let Some(ch) = self.peek() else {
            return Err(err(line, "incomplete dispatch"));
        };
        match ch {
            '_' => self.dispatch_discard(stack),
            '{' => self.dispatch_set(stack),
            '(' => self.dispatch_fn(stack, line),
            '"' => self.dispatch_string(line),
            '?' => self.dispatch_cond(stack),
            ':' => self.dispatch_map(),
            _ => self.dispatch_extension(stack, line),
        }
    }

    fn dispatch_discard(&mut self, stack: &mut Vec<Frame>) -> Result<Produced, ReadError> {
        self.get();
        stack.push(Frame::Discard);
        Ok(Produced::Need)
    }

    fn dispatch_set(&mut self, stack: &mut Vec<Frame>) -> Result<Produced, ReadError> {
        let line = self.line;
        Ok(self.open_coll(stack, '}', CollKind::Set, line, false))
    }

    fn dispatch_fn(&mut self, stack: &mut Vec<Frame>, line: usize) -> Result<Produced, ReadError> {
        Ok(self.open_coll(stack, ')', CollKind::List, line, true))
    }

    fn dispatch_string(&mut self, line: usize) -> Result<Produced, ReadError> {
        self.read_string()?;
        Ok(Produced::Form(Form::Lit { line }))
    }

    fn dispatch_cond(&mut self, stack: &mut Vec<Frame>) -> Result<Produced, ReadError> {
        self.get();
        let splicing = self.peek() == Some('@');
        if splicing {
            self.get();
        }
        stack.push(Frame::Cond { splicing });
        Ok(Produced::Need)
    }

    fn dispatch_map(&mut self) -> Result<Produced, ReadError> {
        self.get();
        self.read_map_qualifier()?;
        Ok(Produced::Need)
    }

    /// Skips the optional namespace after `#:` (or the auto-resolve `#::`).
    fn read_map_qualifier(&mut self) -> Result<(), ReadError> {
        match self.peek() {
            Some(':') => {
                self.get();
                Ok(())
            }
            None | Some('{') => Ok(()),
            Some(ch) if is_ws(ch) => Ok(()),
            Some(_) => self.read_token().map(|_| ()),
        }
    }

    fn dispatch_extension(
        &mut self,
        stack: &mut Vec<Frame>,
        line: usize,
    ) -> Result<Produced, ReadError> {
        self.read_token()?;
        stack.push(Frame::Lit { line });
        Ok(Produced::Need)
    }
}

static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(NUMBER_PATTERN).expect("static number pattern"));

fn is_number(token: &str) -> bool {
    NUMBER.is_match(token)
}

fn close_coll(coll: CollFrame) -> Result<Form, ReadError> {
    let CollFrame {
        kind,
        line,
        items,
        fn_star,
        ..
    } = coll;
    if fn_star {
        let mut all = vec![Form::Sym {
            name: "fn*".to_string(),
            line,
        }];
        all.extend(items);
        return Ok(Form::Coll {
            kind: CollKind::List,
            line,
            items: all,
            pairs: Vec::new(),
        });
    }
    match kind {
        CollKind::Map => map_form(line, items),
        _ => Ok(Form::Coll {
            kind,
            line,
            items,
            pairs: Vec::new(),
        }),
    }
}

fn map_form(line: usize, items: Vec<Form>) -> Result<Form, ReadError> {
    if items.len() % 2 == 1 {
        return Err(err(line, "map literal has an odd number of forms"));
    }
    let mut pairs = Vec::with_capacity(items.len().div_ceil(2));
    let mut iter = items.into_iter();
    while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
        pairs.push((key, value));
    }
    Ok(Form::Coll {
        kind: CollKind::Map,
        line,
        items: Vec::new(),
        pairs,
    })
}

fn wrap_form(name: &str, line: usize, produced: Produced) -> Form {
    let mut items = vec![Form::Sym {
        name: name.to_string(),
        line,
    }];
    if let Produced::Form(form) = produced {
        items.push(form);
    }
    Form::Coll {
        kind: CollKind::List,
        line,
        items,
        pairs: Vec::new(),
    }
}

fn branch_name(feature: &Form) -> Option<&str> {
    match feature {
        Form::Kw { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

/// Picks the body of the first `:clj` or `:default` branch of a reader conditional.
fn chosen_branch(mut items: Vec<Form>) -> Option<Form> {
    let index = (0..items.len() / 2)
        .find(|&k| matches!(branch_name(&items[2 * k]), Some("clj" | "default")))?;
    Some(items.swap_remove(2 * index + 1))
}

fn splice(chosen: Form) -> Option<Vec<Form>> {
    match chosen {
        Form::Coll {
            kind: CollKind::List | CollKind::Vector,
            items,
            ..
        } => Some(items),
        _ => None,
    }
}

/// Top-level forms, plus a warning if a later form could not be read.
/// The warning is `"<line>: <message>"`; forms read before the error are kept.
pub fn read_source(text: &str) -> (Vec<Form>, Option<String>) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader::new(text);
    reader.skip_shebang();
    let mut forms = Vec::new();
    loop {
        reader.skip_ws();
        if reader.eof() {
            return (forms, None);
        }
        match reader.read_form() {
            Ok(Produced::Form(form)) => forms.push(form),
            Ok(_) => {}
            Err(error) => return (forms, Some(format!("{}: {}", error.line, error.message))),
        }
    }
}

/// Whether a top-level form is a non-empty list whose head is not `ns`.
pub fn is_candidate_form(form: &Form) -> bool {
    match form {
        Form::Coll {
            kind: CollKind::List,
            items,
            ..
        } => match items.first() {
            Some(Form::Sym { name, .. }) => name != "ns",
            Some(_) => true,
            None => false,
        },
        _ => false,
    }
}

/// Largest source line mentioned anywhere in the form. Iterative: no recursion.
pub fn max_line(form: &Form) -> usize {
    let mut best = form.line();
    let mut stack: Vec<&Form> = vec![form];
    while let Some(current) = stack.pop() {
        best = best.max(current.line());
        if let Form::Coll { items, pairs, .. } = current {
            stack.extend(items.iter());
            stack.extend(pairs.iter().flat_map(|(key, value)| [key, value]));
        }
    }
    best
}

enum Task<'a> {
    Visit(&'a Form, bool),
    Build(&'a Form, usize),
}

/// dry4clj's `normalize-form` with `head = false`. Post-order over an explicit
/// task stack; child results are moved, never copied, so cost is linear.
pub fn normalize(form: &Form) -> Norm {
    let mut tasks = vec![Task::Visit(form, false)];
    let mut done: Vec<Norm> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(node, head) => visit(node, head, &mut tasks, &mut done),
            Task::Build(node, count) => {
                let kids = done.split_off(done.len().saturating_sub(count));
                done.push(assemble(node, kids));
            }
        }
    }
    done.pop().unwrap_or(Norm::Kw("literal"))
}

fn visit<'a>(node: &'a Form, head: bool, tasks: &mut Vec<Task<'a>>, done: &mut Vec<Norm>) {
    if !matches!(node, Form::Coll { .. }) {
        done.push(atom_norm(node, head));
        return;
    }
    let children = norm_children(node);
    tasks.push(Task::Build(node, children.len()));
    for (child, child_head) in children.into_iter().rev() {
        tasks.push(Task::Visit(child, child_head));
    }
}

fn atom_norm(form: &Form, head: bool) -> Norm {
    match form {
        Form::Sym { name, .. } if head => {
            Norm::List(vec![Norm::Kw("symbol"), Norm::Str(name.clone())])
        }
        Form::Sym { .. } => Norm::Kw("symbol"),
        Form::Kw { .. } => Norm::Kw("keyword"),
        _ => Norm::Kw("literal"),
    }
}

/// Children in normalization order; a list's first item is the head.
fn norm_children(node: &Form) -> Vec<(&Form, bool)> {
    match node {
        Form::Coll {
            kind: CollKind::List,
            items,
            ..
        } => items
            .iter()
            .enumerate()
            .map(|(index, item)| (item, index == 0))
            .collect(),
        Form::Coll {
            kind: CollKind::Vector | CollKind::Set,
            items,
            ..
        } => items.iter().map(|item| (item, false)).collect(),
        Form::Coll {
            kind: CollKind::Map,
            pairs,
            ..
        } => pairs
            .iter()
            .flat_map(|(key, value)| [(key, false), (value, false)])
            .collect(),
        _ => Vec::new(),
    }
}

fn assemble(node: &Form, kids: Vec<Norm>) -> Norm {
    match node {
        Form::Coll {
            kind: CollKind::List,
            ..
        } if kids.is_empty() => Norm::List(vec![Norm::Kw("list"), Norm::Kw("literal")]),
        Form::Coll {
            kind: CollKind::List,
            ..
        } => with_tag("list", kids),
        Form::Coll {
            kind: CollKind::Vector,
            ..
        } => with_tag("vector", kids),
        Form::Coll {
            kind: CollKind::Set,
            ..
        } => with_tag("set", kids),
        Form::Coll {
            kind: CollKind::Map,
            ..
        } => map_norm(kids),
        _ => Norm::Kw("literal"),
    }
}

fn with_tag(tag: &'static str, kids: Vec<Norm>) -> Norm {
    let mut out = vec![Norm::Kw(tag)];
    out.extend(kids);
    Norm::List(out)
}

fn map_norm(kids: Vec<Norm>) -> Norm {
    let mut out = vec![Norm::Kw("map")];
    let mut iter = kids.into_iter();
    while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
        out.push(Norm::List(vec![key, value]));
    }
    Norm::List(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::pr;

    fn read_clean(text: &str) -> Vec<Form> {
        let (forms, warning) = read_source(text);
        assert_eq!(warning, None, "source: {text:?}");
        forms
    }

    fn warning_of(text: &str) -> Option<String> {
        read_source(text).1
    }

    fn items_of(form: &Form) -> &[Form] {
        match form {
            Form::Coll { items, .. } => items,
            other => panic!("not a collection: {other:?}"),
        }
    }

    fn pairs_of(form: &Form) -> &[(Form, Form)] {
        match form {
            Form::Coll { pairs, .. } => pairs,
            other => panic!("not a map: {other:?}"),
        }
    }

    fn name_of(form: &Form) -> &str {
        match form {
            Form::Sym { name, .. } | Form::Kw { name, .. } => name,
            other => panic!("no name: {other:?}"),
        }
    }

    fn sym(name: &str) -> Form {
        Form::Sym {
            name: name.to_string(),
            line: 1,
        }
    }

    fn coll(kind: CollKind, items: Vec<Form>) -> Form {
        Form::Coll {
            kind,
            line: 1,
            items,
            pairs: Vec::new(),
        }
    }

    /// Runs a job on a 512 MiB stack, as the CLI does for deep forms.
    fn on_big_stack(job: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .spawn(job)
            .expect("spawn big-stack thread")
            .join()
            .expect("big-stack job panicked");
    }

    // DR-8: top-level forms keep their start lines.
    #[test]
    fn dr8_top_level_forms_keep_their_start_lines() {
        let forms = read_clean("(ns a)\n\n(defn b [x] x)\n");
        let lines: Vec<usize> = forms.iter().map(Form::line).collect();
        assert_eq!(lines, vec![1, 3]);
    }

    // DR-8: a leading BOM and a shebang line are not forms.
    #[test]
    fn dr8_bom_and_shebang_are_not_forms() {
        let forms = read_clean("\u{feff}#!/usr/bin/env bb\n(defn alpha [x]\n  (inc x))\n");
        assert_eq!(forms.len(), 1);
        assert_eq!(name_of(&items_of(&forms[0])[0]), "defn");
        assert!(read_clean("#!only").is_empty());
    }

    // DR-8: escaped strings and character literals read cleanly.
    #[test]
    fn dr8_reads_escaped_strings_and_characters() {
        let forms = read_clean("(defn alpha [x]\n  (str \"a\\\\nb\" \\newline \\a))\n");
        assert_eq!(name_of(&items_of(&forms[0])[0]), "defn");
    }

    // DR-8: a character name is consumed as one literal.
    #[test]
    fn dr8_a_character_name_is_consumed() {
        let forms = read_clean("(defn alpha []\n  (str \\newline))\n");
        let call = &items_of(&forms[0])[3];
        let kinds: Vec<&str> = items_of(call)
            .iter()
            .map(|item| match item {
                Form::Sym { .. } => "Sym",
                Form::Lit { .. } => "Lit",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["Sym", "Lit"]);
    }

    // DR-8: numbers are literals; other tokens are symbols.
    #[test]
    fn dr8_numbers_are_literals_and_other_tokens_are_symbols() {
        let forms = read_clean("(f 1 1.5 1/2 0xFF 1M -2e3 foo)\n");
        let kinds: Vec<&str> = items_of(&forms[0])
            .iter()
            .map(|item| match item {
                Form::Sym { .. } => "Sym",
                Form::Lit { .. } => "Lit",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["Sym", "Lit", "Lit", "Lit", "Lit", "Lit", "Lit", "Sym"]
        );
    }

    // DR-8: reader errors carry the line and keep the forms read before them.
    #[test]
    fn dr8_unterminated_list_keeps_forms_read_so_far() {
        let (forms, warning) =
            read_source("(ns left)\n\n(defn alpha [xs]\n  (inc xs))\n(defn broken [x]\n");
        assert_eq!(warning.as_deref(), Some("5: unterminated list"));
        assert_eq!(forms.len(), 2);
    }

    // DR-8: broken strings, characters, and collections report the right message.
    #[test]
    fn dr8_reader_error_messages_match_the_python_reader() {
        let cases = [
            ("(defn alpha [x]\n  \"abc)\n", "2: unterminated string"),
            ("(defn alpha [x] \"abc\\", "1: unterminated string"),
            ("(defn alpha [x] \\", "1: incomplete character"),
            ("'", "1: unexpected end of file"),
            ("#", "1: incomplete dispatch"),
            ("#[", "1: expected a symbol"),
            (")", "1: expected a symbol"),
            ("[1 2", "1: unterminated vector"),
            ("#{1", "1: unterminated set"),
            ("{:a}", "1: map literal has an odd number of forms"),
            ("#?[:clj 1]", "1: reader conditional body must be a list"),
            (
                "#?@(:clj {:a 1})",
                "1: splicing reader conditional needs a collection",
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(
                warning_of(source).as_deref(),
                Some(expected),
                "source: {source:?}"
            );
        }
    }

    // DR-11: reader conditionals keep the :clj (or :default) branch.
    #[test]
    fn dr11_reader_conditional_selects_clj_or_default() {
        let forms = read_clean("#?(:cljs (inc x) :default (dec x))\n");
        assert_eq!(name_of(&items_of(&forms[0])[0]), "dec");
        let forms = read_clean("#?(:clj (inc x) :extra)\n");
        assert_eq!(name_of(&items_of(&forms[0])[0]), "inc");
    }

    // DR-11: a conditional with no matching branch yields nothing.
    #[test]
    fn dr11_reader_conditional_without_a_match_is_dropped() {
        assert!(read_clean("#?(:cljs (dec x) :extra)\n").is_empty());
        assert!(read_clean("#?((inc x) (dec x))\n").is_empty());
    }

    // DR-11: #?@ splices a list or vector body into the enclosing collection.
    #[test]
    fn dr11_splicing_reader_conditional_splices_a_collection() {
        let forms = read_clean("(defn alpha []\n  [#?@(:clj [:a :b])])\n");
        let spliced: Vec<&str> = items_of(&items_of(&forms[0])[3])
            .iter()
            .map(name_of)
            .collect();
        assert_eq!(spliced, vec!["a", "b"]);
    }

    // DR-11: a reader conditional body that is not a list is a read error.
    #[test]
    fn dr11_reader_conditional_body_must_be_a_list() {
        assert_eq!(
            warning_of("#?[:clj (inc x)]\n").as_deref(),
            Some("1: reader conditional body must be a list")
        );
    }

    // DR-11: #(...) reads as (fn* ...).
    #[test]
    fn dr11_fn_literal_reads_as_fn_star_list() {
        let forms = read_clean("#(inc %)\n");
        let items = items_of(&forms[0]);
        assert_eq!(items.len(), 3);
        assert_eq!(name_of(&items[0]), "fn*");
    }

    // DR-8: #_ discards the following form and does not wrap anything.
    #[test]
    fn dr8_discard_reads_nothing_and_quote_discard_wraps_nothing() {
        let forms = read_clean("#_(println xs)\n(inc xs)\n");
        assert_eq!(forms.len(), 1);
        assert_eq!(items_of(&forms[0]).len(), 2);
        let forms = read_clean("'#_x\n");
        let names: Vec<&str> = items_of(&forms[0]).iter().map(name_of).collect();
        assert_eq!(names, vec!["quote"]);
    }

    // DR-8: metadata (^) is dropped and the following form is kept.
    #[test]
    fn dr8_metadata_is_not_structure() {
        let forms = read_clean("(defn ^String beta [items]\n  (inc items))\n");
        let items = items_of(&forms[0]);
        assert_eq!(items.len(), 4);
        assert_eq!(name_of(&items[1]), "beta");
        let forms = read_clean("^{:a 1} (foo)\n");
        assert_eq!(name_of(&items_of(&forms[0])[0]), "foo");
    }

    // DR-8: reader macros wrap the following form in a named list.
    #[test]
    fn dr8_reader_macros_wrap_the_following_form() {
        let forms = read_clean("'x\n@x\n`x\n~x\n~@x\n");
        let heads: Vec<&str> = forms
            .iter()
            .map(|form| name_of(&items_of(form)[0]))
            .collect();
        assert_eq!(
            heads,
            vec![
                "quote",
                "deref",
                "syntax-quote",
                "unquote",
                "unquote-splicing"
            ]
        );
        assert_eq!(name_of(&items_of(&forms[0])[1]), "x");
    }

    // DR-8: tagged literals become a single literal.
    #[test]
    fn dr8_tagged_literal_becomes_one_literal() {
        let forms = read_clean("#inst \"2020-01-01\"\n");
        assert_eq!(forms.len(), 1);
        assert!(matches!(forms[0], Form::Lit { .. }));
    }

    // DR-8: a bare colon is an empty keyword.
    #[test]
    fn dr8_a_bare_colon_is_an_empty_keyword() {
        let forms = read_clean(":\n");
        assert_eq!(name_of(&forms[0]), "");
    }

    // DR-8: namespaced map prefixes keep the map.
    #[test]
    fn dr8_namespaced_maps_keep_the_map() {
        for source in ["#:foo{:a 1}\n", "#::{:a 1}\n", "#:{:a 1}\n"] {
            let forms = read_clean(source);
            assert!(
                matches!(
                    forms[0],
                    Form::Coll {
                        kind: CollKind::Map,
                        ..
                    }
                ),
                "source: {source:?}"
            );
            assert_eq!(name_of(&pairs_of(&forms[0])[0].0), "a");
        }
    }

    // DR-8: map pairs keep source order and their own lines.
    #[test]
    fn dr8_map_pairs_keep_source_order_and_lines() {
        let forms = read_clean("{:a 1 :b 2}\n");
        let keys: Vec<&str> = pairs_of(&forms[0])
            .iter()
            .map(|(key, _)| name_of(key))
            .collect();
        assert_eq!(keys, vec!["a", "b"]);
        let forms = read_clean("{:a 1\n :b 2}\n");
        let lines: Vec<usize> = pairs_of(&forms[0])
            .iter()
            .map(|(_, value)| value.line())
            .collect();
        assert_eq!(lines, vec![1, 2]);
    }

    // DR-9: an empty list normalizes to a literal list.
    #[test]
    fn dr9_empty_list_normalizes_to_a_literal_list() {
        let forms = read_clean("(defn alpha [x]\n  ())\n");
        assert!(pr(&normalize(&forms[0])).contains("[:list :literal]"));
    }

    // DR-9: call heads keep their spelling; arguments and literals are erased.
    #[test]
    fn dr9_call_heads_keep_spelling_and_arguments_are_erased() {
        let forms = read_clean("(filter odd? xs)\n");
        let printed = pr(&normalize(&forms[0]));
        assert!(printed.contains("\"filter\""), "printed: {printed}");
        assert!(!printed.contains("odd"), "printed: {printed}");
        assert!(!printed.contains("xs"), "printed: {printed}");
    }

    // DR-9: keywords, literals, and tree shape survive normalization exactly.
    #[test]
    fn dr9_normalization_keeps_kinds_and_tree_shape() {
        let forms = read_clean("(foo :a \"s\" 1)\n");
        assert_eq!(
            pr(&normalize(&forms[0])),
            "[:list [:symbol \"foo\"] :keyword :literal :literal]"
        );
    }

    // DR-9: map keys and values are erased but keep their order.
    #[test]
    fn dr9_map_names_are_erased() {
        let left = read_clean("{foo 1}\n");
        let right = read_clean("{bar 2}\n");
        let printed = pr(&normalize(&left[0]));
        assert!(!printed.contains("foo"), "printed: {printed}");
        assert_eq!(printed, pr(&normalize(&right[0])));
        let symbols = read_clean("{foo bar}\n");
        let printed = pr(&normalize(&symbols[0]));
        assert!(
            !printed.contains("foo") && !printed.contains("bar"),
            "printed: {printed}"
        );
    }

    // DR-9: a bare symbol that is not at the head of a list is erased.
    #[test]
    fn dr9_a_bare_symbol_form_is_erased() {
        let forms = read_clean("foo\n");
        assert_eq!(normalize(&forms[0]), Norm::Kw("symbol"));
    }

    // DR-9: set members are erased.
    #[test]
    fn dr9_names_inside_a_set_are_erased() {
        let forms = read_clean("#{foo bar}\n");
        let printed = pr(&normalize(&forms[0]));
        assert!(printed.starts_with("[:set "), "printed: {printed}");
        assert!(!printed.contains("foo"), "printed: {printed}");
    }

    // DR-11: only a non-empty list whose head is not ns is a candidate.
    #[test]
    fn dr11_only_a_non_empty_list_that_is_not_ns_is_a_candidate() {
        assert!(!is_candidate_form(&coll(CollKind::Vector, vec![sym("a")])));
        assert!(!is_candidate_form(&coll(CollKind::List, Vec::new())));
        assert!(!is_candidate_form(&sym("a")));
        assert!(!is_candidate_form(&coll(
            CollKind::List,
            vec![sym("ns"), sym("demo")]
        )));
        assert!(is_candidate_form(&coll(CollKind::List, vec![sym("defn")])));
    }

    // DR-8: max_line reports the deepest line mentioned in the form.
    #[test]
    fn dr8_max_line_finds_the_deepest_line() {
        let forms = read_clean("(a\n\n(b\n c))\n");
        assert_eq!(max_line(&forms[0]), 4);
        assert_eq!(max_line(&forms[0]), max_line(&forms[0]));
    }

    // DR-8: a 1,200-deep nesting reads, measures, and normalizes without overflow.
    #[test]
    fn dr8_deep_nesting_reads_and_normalizes_without_overflow() {
        on_big_stack(|| {
            let depth = 1200;
            let source = format!("{}x{}\n", "(".repeat(depth), ")".repeat(depth));
            let (forms, warning) = read_source(&source);
            assert_eq!(warning, None);
            assert_eq!(forms.len(), 1);
            assert!(is_candidate_form(&forms[0]));
            assert_eq!(max_line(&forms[0]), 1);
            assert!(pr(&normalize(&forms[0])).starts_with("[:list "));
        });
    }

    // DR-8: an unterminated 1,200-deep nesting reports the innermost open collection.
    #[test]
    fn dr8_deep_unterminated_nesting_reports_without_overflow() {
        on_big_stack(|| {
            let source = "[".repeat(1200);
            let (forms, warning) = read_source(&source);
            assert!(forms.is_empty());
            assert_eq!(warning.as_deref(), Some("1: unterminated vector"));
        });
    }

    // DR-8: a regex literal reads as one literal and normalizes as one.
    #[test]
    fn dr8_regex_literal_reads_as_one_literal() {
        let forms = read_clean("#\"a.*b\"\n");
        assert_eq!(forms.len(), 1);
        assert!(matches!(forms[0], Form::Lit { .. }));
        assert_eq!(pr(&normalize(&forms[0])), ":literal");
    }

    // DR-9: a regex literal inside a form normalizes to a literal argument.
    #[test]
    fn dr9_regex_literal_inside_a_form_normalizes_to_literal() {
        let forms = read_clean("(re-find #\"a.*b\" s)\n");
        assert_eq!(
            pr(&normalize(&forms[0])),
            "[:list [:symbol \"re-find\"] :literal :symbol]"
        );
    }

    // DR-8: an escaped quote does not end a regex literal.
    #[test]
    fn dr8_regex_literal_with_escaped_quote_reads_cleanly() {
        let forms = read_clean("#\"a\\\"b\"\n");
        assert_eq!(forms.len(), 1);
        assert!(matches!(forms[0], Form::Lit { .. }));
    }

    // DR-8: an unterminated regex literal reports an unterminated string.
    #[test]
    fn dr8_unterminated_regex_literal_reports_unterminated_string() {
        assert_eq!(
            warning_of("#\"abc\n").as_deref(),
            Some("1: unterminated string")
        );
    }

    // DR-8: a comment inside a form is skipped up to the end of its line.
    #[test]
    fn dr8_comment_inside_a_form_is_skipped() {
        let forms = read_clean("(foo ; ignored x y z\n bar)\n");
        let names: Vec<&str> = items_of(&forms[0]).iter().map(name_of).collect();
        assert_eq!(names, vec!["foo", "bar"]);
    }

    // DR-8: a comment at top level before a form is skipped.
    #[test]
    fn dr8_top_level_comment_before_a_form_is_skipped() {
        let forms = read_clean("; header\n(foo)\n");
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].line(), 2);
    }

    // DR-8: an auto-resolved keyword keeps its name without the colons.
    #[test]
    fn dr8_auto_resolved_keyword_keeps_its_name() {
        let forms = read_clean("::auto\n");
        assert_eq!(name_of(&forms[0]), "auto");
    }

    // DR-8: a bare double colon is an empty keyword.
    #[test]
    fn dr8_bare_double_colon_is_an_empty_keyword() {
        let forms = read_clean("::\n");
        assert_eq!(name_of(&forms[0]), "");
    }

    // DR-8: a keyword may contain a slash for its namespace.
    #[test]
    fn dr8_namespaced_keyword_keeps_the_slash() {
        let forms = read_clean(":a/b\n");
        assert_eq!(name_of(&forms[0]), "a/b");
    }
}
