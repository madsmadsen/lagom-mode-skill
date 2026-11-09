#!/usr/bin/env python3
"""Write tests/golden/<lang>/ from the ORIGINAL mutator on each parity fixture (AC-11).

Run: /tmp/mu/venv/bin/python mutator/parity/golden.py
Each golden folder holds: scan.out, run.out, run.exit, and snapshots/<path>.edn (tested-at removed).
The Rust test tests/fixtures_langs.rs replays the port on the same fixtures and compares.
"""

import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

PARITY = Path(__file__).resolve().parent
FIXTURES = PARITY / "fixtures"
GOLDEN = PARITY.parent / "tests" / "golden"
ORIGINAL = [os.environ.get("ORIGINAL_PYTHON", "/tmp/mu/venv/bin/python"), "-m", "mutator"]
GIT_INIT = "git init -q && git add -A && git -c user.name=t -c user.email=t@t commit -qm init"
TESTED_AT = re.compile(r':tested-at "[^"]*" ?')


def run(args, cwd):
    done = subprocess.run(ORIGINAL + args, cwd=cwd, capture_output=True, text=True)
    root = str(Path(cwd).resolve())
    return done.returncode, done.stdout.replace(root, "<ROOT>")


def main():
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
