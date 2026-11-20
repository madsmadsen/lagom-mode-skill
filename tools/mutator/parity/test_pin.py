#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Tests for the pin check of parity/run.py (MU-PB-1, MU-PB-2).

MU-PB-2: a parity run compares the port with the original at the commits of
`parity/upstream.toml`. The runner reads each commit from the clean checkout
that the venv installs the package from, else from the pip metadata of a
site-packages install, and stops with a message when it cannot read a commit or
reads a different one. It also stops before any run, and it stops when the
original and the interpreter that runs run.py differ.

Stdlib only, with stub processes and no network. Run from tools/mutator/:

    python3.12 -m unittest parity/test_pin.py
"""

from __future__ import annotations

import contextlib
import io
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from parity.test_status import run as runner

PIN = runner.pinned_commit()
CRAPPER_PIN = runner.pinned_commit(section="crapper")


def direct_url(commit: str) -> str:
    return ('{"url": "file:///src/mutator", '
            f'"vcs_info": {{"vcs": "git", "commit_id": "{commit}"}}}}')


class Tree:
    """The answers of a stub for `subprocess.run`.

    `packages` maps a question to the path that the interpreter prints: `"mutator"` and
    `"crapper"`. `commits` maps a directory to its HEAD. `dirty` holds the directories
    whose checkout has uncommitted changes. `toplevels` maps a directory to the top of its
    checkout, and defaults to that same directory. `pip` holds the text of
    direct_url.json, keyed by distribution name. `git_fails` lists the directories where
    git does not work at all.
    """

    def __init__(self, packages=None, commits=None, dirty=(), pip=None, git_fails=(),
                 toplevels=None, interpreter="/usr/bin/python3.12"):
        self.packages = packages or {}
        self.commits = commits or {}
        self.dirty = set(dirty)
        self.pip = pip or {}
        self.git_fails = set(git_fails)
        self.toplevels = toplevels or {}
        self.interpreter = interpreter
        self.calls: list[list[str]] = []

    def __call__(self, argv, **_kwargs):
        self.calls.append(list(argv))
        text, code = self.answer(argv)
        return subprocess.CompletedProcess(argv, code, text, "")

    def answer(self, argv):
        if argv[0] == "git":
            return self.git(argv)
        script = argv[-1]
        if "sys.executable" in script:
            return self.interpreter, 0
        if "ensure_crapper" in script:
            return self.packages.get("crapper", ""), 0 if self.packages.get("crapper") else 1
        if "import mutator" in script and "importlib.metadata" not in script:
            return self.packages.get("mutator", ""), 0 if self.packages.get("mutator") else 1
        if "importlib.metadata" in script:
            name = script.split("distribution('")[1].split("'")[0]
            return self.pip.get(name, ""), 0 if self.pip.get(name) else 1
        return "", 1

    def git(self, argv):
        directory = argv[argv.index("-C") + 1]
        if directory in self.git_fails or not self.commits:
            return "fatal: not a git repository\n", 128
        if argv[-1] == "--show-toplevel":
            return self.toplevels.get(directory, directory), 0
        if argv[-1] == "--porcelain":
            text = " M src/x.py\n" if directory in self.dirty else "\n"
            return text, 0
        if argv[-1] == "HEAD":
            commit = self.commits.get(directory, "")
            return (commit + "\n", 0) if commit else ("fatal: bad revision\n", 128)
        return "", 128


def checkout(commit: str, root: str = "/src/mutator") -> Tree:
    """A venv that installs the original from a clean checkout at `commit`, with crapper
    next to it at the pin."""
    return Tree(packages={"mutator": f"{root}/mutator/__init__.py",
                          "crapper": f"/src/crapper/crapper/__init__.py"},
                commits={f"{root}/mutator": commit, "/src/crapper/crapper": CRAPPER_PIN})


class PinnedCommit(unittest.TestCase):
    def test_mu_pb_1_the_real_upstream_toml_pins_both_commits(self):
        self.assertEqual(len(PIN), 40)
        self.assertEqual(len(CRAPPER_PIN), 40)
        self.assertTrue(all(c in "0123456789abcdef" for c in PIN + CRAPPER_PIN))

    def test_mu_pb_2_a_pin_that_is_not_a_full_commit_stops_the_runner(self):
        # A pin that is not a full commit cannot name the source, so the runner must stop
        # instead of comparing against an unknown commit.
        texts = ['[original]\ncommit = "abc"\n[crapper]\ncommit = "%s"\n' % CRAPPER_PIN,
                 f"[original]\n[crapper]\ncommit = \"{CRAPPER_PIN}\"\n",
                 f'[original]\ncommit = "{PIN}"\n[crapper]\ncommit = "abc"\n']
        for text in texts:
            with self.subTest(text=text), tempfile.TemporaryDirectory() as tmp:
                path = Path(tmp) / "upstream.toml"
                path.write_text(text, encoding="utf-8")
                with self.assertRaises(RuntimeError):
                    runner.check_pin("/v/bin/python", path, run=Tree())
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "upstream.toml"
            for section, text in (("original", 'commit = "abc"\n'),
                                  ("crapper", f'[crapper]\ncommit = "abc"\n')):
                path.write_text(text, encoding="utf-8")
                with self.subTest(section=section), self.assertRaises(RuntimeError):
                    runner.pinned_commit(path, section)


class OriginalCommit(unittest.TestCase):
    def test_mu_pb_1_the_commit_of_the_checkout_the_venv_installs_from(self):
        commit = "1" * 40
        tree = checkout(commit)
        self.assertEqual(runner.original_commit("/v/bin/python", tree), commit)

    def test_mu_pb_1_the_pip_metadata_answers_only_for_site_packages(self):
        # A venv built from a site-packages install has no checkout, and pip records the
        # commit in direct_url.json instead. A PYTHONPATH copy is not a pip install, so its
        # direct_url.json proves nothing (MU-PB-1).
        commit = "2" * 40
        for path, want in [
            ("/v/lib/site-packages/mutator/__init__.py", commit),
            ("/elsewhere/mutator/__init__.py", ""),
        ]:
            with self.subTest(path=path):
                tree = Tree(packages={"mutator": path},
                            pip={"mutator": direct_url(commit)})
                self.assertEqual(runner.original_commit("/v/bin/python", tree), want)

    def test_mu_pb_1_a_dirty_checkout_is_not_the_pin(self):
        commit = "1" * 40
        tree = checkout(commit)
        tree.dirty = {"/src/mutator/mutator"}
        self.assertEqual(runner.original_commit("/v/bin/python", tree), "")

    def test_mu_pb_1_a_checkout_that_does_not_hold_the_package_is_not_the_pin(self):
        # A venv inside another repository gives that repository's HEAD, which is not the
        # original at all.
        tree = checkout("1" * 40)
        tree.toplevels = {"/src/mutator/mutator": "/elsewhere"}
        self.assertEqual(runner.original_commit("/v/bin/python", tree), "")

    def test_mu_pb_1_no_commit_at_all_is_empty(self):
        self.assertEqual(runner.original_commit("/v/bin/python", Tree()), "")

    def test_mu_pb_1_a_python_that_cannot_start_gives_no_commit(self):
        def cannot_start(*_args, **_kwargs):
            raise FileNotFoundError(2, "No such file or directory")

        self.assertEqual(runner.original_commit("/no/such/python", cannot_start), "")
        with self.assertRaises(RuntimeError):
            runner.check_pin("/no/such/python", run=cannot_start)


class CheckoutCommit(unittest.TestCase):
    """The git questions of the runner: `-C` names the directory, and the checkout must
    hold the package and be clean."""

    def test_mu_pb_1_the_git_arguments_name_the_package_and_its_checkout(self):
        tree = checkout("1" * 40)
        runner.original_commit("/v/bin/python", tree)
        git_calls = [call for call in tree.calls if call[0] == "git"]
        self.assertEqual(git_calls, [
            ["git", "-C", "/src/mutator/mutator", "rev-parse", "--show-toplevel"],
            ["git", "-C", "/src/mutator/mutator", "status", "--porcelain"],
            ["git", "-C", "/src/mutator/mutator", "rev-parse", "HEAD"],
        ])

    def test_mu_pb_1_a_git_that_fails_falls_to_the_pip_metadata(self):
        # The package is present, but git cannot read a commit for it. The pip record of a
        # site-packages install still names the pin.
        commit = "2" * 40
        tree = Tree(packages={"mutator": "/v/lib/site-packages/mutator/__init__.py"},
                    pip={"mutator": direct_url(commit)}, git_fails={"/v/lib/site-packages"})
        self.assertEqual(runner.original_commit("/v/bin/python", tree), commit)

    def test_mu_pb_1_a_checkout_and_pip_both_answering_prefers_the_checkout(self):
        commit = "3" * 40
        tree = Tree(packages={"mutator": "/v/lib/site-packages/mutator/__init__.py"},
                    commits={"/v/lib/site-packages/mutator": commit},
                    pip={"mutator": direct_url("4" * 40)})
        self.assertEqual(runner.original_commit("/v/bin/python", tree), commit)


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
    def test_mu_pb_2_a_venv_at_both_pins_passes(self):
        got = runner.check_pin("/v/bin/python", run=checkout(PIN))
        self.assertEqual(got, PIN)

    def test_mu_pb_2_an_original_at_another_commit_stops_and_names_both(self):
        other = "3" * 40
        with self.assertRaises(RuntimeError) as caught:
            runner.check_pin("/v/bin/python", run=checkout(other))
        message = str(caught.exception)
        self.assertIn(other, message)
        self.assertIn(PIN, message)

    def test_mu_pb_2_a_venv_without_a_readable_commit_stops(self):
        with self.assertRaises(RuntimeError) as caught:
            runner.check_pin("/v/bin/python", run=Tree())
        self.assertIn(PIN, str(caught.exception))

    def test_mu_pb_1_a_crapper_checkout_at_another_commit_stops_and_names_both(self):
        tree = checkout(PIN)
        tree.commits["/src/crapper/crapper"] = "5" * 40
        with self.assertRaises(RuntimeError) as caught:
            runner.check_pin("/v/bin/python", run=tree)
        message = str(caught.exception)
        self.assertIn("5" * 40, message)
        self.assertIn(CRAPPER_PIN, message)

    def test_mu_pb_1_a_missing_crapper_checkout_stops(self):
        tree = checkout(PIN)
        tree.packages.pop("crapper")
        with self.assertRaises(RuntimeError) as caught:
            runner.check_pin("/v/bin/python", run=tree)
        self.assertIn(CRAPPER_PIN, str(caught.exception))


class OriginalPython(unittest.TestCase):
    def test_mu_pb_2_the_runner_needs_original_python(self):
        # There is no default venv path: a path that the host does not have only moves the
        # error to the first run, so the runner asks for the variable.
        with mock.patch.dict("os.environ", {}, clear=True), \
                self.assertRaises(SystemExit) as caught:
            runner.original_python()
        self.assertIn("ORIGINAL_PYTHON", str(caught.exception))
        with mock.patch.dict("os.environ", {"ORIGINAL_PYTHON": "/v/bin/python"}):
            self.assertEqual(runner.original_python(), "/v/bin/python")
            self.assertEqual(runner.original_cmd(), ["/v/bin/python", "-m", "mutator"])


class Main(unittest.TestCase):
    def test_mu_pb_2_main_stops_before_any_run_when_the_pin_check_fails(self):
        # The pin check runs before the first entry. Deleting its call must fail this test.
        with mock.patch.dict("os.environ", {"ORIGINAL_PYTHON": "/v/bin/python"}), \
                mock.patch.object(runner, "run_all") as ran, \
                mock.patch.object(runner, "edn_reader") as reader, \
                contextlib.redirect_stderr(io.StringIO()):
            code = runner.main([], run=checkout("6" * 40))
        self.assertEqual(code, 1)
        ran.assert_not_called()
        reader.assert_not_called()

    def test_mu_pb_2_main_names_the_pin_on_stderr(self):
        tree = checkout("6" * 40)
        err = io.StringIO()
        with mock.patch.dict("os.environ", {"ORIGINAL_PYTHON": "/v/bin/python"}), \
                mock.patch.object(runner, "run_all"), contextlib.redirect_stderr(err):
            code = runner.main([], run=tree)
        self.assertEqual(code, 1)
        self.assertIn(PIN, err.getvalue())
        self.assertIn("6" * 40, err.getvalue())


class SameInterpreter(unittest.TestCase):
    def test_mu_pb_1_an_original_python_that_differs_from_this_one_stops(self):
        # The EDN reader comes from the interpreter that runs run.py, so the two must match.
        tree = Tree(interpreter="/usr/bin/python3.12")
        with mock.patch.object(runner.sys, "executable", "/usr/bin/python3.12"):
            self.assertTrue(runner.same_interpreter("/v/bin/python", tree))
        with mock.patch.object(runner.sys, "executable", "/other/python3.12"):
            self.assertFalse(runner.same_interpreter("/v/bin/python", tree))


class EdnReader(unittest.TestCase):
    def test_mu_pb_1_an_original_python_that_differs_from_this_one_stops(self):
        tree = Tree()
        with mock.patch.object(runner.sys, "executable", "/other/python3.12"), \
                self.assertRaises(SystemExit) as caught:
            runner.edn_reader("/v/bin/python", tree)
        self.assertIn("ORIGINAL_PYTHON", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
