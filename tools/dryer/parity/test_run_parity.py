#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Tests for run_side, record, check, and the runner steps (DR-NFR-2, AC-10).

DR-NFR-2: the runner reports the wall-clock time of one program. run_side runs
one side 5 times. Run 1 gives the result that the runner compares, and runs 2
to 5 only measure time. A snapshot from an earlier run must not pass the check
of a later run, so run_side removes .metrics before each timing run. A timing
run must also exit like run 1. Its error names the case, the side, and the
stderr of that run. --record deletes parity/expected/ only after every case ran
fine, so a failed case keeps the old recordings.

The fake program counts its runs, prints the count, and writes the count into
.metrics/dry.edn. It also records whether a snapshot was already on disk when
it started. Three tests run it. Two run CRASH, and one each runs QUIET and
FAILING. The other tests replace run_case, run_side, or make_copy with a stub.

Eighteen tests run. Some of them fail on the runner before this change, and
some pass there too: the two median tests only check that median_seconds
reports the middle sample, and that function did not change. Several tests
pin behavior that an earlier commit added, such as the result of run 1 and the
exit code check.

Stdlib only. Run from tools/dryer/ with Python 3.9 or later:

    python3 -m unittest parity/test_run_parity.py

crapper runs `python3 -m unittest discover -s .` from the crate root. mutator
runs `python3 -m unittest discover` there, with no `-s`, and finds the same
tests because parity/ is a package. cargo test runs them with python3.12.
"""

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

# No resolve(): mutator runs this test inside an overlay where this file is a
# symlink and run_parity.py is the mutated copy. resolve() would follow the
# symlink back to the real runner, and every mutant would load the original.
RUN_PATH = Path(__file__).parent / "run_parity.py"

# A stand-in for one side of a parity case. It reads --root, counts its runs in
# <root>/count, notes a snapshot that is already there in <root>/seen, then
# writes its own snapshot and prints its run number.
COUNTER = """count=$(cat "$root/count" 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > "$root/count"
"""

FAKE = (
    """#!/bin/sh
root=""
prev=""
for arg in "$@"; do
    if [ "$prev" = "--root" ]; then root="$arg"; fi
    prev="$arg"
done
"""
    + COUNTER
    + """if [ -e "$root/.metrics/dry.edn" ]; then
    echo stale >> "$root/seen"
else
    echo fresh >> "$root/seen"
fi
mkdir -p "$root/.metrics"
echo "$count" > "$root/.metrics/dry.edn"
echo "run $count"
"""
)

# A side that fails on run 3 with exit code 7, like a program that crashes
# after two good runs. A crashed timing run must not lower the median.
CRASH = (
    """#!/bin/sh
root=""
prev=""
for arg in "$@"; do
    if [ "$prev" = "--root" ]; then root="$arg"; fi
    prev="$arg"
done
"""
    + COUNTER
    + """if [ "$count" -ge 3 ]; then
    echo "boom $count" >&2
    exit 7
fi
mkdir -p "$root/.metrics"
echo "$count" > "$root/.metrics/dry.edn"
echo "run $count"
"""
)

# A side that exits 1 on every run, like a program that cannot analyze its input.
FAILING = (
    """#!/bin/sh
root=""
prev=""
for arg in "$@"; do
    if [ "$prev" = "--root" ]; then root="$arg"; fi
    prev="$arg"
done
"""
    + COUNTER
    + """echo "no files $count" >&2
exit 1
"""
)

# A side that writes no snapshot at all, like a program with nothing to report.
QUIET = (
    """#!/bin/sh
root=""
prev=""
for arg in "$@"; do
    if [ "$prev" = "--root" ]; then root="$arg"; fi
    prev="$arg"
done
"""
    + COUNTER
    + """echo "run $count"
"""
)


def load_run():
    spec = importlib.util.spec_from_file_location("dryer_parity_run", RUN_PATH)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


run = load_run()


class RunSide(unittest.TestCase):
    def setUp(self):
        self.addCleanup(self.tmp_cleanup)
        tmp = tempfile.TemporaryDirectory(prefix="run-side-test-")
        self.tmp = tmp
        base = Path(tmp.name)
        self.root = base / "proj"
        self.root.mkdir()
        self.prefix = [str(self.fake_program(base))]
        self.port = base / "dryer"

    def tmp_cleanup(self):
        self.tmp.cleanup()

    def fake_program(self, base):
        return self.write_program(base, "fake_dryer", FAKE)

    def write_program(self, base, name, text):
        script = base / name
        script.write_text(text, encoding="utf-8")
        os.chmod(script, 0o755)
        return script

    def run_side(self):
        return run.run_side(self.prefix, self.root, [], {"PATH": "/usr/bin:/bin"},
                            "case port")

    def lines(self, name):
        return (self.root / name).read_text(encoding="utf-8").split()

    def stubbed_main(self, cases, run_case, expected, fixes=None):
        """Patches what main reads and starts: the pinned original, the cases, the fixes,
        the case runner, the port binary, and the expected directory. No process runs.
        The mocks stay on self.mocks, so a test can count the port binary calls."""
        mocks = {
            "ensure_original": mock.Mock(return_value=Path("/nonexistent")),
            "load_cases": mock.Mock(return_value=cases),
            "load_fixes": mock.Mock(return_value=fixes or {}),
            "run_case": mock.Mock(side_effect=run_case),
            "port_binary": mock.Mock(return_value=self.port),
        }
        mocks["EXPECTED"] = expected
        self.mocks = mocks
        return mock.patch.multiple(run, **mocks)

    # DR-NFR-2: the runner compares the result of run 1, not the result of run 5.
    def test_dr_nfr_2_the_compared_result_comes_from_run_1(self):
        res = self.run_side()
        self.assertEqual(res.code, 0)
        self.assertEqual(res.stdout, "run 1\n")
        self.assertEqual(res.edn, b"1\n")

    # DR-NFR-2: the reported time is the median of the 5 runs. The stub records
    # the samples and returns 123.456, and the fake clock gives spans 1 to 5.
    def test_dr_nfr_2_the_reported_time_is_the_median_of_five_samples(self):
        clock = mock.Mock()
        clock.perf_counter.side_effect = [
            0.0, 1.0, 10.0, 12.0, 20.0, 23.0, 30.0, 34.0, 40.0, 45.0,
        ]
        seen = []

        def stub(samples):
            seen.extend(samples)
            return 123.456

        with mock.patch.object(run, "time", clock), mock.patch.object(
            run, "median_seconds", stub
        ):
            res = self.run_side()
        self.assertEqual(self.lines("count"), ["5"])
        self.assertEqual(seen, [1.0, 2.0, 3.0, 4.0, 5.0])
        self.assertEqual(res.seconds, 123.456)

    # DR-NFR-2: a timing run that exits unlike run 1 must not lower the median.
    # The fake side exits 7 on run 3, so run_side stops with an error.
    def test_dr_nfr_2_a_timing_run_with_a_different_exit_code_fails(self):
        base = self.root.parent
        self.prefix = [str(self.write_program(base, "crash_dryer", CRASH))]
        with self.assertRaises(SystemExit) as caught:
            self.run_side()
        self.assertEqual(
            str(caught.exception),
            "case port: timing run 3 exited 7, but run 1 exited 0; stderr: boom 3",
        )
        self.assertEqual(self.lines("count"), ["3"])

    # DR-NFR-2: the runner reports the middle sample. The samples 1, 2 and 100
    # tell the median (2) from the minimum (1) and the mean (34.33).
    def test_dr_nfr_2_median_seconds_reports_the_middle_sample(self):
        self.assertEqual(run.median_seconds([5, 1, 4, 2, 3]), 3)
        self.assertEqual(run.median_seconds([1, 2, 100]), 2)
        self.assertEqual(run.median_seconds([4, 1, 3, 2]), 2.5)

    # DR-NFR-2: a side that cannot analyze its input exits 1 on every run. Run 1
    # sets the compared exit code, so runs 2 to 5 exit like it and run_side does
    # not fail. A check of `returncode != 0` would stop this side.
    def test_dr_nfr_2_a_side_that_always_exits_one_does_not_raise(self):
        base = self.root.parent
        self.prefix = [str(self.write_program(base, "failing_dryer", FAILING))]
        res = self.run_side()
        got = (res.code, res.stdout, res.stderr, res.edn)
        self.assertEqual(got, (1, "", "no files 1\n", None))
        self.assertEqual(self.lines("count"), ["5"])

    # DR-NFR-2: the error of a failed timing run names the case, the side, and
    # the stderr of the run that failed.
    def test_dr_nfr_2_a_failed_timing_run_names_the_case_the_side_and_its_stderr(self):
        base = self.root.parent
        self.prefix = [str(self.write_program(base, "crash_dryer", CRASH))]
        with self.assertRaises(SystemExit) as caught:
            run.run_side(self.prefix, self.root, [], {"PATH": "/usr/bin:/bin"},
                         "help port")
        self.assertEqual(
            str(caught.exception),
            "help port: timing run 3 exited 7, but run 1 exited 0; stderr: boom 3",
        )

    # DR-NFR-2: --record keeps the old recordings when a case fails. A delete
    # before the run would leave expected/ empty, and the replay would find
    # nothing to check.
    def test_dr_nfr_2_record_keeps_the_old_recordings_when_a_case_fails(self):
        base = self.root.parent
        expected = base / "expected"
        first = expected / "first"
        first.mkdir(parents=True)
        (first / "exit_code").write_text("0\n", encoding="utf-8")
        (first / "stdout").write_text("old stdout\n", encoding="utf-8")
        cases = [{"name": "first"}, {"name": "second"}]

        def run_one(case):
            if case["name"] == "second":
                raise SystemExit("second port: timing run 2 exited 1, but run 1 exited 0")
            return run.Result(0, "new stdout\n", None, 0.0, ""), None

        with self.assertRaises(SystemExit):
            run.record(cases, expected, run_one, replace=True)
        self.assertEqual((first / "stdout").read_text(encoding="utf-8"), "old stdout\n")
        self.assertFalse((expected / "second").exists())

    # DR-NFR-2: --record replaces the old recordings once every case ran fine,
    # and a fix case also records the port's own output under port/.
    def test_dr_nfr_2_record_replaces_the_old_recordings_after_every_case_ran(self):
        base = self.root.parent
        expected = base / "expected"
        stale = expected / "gone" / "stdout"
        stale.parent.mkdir(parents=True)
        stale.write_text("stale\n", encoding="utf-8")
        cases = [{"name": "only"}, {"name": "fix"}]

        def run_one(case):
            port = run.Result(0, "port stdout\n", None, 0.0, "") if case["name"] == "fix" else None
            return run.Result(0, "new stdout\n", b"1\n", 0.0, ""), port

        self.assertEqual(run.record(cases, expected, run_one, replace=True), 0)
        self.assertFalse((expected / "gone").exists())
        self.assertEqual((expected / "only" / "stdout").read_text(encoding="utf-8"),
                         "new stdout\n")
        self.assertEqual((expected / "only" / "dry.edn").read_bytes(), b"1\n")
        self.assertEqual((expected / "fix" / "port" / "stdout").read_text(encoding="utf-8"),
                         "port stdout\n")
        self.assertFalse((expected / "fix" / "port" / "dry.edn").exists())

        # The first --record of a checkout has no expected/ yet, so the delete
        # of the old recordings must accept a missing directory.
        fresh = base / "fresh" / "expected"
        self.assertEqual(run.record(cases[:1], fresh, run_one, replace=True), 0)
        self.assertEqual((fresh / "only" / "stdout").read_text(encoding="utf-8"),
                         "new stdout\n")

    # DR-NFR-2: --record through main keeps the old recordings when a later case
    # fails, because the delete waits until every side ran. The stub replaces the
    # original checkout, the port binary, and the case runner.
    def test_dr_nfr_2_main_record_keeps_the_old_recordings_when_a_case_fails(self):
        base = self.root.parent
        expected = base / "expected"
        (expected / "first").mkdir(parents=True)
        (expected / "first" / "stdout").write_text("old\n", encoding="utf-8")
        cases = [{"name": "first", "fixture": "f", "args": []},
                 {"name": "second", "fixture": "f", "args": []}]

        def run_case(case, *_args):
            if case["name"] == "second":
                raise SystemExit("second original: timing run 2 exited 1, but run 1 exited 0")
            return run.Result(0, "new\n", None, 0.0, ""), None

        with self.stubbed_main(cases, run_case, expected):
            with self.assertRaises(SystemExit):
                run.main(["--record"])
        self.assertEqual((expected / "first" / "stdout").read_text(encoding="utf-8"), "old\n")

    # DR-NFR-2: main starts the port binary for a check case and for a --record
    # case that has a fix, and it runs one port for all cases. A --record case
    # without a fix runs only the original, so main leaves the port binary alone.
    def test_dr_nfr_2_main_starts_the_port_only_when_a_case_needs_it(self):
        base = self.root.parent
        expected = base / "expected"
        got = run.Result(0, "same\n", None, 0.0, "")
        port = run.Result(0, "port\n", None, 0.0, "")
        fix = [{"id": "f1", "fixture": "fixed"}]
        cases = [{"name": "case", "fixture": "f", "args": []},
                 {"name": "fixed", "fixture": "f", "args": [], "fix": "f1"}]
        answers = {"case": (got, got), "fixed": (got, port)}

        def runner(case, _orig_prefix, _orig_env, port_prefix):
            orig, port = answers[case["name"]]
            return orig, port if port_prefix is not None else None

        with self.stubbed_main(cases, runner, expected, {"fixed": fix}):
            self.assertEqual(run.main(["--record"]), 0)
        self.assertEqual(self.mocks["port_binary"].call_count, 1)
        self.assertEqual((expected / "fixed" / "port" / "stdout").read_text(encoding="utf-8"),
                         "port\n")
        self.assertFalse((expected / "case" / "port").exists())

        with self.stubbed_main(cases[:1], runner, expected, {"fixed": fix}):
            self.assertEqual(run.main(["--record", "--case", "case"]), 0)
        self.assertEqual(self.mocks["port_binary"].call_count, 0)

        # A check case has no fix, and main still sends it to both sides.
        seen = []

        def checking(case, _orig_prefix, _orig_env, port_prefix):
            seen.append(port_prefix)
            return got, got

        with self.stubbed_main(cases[:1], checking, expected):
            self.assertEqual(run.main([]), 0)
        self.assertEqual(self.mocks["port_binary"].call_count, 1)
        self.assertEqual(seen, [[str(self.port)]])

    # DR-NFR-2: a case that differs makes main return 1, and a case that matches
    # makes it return 0.
    def test_dr_nfr_2_main_returns_one_when_a_case_fails(self):
        base = self.root.parent
        expected = base / "expected"
        cases = [{"name": "case", "fixture": "f", "args": []}]
        same = run.Result(0, "same\n", None, 0.0, "")
        other = run.Result(0, "other\n", None, 0.0, "")
        with self.stubbed_main(cases, lambda *_args: (same, same), expected):
            self.assertEqual(run.main([]), 0)
        with self.stubbed_main(cases, lambda *_args: (same, other), expected):
            self.assertEqual(run.main([]), 1)

    # DR-NFR-2: check counts the verdicts of its cases and prints one summary
    # line. An accepted fix is not a failure.
    def test_dr_nfr_2_check_counts_each_verdict_and_passes_when_none_fails(self):
        base = self.root.parent
        expected = base / "expected"
        fix = [{"id": "f1", "fixture": "fix"}]
        good = run.Result(0, "good\n", None, 0.0, "")
        bad = run.Result(0, "bad\n", None, 0.0, "")
        (expected / "fix" / "port").mkdir(parents=True)
        (expected / "fix" / "port" / "exit_code").write_text("0\n", encoding="utf-8")
        (expected / "fix" / "port" / "stdout").write_text("port\n", encoding="utf-8")
        port = run.Result(0, "port\n", None, 0.0, "")
        cases = [{"name": "ok"}, {"name": "fix"}]
        fixes = {"fix": fix}
        answers = {"ok": (good, good), "fix": (bad, port)}

        def run_one(case):
            return answers[case["name"]]

        with mock.patch.object(run, "EXPECTED", expected):
            self.assertEqual(run.check(cases, fixes, run_one), 0)
            answers["ok"] = (good, bad)
            self.assertEqual(run.check(cases[:1], {}, run_one), 1)

    # DR-NFR-2, DR-PB-3: a fix case compares the port with its own recorded
    # output. It fails when that recording is missing, and it fails when the port
    # stops differing from the original.
    def test_dr_nfr_2_a_fix_case_compares_the_port_with_its_own_recording(self):
        base = self.root.parent
        expected = base / "expected"
        recorded = run.Result(0, "port\n", None, 0.0, "")
        orig = run.Result(0, "orig\n", None, 0.0, "")
        fix = [{"id": "f1", "fixture": "case"}]

        self.assertEqual(run.check_case("case", fix, orig, recorded),
                         (["no recorded parity/expected/case/port/"], "FAIL"))
        (expected / "case" / "port").mkdir(parents=True)
        (expected / "case" / "port" / "exit_code").write_text("0\n", encoding="utf-8")
        (expected / "case" / "port" / "stdout").write_text("port\n", encoding="utf-8")
        with mock.patch.object(run, "EXPECTED", expected):
            self.assertEqual(run.check_case("case", fix, orig, recorded), ([], "PENDING"))
            self.assertEqual(run.check_case("case", fix, recorded, recorded)[1], "FAIL")

    # DR-NFR-2: run_case runs the original and the port on their own copies, and
    # each timing error names the case and the side. An original that prints a
    # traceback stops the case at once.
    def test_dr_nfr_2_run_case_runs_both_sides_on_their_own_copies(self):
        case = {"name": "case", "fixture": "f", "args": ["--x"]}
        orig = run.Result(0, "orig\n", None, 0.0, "")
        port = run.Result(0, "port\n", None, 0.0, "")
        seen = []

        def fake_run_side(prefix, root, args, env, label):
            seen.append((prefix, str(root), args, env, label))
            return orig if label.endswith("original") else port

        with mock.patch.object(run, "make_copy", side_effect=self.copy_named), \
             mock.patch.object(run, "run_side", side_effect=fake_run_side):
            self.assertEqual(run.run_case(case, ["o"], {"A": "1"}, ["p"]), (orig, port))
        self.assertEqual([s[4] for s in seen], ["case original", "case port"])
        self.assertEqual([s[0] for s in seen], [["o"], ["p"]])
        self.assertEqual([Path(s[1]).name for s in seen], ["original", "port"])
        self.assertEqual(seen[1][2], ["--x"])
        self.assertIsNone(seen[1][3])

    # DR-NFR-2: an original that prints a traceback stops the case, and the error
    # names the case.
    def test_dr_nfr_2_run_case_stops_on_a_traceback_from_the_original(self):
        case = {"name": "case", "fixture": "f", "args": []}
        crashed = run.Result(1, "", None, 0.0, "Traceback (most recent call last):\n")
        with mock.patch.object(run, "make_copy", side_effect=self.copy_named), \
             mock.patch.object(run, "run_side", return_value=crashed):
            with self.assertRaises(SystemExit) as caught:
                run.run_case(case, ["o"], {}, ["p"])
        self.assertIn("original crashed on case", str(caught.exception))

    # DR-NFR-2: a case without a port side runs only the original.
    def test_dr_nfr_2_run_case_skips_the_port_when_there_is_none(self):
        case = {"name": "case", "fixture": "f", "args": []}
        orig = run.Result(0, "orig\n", None, 0.0, "")
        with mock.patch.object(run, "make_copy", side_effect=self.copy_named), \
             mock.patch.object(run, "run_side", return_value=orig) as side:
            self.assertEqual(run.run_case(case, ["o"], {}, None), (orig, None))
        self.assertEqual(side.call_count, 1)

    def copy_named(self, fixture, git_setup, base):
        base.mkdir(parents=True)
        return base

    # DR-NFR-2: no timing run sees the snapshot of an earlier run.
    def test_dr_nfr_2_no_timing_run_sees_an_earlier_snapshot(self):
        self.run_side()
        self.assertEqual(self.lines("seen"), ["fresh"] * 5)

    # DR-NFR-2: a side that writes no snapshot still runs 5 times. The cleanup
    # before a timing run has nothing to remove, and run_side does not fail.
    def test_dr_nfr_2_a_side_without_a_snapshot_still_runs_five_times(self):
        base = self.root.parent
        self.prefix = [str(self.write_program(base, "quiet_dryer", QUIET))]
        res = self.run_side()
        self.assertEqual(res.code, 0)
        self.assertEqual(res.stdout, "run 1\n")
        self.assertIsNone(res.edn)
        self.assertEqual(self.lines("count"), ["5"])
