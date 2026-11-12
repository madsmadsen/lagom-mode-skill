// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! End-to-end checks of the upstream fixes of PRD section 5.4: MU-22 (AC-14), MU-23 (AC-15),
//! and MU-24 (AC-16). Each test fails on the behavior of the original.

mod common;

use common::{git_commit_all, project, run, write};

const INDEX_TS: &str = "export function big(x: number): boolean {\n  return x > 10;\n}\n";
const INDEX_TEST_TS: &str = "import { expect, it } from 'vitest';\nimport { big } from '.';\n\nit('eleven is big', () => expect(big(11)).toBe(true));\nit('ten is not big', () => expect(big(10)).toBe(false));\n";

// The test file stays a link in a worker when it is not copied. A link means Node loads the
// original file, so the command passes at once. A real copy sees the mutated `index.ts`.
const LINK_AWARE_COMMAND: &str =
    "if [ -L src/calc/index.test.ts ]; then exit 0; fi; grep -q 'x > 10' src/calc/index.ts";

const OPS2_PY: &str = "def ops(a, b, items, m):\n    if a is None:\n        return m @ m\n    if b in items:\n        return a // b\n    a += b\n    return a % b + a ** b\n";
const OPS3_PY: &str = "def more(a, b, items, c):\n    if a is not b and c not in items:\n        a -= 1\n        a *= 2\n        a /= 3\n    return a\n";

const BRANCHES_PY: &str = "def classify(x):\n    if x > 10:\n        return \"high\"\n    elif x > 0:\n        return \"pos\"\n    return \"non-pos\"\n";
const CLAUSES_PY: &str = "def f(x, y):\n    try:\n        run()\n    except (A if x > 1 else B):\n        pass\n    match y:\n        case 1 if y > 0:\n            pass\n    return 0\n";
const PY_LCOV: &str = "target/coverage/python/lcov.info";

// AC-14 (MU-22): `src/calc/index.test.ts` imports `big` from '.', so the worker holds a copy of
// it, and the mutant `>` to `>=` on line 2 is killed.
#[test]
fn mu_22_ac14_a_test_that_imports_dot_is_copied_so_the_mutant_is_killed() {
    let dir = project();
    write(dir.path(), "src/calc/index.ts", INDEX_TS);
    write(dir.path(), "src/calc/index.test.ts", INDEX_TEST_TS);
    git_commit_all(dir.path());
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--max-workers",
            "1",
            "--test-command",
            LINK_AWARE_COMMAND,
            "src/calc/index.ts",
        ],
    );
    assert_eq!(
        out.code, 0,
        "stdout:\n{}\nstderr:\n{}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .lines()
            .any(|line| line == "KILLED    src/calc/index.ts:2 > -> >="),
        "stdout:\n{}",
        out.stdout
    );
}

/// The lines of a scan that name the site `original -> mutant` on `line`.
fn scan_has(stdout: &str, line: usize, description: &str) -> bool {
    let want = format!(":{line} {description}");
    stdout.lines().any(|text| text.contains(&want))
}

// AC-15 (MU-23): a scan of `src/demo/ops2.py` lists a site for each operator of issue 10.
#[test]
fn mu_23_ac15_scan_lists_a_site_for_each_operator_of_issue_10() {
    let dir = project();
    write(dir.path(), "src/demo/ops2.py", OPS2_PY);
    let out = run(dir.path(), &["--scan", "--no-coverage", "src/demo/ops2.py"]);
    assert_eq!(out.code, 0, "stderr:\n{}", out.stderr);
    for (line, description) in [
        (2, "is -> is not"),
        (3, "@ -> *"),
        (4, "in -> not in"),
        (5, "// -> /"),
        (6, "+= -> -="),
        (7, "% -> *"),
        (7, "** -> *"),
        (7, "+ -> -"),
    ] {
        assert!(
            scan_has(&out.stdout, line, description),
            "missing {line} {description}:\n{}",
            out.stdout
        );
    }
}

// AC-15 (MU-23): a scan of a file with `is not`, `not in`, `-=`, `*=`, and `/=` lists each one.
#[test]
fn mu_23_ac15_scan_lists_the_reverse_operators() {
    let dir = project();
    write(dir.path(), "src/demo/ops3.py", OPS3_PY);
    let out = run(dir.path(), &["--scan", "--no-coverage", "src/demo/ops3.py"]);
    assert_eq!(out.code, 0, "stderr:\n{}", out.stderr);
    for (line, description) in [
        (2, "is not -> is"),
        (2, "not in -> in"),
        (3, "-= -> +="),
        (4, "*= -> /="),
        (5, "/= -> *="),
    ] {
        assert!(
            scan_has(&out.stdout, line, description),
            "missing {line} {description}:\n{}",
            out.stdout
        );
    }
}

// AC-16 (MU-24): the tests cover line 2 and not lines 4 and 5. The two sites on line 4 are
// uncovered, so no mutant runs for them, and the site on line 2 runs.
#[test]
fn mu_24_ac16_elif_sites_on_an_uncovered_line_are_uncovered() {
    let dir = project();
    write(dir.path(), "src/demo/branches.py", BRANCHES_PY);
    write(
        dir.path(),
        PY_LCOV,
        "SF:src/demo/branches.py\nDA:2,1\nDA:4,0\nDA:5,0\nend_of_record\n",
    );
    git_commit_all(dir.path());
    let out = run(
        dir.path(),
        &[
            "--use-existing-coverage",
            "--max-workers",
            "1",
            "--test-command",
            "true",
            "src/demo/branches.py",
        ],
    );
    assert_eq!(
        out.code, 3,
        "stdout:\n{}\nstderr:\n{}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .contains("SURVIVED  src/demo/branches.py:2 > -> >="),
        "stdout:\n{}",
        out.stdout
    );
    for site in ["> -> >=", "0 -> 1"] {
        let want = format!("UNCOVERED src/demo/branches.py:4 {site}");
        assert!(
            out.stdout.lines().any(|line| line == want),
            "missing {want}:\n{}",
            out.stdout
        );
    }
    assert!(
        !out.stdout.contains("SURVIVED  src/demo/branches.py:4"),
        "line 4 must not run:\n{}",
        out.stdout
    );
}

// AC-16 (MU-24): the same rule for a site in an `except` clause and in a `case` guard.
#[test]
fn mu_24_ac16_except_and_case_guard_sites_on_an_uncovered_line_are_uncovered() {
    let dir = project();
    write(dir.path(), "src/demo/clauses.py", CLAUSES_PY);
    write(
        dir.path(),
        PY_LCOV,
        "SF:src/demo/clauses.py\nDA:2,1\nDA:3,1\nDA:4,0\nDA:6,1\nDA:7,0\nDA:9,1\nend_of_record\n",
    );
    let out = run(
        dir.path(),
        &["--scan", "--use-existing-coverage", "src/demo/clauses.py"],
    );
    assert_eq!(out.code, 0, "stderr:\n{}", out.stderr);
    for (line, description) in [(4, "> -> >="), (7, "> -> >=")] {
        let want = format!(":{line} {description} uncovered");
        assert!(
            out.stdout.lines().any(|text| text.contains(&want)),
            "missing {want}:\n{}",
            out.stdout
        );
    }
}
