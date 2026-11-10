//! Port of mutator/src/mutator/edn.py (BSD Zero Clause License, Robert C. Martin).

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Keyword(String),
    Symbol(String),
    Vector(Vec<Value>),
    List(Vec<Value>),
    Set(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

/// Renders a value as compact EDN, byte for byte like the Python `edn.dumps`.
pub fn render(value: &Value) -> String {
    match value {
        Value::Vector(items) => wrap("[", items.iter().map(render).collect(), "]"),
        Value::List(items) => wrap("(", items.iter().map(render).collect(), ")"),
        Value::Set(items) => wrap("#{", items.iter().map(render).collect(), "}"),
        Value::Map(pairs) => wrap("{", pairs.iter().map(render_pair).collect(), "}"),
        _ => render_atom(value),
    }
}

/// Scalars render without delimiters; strings, keywords, and symbols go to `render_text`.
fn render_atom(value: &Value) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Int(number) => number.to_string(),
        Value::Float(number) => render_float(*number),
        _ => render_text(value),
    }
}

/// Text values: a string is quoted, a keyword gets a colon, and a symbol is its name.
fn render_text(value: &Value) -> String {
    match value {
        Value::Str(text) => format!("\"{}\"", escape(text)),
        Value::Keyword(name) => format!(":{name}"),
        _ => value.as_str().unwrap_or_default().to_string(),
    }
}

fn wrap(open: &str, parts: Vec<String>, close: &str) -> String {
    format!("{open}{}{close}", parts.join(" "))
}

fn render_pair((key, value): &(Value, Value)) -> String {
    format!("{} {}", render(key), render(value))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Four decimals with trailing zeros trimmed, keeping one digit after the point.
fn render_float(number: f64) -> String {
    let text = if number.is_nan() {
        "nan".to_string()
    } else {
        format!("{number:.4}")
    };
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.contains('.') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.0")
    }
}

/// Parses one EDN value. Keywords stay distinct from strings.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut reader = Reader {
        chars: text.chars().collect(),
        index: 0,
    };
    reader.parse_document()
}

impl Value {
    /// Map lookup by key name; matches a keyword or a string key. Last duplicate wins.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(pairs) => pairs
                .iter()
                .rev()
                .find(|(name, _)| name.is_key_named(key))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn is_key_named(&self, key: &str) -> bool {
        match self {
            Value::Keyword(name) | Value::Str(name) => name.as_str() == key,
            _ => false,
        }
    }

    /// Text of a string, keyword, or symbol.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) | Value::Keyword(text) | Value::Symbol(text) => Some(text.as_str()),
            _ => None,
        }
    }

    /// Items of a vector or list.
    pub fn as_vec(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Vector(items) | Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// The integer, when this is an integer.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(number) => Some(*number),
            _ => None,
        }
    }
}

struct Reader {
    chars: Vec<char>,
    index: usize,
}

impl Reader {
    fn parse_document(&mut self) -> Result<Value, String> {
        self.skip();
        if self.peek().is_none() {
            return Err("empty EDN".to_string());
        }
        let value = self.value()?;
        self.skip();
        if self.peek().is_some() {
            return Err(format!("trailing EDN at {}", self.index));
        }
        Ok(value)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn peek_or_err(&self) -> Result<char, String> {
        self.peek()
            .ok_or_else(|| "unexpected end of EDN".to_string())
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += 1;
        Some(ch)
    }

    fn skip(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == ';' {
                self.skip_comment();
            } else if is_skippable(ch) {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        while self.peek().is_some_and(|ch| ch != '\n') {
            self.index += 1;
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip();
        match self.peek_or_err()? {
            '"' => self.string().map(Value::Str),
            ':' => self.keyword().map(Value::Keyword),
            '#' => self.set(),
            '{' | '[' | '(' => self.container(),
            _ => self.atom(),
        }
    }

    fn container(&mut self) -> Result<Value, String> {
        match self.peek_or_err()? {
            '{' => Ok(Value::Map(self.collect('}', Self::pair)?)),
            '[' => Ok(Value::Vector(self.collect(']', Self::value)?)),
            _ => Ok(Value::List(self.collect(')', Self::value)?)),
        }
    }

    /// Reads items from the opener at the cursor up to `closing`.
    fn collect<T>(
        &mut self,
        closing: char,
        mut item: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        self.index += 1;
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.peek_or_err()? == closing {
                self.index += 1;
                return Ok(items);
            }
            items.push(item(self)?);
        }
    }

    fn pair(&mut self) -> Result<(Value, Value), String> {
        let key = self.value()?;
        self.skip();
        let value = self.value()?;
        Ok((key, value))
    }

    fn set(&mut self) -> Result<Value, String> {
        self.index += 1;
        if self.peek_or_err()? != '{' {
            return Err("unsupported EDN dispatch".to_string());
        }
        Ok(Value::Set(self.collect('}', Self::value)?))
    }

    fn string(&mut self) -> Result<String, String> {
        self.index += 1;
        let mut text = String::new();
        loop {
            match self.next_char().ok_or("unterminated string")? {
                '"' => return Ok(text),
                '\\' => text.push(self.escape()?),
                ch => text.push(ch),
            }
        }
    }

    fn escape(&mut self) -> Result<char, String> {
        let ch = self.next_char().ok_or("unterminated string escape")?;
        Ok(match ch {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            other => other,
        })
    }

    fn keyword(&mut self) -> Result<String, String> {
        self.index += 1;
        let name = self.take_symbol();
        if name.is_empty() {
            return Err("empty keyword".to_string());
        }
        Ok(name)
    }

    fn atom(&mut self) -> Result<Value, String> {
        let token = self.take_symbol();
        if token.is_empty() {
            return Err(format!("unexpected character {:?}", self.peek_or_err()?));
        }
        classify(&token)
    }

    fn take_symbol(&mut self) -> String {
        let start = self.index;
        while self.peek().is_some_and(is_symbol_char) {
            self.index += 1;
        }
        self.chars[start..self.index].iter().collect()
    }
}

fn is_skippable(ch: char) -> bool {
    ch.is_whitespace() || ch == ',' || ('\u{1c}'..='\u{1f}').contains(&ch)
}

fn is_symbol_char(ch: char) -> bool {
    !matches!(
        ch,
        ' ' | '\t' | '\r' | '\n' | ',' | '{' | '}' | '[' | ']' | '(' | ')' | '"' | '\\' | ';'
    )
}

fn classify(token: &str) -> Result<Value, String> {
    match token {
        "nil" => Ok(Value::Nil),
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        _ if is_int(token) => token
            .parse()
            .map(Value::Int)
            .map_err(|_| format!("integer out of range: {token}")),
        _ if is_float(token) => token
            .parse()
            .map(Value::Float)
            .map_err(|error| format!("{error}: {token}")),
        _ => Ok(Value::Symbol(token.to_string())),
    }
}

fn strip_sign(token: &str) -> &str {
    token.strip_prefix('-').unwrap_or(token)
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_int(token: &str) -> bool {
    all_digits(strip_sign(token))
}

fn is_float(token: &str) -> bool {
    strip_sign(token)
        .split_once('.')
        .is_some_and(|(whole, fraction)| all_digits(whole) && all_digits(fraction))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(name: &str) -> Value {
        Value::Keyword(name.to_string())
    }

    fn text(value: &str) -> Value {
        Value::Str(value.to_string())
    }

    #[test]
    // MU-4: a snapshot-shaped map renders with keyword keys, as Python dumps does.
    fn mu_4_render_snapshot_shape_matches_python_dumps() {
        let value = Value::Map(vec![
            (kw("version"), Value::Int(1)),
            (kw("source"), text("src/demo/say \"hi\".py")),
            (
                kw("outcomes"),
                Value::Map(vec![(text("[\"a\"]"), kw("killed"))]),
            ),
            (kw("forms"), Value::Vector(vec![])),
        ]);
        assert_eq!(
            render(&value),
            r##"{:version 1 :source "src/demo/say \"hi\".py" :outcomes {"[\"a\"]" :killed} :forms []}"##
        );
    }

    #[test]
    // MU-4: scalars render like Python dumps, including trimmed floats.
    fn mu_4_render_scalars() {
        assert_eq!(render(&Value::Nil), "nil");
        assert_eq!(render(&Value::Bool(true)), "true");
        assert_eq!(render(&Value::Bool(false)), "false");
        assert_eq!(render(&Value::Int(-3)), "-3");
        assert_eq!(render(&Value::Float(1.5)), "1.5");
        assert_eq!(render(&Value::Float(2.0)), "2.0");
        assert_eq!(render(&Value::Float(-1.5)), "-1.5");
        assert_eq!(render(&Value::Float(1.0 / 3.0)), "0.3333");
        assert_eq!(render(&Value::Float(0.00001)), "0.0");
        assert_eq!(render(&Value::Float(f64::NAN)), "nan.0");
        assert_eq!(render(&Value::Float(f64::INFINITY)), "inf.0");
    }

    #[test]
    // MU-4: strings escape backslash and double quote only.
    fn mu_4_render_escapes_backslash_and_quote() {
        assert_eq!(render(&text(r#"a\"b"#)), r#""a\\\"b""#);
    }

    #[test]
    // MU-4: a keyword renders with a colon; the same word as a string keeps quotes.
    fn mu_4_render_keyword_and_string_differ() {
        assert_eq!(render(&kw("killed")), ":killed");
        assert_eq!(render(&text("killed")), "\"killed\"");
    }

    #[test]
    // MU-4: collections render with single spaces; empty vector is "[]".
    fn mu_4_render_collections() {
        assert_eq!(render(&Value::Vector(vec![])), "[]");
        assert_eq!(
            render(&Value::Vector(vec![Value::Int(1), text("x")])),
            "[1 \"x\"]"
        );
        assert_eq!(
            render(&Value::List(vec![Value::Int(1), Value::Int(2)])),
            "(1 2)"
        );
        assert_eq!(render(&Value::Set(vec![Value::Int(1)])), "#{1}");
        assert_eq!(render(&Value::Map(vec![])), "{}");
    }

    #[test]
    // MU-10: a symbol renders bare inside a map, and a set nests inside it unchanged.
    fn mu_10_render_symbol_and_set_inside_map() {
        let value = Value::Map(vec![(
            Value::Symbol("x".to_string()),
            Value::Set(vec![Value::Bool(true)]),
        )]);
        assert_eq!(render(&value), "{x #{true}}");
    }

    #[test]
    // MU-4: a snapshot-shaped value survives render then parse unchanged.
    fn mu_4_round_trip_snapshot_shape() {
        let value = Value::Map(vec![
            (kw("version"), Value::Int(1)),
            (kw("tested-at"), text("2026-09-30T12:00:00-05:00")),
            (kw("namespace"), text("demo.Board")),
            (
                kw("outcomes"),
                Value::Map(vec![(text("[\"a\",1]"), kw("survived"))]),
            ),
            (
                kw("forms"),
                Value::Vector(vec![Value::Map(vec![
                    (kw("id"), text("defn/place")),
                    (kw("end-line"), Value::Int(9)),
                    (kw("killed"), Value::Int(1)),
                ])]),
            ),
        ]);
        let parsed = parse(&render(&value)).expect("round trip parses");
        assert_eq!(parsed, value);
        assert_eq!(
            parsed.get("namespace").and_then(Value::as_str),
            Some("demo.Board")
        );
    }

    #[test]
    // MU-4: keywords and strings parse to different variants, both read as text.
    fn mu_4_parse_keyword_and_string_are_distinct() {
        assert_eq!(parse(":killed"), Ok(kw("killed")));
        assert_eq!(parse("\"killed\""), Ok(text("killed")));
        assert_eq!(parse(":killed").unwrap().as_str(), Some("killed"));
        assert_eq!(parse("\"killed\"").unwrap().as_str(), Some("killed"));
    }

    #[test]
    // MU-4: map lookup by name finds keyword keys and string keys.
    fn mu_4_parse_map_lookup_by_name() {
        let value = parse("{:a 1 \"b\" 2}").expect("map parses");
        assert_eq!(value.get("a"), Some(&Value::Int(1)));
        assert_eq!(value.get("b"), Some(&Value::Int(2)));
        assert_eq!(value.get("missing"), None);
    }

    #[test]
    // MU-4: lookup skips entries whose key is neither a keyword nor a string, even when they come last.
    fn mu_4_get_skips_non_text_keys() {
        let value = parse("{:a 1 2 3 nil 4 true 5}").expect("map parses");
        assert_eq!(value.get("a"), Some(&Value::Int(1)));
        assert_eq!(value.get("missing"), None);
    }

    #[test]
    // MU-4: string escapes n t r " and \ decode; other escapes keep the character.
    fn mu_4_parse_string_escapes() {
        assert_eq!(parse(r#""a\nb\t\"\\\q""#), Ok(text("a\nb\t\"\\q")));
        assert_eq!(parse("\"a\rb\""), Ok(text("a\rb")));
    }

    #[test]
    // MU-4: bare tokens read as nil, booleans, ints, floats, or symbols.
    fn mu_4_parse_atoms() {
        assert_eq!(parse("nil"), Ok(Value::Nil));
        assert_eq!(parse("true"), Ok(Value::Bool(true)));
        assert_eq!(parse("false"), Ok(Value::Bool(false)));
        assert_eq!(parse("12"), Ok(Value::Int(12)));
        assert_eq!(parse("-3"), Ok(Value::Int(-3)));
        assert_eq!(parse("1.50"), Ok(Value::Float(1.5)));
        assert_eq!(parse("foo"), Ok(Value::Symbol("foo".into())));
        assert_eq!(parse("+5"), Ok(Value::Symbol("+5".into())));
        assert_eq!(parse("1."), Ok(Value::Symbol("1.".into())));
        assert_eq!(parse("1e5"), Ok(Value::Symbol("1e5".into())));
    }

    #[test]
    // MU-4: maps, vectors, lists, and sets parse with their nesting intact.
    fn mu_4_parse_collections() {
        assert_eq!(
            parse("{:forms [] :outcomes {} :note nil}"),
            Ok(Value::Map(vec![
                (kw("forms"), Value::Vector(vec![])),
                (kw("outcomes"), Value::Map(vec![])),
                (kw("note"), Value::Nil),
            ]))
        );
        assert_eq!(
            parse("[1 (2) #{3}]"),
            Ok(Value::Vector(vec![
                Value::Int(1),
                Value::List(vec![Value::Int(2)]),
                Value::Set(vec![Value::Int(3)]),
            ]))
        );
    }

    #[test]
    // MU-4: semicolon comments and commas are skipped as whitespace.
    fn mu_4_parse_skips_comments_and_commas() {
        assert_eq!(
            parse("; header\n{:a 1,, :b 2} ; tail\n"),
            Ok(Value::Map(vec![
                (kw("a"), Value::Int(1)),
                (kw("b"), Value::Int(2)),
            ]))
        );
    }

    #[test]
    // MU-4: empty input, comment-only input, and trailing values are errors.
    fn mu_4_parse_rejects_empty_and_trailing_input() {
        assert!(parse("").is_err());
        assert!(parse("   ").is_err());
        assert!(parse("; only comment").is_err());
        assert!(parse("1 2").is_err());
        assert!(parse("{} {}").is_err());
    }

    #[test]
    // MU-4: malformed collections, strings, keywords, dispatches, and numbers are errors.
    fn mu_4_parse_rejects_malformed_input() {
        for input in [
            "\"abc",
            "\"a\\",
            "{:a}",
            "[1",
            "]",
            "#x",
            ":",
            "#{1",
            "99999999999999999999",
        ] {
            assert!(parse(input).is_err(), "expected error for {input:?}");
        }
    }

    #[test]
    // MU-4: accessors return values only for the matching variants.
    fn mu_4_accessors_match_only_their_variant() {
        let list = Value::List(vec![Value::Int(1)]);
        assert_eq!(list.as_vec(), Some(&vec![Value::Int(1)]));
        assert_eq!(Value::Vector(vec![]).as_vec(), Some(&vec![]));
        assert_eq!(Value::Set(vec![]).as_vec(), None);
        assert_eq!(Value::Int(7).as_i64(), Some(7));
        assert_eq!(text("7").as_i64(), None);
        assert_eq!(Value::Symbol("s".into()).as_str(), Some("s"));
        assert_eq!(Value::Int(1).as_str(), None);
        assert_eq!(Value::Int(1).get("a"), None);
    }
}
