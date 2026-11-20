#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Tests for run_side, record, check, and the runner steps (DR-NFR-2, AC-10),
and for the case list reader (AC-1).

DR-NFR-2: the runner reports the wall-clock time of one program. run_side runs
one side 5 times. Run 1 gives the result that the runner compares, and runs 2
to 5 only measure time. A snapshot from an earlier run must not pass the check
of a later run, so run_side removes .metrics before each timing run. A timing
run must also exit like run 1. Its error names the case, the side, and the
stderr of that run. --record deletes parity/expected/ only after every case ran
fine, so a failed case keeps the old recordings.

AC-10: --check-timing turns the time into a verdict. A case fails when the
median of the port is not lower than the median of the original, and an equal
median fails. --timing-runs sets the count of runs per side. check_timing
reports the two medians in one line.

The fake program counts its runs, prints the count, and writes the count into
.metrics/dry.edn. It also records whether a snapshot was already on disk when
it started. Several tests run one of these stubs, and the timing tests run
FAKE against SLOW, which sleeps 0.05 s on each run. The other tests replace
run_case, run_side, or make_copy with a stub.

AC-1: load_cases reads parity/coverage.toml and rejects a case that lacks a
name, a fixture, or args, a duplicate name, and a selected name that no case
has.

Fifty-five tests run. Some of them fail on the runner before this change, and
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

import contextlib
import importlib.util
import io
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

# A side that sleeps 0.05 s before it reports, like a program that is slower than
# the port on the same input (DR-NFR-2, AC-10).
SLOW = (
    """#!/bin/sh
root=""
prev=""
for arg in "$@"; do
    if [ "$prev" = "--root" ]; then root="$arg"; fi
    prev="$arg"
done
"""
    + COUNTER
    + """sleep 0.05
mkdir -p "$root/.metrics"
echo "$count" > "$root/.metrics/dry.edn"
echo "run $count"
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

    def main_with_one_case(self, answers, argv):
        """Runs main for one case, and returns its exit code. Each answer is a pair of a
        Result for the original and one for the port, and one answer is used per case run."""
        calls = iter(answers)
        with self.stubbed_main([{"name": "case", "fixture": "f", "args": []}],
                               lambda *_args: next(calls), self.root.parent / "expected"):
            return run.main(argv)

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

        def runner(case, _orig_prefix, _orig_env, port_prefix, _runs):
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

        def checking(case, _orig_prefix, _orig_env, port_prefix, _runs):
            seen.append(port_prefix)
            return got, got

        with self.stubbed_main(cases[:1], checking, expected):
            self.assertEqual(run.main([]), 0)
        self.assertEqual(self.mocks["port_binary"].call_count, 1)
        self.assertEqual(seen, [[str(self.port)]])

    # DR-NFR-2: a case that differs makes main return 1, and a case that matches
    # makes it return 0.
    def test_dr_nfr_2_main_returns_one_when_a_case_fails(self):
        same = run.Result(0, "same\n", None, 0.0, "")
        other = run.Result(0, "other\n", None, 0.0, "")
        self.assertEqual(self.main_with_one_case([(same, same)], []), 0)
        self.assertEqual(self.main_with_one_case([(same, other)], []), 1)

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

        def fake_run_side(prefix, root, args, env, label, _runs):
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

    # AC-10: one stub side in its own project, timed by run_side. The two sides
    # of a case never share a copy, just as the runner gives each its own.
    def timed_side(self, name, text):
        base = self.root.parent
        prefix = [str(self.write_program(base, name, text))]
        root = base / f"root-{name}"
        root.mkdir()
        return run.run_side(prefix, root, [], {"PATH": "/usr/bin:/bin"}, name)

    # AC-10, DR-NFR-2: a port that is faster than the original passes the timing
    # check. The original sleeps 0.05 s on each run, and the port does not, so
    # the median of the port is lower.
    def test_ac_10_a_port_that_is_faster_passes_the_timing_check(self):
        original = self.timed_side("slow_dryer", SLOW)
        port = self.timed_side("fake_dryer", FAKE)
        self.assertLess(port.seconds, original.seconds)
        self.assertEqual(run.check_timing(original, port), [])

    # AC-10, DR-NFR-2: a port that is slower than the original fails the case,
    # and the diff line names both medians. The two stub sides write the same
    # stdout and snapshot, so the timing line is the only difference.
    def test_ac_10_a_port_that_is_not_faster_fails_the_case(self):
        original = self.timed_side("fake_dryer", FAKE)
        port = self.timed_side("slow_dryer", SLOW)
        diffs, status = run.check_case("case", [], original, port, timing=True)
        self.assertEqual(status, "FAIL")
        self.assertEqual(len(diffs), 1)
        self.assertTrue(diffs[0].startswith("timing: port median "), diffs[0])

    # AC-10: AC-10 asks for a median that is lower, so an equal median fails.
    def test_ac_10_a_port_that_is_as_fast_as_the_original_fails(self):
        same = run.Result(0, "", None, 0.5, "")
        self.assertEqual(
            run.check_timing(same, same),
            ["timing: port median 0.500s is not lower than original median 0.500s"],
        )

    # AC-10, DR-NFR-2: DR-NFR-2 names 5 runs, so the default is 5, and a caller
    # can set the count. A median of 3 runs is the middle of 3 samples.
    def test_ac_10_the_timing_check_runs_five_times_by_default_and_n_when_set(self):
        self.assertEqual(run.TIMED_RUNS, 5)
        self.run_side()
        self.assertEqual(self.lines("count"), ["5"])
        root = self.root.parent / "three"
        root.mkdir()
        res = run.run_side(self.prefix, root, [], {"PATH": "/usr/bin:/bin"},
                           "case port", runs=3)
        self.assertEqual(res.stdout, "run 1\n")
        self.assertEqual((root / "count").read_text(encoding="utf-8").split(), ["3"])

    # AC-10: main turns the timing into a check only with --check-timing. A
    # slower port fails the run with the flag and passes without it, because
    # the outputs of the two sides match.
    def test_ac_10_main_checks_the_time_only_when_the_flag_is_set(self):
        fast = run.Result(0, "same\n", None, 0.1, "")
        slow = run.Result(0, "same\n", None, 9.0, "")
        self.assertEqual(self.main_with_one_case([(slow, fast)], []), 0)
        self.assertEqual(self.main_with_one_case([(fast, slow)], []), 0)
        self.assertEqual(self.main_with_one_case([(fast, slow)], ["--check-timing"]), 1)

    # AC-10: --timing-runs rejects a count that cannot give a median, and it reads
    # a whole number as that number.
    def test_ac_10_timing_runs_takes_a_whole_number_of_at_least_two(self):
        self.assertEqual(run.positive_int("7"), 7)
        for bad in ("1", "0", "-3", "two", "2.5"):
            with self.assertRaises(run.argparse.ArgumentTypeError) as caught:
                run.positive_int(bad)
            self.assertIn("runs", str(caught.exception))

    # AC-10, DR-PB-6: report_case prints the verdict, the diff lines, and the
    # approval line, and it counts the time only when the caller asks for it.
    # The approved fix prints "accepted", and a PASS prints no approval line.
    def test_ac_10_report_case_prints_the_verdict_and_the_approval_line(self):
        base = self.root.parent
        expected = base / "expected"
        approved = [{"id": "f1", "fixture": "fixed", "approved_by": "Mads Madsen"}]
        pending = [{"id": "f2", "fixture": "fixed"}]
        (expected / "fixed" / "port").mkdir(parents=True)
        (expected / "fixed" / "port" / "exit_code").write_text("0\n", encoding="utf-8")
        (expected / "fixed" / "port" / "stdout").write_text("port\n", encoding="utf-8")
        recorded = run.Result(0, "port\n", None, 0.0, "")
        orig = run.Result(0, "orig\n", None, 1.0, "")
        slow = run.Result(0, "orig\n", None, 9.0, "")

        def speak(*args, **kwargs):
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                status = run.report_case(*args, **kwargs)
            return status, out.getvalue()

        with mock.patch.object(run, "EXPECTED", expected):
            status, text = speak("fixed", approved, orig, recorded)
            self.assertEqual(status, "ACCEPTED")
            self.assertIn("accepted by: f1", text)
            status, text = speak("fixed", pending, orig, recorded)
            self.assertEqual(status, "PENDING")
            self.assertIn("awaiting approval by: f2", text)
            status, text = speak("case", [], orig, orig)
            self.assertEqual(status, "PASS")
            self.assertIn("original=1.00s port=1.00s", text)
            self.assertNotIn(" by: ", text)
            # The default is no timing check, so a slower port still passes.
            status, _ = speak("case", [], orig, slow)
            self.assertEqual(status, "PASS")
            status, text = speak("case", [], orig, slow, True)
            self.assertEqual(status, "FAIL")
            self.assertIn("timing: port median 9.000s", text)

    # AC-10: check counts each case once, so the summary line reports the number
    # of cases that failed. It prints one line for each case.
    def test_ac_10_check_counts_one_failure_as_one(self):
        good = run.Result(0, "same\n", None, 0.1, "")
        bad = run.Result(0, "other\n", None, 0.1, "")
        answers = {"ok": (good, good), "no": (good, bad)}

        def run_one(case):
            return answers[case["name"]]

        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(run.check([{"name": "ok"}, {"name": "no"}], {}, run_one), 1)
        text = out.getvalue()
        self.assertIn("2 case(s), 1 failed", text)
        self.assertIn("PASS     ok", text)
        self.assertIn("FAIL     no", text)

    # AC-10: --timing-runs sets the count of runs per side that main asks for.
    def test_ac_10_main_passes_the_run_count_to_the_case(self):
        base = self.root.parent
        expected = base / "expected"
        cases = [{"name": "case", "fixture": "f", "args": []}]
        same = run.Result(0, "same\n", None, 0.1, "")
        with self.stubbed_main(cases, lambda *_args: (same, same), expected):
            self.assertEqual(run.main(["--timing-runs", "2"]), 0)
        self.assertEqual(self.mocks["run_case"].call_args.args[-1], 2)


# AC-1: the case list of parity/coverage.toml is what the runner compares. A
# case needs a name, a fixture, and args, a name appears once, and a selected
# name must name a case. Each test writes the case list it needs into a temp
# directory and points load_cases at it, so the checks run for real.
class TempParity(unittest.TestCase):
    """Points PARITY at a temp directory, so a test writes the tables it reads."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="parity-table-test-")
        self.addCleanup(tmp.cleanup)
        self.base = Path(tmp.name)
        patcher = mock.patch.object(run, "PARITY", self.base)
        patcher.start()
        self.addCleanup(patcher.stop)

    def write_table(self, filename, table, *entries):
        lines = []
        for fields in entries:
            lines.append(f"[[{table}]]")
            lines.extend(f"{key} = {value}" for key, value in fields.items())
        (self.base / filename).write_text("\n".join(lines) + "\n", encoding="utf-8")


class LoadCases(TempParity):
    def write_cases(self, *cases):
        self.write_table("coverage.toml", "case", *cases)

    def load(self, selected=()):
        return run.load_cases(list(selected))

    # AC-1: every case of the file comes back in file order, with the args it
    # names. A flag case and a plain case must both stay whole.
    def test_ac_1_load_cases_reads_each_case_in_file_order(self):
        self.write_cases(
            {"name": '"threshold"', "fixture": '"a"', "args": '["--threshold", "0.8"]'},
            {"name": '"plain"', "fixture": '"b"', "args": "[]"},
        )
        self.assertEqual(
            self.load(),
            [
                {"name": "threshold", "fixture": "a", "args": ["--threshold", "0.8"]},
                {"name": "plain", "fixture": "b", "args": []},
            ],
        )

    # AC-1: a case that lacks one of its three keys stops the runner and names
    # the key, so a half-written case cannot reach the comparison.
    def test_ac_1_load_cases_rejects_a_case_without_a_name_a_fixture_or_args(self):
        for missing in ("name", "fixture", "args"):
            fields = {"name": '"a"', "fixture": '"b"', "args": "[]"}
            del fields[missing]
            self.write_cases(fields)
            with self.assertRaises(SystemExit) as caught:
                self.load()
            self.assertIn(f"case missing {missing}", str(caught.exception))

    # AC-1: two cases with one name would run the same verdict twice, so the
    # second one stops the runner.
    def test_ac_1_load_cases_rejects_a_duplicate_case_name(self):
        self.write_cases(
            {"name": '"same"', "fixture": '"a"', "args": "[]"},
            {"name": '"same"', "fixture": '"b"', "args": "[]"},
        )
        with self.assertRaises(SystemExit) as caught:
            self.load()
        self.assertEqual(str(caught.exception), "coverage.toml: duplicate case same")

    # AC-1: --case keeps the named cases in file order, not in the order of the
    # options, and it drops the cases that the caller did not name.
    def test_ac_1_load_cases_returns_only_the_selected_cases(self):
        self.write_cases(
            {"name": '"first"', "fixture": '"a"', "args": "[]"},
            {"name": '"second"', "fixture": '"b"', "args": "[]"},
            {"name": '"third"', "fixture": '"c"', "args": "[]"},
        )
        self.assertEqual([case["name"] for case in self.load(["third", "first"])],
                         ["first", "third"])

    # AC-1: a selected name that no case has stops the runner, so a mistyped
    # --case cannot pass as a run of every case.
    def test_ac_1_load_cases_rejects_an_unknown_selected_case(self):
        self.write_cases({"name": '"known"', "fixture": '"a"', "args": "[]"})
        with self.assertRaises(SystemExit) as caught:
            self.load(["typo", "known"])
        self.assertEqual(str(caught.exception), "unknown case(s): typo")

    # AC-1: the real case list loads, and each case carries the three keys. The
    # runner reads this file on every run, so a broken case stops every run.
    def test_ac_1_load_cases_reads_the_real_coverage_toml(self):
        real_parity = Path(run.__file__).resolve().parent
        with mock.patch.object(run, "PARITY", real_parity):
            cases = run.load_cases([])
            self.assertGreater(len(cases), 0)
            names = [case["name"] for case in cases]
            self.assertEqual(len(names), len(set(names)))
            for case in cases:
                self.assertTrue({"name", "fixture", "args"} <= set(case), case)
            first = run.load_cases([names[0]])
        self.assertEqual(first, cases[:1])


# DR-PB-6: load_fixes reads parity/fixes.toml and groups the entries by fixture,
# because a case looks up its own entries and a case without an entry is a plain
# comparison. A fix without an id or a fixture stops the runner.
class LoadFixes(TempParity):
    def write_fixes(self, *fixes):
        self.write_table("fixes.toml", "fix", *fixes)

    # DR-PB-6: the real file groups its entries by fixture, and each entry keeps
    # the id that the verdict prints.
    def test_dr_pb_6_load_fixes_groups_the_real_entries_by_fixture(self):
        real_parity = Path(run.__file__).resolve().parent
        with mock.patch.object(run, "PARITY", real_parity):
            by_case = run.load_fixes()
        self.assertIn("jsx-fix", by_case)
        self.assertEqual([fix["id"] for fix in by_case["jsx-fix"]], ["js-read-as-typescript"])

    # DR-PB-6: two fixes can name one fixture, and both stay in that fixture's
    # list, so the verdict can name both ids.
    def test_dr_pb_6_load_fixes_keeps_every_fix_of_one_fixture(self):
        self.write_fixes(
            {"id": '"a"', "fixture": '"case"'},
            {"id": '"b"', "fixture": '"case"'},
            {"id": '"c"', "fixture": '"other"'},
        )
        by_case = run.load_fixes()
        self.assertEqual(sorted(by_case), ["case", "other"])
        self.assertEqual([fix["id"] for fix in by_case["case"]], ["a", "b"])

    # DR-PB-6: a fix without an id or a fixture cannot be named or placed, so it
    # stops the runner.
    def test_dr_pb_6_load_fixes_rejects_a_fix_without_an_id_or_a_fixture(self):
        for missing in ("id", "fixture"):
            fields = {"id": '"a"', "fixture": '"case"'}
            del fields[missing]
            self.write_fixes(fields)
            with self.assertRaises(SystemExit) as caught:
                run.load_fixes()
            self.assertIn(f"fix missing {missing}", str(caught.exception))

    # DR-PB-6: a checkout with no fixes.toml reads as no fix, so every case is a
    # plain comparison.
    def test_dr_pb_6_load_fixes_reads_a_missing_table_as_no_fix(self):
        (self.base / "fixes.toml").write_text("# no entries\n", encoding="utf-8")
        self.assertEqual(run.load_fixes(), {})


# AC-1: the runner needs the built port to compare it with the original. A path
# on the command line wins, else the release binary, else the debug one, and a
# missing binary stops the run with the path that it looked for.
class PortBinary(TempParity):
    def setUp(self):
        super().setUp()
        patcher = mock.patch.object(run, "CRATE", self.base)
        patcher.start()
        self.addCleanup(patcher.stop)

    def build(self, profile):
        binary = self.base / "target" / profile / "dryer"
        binary.parent.mkdir(parents=True)
        binary.write_text("#!/bin/sh\n", encoding="utf-8")
        return binary

    # AC-1: a path given on the command line is the binary, and it must exist.
    def test_ac_1_port_binary_takes_the_path_that_the_caller_gives(self):
        binary = self.base / "my-dryer"
        binary.write_text("#!/bin/sh\n", encoding="utf-8")
        self.assertEqual(run.port_binary(str(binary)), binary)
        with self.assertRaises(SystemExit) as caught:
            run.port_binary(str(self.base / "gone"))
        self.assertIn("port binary not found", str(caught.exception))

    # AC-1: without a path, the release build wins over the debug build, and the
    # debug build is the fallback when there is no release build.
    def test_ac_1_port_binary_prefers_the_release_build(self):
        debug = self.build("debug")
        self.assertEqual(run.port_binary(None), debug)
        release = self.build("release")
        self.assertEqual(run.port_binary(None), release)

    # AC-1: with no build at all, the error names the release path that the
    # runner would use, so the caller knows to build the crate first.
    def test_ac_1_port_binary_names_the_release_path_when_nothing_is_built(self):
        with self.assertRaises(SystemExit) as caught:
            run.port_binary(None)
        message = str(caught.exception)
        self.assertIn("build the crate first", message)
        self.assertIn(str(self.base / "target" / "release" / "dryer"), message)


# AC-1: a case compares the original with the port "after both are parsed as
# EDN", so the snapshot reader must read every form that the port writes, and
# it must reject text that is not one EDN form. These tests read text, the way
# the runner reads parity/expected/<case>/dry.edn.
class Snapshots(unittest.TestCase):
    # AC-1: the atoms of a snapshot read as their values: keyword, nil, the two
    # booleans, a whole number, a fraction, a symbol, and a string.
    def test_ac_1_snapshot_atoms_read_as_their_values(self):
        read = run.parse_edn
        self.assertEqual(read(':score'), ("kw", "score"))
        self.assertIsNone(read("nil"))
        self.assertIs(read("true"), True)
        self.assertIs(read("false"), False)
        self.assertEqual(read("-12"), -12)
        self.assertEqual(read("1.5"), 1.5)
        self.assertEqual(read("-1.5e3"), -1500.0)
        self.assertEqual(read("bare"), ("sym", "bare"))
        self.assertEqual(read('"a b"'), "a b")

    # AC-1: the port writes a map of lists of maps, and the reader keeps that
    # shape, so a recorded snapshot compares equal to a fresh one.
    def test_ac_1_snapshot_collections_read_as_lists_and_maps(self):
        text = '{:candidates [ {:score 1.0 :left {:file "a.py"}} ]}'
        self.assertEqual(
            run.parse_edn(text),
            {("kw", "candidates"): [{("kw", "score"): 1.0,
                                     ("kw", "left"): {("kw", "file"): "a.py"}}]},
        )
        self.assertEqual(run.parse_edn("[]"), [])
        self.assertEqual(run.parse_edn("{}"), {})

    # AC-1: text that is not one EDN form is an error, and the message says
    # which rule it broke. A snapshot that the reader cannot read must never
    # compare equal to another snapshot.
    def test_ac_1_a_snapshot_that_is_not_one_edn_form_is_an_error(self):
        cases = {
            "{:a 1": "unterminated EDN collection",
            "{:a}": "EDN map has an odd number of forms",
            "}": "unexpected EDN delimiter }",
            "1 2": "trailing EDN forms",
            "a[": "trailing EDN forms",
            '"a': "bad EDN at offset 0",
        }
        for text, expected in cases.items():
            with self.assertRaises(ValueError) as caught:
                run.parse_edn(text)
            self.assertEqual(str(caught.exception), expected)


# AC-1: a case compares the two snapshots, so edn_diffs must tell a missing
# snapshot from a different one, and it must say that a snapshot it cannot read
# is not equal to the other. A quiet pass here would pass a changed snapshot.
class EdnDiffs(unittest.TestCase):
    # AC-1: the same values give no diff, even when the two files differ in
    # whitespace, because the runner compares the parsed forms.
    def test_ac_1_edn_diffs_is_empty_for_equal_values(self):
        self.assertEqual(run.edn_diffs(b"{:a 1}", b"{\n :a 1\n}"), [])

    # AC-1: a snapshot that only one side wrote is a diff that names the side,
    # and two missing snapshots are not a diff.
    def test_ac_1_edn_diffs_names_the_side_that_wrote_the_snapshot(self):
        self.assertEqual(run.edn_diffs(None, None), [])
        self.assertEqual(run.edn_diffs(None, b"{:a 1}"),
                         ["snapshot presence: original=False port=True"])
        self.assertEqual(run.edn_diffs(b"{:a 1}", None),
                         ["snapshot presence: original=True port=False"])

    # AC-1: two snapshots with different values are a diff that shows both
    # values, so the report names what changed.
    def test_ac_1_edn_diffs_shows_both_values_when_they_differ(self):
        diffs = run.edn_diffs(b"{:score 1.0}", b"{:score 0.9}")
        self.assertEqual(diffs[0], "dry.edn values differ")
        self.assertIn("1.0", diffs[1])
        self.assertIn("0.9", diffs[2])

    # AC-1: a snapshot that is not EDN is reported as unreadable, not as equal.
    def test_ac_1_a_snapshot_that_does_not_parse_is_not_equal(self):
        self.assertEqual(run.edn_diffs(b"{:a", b"{:a"),
                         ["dry.edn does not parse: unterminated EDN collection"])
        self.assertEqual(run.edn_diffs(b"\xff", b"{:a 1}"),
                         ["dry.edn does not parse: 'utf-8' codec can't decode byte 0xff in "
                          "position 0: invalid start byte"])


# AC-1: the setup that a case needs before it runs. A temp path with a walker
# directory is refused, the --changed setup stages and commits known files, and
# the original must sit at the pinned commit. Each test reads the result the way
# the runner does: the files on disk and the error text.
class Setup(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="setup-test-")
        self.addCleanup(tmp.cleanup)
        self.base = Path(tmp.name)

    def write_source(self, name, text="def f():\n    return 1\n"):
        path = self.base / "src" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    # AC-1: a temp path below test, tests, or spec would be skipped by the
    # walker, so the runner refuses it and names the parts.
    def test_ac_1_check_path_refuses_a_walker_directory(self):
        run.check_path(self.base / "proj")
        with self.assertRaises(SystemExit) as caught:
            run.check_path(self.base / "tests" / "proj")
        self.assertIn("has components ['tests']", str(caught.exception))

    # AC-1: the --changed setup commits the fixture, then leaves one file
    # modified, one file staged, and a new untracked copy, so --changed has a
    # known selection. The suffix picks the comment marker of the language.
    def test_ac_1_apply_git_setup_changed_leaves_a_modified_a_staged_and_a_new_file(self):
        one = self.write_source("a.py")
        two = self.write_source("b.py")
        run.apply_git_setup(self.base, "changed")
        self.assertIn("# dryer parity change\n", one.read_text(encoding="utf-8"))
        self.assertIn("# dryer parity change\n", two.read_text(encoding="utf-8"))
        self.assertTrue((self.base / "src" / "changed_new.py").is_file())
        status = run.subprocess.run(["git", "-C", str(self.base), "status", "--porcelain"],
                                    capture_output=True, text=True, check=True).stdout
        self.assertIn(" M src/a.py", status)
        self.assertIn("M  src/b.py", status)
        self.assertIn("?? src/changed_new.py", status)

    # AC-1: a setup that the runner does not know, and a fixture with fewer than
    # two source files, stop the case with an error, because --changed would
    # select nothing.
    def test_ac_1_apply_git_setup_refuses_an_unknown_mode_and_a_thin_fixture(self):
        with self.assertRaises(SystemExit) as caught:
            run.apply_git_setup(self.base, "renamed")
        self.assertEqual(str(caught.exception), "unknown git_setup 'renamed'")
        self.write_source("a.py")
        with self.assertRaises(SystemExit) as caught:
            run.apply_git_setup(self.base, "changed")
        self.assertIn("needs at least two source files", str(caught.exception))

    # AC-1: a fixture that does not exist stops the case, and a fixture that
    # exists is copied to a fresh directory with its setup applied.
    def test_ac_1_make_copy_copies_the_fixture_and_refuses_a_missing_one(self):
        fixture = self.base / "fixtures" / "python"
        (fixture / "src").mkdir(parents=True)
        (fixture / "src" / "a.py").write_text("def f():\n    return 1\n", encoding="utf-8")
        with mock.patch.object(run, "FIXTURES", self.base / "fixtures"):
            proj = run.make_copy("python", None, self.base / "copy")
            self.assertEqual((proj / "src" / "a.py").read_text(encoding="utf-8"),
                             "def f():\n    return 1\n")
            with self.assertRaises(SystemExit) as caught:
                run.make_copy("java", None, self.base / "other")
        self.assertIn("fixture not found", str(caught.exception))

    # AC-1: the original must be at the pinned commit. One that is elsewhere
    # stops the run with both commits, so a parity result cannot come from
    # another commit (DR-PB-1).
    def test_ac_1_ensure_original_refuses_another_commit(self):
        root = self.base / "original"
        (root / "src").mkdir(parents=True)
        (root / "src" / "a.py").write_text("def f():\n    return 1\n", encoding="utf-8")
        run.run_git(root, "init", "-q")
        run.run_git(root, "add", "-A")
        run.run_git(root, "commit", "-q", "-m", "base")
        head = run.subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                                  capture_output=True, text=True, check=True).stdout.strip()
        self.assertEqual(run.ensure_original(str(root), {"commit": head}), root.resolve())
        with self.assertRaises(SystemExit) as caught:
            run.ensure_original(str(root), {"commit": "0" * 40})
        self.assertEqual(str(caught.exception),
                         f"original at {root.resolve()} is {head}; pinned commit is " + "0" * 40)

    # AC-1: without a checkout on the command line, the runner clones into its
    # cache and checks out the pinned commit. A cache that already holds the
    # clone gets only the checkout, so a rerun does not clone again.
    def test_ac_1_ensure_original_clones_into_the_cache_once(self):
        cache = self.base / "cache"
        calls = []
        head = "a" * 40

        def fake_run(argv, **_kwargs):
            calls.append(argv)
            if "clone" in argv:
                (Path(argv[-1]) / ".git").mkdir(parents=True)
            return run.subprocess.CompletedProcess(argv, 0, stdout=head, stderr="")

        with mock.patch.object(run, "CACHE", cache), \
             mock.patch.object(run.subprocess, "run", side_effect=fake_run):
            root = run.ensure_original(None, {"repository": "https://example.invalid/dryer",
                                              "commit": head})
            self.assertEqual(root, cache / "dryer-original")
            self.assertTrue(any("clone" in call for call in calls))
            calls.clear()
            run.ensure_original(None, {"commit": head})
        self.assertFalse(any("clone" in call for call in calls))
        self.assertIn(["git", "-C", str(cache / "dryer-original"), "checkout", "-q", head],
                      calls)


# AC-1: `load_toml` is the reader of both parity tables, and it must read the
# plain table and refuse a value that it cannot read. A silent skip would hide a
# key, and a table that reads as the wrong shape would hide a case.
class Toml(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="toml-test-")
        self.addCleanup(tmp.cleanup)
        self.base = Path(tmp.name)

    def write(self, text):
        path = self.base / "table.toml"
        path.write_text(text, encoding="utf-8")
        return path

    # AC-1: a plain `[table]` and a repeated `[[table]]` both read, and a
    # comment and a blank line change nothing. An array may span lines.
    def test_ac_1_load_toml_reads_a_table_and_a_repeated_table(self):
        path = self.write(
            "# a comment\n"
            "[meta]\n"
            "name = \"dryer\"  # trailing\n"
            "\n"
            "[[case]]\n"
            "name = \"one\"\n"
            "args = [\n"
            "  \"--threshold\",\n"
            "  \"0.8\",\n"
            "]\n"
            "[[case]]\n"
            "name = \"two\"\n"
        )
        self.assertEqual(
            run.load_toml(path),
            {
                "meta": {"name": "dryer"},
                "case": [{"name": "one", "args": ["--threshold", "0.8"]},
                         {"name": "two"}],
            },
        )

    # AC-1: a line that is not a key and a value that is not a string or a list
    # stop the reader with the file and the text, so a broken table cannot pass
    # as an empty one.
    def test_ac_1_load_toml_refuses_a_bad_line_and_a_bad_value(self):
        path = self.write("name = \"ok\"\nthis line has no equals sign\n")
        with self.assertRaises(ValueError) as caught:
            run.load_toml(path)
        self.assertEqual(str(caught.exception),
                         f"{path}: bad line: this line has no equals sign")
        path = self.write("flag = true\n")
        with self.assertRaises(ValueError) as caught:
            run.load_toml(path)
        self.assertEqual(str(caught.exception), "unsupported TOML value: true")
