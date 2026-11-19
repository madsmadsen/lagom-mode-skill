#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Tests for the approval step of parity/run.py (AC-1, MU-PB-6).

AC-1: a difference that `fixes.toml` lists does not count as a failure. MU-PB-6:
Mads Madsen approves each entry, so a listed entry that is not approved excuses
nothing. A run names the fix it expects with `fix = "<id>"`.

The tests also cover `problems_for`, because an approval excuses only the
stdout difference that it finds there.

Stdlib only. Run from tools/mutator/ with Python 3.11 or later:

    python3.12 -m unittest parity/test_status.py

or let discovery find it, which is how crapper measures the coverage:

    python3.12 -m unittest discover -s .

`cargo test` also runs that command, through `tests/parity_map.rs`.
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

# No resolve(): mutator runs this file as a symlink in a worker directory beside the
# mutated run.py, and resolve() would follow the link back to the unmutated file.
RUN_PATH = Path(__file__).parent / "run.py"


def load_run():
    spec = importlib.util.spec_from_file_location("parity_run", RUN_PATH)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


run = load_run()
APPROVED = {"max-memory-limit"}
PROBLEM = ["stdout differs in 2 lines"]
HELP = {"name": "flag-help-short", "fix": "max-memory-limit"}


class StatusFor(unittest.TestCase):
    def test_ac_1_and_mu_pb_6_the_status_of_a_run(self):
        # One row per branch of the rule: no problem, no fix, an unapproved fix, a
        # partly approved list, an approval that covers the stdout difference, and an
        # approval that covers nothing else (MU-PB-6).
        cases = [
            ("PASS", [], HELP),
            ("FAIL", PROBLEM, {"name": "flag-root"}),
            ("FAIL", PROBLEM, HELP | {"fix": "second-run-near-timeout"}),
            ("FAIL", PROBLEM, {"fix": ["max-memory-limit", "worker-cap-half-the-cores"]}),
            ("DIFF-ALLOWED", PROBLEM, HELP),
            ("FAIL", PROBLEM, {"name": "fixture-python", "fixture": "fixtures/python"}),
            ("FAIL", PROBLEM, {"name": "flag-help-short", "fix": []}),
            ("FAIL", ["exit 0 (original) vs 1 (port)"], HELP),
            ("FAIL", ["metrics .metrics/mutate/demo.edn differs"], HELP),
            ("FAIL", ["error: setup step failed (1): sh check.sh"], HELP),
            ("FAIL", ["exit 0 (original) vs 1 (port)", *PROBLEM], HELP),
        ]
        for expected, problems, entry in cases:
            with self.subTest(expected=expected, problems=problems, entry=entry):
                self.assertEqual(run.status_for(problems, entry, APPROVED), expected)

    def test_ac_1_every_named_fix_must_be_approved(self):
        entry = {"fix": ["max-memory-limit", "worker-cap-half-the-cores"]}
        both = APPROVED | {"worker-cap-half-the-cores"}
        self.assertEqual(run.status_for(PROBLEM, entry, both), "DIFF-ALLOWED")


def side(tree: str = "/r", code: int = 0, stdout: str = "", stderr: str = "",
         metrics: dict | None = None, status: list[str] | None = None):
    return run.Outcome(Path(tree), code, stdout, stderr, metrics or {}, status or [])


class ProblemsFor(unittest.TestCase):
    def test_ac_1_the_problems_of_a_pair_of_runs(self):
        # Each row names the problem that one side shows. stdout is compared on both
        # sides, with no per-run way to skip it (MU-PB-6).
        cases = [
            ([], {}, {"original": side(), "port": side()}),
            ([], {}, {"original": side(stderr="a\n"), "port": side(stderr="b\n")}),
            (["original exit 0, expected 1", "port exit 0, expected 1"], {"expect_exit": 1},
             {"original": side(), "port": side()}),
            (["exit 0 (original) vs 1 (port)"], {},
             {"original": side(), "port": side(code=1)}),
            (["metrics files differ (original ['a'], port ['b'])"], {},
             {"original": side(metrics={"a": 1}), "port": side(metrics={"b": 1})}),
            (["metrics a differs"], {},
             {"original": side(metrics={"a": 1}), "port": side(metrics={"a": 2})}),
            (["git status differs (original [], port ['M src/x.py'])"], {},
             {"original": side(), "port": side(status=["M src/x.py"])}),
            (["stdout differs in 2 lines"], {},
             {"original": side(stdout="a\n"), "port": side(stdout="b\n")}),
        ]
        for expected, entry, sides in cases:
            with self.subTest(expected=expected, entry=entry):
                self.assertEqual(run.problems_for(entry, sides), expected)

    def test_ac_1_an_original_only_run_reports_its_own_exit_code(self):
        # `--original-only` has no port side, so only the expected exit code applies.
        sides = {"original": side(code=1)}
        self.assertEqual(run.problems_for({"expect_exit": 1}, sides), [])
        self.assertEqual(run.problems_for({"expect_exit": 0}, sides),
                         ["original exit 1, expected 0"])


class ApprovedFixIds(unittest.TestCase):
    def test_mu_pb_6_only_approved_ids_are_returned(self):
        text = (
            "[[fix]]\n"
            'id = "listed"\n'
            "approved = false\n"
            "\n"
            "[[fix]]\n"
            'id = "agreed"\n'
            'fixture = "tests/foo.rs (project built inline)"\n'
            "approved = true\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "fixes.toml"
            path.write_text(text, encoding="utf-8")
            self.assertEqual(run.approved_fix_ids(path), {"agreed"})

    def test_mu_pb_6_a_string_true_is_not_an_approval(self):
        # TOML `approved = "true"` is a string, not the boolean that MU-PB-6 needs.
        text = '[[fix]]\nid = "agreed"\napproved = "true"\n'
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "fixes.toml"
            path.write_text(text, encoding="utf-8")
            self.assertEqual(run.approved_fix_ids(path), set())

    def test_mu_pb_6_the_real_fixes_toml_has_nothing_approved(self):
        # AGENTS.md "Parity with the original" rule 3: only Mads Madsen approves an
        # entry, so this test fails as soon as a worker approves one. When Mads Madsen
        # approves an entry on purpose, change this test to name the approved id. Do
        # not delete it: the rule stays, and so does the test that guards it.
        self.assertEqual(run.approved_fix_ids(), set())


class CoverageFixIds(unittest.TestCase):
    def test_ac_1_every_named_fix_exists_in_fixes_toml(self):
        # A typo would make the run FAIL for a difference that fixes.toml lists.
        entries = run.tomllib.loads(run.COVERAGE_TOML.read_text(encoding="utf-8"))["run"]
        known = {fix["id"] for fix in run.tomllib.loads(run.FIXES_TOML.read_text(encoding="utf-8"))["fix"]}
        named = {fix for entry in entries for fix in run.named_fixes(entry)}
        self.assertTrue(named, "coverage.toml names no fix")
        self.assertEqual(named - known, set())


if __name__ == "__main__":
    unittest.main()
