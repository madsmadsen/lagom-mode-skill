// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! End-to-end tests for the `dryer` binary. Each test runs the compiled CLI in
//! a fresh project directory named `proj` and checks what a user would see.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const A_PY: &str = "def total_price(items, tax):\n    subtotal = 0\n    for item in items:\n        subtotal += item.price * item.quantity\n    result = subtotal + subtotal * tax\n    result = result + 1\n    return round(result, 2)\n";
const B_PY: &str = "def sum_cost(entries, rate):\n    running = 0\n    for entry in entries:\n        running += entry.cost * entry.count\n    final = running + running * rate\n    return round(final, 2)\n";
const TSX: &str = "export function totalPrice(items: Item[], tax: number): number {\n  let subtotal = 0;\n  for (const item of items) {\n    subtotal += item.price * item.quantity;\n  }\n  const result = subtotal + subtotal * tax;\n  return result;\n}\n";
const TS: &str = "export function sumCost(entries: Item[], rate: number): number {\n  let running = 0;\n  for (const entry of entries) {\n    running += entry.cost * entry.count;\n  }\n  const final = running + running * rate;\n  return final;\n}\n";
const JAVA: &str = "class Totals {\n    int totalPrice(int[] items, int tax) {\n        int subtotal = 0;\n        for (int item : items) {\n            subtotal += item * tax;\n        }\n        int result = subtotal + subtotal * tax;\n        result = result + 1;\n        return result;\n    }\n}\n";
const JSX_A: &str = "export function totalPrice(items, tax) {\n  let subtotal = 0;\n  for (const item of items) {\n    subtotal += item.price * item.quantity;\n  }\n  const result = subtotal + subtotal * tax;\n  return <span>{result}</span>;\n}\n";
const JSX_B: &str = "export function sumCost(entries, rate) {\n  let running = 0;\n  for (const entry of entries) {\n    running += entry.cost * entry.count;\n  }\n  const final = running + running * rate;\n  tally = tally + 1;\n  return <span>{final}</span>;\n}\n";

const PAIR_BLOCK: &str = "DUPLICATE score=0.86\n  a.py:1-7\n  b.py:1-6\n";
const NO_CANDIDATES: &str = "No duplicate candidates found.\n";
const NO_SOURCES: &str = "No source files to analyze.\n";
const PAIR_EDN: &str = "{:candidates [\n {:score 0.857142857143\n  :language \"python\"\n  :left {:file \"a.py\", :start-line 1, :end-line 7}\n  :right {:file \"b.py\", :start-line 1, :end-line 6}\n  :left-nodes 84\n  :right-nodes 71}\n]}\n";

struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path().join("proj");
        fs::create_dir(&root).expect("create proj dir");
        Self { _dir: dir, root }
    }

    fn write(&self, rel: &str, content: &str) -> &Self {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().expect("fixture has a parent")).expect("create dirs");
        fs::write(&path, content).expect("write fixture");
        self
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dryer"))
            .current_dir(&self.root)
            .args(args)
            .output()
            .expect("run dryer")
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(&self.root)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn canonical_root(&self) -> PathBuf {
        fs::canonicalize(&self.root).expect("canonicalize project root")
    }
}

fn text_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf8")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf8")
}

// DR-1
#[test]
fn dr1_help_prints_usage_and_exits_0() {
    let project = Project::new();
    let out = project.run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text_of(&out).starts_with("Usage: dryer"));
}

// DR-13, AC-3
#[test]
fn dr13_unknown_format_exits_2() {
    let project = Project::new();
    let out = project.run(&["--format", "xml"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stderr_of(&out), "Unknown format: xml\n");
}

// DR-13, AC-3
#[test]
fn dr13_unknown_option_exits_1() {
    let project = Project::new();
    let out = project.run(&["--nope"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr_of(&out).starts_with("Unknown option: --nope"));
}

// DR-13, AC-3
#[test]
fn dr13_threshold_without_value_exits_1() {
    let project = Project::new();
    let out = project.run(&["--threshold"]);
    assert_eq!(out.status.code(), Some(1));
}

// DR-2, AC-6
#[test]
fn dr2_rejected_flag_forms_exit_1() {
    let project = Project::new();
    project.write("a.py", A_PY);
    let cases: [&[&str]; 6] = [
        &["--threshold", "abc"],
        &["--min-lines", "-1"],
        &["--min-lines", "2.5"],
        &["--min-nodes"],
        &["--root"],
        &["--bogus"],
    ];
    for args in cases {
        let out = project.run(args);
        assert_eq!(out.status.code(), Some(1), "args {args:?}");
    }
}

// DR-3, DR-4
#[test]
fn dr3_dr4_text_report_and_snapshot_match_edn_stdout() {
    let project = Project::new();
    project.write("a.py", A_PY).write("b.py", B_PY);

    let text = project.run(&[]);
    assert_eq!(text.status.code(), Some(0));
    assert_eq!(text_of(&text), PAIR_BLOCK);
    let expected_last = format!(
        "Wrote {}/.metrics/dry.edn",
        project.canonical_root().display()
    );
    assert_eq!(
        stderr_of(&text).lines().last(),
        Some(expected_last.as_str())
    );

    let edn = project.run(&["--edn"]);
    assert_eq!(edn.status.code(), Some(0));
    assert_eq!(text_of(&edn), PAIR_EDN);
    let snapshot = fs::read(project.root.join(".metrics/dry.edn")).expect("read snapshot");
    assert_eq!(edn.stdout, snapshot);
}

// DR-12, AC-5
#[test]
fn dr12_threshold_controls_which_pairs_are_reported() {
    let project = Project::new();
    project.write("a.py", A_PY).write("b.py", B_PY);

    let low = project.run(&["--threshold", "0.8"]);
    assert_eq!(text_of(&low), PAIR_BLOCK);

    let high = project.run(&["--threshold", "0.9"]);
    assert_eq!(text_of(&high), NO_CANDIDATES);
}

// DR-12, AC-5
#[test]
fn dr12_min_lines_keeps_form_with_exactly_that_many_lines() {
    // b.py has exactly 6 lines and a.py has 7.
    let project = Project::new();
    project.write("a.py", A_PY).write("b.py", B_PY);

    let at_limit = project.run(&["--min-lines", "6"]);
    assert_eq!(text_of(&at_limit), PAIR_BLOCK);

    let over_limit = project.run(&["--min-lines", "7"]);
    assert_eq!(text_of(&over_limit), NO_CANDIDATES);
}

// DR-12, AC-5
#[test]
fn dr12_min_nodes_keeps_form_with_exactly_that_many_nodes() {
    // b.py has exactly 71 normalized nodes (`:right-nodes 71` in the EDN).
    let project = Project::new();
    project.write("a.py", A_PY).write("b.py", B_PY);

    let at_limit = project.run(&["--min-nodes", "71"]);
    assert_eq!(text_of(&at_limit), PAIR_BLOCK);

    let over_limit = project.run(&["--min-nodes", "72"]);
    assert_eq!(text_of(&over_limit), NO_CANDIDATES);
}

// DR-6
#[test]
fn dr6_skipped_directories_are_not_compared() {
    let project = Project::new();
    for dir in ["node_modules", "vendor", "target", "test", "spec"] {
        project
            .write(&format!("{dir}/a.py"), A_PY)
            .write(&format!("{dir}/b.py"), B_PY);
    }
    project.write("src/a.py", A_PY).write("src/b.py", B_PY);

    let out = project.run(&[]);
    assert_eq!(
        text_of(&out),
        "DUPLICATE score=0.86\n  src/a.py:1-7\n  src/b.py:1-6\n"
    );
}

// DR-5, DR-14
#[test]
fn dr5_unknown_extensions_are_ignored() {
    let project = Project::new();
    project
        .write("a.txt", A_PY)
        .write("b.txt", B_PY)
        .write("c.rb", A_PY)
        .write("d.rb", B_PY)
        .write("e.d.ts", TS)
        .write("f.d.ts", TS);

    let out = project.run(&[]);
    assert_eq!(text_of(&out), NO_SOURCES);
}

// DR-14, AC-11, AC-9: the test of fix js-read-as-typescript in parity/fixes.toml.
// The original prints "No source files to analyze." here.
#[test]
fn dr14_jsx_only_project_reports_the_pair() {
    let project = Project::new();
    project
        .write("src/a.jsx", JSX_A)
        .write("src/b.jsx", JSX_B)
        .write("src/a.test.jsx", JSX_A);

    let out = project.run(&["--threshold", "0.8"]);
    assert_eq!(
        text_of(&out),
        "DUPLICATE score=0.85\n  src/a.jsx:1-8\n  src/b.jsx:1-9\n"
    );
    let high = project.run(&["--threshold", "0.9"]);
    assert_eq!(text_of(&high), NO_CANDIDATES);
    let edn = project.run(&["--edn", "--threshold", "0.8"]);
    assert!(text_of(&edn).contains(":language \"typescript\""));
}

// DR-14
#[test]
fn dr14_each_javascript_extension_is_scanned() {
    let a_js = JSX_A.replace("<span>{result}</span>", "result");
    let b_js = JSX_B.replace("<span>{final}</span>", "final");
    for ext in ["js", "mjs", "cjs"] {
        let project = Project::new();
        project
            .write(&format!("a.{ext}"), &a_js)
            .write(&format!("b.{ext}"), &b_js);

        let out = project.run(&["--threshold", "0.5"]);
        let text = text_of(&out);
        assert!(text.starts_with("DUPLICATE score="), "{ext}: {text}");
        assert_ne!(text, NO_SOURCES, "{ext}");
    }
}

// DR-5
#[test]
fn dr5_tsx_and_ts_are_compared_as_typescript() {
    let project = Project::new();
    project.write("a.tsx", TSX).write("b.ts", TS);

    let edn = project.run(&["--edn"]);
    assert_eq!(
        text_of(&edn),
        "{:candidates [\n {:score 1.0\n  :language \"typescript\"\n  :left {:file \"a.tsx\", :start-line 1, :end-line 8}\n  :right {:file \"b.ts\", :start-line 1, :end-line 8}\n  :left-nodes 86\n  :right-nodes 86}\n]}\n"
    );
}

// DR-7, AC-4
#[test]
fn dr7_changed_compares_only_changed_files() {
    let project = Project::new();
    project.write("p.py", A_PY).write("q.py", B_PY);
    project.git(&["init", "-q"]);
    project.git(&["add", "-A"]);
    project.git(&[
        "-c",
        "user.email=a@b",
        "-c",
        "user.name=n",
        "commit",
        "-qm",
        "base",
    ]);

    // p.py is modified, r.py is staged, s.py is untracked; q.py is unchanged.
    project.write("p.py", &format!("{A_PY}# edit\n"));
    project.write("r.py", A_PY);
    project.git(&["add", "r.py"]);
    project.write("s.py", B_PY);

    let out = project.run(&["--changed"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        text_of(&out),
        "DUPLICATE score=1.00\n  p.py:1-7\n  r.py:1-7\n\nDUPLICATE score=0.86\n  p.py:1-7\n  s.py:1-6\n\nDUPLICATE score=0.86\n  r.py:1-7\n  s.py:1-6\n"
    );
}

// DR-7, AC-4
#[test]
fn dr7_changed_outside_git_exits_128() {
    let project = Project::new();
    project.write("p.py", A_PY);

    let out = project.run(&["--changed"]);
    assert_eq!(out.status.code(), Some(128));
    assert!(!stderr_of(&out).is_empty());
}

// DR-NFR-1, AC-7
#[test]
fn dr_nfr_1_repeated_runs_write_identical_snapshot() {
    let project = Project::new();
    project.write("a.py", A_PY).write("b.py", B_PY);

    project.run(&[]);
    let first = fs::read(project.root.join(".metrics/dry.edn")).expect("read first snapshot");
    project.run(&[]);
    let second = fs::read(project.root.join(".metrics/dry.edn")).expect("read second snapshot");
    assert_eq!(first, second);
}

// DR-10
#[test]
fn dr10_python_function_and_java_method_are_not_paired() {
    let project = Project::new();
    project.write("a.py", A_PY).write("Totals.java", JAVA);

    let out = project.run(&["--threshold", "0.1"]);
    assert_eq!(text_of(&out), NO_CANDIDATES);
}
