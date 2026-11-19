#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Tests for run_side in parity/run_parity.py (DR-NFR-2, AC-1).

DR-NFR-2: the runner reports the wall-clock time of one program. run_side runs
one side 5 times. Run 1 gives the result that the runner compares, and runs 2
to 5 only measure time. A snapshot from an earlier run must not pass the check
of a later run, so run_side removes .metrics before each timing run.

The fake program counts its runs, prints the count, and writes the count into
.metrics/dry.edn. It also records whether a snapshot was already on disk when
it started. Both tests fail on the older code, which kept the last run.

Stdlib only. Run from tools/dryer/ with Python 3.9 or later:

    python3 -m unittest parity/test_run_parity.py

crapper finds this test with `python3 -m unittest discover -s .` from the
project root, which needs parity/__init__.py.
"""

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path

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
        return run.run_side(self.prefix, self.root, [], {"PATH": "/usr/bin:/bin"})

    def lines(self, name):
        return (self.root / name).read_text(encoding="utf-8").split()

    # DR-NFR-2: the runner compares the result of run 1, not the result of run 5.
    def test_dr_nfr_2_the_compared_result_comes_from_run_1(self):
        res = self.run_side()
        self.assertEqual(res.code, 0)
        self.assertEqual(res.stdout, "run 1\n")
        self.assertEqual(res.edn, b"1\n")

    # DR-NFR-2: the side still runs 5 times, so the time is a median of 5 samples.
    def test_dr_nfr_2_the_side_runs_five_times_for_the_median(self):
        res = self.run_side()
        self.assertEqual(self.lines("count"), ["5"])
        self.assertGreaterEqual(res.seconds, 0.0)

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
