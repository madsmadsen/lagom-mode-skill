# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Record the ORIGINAL Python crapper's output for every run in coverage.toml.

For each run: copy the fixture into a fresh temporary directory, run its
setup commands there, run `python -m crapper <args>` with the original
interpreter, then write parity/recorded/<id>/{exit.txt, stdout.txt, crap.edn}.
Runs whose `requires` executables are missing are skipped and reported.

Usage: python3 parity/record_original.py [run-id ...]
Environment: CRAPPER_ORIGINAL_PYTHON overrides the original interpreter.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURES = HERE / "fixtures"
RECORDED = HERE / "recorded"
CONTRACT = HERE / "coverage.toml"
DEFAULT_PYTHON = "/tmp/crapper-work/venv/bin/python"


def load_runs() -> list[dict]:
    return tomllib.loads(CONTRACT.read_text(encoding="utf-8"))["run"]


def missing_tools(run: dict) -> list[str]:
    return [tool for tool in run.get("requires", []) if shutil.which(tool) is None]


def make_work_dir(run: dict, base: Path) -> Path:
    work = base / run["id"]
    shutil.copytree(FIXTURES / run["fixture"], work)
    for command in run.get("setup", []):
        subprocess.run(command, shell=True, cwd=work, check=True)
    return work


def run_env(run: dict, work: Path) -> dict[str, str]:
    """A `.stubs` directory in the fixture goes first on PATH. Each `env` entry of the run
    (`NAME=VALUE`, `{project}` is the work directory) is set."""
    env = dict(os.environ)
    stubs = work / ".stubs"
    if stubs.is_dir():
        env["PATH"] = f"{stubs}{os.pathsep}{env.get('PATH', '')}"
    for entry in run.get("env", []):
        name, _, value = entry.partition("=")
        env[name] = value.replace("{project}", str(work))
    return env


def run_original(python: str, run: dict, work: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [python, "-m", "crapper", *run["args"]],
        cwd=work,
        capture_output=True,
        text=True,
        check=False,
        env=run_env(run, work),
    )


def metrics_root(work: Path, args: list[str]) -> Path:
    """The original writes .metrics/crap.edn under --root (default: cwd)."""
    if "--root" in args:
        index = args.index("--root")
        if index + 1 < len(args):
            return work / args[index + 1]
    return work


def record(run: dict, python: str, base: Path) -> int:
    work = make_work_dir(run, base)
    result = run_original(python, run, work)
    out = RECORDED / run["id"]
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    (out / "exit.txt").write_text(f"{result.returncode}\n", encoding="utf-8")
    (out / "stdout.txt").write_text(result.stdout, encoding="utf-8")
    edn = metrics_root(work, run["args"]) / ".metrics" / "crap.edn"
    if edn.is_file():
        shutil.copyfile(edn, out / "crap.edn")
    return result.returncode


def selected(runs: list[dict], ids: list[str]) -> list[dict]:
    if not ids:
        return runs
    return [run for run in runs if run["id"] in ids]


def main(argv: list[str]) -> int:
    python = os.environ.get("CRAPPER_ORIGINAL_PYTHON", DEFAULT_PYTHON)
    RECORDED.mkdir(exist_ok=True)
    skipped: list[str] = []
    with tempfile.TemporaryDirectory(prefix="crapper-parity-") as tmp:
        for run in selected(load_runs(), argv):
            missing = missing_tools(run)
            if missing:
                skipped.append(f"{run['id']} (missing: {', '.join(missing)})")
                continue
            code = record(run, python, Path(tmp))
            flag = "" if code == run.get("expect_exit", code) else " UNEXPECTED"
            print(f"recorded {run['id']} exit={code}{flag}")
    for line in skipped:
        print(f"SKIPPED {line}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
