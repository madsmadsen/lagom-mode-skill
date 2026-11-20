#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Write tests/golden/<lang>/ from the ORIGINAL mutator on each parity fixture (AC-11).

Run, with the venv of the original (see parity/README.md):

    ORIGINAL_PYTHON=<venv>/bin/python <venv>/bin/python parity/golden.py

`ORIGINAL_PYTHON` is required and has no default. Each golden folder holds: scan.out,
run.out, run.exit, and snapshots/<path>.edn (tested-at removed).
The Rust test tests/fixtures_langs.rs replays the port on the same fixtures and compares.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

PARITY = Path(__file__).resolve().parent
FIXTURES = PARITY / "fixtures"
GOLDEN = PARITY.parent / "tests" / "golden"
ORIGINAL: list[str] = ["", "-m", "mutator"]
GIT_INIT = "git init -q && git add -A && git -c user.name=t -c user.email=t@t commit -qm init"
TESTED_AT = re.compile(r':tested-at "[^"]*" ?')


def original_cmd() -> list[str]:
    """The command of the original, from the `original_python` check of run.py, so that the
    two scripts ask for `ORIGINAL_PYTHON` in the same words."""
    sys.path.insert(0, str(PARITY.parent))
    from parity.run import original_cmd as command

    return command()


def run(args, cwd):
    done = subprocess.run(ORIGINAL + args, cwd=cwd, capture_output=True, text=True)
    root = str(Path(cwd).resolve())
    return done.returncode, done.stdout.replace(root, "<ROOT>")


def main():
    ORIGINAL[:] = original_cmd()
    for fixture in sorted(p for p in FIXTURES.iterdir() if p.is_dir()):
        out = GOLDEN / fixture.name
        shutil.rmtree(out, ignore_errors=True)
        (out / "snapshots").mkdir(parents=True)
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / fixture.name
            shutil.copytree(fixture, work)
            subprocess.run(GIT_INIT, shell=True, cwd=work, check=True, capture_output=True)
            _, scan = run(["--scan", "--no-coverage"], work)
            (out / "scan.out").write_text(scan)
            code, text = run(
                ["--no-coverage", "--test-command", "sh check.sh", "--max-workers", "2"], work)
            (out / "run.out").write_text(text)
            (out / "run.exit").write_text(f"{code}\n")
            base = work / ".metrics" / "mutate"
            for edn in sorted(base.rglob("*.edn")):
                target = out / "snapshots" / edn.relative_to(base)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(TESTED_AT.sub("", edn.read_text()))
        print("golden", fixture.name)


if __name__ == "__main__":
    main()
