#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Parity runner: the original mutator (Python) against the Rust port.

Run from the crate directory, with the Python that installs the original at the
pinned commit:

    ORIGINAL_PYTHON=<venv>/bin/python MUTATOR_BIN=target/release/mutator \\
        <venv>/bin/python parity/run.py

`ORIGINAL_PYTHON` is required and has no default. Run run.py with that same
interpreter, because the EDN reader of the original comes from the interpreter
that runs this file. `parity/README.md` gives the venv setup.

Each [[run]] in coverage.toml runs twice, in two copies of its fixture: once
with the original (`python -m mutator` from the venv) and once with the port
(MUTATOR_BIN, default tools/mutator/target/debug/mutator). Both sides must agree on:

- the exit code;
- every file under .metrics/ after the original's EDN reader parses the .edn
  files and the top-level `tested-at` key is dropped;
- the `git status --porcelain` lines of the fixture tree (AC-5: sources untouched);
- stdout, after each side's temp root is replaced with `<ROOT>`;

stderr is normalized the same way (root replaced, `Wrote <path>` lines replaced
with `Wrote <PATH>`) and its differences are reported as information only.

Options:
  --original-only   run only the original side (the port need not be built)
  --only NAME       run only the named entry (repeatable)
  --show            print the unified diff of normalized stdout and stderr
                    for every run where they differ

Exit 1 when any run FAILs. A FAIL whose run names, with `fix = "<id>"` or a
list of ids, only entries of fixes.toml that Mads Madsen approved prints
DIFF-ALLOWED and does not set exit 1 (AC-1, MU-PB-6). An approval excuses a
stdout difference and a snapshot difference (`metrics ...`) of its run, and
nothing else.

Before the first run, the runner reads the commits that upstream.toml pins
(MU-PB-1, MU-PB-2): the original, and the crapper checkout that the original
imports. It reads each commit from the clean checkout that the venv installs
the package from (`git rev-parse --show-toplevel`, `git status --porcelain`, and
`git rev-parse HEAD` in the package directory), else from the commit that pip
records in the `direct_url.json` of a site-packages install. A dirty checkout,
a checkout that does not hold the package, and a commit that it cannot read all
stop the runner before any run with a message that names the pin. Nothing is
cloned.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

PARITY = Path(__file__).resolve().parent
COVERAGE_TOML = PARITY / "coverage.toml"
FIXES_TOML = PARITY / "fixes.toml"
UPSTREAM_TOML = PARITY / "upstream.toml"
ORIGINAL_PACKAGE = "mutator"
PORT_CMD = [os.environ.get("MUTATOR_BIN", str(PARITY.parent / "target" / "debug" / "mutator"))]
WORKER_CAP = ["--max-workers", "2"]
# Runs with these flags keep their own worker setting, or print help, or only scan.
CAP_EXEMPT = {"-h", "--help", "--scan", "--max-workers"}
GIT_INIT = "git init -q && git add -A && git -c user.name=t -c user.email=t@t commit -qm init"
WROTE_LINE = re.compile(r"^Wrote .*$", re.MULTILINE)
# The problems an approved fix excuses: a stdout difference, and a snapshot
# difference that the fix changes on purpose (AC-1, MU-PB-6). `status_for`
# matches on these two prefixes only. An exit code difference, a crash, and a
# git status difference stay a FAIL.
STDOUT_PREFIX = "stdout differs in "
METRICS_PREFIX = "metrics "
EXCUSED_PREFIXES = (STDOUT_PREFIX, METRICS_PREFIX)
TIMEOUT = 900


@dataclass
class Outcome:
    tree: Path
    code: int
    stdout: str
    stderr: str
    metrics: dict = field(default_factory=dict)
    status: list[str] = field(default_factory=list)


def original_python() -> str:
    """The interpreter of the venv that installs the original. There is no default, because
    a path that the host does not have only moves the error to the first run."""
    path = os.environ.get("ORIGINAL_PYTHON", "")
    if not path:
        sys.exit("run.py: set ORIGINAL_PYTHON to the Python of the venv that installs the "
                 "original at the pinned commit (parity/README.md).")
    return path


def original_cmd() -> list[str]:
    """The command that runs the original, with the interpreter of ORIGINAL_PYTHON."""
    return [original_python(), "-m", "mutator"]


def same_interpreter(python: str, run=subprocess.run) -> bool:
    """Whether `python` is the interpreter that runs this file. The EDN reader comes from
    this interpreter, so the two must be the same Python (MU-PB-1)."""
    there = quiet([python, "-c", "import sys; print(sys.executable)"], run)
    return bool(there) and Path(there).resolve() == Path(sys.executable).resolve()


def edn_reader(python: str | None = None, run=subprocess.run):
    """The EDN reader of the original's package. It is imported from the interpreter that
    runs this file, so `python` must name that same interpreter."""
    if python and not same_interpreter(python, run):
        sys.exit(f"run.py: this file runs on {sys.executable}, but ORIGINAL_PYTHON is "
                 f"{python}. The EDN reader comes from the interpreter that runs this "
                 "file, so run run.py with the interpreter of the original.")
    try:
        from mutator.edn import loads
    except ImportError as exc:
        sys.exit(f"run.py: cannot import the original's mutator.edn ({exc}); "
                 "run run.py with the ORIGINAL_PYTHON interpreter")
    return loads


def approved_fix_ids(path: Path = FIXES_TOML) -> set[str]:
    """Ids of the fixes.toml entries that Mads Madsen approved (MU-PB-6)."""
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    return {fix["id"] for fix in data.get("fix", [])
            if fix.get("approved") is True and "id" in fix}


def pinned_commit(path: Path = UPSTREAM_TOML, section: str = "original") -> str:
    """The commit that upstream.toml pins for `section` (MU-PB-1). A pin that is not a
    full commit stops the runner, because it cannot name the source."""
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    commit = data.get(section, {}).get("commit", "")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise RuntimeError(f"run.py: {path.name} pins no full commit for the {section}: "
                           f"{commit!r}")
    return commit


# The venv answers three questions: where its `mutator` package lives, where the crapper
# package that the original imports lives, and which commit pip recorded for it in
# direct_url.json.
PACKAGE_PATH = "import mutator; print(getattr(mutator, '__file__', '') or '')"
CRAPPER_PATH = ("import mutator.crapper_link as link; "
                "print(getattr(link.ensure_crapper(), '__file__', '') or '')")
PIP_COMMIT = ("import importlib.metadata as m; "
              "print(m.distribution('{name}').read_text('direct_url.json'))")
FULL_COMMIT = re.compile(r"[0-9a-f]{40}")


def quiet(cmd: list[str], run) -> str:
    """The stripped stdout of `cmd`, or "" when it fails to start or exits nonzero."""
    try:
        done = run(cmd, capture_output=True, text=True, encoding="utf-8",
                   errors="replace", timeout=TIMEOUT, check=False)
    except OSError:
        return ""
    return done.stdout.strip() if done.returncode == 0 else ""


def package_location(python: str, package: str, run) -> str:
    """The `__file__` of an installed package, or "" when the interpreter cannot import it.
    The crapper question goes through the original's own import, which is the file that the
    original runs."""
    expr = PACKAGE_PATH if package == ORIGINAL_PACKAGE else CRAPPER_PATH
    return quiet([python, "-c", expr], run)


def checkout_commit(path: str, run) -> str:
    """The HEAD of the git checkout that holds the package at `path`. "" when the directory
    is in no checkout, when the checkout is not clean, when the checkout does not hold the
    package, or when HEAD is not a full commit. A dirty tree is not the pin, and a checkout
    that does not hold the package belongs to some other project (MU-PB-1)."""
    directory = Path(path).parent
    toplevel = quiet(["git", "-C", str(directory), "rev-parse", "--show-toplevel"], run)
    if not toplevel or quiet(["git", "-C", toplevel, "status", "--porcelain"], run):
        return ""
    try:
        directory.resolve().relative_to(Path(toplevel).resolve())
    except ValueError:
        return ""
    commit = quiet(["git", "-C", str(directory), "rev-parse", "HEAD"], run)
    return commit if FULL_COMMIT.fullmatch(commit) else ""


def pip_commit(python: str, package: str, path: str, run) -> str:
    """The commit that pip records in direct_url.json, for an install into site-packages.
    A copy elsewhere is not a pip install, so its metadata is not trusted (MU-PB-1)."""
    if "site-packages" not in path:
        return ""
    try:
        found = str(json.loads(quiet([python, "-c", PIP_COMMIT.format(name=package)], run))
                    ["vcs_info"]["commit_id"])
    except (ValueError, KeyError, TypeError):
        return ""
    return found if FULL_COMMIT.fullmatch(found) else ""


def package_commit(python: str, package: str, run=subprocess.run) -> str:
    """The commit of an installed package: the clean checkout that holds it, else the commit
    that pip records for a site-packages install. "" when neither gives a commit."""
    path = package_location(python, package, run)
    if not path:
        return ""
    return checkout_commit(path, run) or pip_commit(python, package, path, run)


def original_commit(python: str, run=subprocess.run) -> str:
    """The commit of the original in the venv: the clean checkout that it installs from,
    else the commit that pip records in direct_url.json. "" when neither gives a commit."""
    return package_commit(python, ORIGINAL_PACKAGE, run)


def crapper_commit(python: str, run=subprocess.run) -> str:
    """The commit of the crapper checkout that the original imports."""
    return package_commit(python, "crapper", run)


def pin_problem(commit: str, pin: str, what: str, python: str) -> str:
    """The stop message for a package that is not at its pin, or "" when it is."""
    if commit == pin:
        return ""
    if not commit:
        return (f"run.py: cannot read the commit of {what} in {python}. upstream.toml pins "
                f"{pin}. Point ORIGINAL_PYTHON at a clean checkout, or a site-packages "
                "install, of the pinned commit.")
    return (f"run.py: {what} in {python} is at {commit}, but upstream.toml pins {pin}. "
            "Install the pinned commit, or change the pin on purpose (MU-PB-1, MU-PB-2).")


def check_pin(python: str, path: Path = UPSTREAM_TOML, run=subprocess.run) -> str:
    """MU-PB-2: the original, and the crapper checkout that it imports, must be the commits
    that upstream.toml pins. A commit that cannot be read, a dirty checkout, and another
    commit stop the runner before any run."""
    pin = pinned_commit(path, "original")
    for what, package, wanted in (("the original", ORIGINAL_PACKAGE, pin),
                                  ("the crapper checkout that the original imports",
                                   "crapper", pinned_commit(path, "crapper"))):
        problem = pin_problem(package_commit(python, package, run), wanted, what, python)
        if problem:
            raise RuntimeError(problem)
    return pin


def named_fixes(entry: dict) -> list[str]:
    """The fix ids that a run names: `fix = "<id>"` or `fix = ["<id>", ...]`."""
    listed = entry.get("fix", [])
    return [listed] if isinstance(listed, str) else list(listed)


def status_for(problems: list[str], entry: dict, approved: set[str]) -> str:
    """AC-1, MU-PB-6: an approved fix excuses the stdout difference and the snapshot
    difference of its run, and only those. An exit code mismatch, a git status
    difference, and an error stay a FAIL."""
    if not problems:
        return "PASS"
    fixes = named_fixes(entry)
    if not fixes or any(fix not in approved for fix in fixes):
        return "FAIL"
    if all(problem.startswith(EXCUSED_PREFIXES) for problem in problems):
        return "DIFF-ALLOWED"
    return "FAIL"


def worker_capped(args: list[str], expect_exit: int | None) -> list[str]:
    """Mutating runs get `--max-workers 2`. Help, scan, an explicit worker
    count, and usage errors (expect_exit 1) are passed through unchanged."""
    if expect_exit == 1 or CAP_EXEMPT & set(args):
        return list(args)
    return [*args, *WORKER_CAP]


def run_tool(cmd: list[str], args: list[str], tree: Path) -> subprocess.CompletedProcess:
    return subprocess.run([*cmd, *args], cwd=tree, capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=TIMEOUT, check=False)


def run_shell(step: str, tree: Path) -> None:
    done = subprocess.run(step, shell=True, cwd=tree, capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=TIMEOUT, check=False)
    if done.returncode != 0:
        raise RuntimeError(f"setup step failed ({done.returncode}): {step}: {done.stderr.strip()}")


def prepare_tree(parent: Path, name: str, fixture: Path) -> Path:
    tree = parent / name
    shutil.copytree(fixture, tree)
    subprocess.run(["sh", "-c", GIT_INIT], cwd=tree, check=True, capture_output=True)
    return tree


def normalize(text: str, tree: Path) -> str:
    for form in {str(tree), str(tree.resolve())}:
        text = text.replace(form, "<ROOT>")
    return WROTE_LINE.sub("Wrote <PATH>", text)


def read_metrics(tree: Path, loads) -> dict[str, object]:
    base = tree / ".metrics"
    found: dict[str, object] = {}
    if not base.is_dir():
        return found
    for path in sorted(p for p in base.rglob("*") if p.is_file()):
        key = path.relative_to(tree).as_posix()
        text = normalize(path.read_text(encoding="utf-8", errors="replace"), tree)
        value: object = text
        if path.suffix == ".edn":
            try:
                value = loads(text)
            except Exception as exc:  # the reader's own error type; a parse failure is a difference
                value = f"UNPARSEABLE: {exc}"
            if isinstance(value, dict):
                value = {k: v for k, v in value.items() if k != "tested-at"}
        found[key] = value
    return found


def git_status(tree: Path) -> list[str]:
    done = subprocess.run(["git", "status", "--porcelain"], cwd=tree, capture_output=True,
                          text=True, encoding="utf-8", errors="replace", check=True)
    return sorted(done.stdout.splitlines())


def apply_setup(entry: dict, cmd: list[str], tree: Path) -> None:
    for step in entry.get("setup", []):
        if isinstance(step, str):
            run_shell(step, tree)
        else:
            run_tool(cmd, worker_capped(step["mutator"], None), tree)


def run_side(name: str, cmd: list[str], entry: dict, fixture: Path, parent: Path, loads) -> Outcome:
    tree = prepare_tree(parent, name, fixture)
    apply_setup(entry, cmd, tree)
    done = run_tool(cmd, worker_capped(entry.get("args", []), entry.get("expect_exit")), tree)
    return Outcome(tree, done.returncode, done.stdout, done.stderr,
                   read_metrics(tree, loads), git_status(tree))


def metrics_problems(orig: Outcome, port: Outcome) -> list[str]:
    if set(orig.metrics) != set(port.metrics):
        return [f"metrics files differ (original {sorted(orig.metrics)}, "
                f"port {sorted(port.metrics)})"]
    return [f"metrics {key} differs" for key in sorted(orig.metrics)
            if orig.metrics[key] != port.metrics[key]]


def stream_diff(orig: Outcome, port: Outcome, attr: str) -> list[str]:
    before = normalize(getattr(orig, attr), orig.tree).splitlines()
    after = normalize(getattr(port, attr), port.tree).splitlines()
    return list(difflib.unified_diff(before, after, f"original {attr}", f"port {attr}", lineterm=""))


def changed_count(diff: list[str]) -> int:
    """Number of changed lines in a unified diff, skipping the two header lines."""
    return sum(1 for line in diff[2:] if line[:1] in ("+", "-"))


def problems_for(entry: dict, sides: dict[str, Outcome]) -> list[str]:
    found: list[str] = []
    expect = entry.get("expect_exit")
    for side, out in sides.items():
        if expect is not None and out.code != expect:
            found.append(f"{side} exit {out.code}, expected {expect}")
    if "port" not in sides:
        return found
    orig, port = sides["original"], sides["port"]
    if orig.code != port.code:
        found.append(f"exit {orig.code} (original) vs {port.code} (port)")
    found += metrics_problems(orig, port)
    if orig.status != port.status:
        found.append(f"git status differs (original {orig.status}, port {port.status})")
    stdout = changed_count(stream_diff(orig, port, "stdout"))
    if stdout:
        found.append(f"{STDOUT_PREFIX}{stdout} lines")
    return found


def describe(sides: dict[str, Outcome]) -> str:
    codes = " ".join(f"{side}={out.code}" for side, out in sides.items())
    text = f"exit {codes}  metrics={len(sides['original'].metrics)}"
    if "port" in sides:
        orig, port = sides["original"], sides["port"]
        text += (f"  info: stdout-diff={changed_count(stream_diff(orig, port, 'stdout'))}"
                 f" stderr-diff={changed_count(stream_diff(orig, port, 'stderr'))}")
    return text


def show_details(sides: dict[str, Outcome]) -> list[str]:
    if "port" not in sides:
        return []
    rows: list[str] = []
    for attr in ("stdout", "stderr"):
        rows += stream_diff(sides["original"], sides["port"], attr)
    return rows


def run_entry(entry: dict, cmd: list[str], loads, approved: set[str], original_only: bool,
              show: bool) -> tuple[str, list[str]]:
    name = entry["name"]
    fixture = PARITY / entry["fixture"]
    sides: dict[str, Outcome] = {}
    details: list[str] = []
    try:
        with tempfile.TemporaryDirectory(prefix=f"parity-{name}-") as tmp:
            parent = Path(tmp)
            sides["original"] = run_side("original", cmd, entry, fixture, parent, loads)
            if not original_only:
                sides["port"] = run_side("port", PORT_CMD, entry, fixture, parent, loads)
            problems = problems_for(entry, sides)
            summary = describe(sides)
            if show:
                details = show_details(sides)
    except Exception as exc:  # a setup or tool failure is a FAIL of this run only
        problems = [f"error: {exc}"]
        summary = ""
    status = status_for(problems, entry, approved)
    tail = f"  | {'; '.join(problems)}" if problems else ""
    return f"{status:<12} {name:<36} {summary}{tail}", details


def entry_problems(opts, entries: list[dict]) -> str:
    """The message for an `--only` name that coverage.toml does not hold, or ""."""
    known = {entry["name"] for entry in entries}
    missing = sorted(set(opts.only) - known)
    return f"run.py: no run named {', '.join(missing)} in coverage.toml" if missing else ""


def run_all(entries: list[dict], cmd: list[str], loads, approved: set[str],
            original_only: bool, show: bool) -> int:
    """Runs each entry and returns 1 when any run FAILs."""
    failed = 0
    for entry in entries:
        line, details = run_entry(entry, cmd, loads, approved, original_only, show)
        print(line, flush=True)
        for row in details:
            print(row)
        failed += line.startswith("FAIL")
    return 1 if failed else 0


def parser() -> argparse.ArgumentParser:
    found = argparse.ArgumentParser(description="Compare the original mutator with the port.")
    found.add_argument("--original-only", action="store_true",
                       help="run only the original side")
    found.add_argument("--only", action="append", default=[], metavar="NAME",
                       help="run only this entry (repeatable)")
    found.add_argument("--show", action="store_true",
                       help="print unified diffs of normalized stdout and stderr where they differ")
    return found


def main(argv: list[str] | None = None, run=subprocess.run) -> int:
    """Runs every selected entry after the pin check. The pin check comes first, so a venv
    that is not the pin stops the runner before any tool runs (MU-PB-1, MU-PB-2)."""
    opts = parser().parse_args(argv)
    python = original_python()
    try:
        check_pin(python, run=run)
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    loads = edn_reader(python, run)
    approved = approved_fix_ids()
    entries = tomllib.loads(COVERAGE_TOML.read_text(encoding="utf-8")).get("run", [])
    message = entry_problems(opts, entries)
    if message:
        print(message, file=sys.stderr)
        return 1
    if opts.only:
        entries = [entry for entry in entries if entry["name"] in opts.only]
    return run_all(entries, original_cmd(), loads, approved, opts.original_only, opts.show)


if __name__ == "__main__":
    sys.exit(main())
