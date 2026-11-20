#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Tests for the pin check of parity/run.py (MU-PB-1, MU-PB-2).

MU-PB-2: a parity run compares the port with the original at the commit of
`parity/upstream.toml`. The runner reads that commit from the checkout that the
venv installs from, else from the pip metadata of the venv, and stops with a
message when it cannot read a commit or reads a different one.

Stdlib only, with stub processes and no network. Run from tools/mutator/:

    python3.12 -m unittest parity/test_pin.py
"""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

from parity.test_status import run as runner

PIN = runner.pinned_commit()


def fake(package: str = "", git: str = "", pip: str = ""):
    """A stub for `subprocess.run` that answers the three commands of the pin check.

    `package` is the module path that the venv interpreter prints, `git` the answer
    of `git rev-parse`, and `pip` the text of `direct_url.json`. An empty answer
    makes that command fail the way the real one does.
    """
    def answer(argv, **_kwargs):
        if argv[0] == "git":
            text, code = (git + "\n", 0) if git else ("fatal: not a git repository\n", 128)
        elif "importlib.metadata" in argv[-1]:
            text, code = (pip, 0) if pip else ("", 1)
        else:
            text, code = (package, 0) if package else ("", 1)
        return subprocess.CompletedProcess(argv, code, text, "")

    return answer


def direct_url(commit: str) -> str:
    return ('{"url": "file:///src/mutator", '
            f'"vcs_info": {{"vcs": "git", "commit_id": "{commit}"}}}}')


class PinnedCommit(unittest.TestCase):
    def test_mu_pb_1_the_real_upstream_toml_pins_a_commit(self):
        self.assertEqual(len(PIN), 40)
        self.assertTrue(all(c in "0123456789abcdef" for c in PIN))

    def test_mu_pb_2_a_pin_that_is_not_a_full_commit_stops_the_runner(self):
        # A pin that is not a full commit cannot name the original, so the runner must
        # stop instead of comparing against an unknown commit.
        for text in ('[original]\ncommit = "abc"\n', "[original]\n"):
            with self.subTest(text=text), tempfile.TemporaryDirectory() as tmp:
                path = Path(tmp) / "upstream.toml"
                path.write_text(text, encoding="utf-8")
                with self.assertRaises(RuntimeError):
                    runner.pinned_commit(path)


class OriginalCommit(unittest.TestCase):
    def test_mu_pb_2_the_commit_of_the_checkout_the_venv_installs_from(self):
        commit = "1" * 40
        got = runner.original_commit("/v/bin/python",
                                     fake("/src/mutator/mutator", git=commit))
        self.assertEqual(got, commit)

    def test_mu_pb_2_the_commit_of_the_pip_metadata(self):
        # A venv built from a copy into site-packages has no checkout, and pip records
        # the commit in direct_url.json instead.
        commit = "2" * 40
        got = runner.original_commit(
            "/v/bin/python", fake("/v/lib/site-packages/mutator", pip=direct_url(commit)))
        self.assertEqual(got, commit)

    def test_mu_pb_2_no_commit_at_all_is_empty(self):
        self.assertEqual(runner.original_commit("/v/bin/python", fake()), "")

    def test_mu_pb_2_a_python_that_cannot_start_gives_no_commit(self):
        # A path that the host cannot start reads no commit, so the pin check stops.
        def cannot_start(*_args, **_kwargs):
            raise FileNotFoundError(2, "No such file or directory")

        self.assertEqual(runner.original_commit("/no/such/python", cannot_start), "")
        with self.assertRaises(RuntimeError):
            runner.check_pin("/no/such/python", run=cannot_start)


class Quiet(unittest.TestCase):
    def test_mu_pb_2_quiet_asks_for_captured_text_and_no_exception(self):
        # `git rev-parse` and the venv commands must not write to the terminal, must give
        # text, and must not raise on a nonzero exit. A wrong flag changes that.
        seen: list[tuple[list[str], dict]] = []

        def record(cmd, **kwargs):
            seen.append((cmd, kwargs))
            return subprocess.CompletedProcess(cmd, 0, " out \n", "")

        self.assertEqual(runner.quiet(["git"], record), "out")
        (cmd, kwargs), = seen
        self.assertEqual(cmd, ["git"])
        self.assertTrue(kwargs["capture_output"])
        self.assertTrue(kwargs["text"])
        self.assertFalse(kwargs["check"])
        self.assertEqual(kwargs["timeout"], runner.TIMEOUT)


class CheckPin(unittest.TestCase):
    def test_mu_pb_2_a_venv_at_the_pin_passes(self):
        got = runner.check_pin("/v/bin/python",
                               run=fake("/src/mutator/mutator", git=PIN))
        self.assertEqual(got, PIN)

    def test_mu_pb_2_a_venv_at_another_commit_stops_and_names_both(self):
        other = "3" * 40
        with self.assertRaises(RuntimeError) as caught:
            runner.check_pin("/v/bin/python", run=fake("/src/mutator/mutator", git=other))
        message = str(caught.exception)
        self.assertIn(other, message)
        self.assertIn(PIN, message)

    def test_mu_pb_2_a_venv_without_a_readable_commit_stops(self):
        with self.assertRaises(RuntimeError) as caught:
            runner.check_pin("/v/bin/python", run=fake())
        self.assertIn(PIN, str(caught.exception))


if __name__ == "__main__":
    unittest.main()
