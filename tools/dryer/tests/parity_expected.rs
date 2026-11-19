// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Replays the parity suite against the compiled port.
//!
//! Each case in `parity/coverage.toml` runs on a fresh copy of its fixture and
//! is compared with `parity/expected/<case>/`, which the pinned Python original
//! recorded with `parity/run_parity.py --record`. The copy-and-run rules mirror
//! that runner: the copy lives in `proj`, the port gets `--root <copy>` before
//! the case args, the working directory is the copy, and `changed` cases get the
//! same git setup.
//!
//! Requirement IDs covered here: AC-1, AC-2, AC-7, AC-8, AC-9, DR-2..DR-5,
//! DR-8, DR-NFR-1, DR-PB-1, DR-PB-4.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use regex::Regex;

const FORBIDDEN_PARTS: [&str; 3] = ["test", "tests", "spec"];
const SOURCE_EXTS: [&str; 9] = ["clj", "cljc", "cljs", "java", "go", "ts", "tsx", "rs", "py"];
const FLAGS: [&str; 12] = [
    "--help",
    "-h",
    "--root",
    "-s",
    "--source-root",
    "--changed",
    "--threshold",
    "--min-lines",
    "--min-nodes",
    "--format",
    "--edn",
    "--text",
];
const LANGS: [&str; 6] = ["clojure", "java", "go", "typescript", "rust", "python"];
/// Names of fix tests defined in this file. The fix tests live in other files.
const FIX_TESTS_IN_THIS_FILE: &[&str] = &[];

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("parity")
}

fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    if !path.exists() {
        return Ok(None);
    }
    fs::read(path)
        .map(Some)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn load_toml(path: &Path) -> toml::Table {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.parse::<toml::Table>()
        .unwrap_or_else(|e| panic!("{} does not parse as TOML: {e}", path.display()))
}

fn string_list(value: &toml::Value, what: &str) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{what} must be an array"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("{what} must hold strings"))
                .to_owned()
        })
        .collect()
}

// ---------------------------------------------------------------- cases

struct Case {
    name: String,
    fixture: String,
    git_setup: Option<String>,
    fix: Option<String>,
    args: Vec<String>,
    covers: Vec<String>,
}

fn load_cases() -> Vec<Case> {
    let table = load_toml(&parity_dir().join("coverage.toml"));
    let entries = table
        .get("case")
        .and_then(toml::Value::as_array)
        .expect("coverage.toml has no [[case]] tables");
    let mut names = BTreeSet::new();
    let mut cases = Vec::new();
    for entry in entries {
        let t = entry.as_table().expect("[[case]] must be a table");
        let name = t
            .get("name")
            .and_then(toml::Value::as_str)
            .expect("coverage.toml: case missing name")
            .to_owned();
        let fixture = t
            .get("fixture")
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("case {name} missing fixture"))
            .to_owned();
        let args = string_list(
            t.get("args")
                .unwrap_or_else(|| panic!("case {name} missing args")),
            "args",
        );
        let covers = t
            .get("covers")
            .map(|v| string_list(v, "covers"))
            .unwrap_or_default();
        let git_setup = t.get("git_setup").map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("case {name}: git_setup must be a string"))
                .to_owned()
        });
        let fix = t.get("fix").map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("case {name}: fix must be a string"))
                .to_owned()
        });
        assert!(
            names.insert(name.clone()),
            "coverage.toml: duplicate case {name}"
        );
        cases.push(Case {
            name,
            fixture,
            git_setup,
            fix,
            args,
            covers,
        });
    }
    cases
}

// ---------------------------------------------------------------- setup

fn check_path(path: &Path) -> Result<(), String> {
    let bad: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s
                .to_str()
                .filter(|s| FORBIDDEN_PARTS.contains(s))
                .map(str::to_owned),
            _ => None,
        })
        .collect();
    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "temp path {} has components {bad:?}; the walker would skip it. \
             Point TMPDIR at a path without test/tests/spec components.",
            path.display()
        ))
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("git")
        .args([
            "-c",
            "user.email=parity@example.invalid",
            "-c",
            "user.name=parity",
        ])
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("cannot run git {args:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("git {args:?} exited with {status}"))
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn source_files(src: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    collect_files(src, &mut found)?;
    found.retain(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SOURCE_EXTS.contains(&e))
    });
    found.sort();
    Ok(found)
}

fn append_comment(path: &Path) -> Result<(), String> {
    let prefix = match path.extension().and_then(|e| e.to_str()) {
        Some("clj" | "cljc" | "cljs") => ";",
        Some("py") => "#",
        _ => "//",
    };
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    file.write_all(format!("{prefix} dryer parity change\n").as_bytes())
        .map_err(|e| format!("cannot append to {}: {e}", path.display()))
}

/// Same steps as run_parity.py apply_git_setup("changed").
fn apply_changed_setup(root: &Path) -> Result<(), String> {
    git(root, &["init", "-q"])?;
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-q", "-m", "parity base"])?;
    let files = source_files(&root.join("src"))?;
    if files.len() < 2 {
        return Err(format!(
            "changed setup needs at least two source files under {}/src",
            root.display()
        ));
    }
    let (unstaged, staged) = (&files[0], &files[1]);
    append_comment(unstaged)?;
    append_comment(staged)?;
    let staged_rel = staged
        .strip_prefix(root)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    git(root, &["add", staged_rel.as_str()])?;
    let ext = unstaged.extension().and_then(|e| e.to_str()).unwrap_or("");
    fs::copy(
        unstaged,
        unstaged.with_file_name(format!("changed_new.{ext}")),
    )
    .map_err(|e| format!("cannot copy {}: {e}", unstaged.display()))?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(|e| format!("cannot copy {}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// Copies parity/fixtures/<fixture> to <tmp>/proj and applies git_setup.
/// The returned TempDir must stay alive while the copy is used.
fn make_copy(
    fixture: &str,
    git_setup: Option<&str>,
) -> Result<(tempfile::TempDir, PathBuf), String> {
    let base = tempfile::tempdir().map_err(|e| format!("cannot create temp dir: {e}"))?;
    let proj = base.path().join("proj");
    check_path(&proj)?;
    let src = parity_dir().join("fixtures").join(fixture);
    if !src.is_dir() {
        return Err(format!("fixture not found: {}", src.display()));
    }
    copy_dir(&src, &proj)?;
    match git_setup {
        None => {}
        Some("changed") => apply_changed_setup(&proj)?,
        Some(other) => return Err(format!("unknown git_setup {other:?}")),
    }
    Ok((base, proj))
}

// ---------------------------------------------------------------- running

/// Recorded or observed result of one run: exit code, normalized stdout, and
/// the raw bytes of .metrics/dry.edn when it exists.
struct Snapshot {
    code: i32,
    stdout: String,
    edn: Option<Vec<u8>>,
}

/// Replaces the copy path (as given and canonical) with <ROOT>, longest first,
/// the same way run_parity.py normalize_stdout does.
fn normalize_stdout(raw: &[u8], proj: &Path) -> String {
    let mut text = String::from_utf8_lossy(raw).into_owned();
    let mut forms = vec![proj.display().to_string()];
    if let Ok(canonical) = fs::canonicalize(proj) {
        forms.push(canonical.display().to_string());
    }
    forms.sort();
    forms.dedup();
    forms.sort_by_key(|f| std::cmp::Reverse(f.len()));
    for form in forms {
        text = text.replace(&form, "<ROOT>");
    }
    text
}

fn run_port(proj: &Path, args: &[String]) -> Result<Snapshot, String> {
    let out = Command::new(env!("CARGO_BIN_EXE_dryer"))
        .arg("--root")
        .arg(proj)
        .args(args)
        .current_dir(proj)
        .output()
        .map_err(|e| format!("cannot run dryer: {e}"))?;
    Ok(Snapshot {
        code: out.status.code().unwrap_or(-1),
        stdout: normalize_stdout(&out.stdout, proj),
        edn: read_optional(&proj.join(".metrics").join("dry.edn"))?,
    })
}

fn load_recorded(dir: &Path) -> Result<Snapshot, String> {
    if !dir.is_dir() {
        return Err(format!("no recorded {}", dir.display()));
    }
    let code = read_file(&dir.join("exit_code"))?
        .trim()
        .parse::<i32>()
        .map_err(|e| format!("bad exit_code for {}: {e}", dir.display()))?;
    Ok(Snapshot {
        code,
        stdout: read_file(&dir.join("stdout"))?,
        edn: read_optional(&dir.join("dry.edn"))?,
    })
}

fn clip(s: &str) -> String {
    const MAX: usize = 400;
    if s.chars().count() > MAX {
        format!("{}...", s.chars().take(MAX).collect::<String>())
    } else {
        s.to_owned()
    }
}

fn diffs(recorded: &Snapshot, got: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    if recorded.code != got.code {
        out.push(format!(
            "exit code: recorded={} port={}",
            recorded.code, got.code
        ));
    }
    if recorded.stdout != got.stdout {
        out.push(format!(
            "stdout differs:\n      recorded: {}\n      port:     {}",
            clip(&recorded.stdout),
            clip(&got.stdout)
        ));
    }
    match (&recorded.edn, &got.edn) {
        (None, None) => {}
        (Some(want), Some(have)) => match (parse_edn_bytes(want), parse_edn_bytes(have)) {
            (Ok(a), Ok(b)) => {
                if a != b {
                    out.push(format!(
                        "dry.edn values differ:\n      recorded: {}\n      port:     {}",
                        clip(&format!("{a:?}")),
                        clip(&format!("{b:?}"))
                    ));
                }
            }
            (Err(e), _) => out.push(format!("recorded dry.edn does not parse: {e}")),
            (_, Err(e)) => out.push(format!("port dry.edn does not parse: {e}")),
        },
        _ => out.push(format!(
            "snapshot presence: recorded={} port={}",
            recorded.edn.is_some(),
            got.edn.is_some()
        )),
    }
    out
}

fn check_case(case: &Case) -> Result<Vec<String>, String> {
    // A case with a fix differs from the original on purpose (DR-14, DR-15). The port
    // matches its own recording. When the original's recording is present, the port
    // must also still differ from it; run_parity.py makes that check with a live run.
    let expected_dir = match &case.fix {
        Some(_) => parity_dir().join("expected").join(&case.name).join("port"),
        None => parity_dir().join("expected").join(&case.name),
    };
    let recorded = load_recorded(&expected_dir)?;
    let original_expected = parity_dir().join("expected").join(&case.name);
    let original = match &case.fix {
        Some(_) if original_expected.join("exit_code").is_file() => {
            Some(load_recorded(&original_expected)?)
        }
        _ => None,
    };
    let (_base, proj) = make_copy(&case.fixture, case.git_setup.as_deref())?;
    let got = run_port(&proj, &case.args)?;
    let mut found = diffs(&recorded, &got);
    if original.is_some_and(|recording| diffs(&recording, &got).is_empty()) {
        found.push("fix case shows no difference from the original".to_owned());
    }
    Ok(found)
}

// ---------------------------------------------------------------- mini EDN

/// Minimal EDN reader for the shapes dryer writes: maps, vectors, strings,
/// keywords, integers, and floats, plus nil, booleans, and symbols.
/// Equality follows the Python runner: map key order is ignored, and an
/// integer equals a float with the same value.
#[derive(Debug)]
enum Edn {
    Nil,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Kw(String),
    Sym(String),
    Seq(Vec<Edn>),
    Map(Vec<(Edn, Edn)>),
}

impl PartialEq for Edn {
    fn eq(&self, other: &Edn) -> bool {
        match (self, other) {
            (Edn::Nil, Edn::Nil) => true,
            (Edn::Bool(a), Edn::Bool(b)) => a == b,
            (Edn::Int(a), Edn::Int(b)) => a == b,
            (Edn::Float(a), Edn::Float(b)) => a == b,
            (Edn::Int(a), Edn::Float(b)) | (Edn::Float(b), Edn::Int(a)) => (*a as f64) == *b,
            (Edn::Str(a), Edn::Str(b)) | (Edn::Kw(a), Edn::Kw(b)) | (Edn::Sym(a), Edn::Sym(b)) => {
                a == b
            }
            (Edn::Seq(a), Edn::Seq(b)) => a == b,
            (Edn::Map(a), Edn::Map(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v == v2))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
enum Tok {
    Br(char),
    Str(String),
    Atom(String),
}

fn tokenize(text: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(c) = text[pos..].chars().next() {
        if c.is_whitespace() || c == ',' {
            pos += c.len_utf8();
            continue;
        }
        if "[]{}".contains(c) {
            out.push(Tok::Br(c));
            pos += 1;
            continue;
        }
        if c == '"' {
            let mut end = None;
            let mut escaped = false;
            for (i, ch) in text[pos + 1..].char_indices() {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    end = Some(pos + 1 + i + 1);
                    break;
                }
            }
            let end = end.ok_or_else(|| format!("bad EDN at offset {pos}"))?;
            out.push(Tok::Str(text[pos..end].to_owned()));
            pos = end;
            continue;
        }
        let start = pos;
        while let Some(ch) = text[pos..].chars().next() {
            if ch.is_whitespace() || ch == ',' || "[]{}\"".contains(ch) {
                break;
            }
            pos += ch.len_utf8();
        }
        out.push(Tok::Atom(text[start..pos].to_owned()));
    }
    Ok(out)
}

fn hex4(chars: &mut std::str::Chars<'_>) -> Result<u32, String> {
    let hex: String = chars.by_ref().take(4).collect();
    if hex.len() != 4 {
        return Err("short \\u escape in EDN string".to_owned());
    }
    u32::from_str_radix(&hex, 16).map_err(|e| format!("bad \\u escape {hex}: {e}"))
}

/// Decodes a quoted EDN string token with JSON escape rules.
fn unescape(raw: &str) -> Result<String, String> {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| format!("bad EDN string {raw}"))?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let decoded = match chars.next() {
            Some('"') => '"',
            Some('\\') => '\\',
            Some('/') => '/',
            Some('b') => '\u{8}',
            Some('f') => '\u{c}',
            Some('n') => '\n',
            Some('r') => '\r',
            Some('t') => '\t',
            Some('u') => {
                let code = hex4(&mut chars)?;
                let scalar = if (0xD800..0xDC00).contains(&code) {
                    if chars.next() != Some('\\') || chars.next() != Some('u') {
                        return Err("lone surrogate in EDN string".to_owned());
                    }
                    let low = hex4(&mut chars)?;
                    if !(0xDC00..0xE000).contains(&low) {
                        return Err("lone surrogate in EDN string".to_owned());
                    }
                    0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00)
                } else {
                    code
                };
                char::from_u32(scalar).ok_or_else(|| "bad code point in EDN string".to_owned())?
            }
            other => return Err(format!("bad escape {other:?} in EDN string")),
        };
        out.push(decoded);
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    int: Regex,
    float: Regex,
}

impl Parser {
    fn value(&mut self) -> Result<Edn, String> {
        let tok = self
            .toks
            .get(self.pos)
            .cloned()
            .ok_or_else(|| "unexpected end of EDN".to_owned())?;
        self.pos += 1;
        match tok {
            Tok::Str(raw) => unescape(&raw).map(Edn::Str),
            Tok::Br('[') => Ok(Edn::Seq(self.items(']')?)),
            Tok::Br('{') => {
                let flat = self.items('}')?;
                if flat.len() % 2 != 0 {
                    return Err("EDN map has an odd number of forms".to_owned());
                }
                let mut pairs = Vec::new();
                let mut it = flat.into_iter();
                while let (Some(k), Some(v)) = (it.next(), it.next()) {
                    pairs.push((k, v));
                }
                Ok(Edn::Map(pairs))
            }
            Tok::Br(c) => Err(format!("unexpected EDN delimiter {c}")),
            Tok::Atom(text) => self.atom(&text),
        }
    }

    fn items(&mut self, closer: char) -> Result<Vec<Edn>, String> {
        let mut items = Vec::new();
        loop {
            let closing = match self.toks.get(self.pos) {
                None => return Err("unterminated EDN collection".to_owned()),
                Some(Tok::Br(c)) => *c == closer,
                Some(_) => false,
            };
            if closing {
                self.pos += 1;
                return Ok(items);
            }
            items.push(self.value()?);
        }
    }

    fn atom(&self, text: &str) -> Result<Edn, String> {
        if let Some(name) = text.strip_prefix(':') {
            return Ok(Edn::Kw(name.to_owned()));
        }
        match text {
            "nil" => return Ok(Edn::Nil),
            "true" => return Ok(Edn::Bool(true)),
            "false" => return Ok(Edn::Bool(false)),
            _ => {}
        }
        if self.int.is_match(text) {
            return text
                .parse::<i128>()
                .map(Edn::Int)
                .map_err(|e| format!("bad EDN int {text}: {e}"));
        }
        if self.float.is_match(text) {
            return text
                .parse::<f64>()
                .map(Edn::Float)
                .map_err(|e| format!("bad EDN float {text}: {e}"));
        }
        Ok(Edn::Sym(text.to_owned()))
    }
}

fn parse_edn(text: &str) -> Result<Edn, String> {
    let mut parser = Parser {
        toks: tokenize(text)?,
        pos: 0,
        int: Regex::new(r"^[+-]?\d+$").expect("int regex"),
        float: Regex::new(r"^[+-]?\d+(\.\d*)?([eE][+-]?\d+)?$").expect("float regex"),
    };
    let value = parser.value()?;
    if parser.pos != parser.toks.len() {
        return Err("trailing EDN forms".to_owned());
    }
    Ok(value)
}

fn parse_edn_bytes(bytes: &[u8]) -> Result<Edn, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    parse_edn(text)
}

// ---------------------------------------------------------------- tests

/// AC-1, AC-8, DR-2, DR-3, DR-4, DR-14, DR-15: every coverage.toml case, run on
/// the port, matches its recording (exit code, stdout, dry.edn). A fix case
/// matches the port's own recording and still differs from the original's.
#[test]
fn ac1_dr2_dr3_dr4_every_case_matches_recorded_original() {
    let cases = load_cases();
    let mut failures = Vec::new();
    for case in cases.iter() {
        match check_case(case) {
            Ok(found) if found.is_empty() => {}
            Ok(found) => failures.push(format!("FAIL {}\n    {}", case.name, found.join("\n    "))),
            Err(e) => failures.push(format!("FAIL {}\n    {e}", case.name)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} case(s) differ from the recorded original:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// AC-1, DR-2..DR-13: coverage.toml exercises every flag, and every README
/// "What each language compares" row for the six languages.
#[test]
fn ac1_coverage_toml_covers_every_flag_and_readme_row() {
    let cases = load_cases();
    let args: BTreeSet<&str> = cases
        .iter()
        .flat_map(|c| c.args.iter().map(String::as_str))
        .collect();
    let covers: BTreeSet<&str> = cases
        .iter()
        .flat_map(|c| c.covers.iter().map(String::as_str))
        .collect();

    let mut missing: Vec<String> = FLAGS
        .iter()
        .filter(|flag| !args.contains(*flag))
        .map(|flag| format!("flag {flag} in no case's args"))
        .collect();

    let root_exercised = cases
        .iter()
        .find(|c| c.name == "root-flag")
        .is_some_and(|c| c.args.iter().any(|a| a == "--root"));
    if !root_exercised {
        missing.push("case root-flag does not pass --root".to_owned());
    }

    for lang in LANGS {
        let prefix = format!("row:{lang}:");
        let row: Vec<&str> = covers
            .iter()
            .copied()
            .filter(|c| c.starts_with(&prefix))
            .collect();
        let candidate = format!("{prefix}candidate");
        if !covers.contains(candidate.as_str()) {
            missing.push(format!("no {candidate} cover"));
        }
        if row.len() < 2 {
            missing.push(format!("fewer than two {prefix} covers"));
        }
        if !row
            .iter()
            .any(|c| c.ends_with("left-out") || c.ends_with("excluded"))
        {
            missing.push(format!("no {prefix}...left-out or excluded cover"));
        }
    }

    assert!(
        missing.is_empty(),
        "coverage.toml does not cover:\n  {}",
        missing.join("\n  ")
    );
}

/// AC-2, DR-5, DR-8: each of the six languages has a fixture dir with a
/// source file of that language.
#[test]
fn ac2_dr5_dr8_fixture_exists_for_each_language() {
    let table: [(&str, &[&str]); 6] = [
        ("clojure", &["clj", "cljc", "cljs"]),
        ("java", &["java"]),
        ("go", &["go"]),
        ("typescript", &["ts", "tsx"]),
        ("rust", &["rs"]),
        ("python", &["py"]),
    ];
    let mut missing = Vec::new();
    for (lang, exts) in table {
        let dir = parity_dir().join("fixtures").join(lang);
        if !dir.is_dir() {
            missing.push(format!("{lang}: no fixture dir {}", dir.display()));
            continue;
        }
        let mut files = Vec::new();
        collect_files(&dir, &mut files).expect("fixture dir readable");
        let has_source = files.iter().any(|f| {
            f.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
        });
        if !has_source {
            missing.push(format!("{lang}: no file with extension {exts:?}"));
        }
    }
    assert!(
        missing.is_empty(),
        "fixtures incomplete:\n  {}",
        missing.join("\n  ")
    );
}

/// Concatenated text of every tests/*.rs file except this one.
fn other_test_sources() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    let mut text = String::new();
    for entry in entries {
        let path = entry.expect("tests entry").path();
        let is_rust = path.extension().and_then(|e| e.to_str()) == Some("rs");
        let is_this_file = path.file_name().and_then(|n| n.to_str()) == Some("parity_expected.rs");
        if is_rust && !is_this_file {
            text.push_str(&read_file(&path).expect("readable test file"));
        }
    }
    text
}

/// AC-9, DR-PB-4: every [[fix]] entry in parity/fixes.toml has the required
/// keys and a test that names its id. With no entries this passes, but the
/// file must still parse.
#[test]
fn ac9_dr_pb_4_every_fixes_entry_has_a_named_test() {
    let fixes = load_toml(&parity_dir().join("fixes.toml"));
    let empty: &[toml::Value] = &[];
    let entries: &[toml::Value] = match fixes.get("fix") {
        None => empty,
        Some(v) => v
            .as_array()
            .map(Vec::as_slice)
            .expect("fixes.toml: fix must be an array of tables"),
    };
    let others = other_test_sources();
    let mut problems = Vec::new();
    for entry in entries {
        let table = entry
            .as_table()
            .expect("fixes.toml: [[fix]] must be a table");
        for key in [
            "id",
            "requirement",
            "fixture",
            "upstream",
            "changed_behavior",
            "test",
        ] {
            if !table.contains_key(key) {
                problems.push(format!("fix entry missing {key}: {table:?}"));
            }
        }
        let Some(id) = table.get("id").and_then(toml::Value::as_str) else {
            continue;
        };
        if !FIX_TESTS_IN_THIS_FILE.contains(&id) && !others.contains(id) {
            problems.push(format!("no test names fix {id}"));
        }
    }
    assert!(
        problems.is_empty(),
        "parity/fixes.toml entries without named tests:\n  {}",
        problems.join("\n  ")
    );
}

/// DR-PB-3: each `fix` of a case in coverage.toml names an entry in fixes.toml
/// that lists the case as its fixture.
#[test]
fn dr_pb_3_every_fix_case_has_a_fixes_entry() {
    let fixes = load_toml(&parity_dir().join("fixes.toml"));
    let entries = fixes
        .get("fix")
        .and_then(toml::Value::as_array)
        .expect("fixes.toml has no [[fix]] tables");
    let mut problems = Vec::new();
    for case in load_cases() {
        let Some(id) = case.fix else { continue };
        let listed = entries.iter().filter_map(toml::Value::as_table).any(|e| {
            e.get("id").and_then(toml::Value::as_str) == Some(id.as_str())
                && e.get("fixture").and_then(toml::Value::as_str) == Some(case.name.as_str())
        });
        if !listed {
            problems.push(format!("case {} has no fixes.toml entry {id}", case.name));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// DR-PB-1: the parity suite pins the original to a full 40-hex commit.
#[test]
fn dr_pb_1_upstream_toml_pins_full_commit() {
    let upstream = load_toml(&parity_dir().join("upstream.toml"));
    let commit = upstream
        .get("commit")
        .and_then(toml::Value::as_str)
        .expect("upstream.toml has no string key `commit`");
    assert!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "upstream.toml commit must be a 40-hex SHA, got {commit:?}"
    );
}

fn fixture_names() -> Vec<String> {
    let dir = parity_dir().join("fixtures");
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .map(|e| {
            e.expect("fixtures entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| dir.join(n).is_dir())
        .collect();
    names.sort();
    names
}

/// One default run on a fresh copy of the fixture; returns .metrics/dry.edn bytes.
fn default_snapshot(fixture: &str) -> Result<Option<Vec<u8>>, String> {
    let (_base, proj) = make_copy(fixture, None)?;
    Ok(run_port(&proj, &[])?.edn)
}

/// DR-NFR-1, AC-7: two runs on fresh copies of each fixture write byte-identical dry.edn.
#[test]
fn dr_nfr1_ac7_two_runs_write_identical_snapshots_on_every_fixture() {
    let mut failures = Vec::new();
    for fixture in fixture_names() {
        match (default_snapshot(&fixture), default_snapshot(&fixture)) {
            (Ok(first), Ok(second)) => {
                if first != second {
                    failures.push(format!("{fixture}: two runs wrote different dry.edn bytes"));
                }
            }
            (Err(e), _) | (_, Err(e)) => failures.push(format!("{fixture}: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "snapshot determinism failed:\n  {}",
        failures.join("\n  ")
    );
}
