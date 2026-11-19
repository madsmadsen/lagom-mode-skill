#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Tests for the approval step of parity/run.py (AC-1, MU-PB-6).

AC-1: a difference that `fixes.toml` lists does not count as a failure. MU-PB-6:
Mads Madsen approves each entry, so a listed entry that is not approved excuses
nothing. A run names the fix it expects with `fix = "<id>"`.

Stdlib only. Run from tools/mutator/ with Python 3.11 or later:

    /tmp/mu/venv/bin/python -m unittest parity/test_status.py
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

RUN_PATH = Path(__file__).resolve().with_name("run.py")


def load_run():
    spec = importlib.util.spec_from_file_location("parity_run", RUN_PATH)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


run = load_run()
APPROVED = {"max-memory-limit"}
PROBLEM = ["stdout differs in 2 lines"]


class StatusFor(unittest.TestCase):
    def test_ac_1_a_run_with_an_approved_fix_is_diff_allowed(self):
        entry = {"name": "flag-help-short", "fix": "max-memory-limit"}
        self.assertEqual(run.status_for(PROBLEM, entry, APPROVED), "DIFF-ALLOWED")

    def test_ac_1_a_run_with_an_unapproved_fix_still_fails(self):
        # MU-PB-6: a listed entry that Mads has not approved excuses no difference.
        entry = {"name": "flag-help-short", "fix": "second-run-near-timeout"}
        self.assertEqual(run.status_for(PROBLEM, entry, APPROVED), "FAIL")

    def test_ac_1_a_run_with_no_fix_fails_on_any_problem(self):
        entry = {"name": "flag-root"}
        self.assertEqual(run.status_for(PROBLEM, entry, APPROVED), "FAIL")

    def test_ac_1_a_run_without_a_problem_passes(self):
        entry = {"name": "flag-root", "fix": "max-memory-limit"}
        self.assertEqual(run.status_for([], entry, APPROVED), "PASS")

    def test_ac_1_every_named_fix_must_be_approved(self):
        # Two listed fixes explain the help text, so one approval is not enough.
        entry = {"fix": ["max-memory-limit", "worker-cap-half-the-cores"]}
        self.assertEqual(run.status_for(PROBLEM, entry, APPROVED), "FAIL")
        both = APPROVED | {"worker-cap-half-the-cores"}
        self.assertEqual(run.status_for(PROBLEM, entry, both), "DIFF-ALLOWED")

    def test_ac_1_a_fixture_string_grants_no_approval(self):
        # The old code compared the fixture text of a fix with the run path, which
        # never matched. A run that names only a fixture is a FAIL.
        entry = {"name": "fixture-python", "fixture": "fixtures/python"}
        self.assertEqual(run.status_for(PROBLEM, entry, APPROVED), "FAIL")


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
