"""Tests for parity/run.py normalize() (CR-PB-3: output differences from the original).

Stdlib only. Run from crapper/: python3.12 -m unittest parity/test_normalize.py
"""

from __future__ import annotations

import importlib.util
import sys
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
MISSING = Path("/nonexistent")


class NormalizeGoTestTime(unittest.TestCase):
    def test_go_test_elapsed_time_becomes_placeholder(self):
        # CR-PB-3: go test prints wall-clock time, which is runner noise, not tool behaviour.
        stdout = b"ok  \texample.com/x/calc\t0.261s\tcoverage: 22.2% of statements\n"
        expected = b"ok  \texample.com/x/calc\t<TIME>s\tcoverage: 22.2% of statements\n"
        self.assertEqual(run.normalize(stdout, MISSING), expected)

    def test_time_without_surrounding_tabs_is_unchanged(self):
        # CR-PB-3: only the tab-delimited go test elapsed field is replaced.
        stdout = b"Sum  2  0.0%  6.0\nThreshold 0.5s\n"
        self.assertEqual(run.normalize(stdout, MISSING), stdout)

    def test_project_path_is_still_replaced_with_root(self):
        # CR-PB-3: the project-path replacement keeps working alongside the time placeholder.
        stdout = b"/nonexistent/calc.go\t0.264s\tok\n"
        expected = b"<ROOT>/calc.go\t<TIME>s\tok\n"
        self.assertEqual(run.normalize(stdout, MISSING), expected)


if __name__ == "__main__":
    unittest.main()
