#!/usr/bin/env python3
# Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
# SPDX-License-Identifier: MIT
"""Parity runner: the Rust port of dryer against the pinned Python original.

Usage:
  run_parity.py [--case NAME]... [--original DIR] [--port BIN]
  run_parity.py --record [--case NAME]... [--original DIR]

Default mode runs the original and the port on each case and compares exit
code, normalized stdout, and .metrics/dry.edn. Each side runs 5 times and the
runner reports the median wall-clock time (DR-NFR-2, AC-10). --check-timing
also fails a case whose port median is not lower than its original median, and
--timing-runs N sets the count of runs per side (default 5, the count that
DR-NFR-2 names). --check-timing needs at least 3 runs per side, because a median
of 2 samples has no middle sample, and it rejects a smaller count. Run
--check-timing in the named runner: the Ubuntu 22.04 x86_64 guest of
ci/linux-vm, run in qemu with TCG. That guest is emulated, so it measures the
relation of the two medians and not absolute times. A timing on any other host
does not prove AC-10. --record writes
parity/expected/<case>/ (exit_code, stdout, dry.edn if written). It runs every
case before it deletes the old parity/expected/, so a failed case keeps the old
recordings.

A case with a `fix` in coverage.toml differs from the original on purpose. For
that case the runner compares the port with parity/expected/<case>/port/, which
--record writes from the port, and still checks that the two programs differ.

Each case copies fixtures/<fixture>/ into a fresh temp directory for each
program, applies git_setup if set, and runs `<program> --root <copy> ARGS`
with cwd set to the copy. Run 1 gives the compared result. Runs 2 to N reuse
that copy only to measure time, and the runner removes .metrics before each of
them, so a stale snapshot can never pass a check. A timing run that exits
unlike run 1 stops the runner (DR-NFR-2), and the error names the case, the
side, and the stderr of that run. Copy paths in stdout are replaced with <ROOT>.

Stdlib only; Python >= 3.9. Set DRYER_ORIGINAL_PYTHON to choose the interpreter
that has the original's dependencies (for example the venv python).
"""

import argparse
import difflib
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from collections import namedtuple
from pathlib import Path

PARITY = Path(__file__).resolve().parent
CRATE = PARITY.parent
FIXTURES = PARITY / "fixtures"
EXPECTED = PARITY / "expected"
CACHE = PARITY / ".cache"
FORBIDDEN_PARTS = {"test", "tests", "spec"}
SOURCE_EXTS = {".clj", ".cljc", ".cljs", ".java", ".go", ".ts", ".tsx", ".rs", ".py"}
COMMENT_PREFIX = {".clj": ";", ".cljc": ";", ".cljs": ";", ".py": "#"}
GIT = ["git", "-c", "user.email=parity@example.invalid", "-c", "user.name=parity"]
MAX_DIFF_LINES = 40

Result = namedtuple("Result", "code stdout edn seconds stderr")


# ---------------------------------------------------------------- mini TOML
_TOML_COMMENT = re.compile(r'("(?:[^"\\]|\\.)*")|#.*$')
_TOML_STRING = re.compile(r'"(?:[^"\\]|\\.)*"')


def _strip_comment(line):
    return _TOML_COMMENT.sub(lambda m: m.group(1) or "", line).strip()


def _toml_value(text):
    if text.startswith('"'):
        return json.loads(text)
    if text.startswith("["):
        return [json.loads(s) for s in _TOML_STRING.findall(text)]
    raise ValueError(f"unsupported TOML value: {text}")


def load_toml(path):
    data = {}
    table = data
    lines = [_strip_comment(x) for x in Path(path).read_text(encoding="utf-8").splitlines()]
    i = 0
    while i < len(lines):
        line = lines[i]
        i += 1
        if not line:
            continue
        if line.startswith("[["):
            name = line[2 : line.index("]]")].strip()
            table = {}
            data.setdefault(name, []).append(table)
        elif line.startswith("["):
            name = line[1 : line.index("]")].strip()
            table = data.setdefault(name, {})
        else:
            key, sep, value = line.partition("=")
            if not sep:
                raise ValueError(f"{path}: bad line: {line}")
            value = value.strip()
            while value.startswith("[") and not value.endswith("]"):
                value += " " + lines[i]
                i += 1
            table[key.strip()] = _toml_value(value)
    return data


# ---------------------------------------------------------------- mini EDN
_EDN_TOKEN = re.compile(
    r'[\s,]+|(?P<br>[\[\]{}])|(?P<str>"(?:[^"\\]|\\.)*")|(?P<atom>[^\s,\[\]{}"]+)'
)
_EDN_INT = re.compile(r"[+-]?\d+\Z")
_EDN_FLOAT = re.compile(r"[+-]?\d+(\.\d*)?([eE][+-]?\d+)?\Z")


def _edn_tokens(text):
    out, pos = [], 0
    while pos < len(text):
        m = _EDN_TOKEN.match(text, pos)
        if m is None:
            raise ValueError(f"bad EDN at offset {pos}")
        pos = m.end()
        if m.lastgroup:
            out.append((m.lastgroup, m.group(m.lastgroup)))
    return out


def _edn_atom(text):
    if text.startswith(":"):
        return ("kw", text[1:])
    if text == "nil":
        return None
    if text in ("true", "false"):
        return text == "true"
    if _EDN_INT.match(text):
        return int(text)
    if _EDN_FLOAT.match(text):
        return float(text)
    return ("sym", text)


def _edn_seq(tokens, pos, closer):
    items = []
    while True:
        if pos >= len(tokens):
            raise ValueError("unterminated EDN collection")
        kind, text = tokens[pos]
        if kind == "br" and text == closer:
            return items, pos + 1
        value, pos = _edn_value(tokens, pos)
        items.append(value)


def _edn_value(tokens, pos):
    kind, text = tokens[pos]
    if kind == "str":
        return json.loads(text), pos + 1
    if kind == "br":
        if text == "[":
            return _edn_seq(tokens, pos + 1, "]")
        if text == "{":
            items, nxt = _edn_seq(tokens, pos + 1, "}")
            if len(items) % 2:
                raise ValueError("EDN map has an odd number of forms")
            return dict(zip(items[::2], items[1::2])), nxt
        raise ValueError(f"unexpected EDN delimiter {text}")
    return _edn_atom(text), pos + 1


def parse_edn(text):
    tokens = _edn_tokens(text)
    value, pos = _edn_value(tokens, 0)
    if pos != len(tokens):
        raise ValueError("trailing EDN forms")
    return value


# ---------------------------------------------------------------- setup
def check_path(path):
    bad = FORBIDDEN_PARTS.intersection(Path(path).parts)
    if bad:
        raise SystemExit(
            f"temp path {path} has components {sorted(bad)}; the walker would skip it. "
            "Point TMPDIR at a path without test/tests/spec components."
        )


def run_git(cwd, *args):
    subprocess.run(GIT + list(args), cwd=cwd, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def append_comment(path):
    prefix = COMMENT_PREFIX.get(path.suffix, "//")
    with path.open("a", encoding="utf-8") as fh:
        fh.write(f"{prefix} dryer parity change\n")


def apply_git_setup(root, mode):
    if mode != "changed":
        raise SystemExit(f"unknown git_setup {mode!r}")
    run_git(root, "init", "-q")
    run_git(root, "add", "-A")
    run_git(root, "commit", "-q", "-m", "parity base")
    files = sorted(
        p for p in (root / "src").rglob("*") if p.is_file() and p.suffix in SOURCE_EXTS
    )
    if len(files) < 2:
        raise SystemExit(f"changed setup needs at least two source files under {root}/src")
    unstaged, staged = files[0], files[1]
    append_comment(unstaged)
    append_comment(staged)
    run_git(root, "add", staged.relative_to(root).as_posix())
    shutil.copyfile(unstaged, unstaged.with_name("changed_new" + unstaged.suffix))


def make_copy(fixture, git_setup, base):
    proj = base / "proj"
    check_path(proj)
    src = FIXTURES / fixture
    if not src.is_dir():
        raise SystemExit(f"fixture not found: {src}")
    shutil.copytree(src, proj)
    if git_setup:
        apply_git_setup(proj, git_setup)
    return proj


def ensure_original(original_arg, pinned):
    if original_arg:
        root = Path(original_arg).resolve()
    else:
        root = CACHE / "dryer-original"
        if not (root / ".git").exists():
            CACHE.mkdir(exist_ok=True)
            subprocess.run(["git", "clone", "-q", pinned["repository"], str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "checkout", "-q", pinned["commit"]], check=True)
    head = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                          capture_output=True, text=True, check=True).stdout.strip()
    if head != pinned["commit"]:
        raise SystemExit(f"original at {root} is {head}; pinned commit is {pinned['commit']}")
    return root


def port_binary(arg):
    if arg:
        path = Path(arg)
    else:
        candidates = [CRATE / "target" / "release" / "dryer", CRATE / "target" / "debug" / "dryer"]
        path = next((p for p in candidates if p.exists()), candidates[0])
    if not path.exists():
        raise SystemExit(f"port binary not found: {path} (build the crate first)")
    return path


def load_cases(selected):
    cases = load_toml(PARITY / "coverage.toml").get("case", [])
    names = set()
    for case in cases:
        for key in ("name", "fixture", "args"):
            if key not in case:
                raise SystemExit(f"coverage.toml: case missing {key}: {case}")
        if case["name"] in names:
            raise SystemExit(f"coverage.toml: duplicate case {case['name']}")
        names.add(case["name"])
    if selected:
        unknown = sorted(set(selected) - names)
        if unknown:
            raise SystemExit(f"unknown case(s): {', '.join(unknown)}")
        cases = [c for c in cases if c["name"] in selected]
    return cases


def load_fixes():
    """fixes.toml entries by case name. An entry with a non-empty approved_by is approved (DR-PB-6)."""
    by_case = {}
    for fix in load_toml(PARITY / "fixes.toml").get("fix", []):
        for key in ("id", "fixture"):
            if key not in fix:
                raise SystemExit(f"fixes.toml: fix missing {key}: {fix}")
        by_case.setdefault(fix["fixture"], []).append(fix)
    return by_case


def judge(diffs, case_fixes, differs):
    """Status of one case: PASS, ACCEPTED (approved fix), PENDING (unapproved fix), or FAIL.

    A fix case passes only when the port matches its recorded expected output and
    still differs from the original.
    """
    if case_fixes:
        if diffs or not differs:
            return "FAIL"
        if any(fix.get("approved_by") for fix in case_fixes):
            return "ACCEPTED"
        return "PENDING"
    return "PASS" if not diffs else "FAIL"


# ---------------------------------------------------------------- running
def normalize_stdout(raw, root):
    text = raw.decode("utf-8", "replace")
    for path in sorted({str(root), os.path.realpath(root)}, key=len, reverse=True):
        text = text.replace(path, "<ROOT>")
    return text


def median_seconds(samples):
    return statistics.median(samples)


def positive_int(text):
    """Argument type of --timing-runs: a whole number of runs, at least 2. Two runs give a
    time, and --check-timing asks for at least 3, because a median of 2 samples has no
    middle sample."""
    try:
        value = int(text)
    except ValueError:
        raise argparse.ArgumentTypeError(f"needs a whole number of at least 2 runs, got {text!r}")
    if value < 2:
        raise argparse.ArgumentTypeError(f"needs a whole number of at least 2 runs, got {text!r}")
    return value


# DR-NFR-2, AC-10: the least count of runs per side that gives a median a middle sample.
MIN_TIMING_RUNS = 3


# DR-NFR-2, AC-10: wall-clock time of one program on one copy, median of 5 runs.
TIMED_RUNS = 5


def run_side(prefix, root, args, env, label, runs=TIMED_RUNS):
    """Run one side `runs` times, 5 by default. Run 1 gives the compared result. Runs 2 to
    the last only measure time, each after the old .metrics is gone, so no snapshot of an
    earlier run can pass a check. A timing run must exit like run 1, and the error names
    the case, the side, and the stderr of that run. seconds is the median of the runs
    (DR-NFR-2, AC-10).
    """
    samples = []
    res = None
    for index in range(runs):
        if index:
            shutil.rmtree(root / ".metrics", ignore_errors=True)
        start = time.perf_counter()
        proc = subprocess.run(prefix + ["--root", str(root)] + args, cwd=root, env=env,
                              capture_output=True)
        samples.append(time.perf_counter() - start)
        if index == 0:
            snap = root / ".metrics" / "dry.edn"
            edn = snap.read_bytes() if snap.exists() else None
            res = Result(proc.returncode, normalize_stdout(proc.stdout, root), edn, 0.0,
                         proc.stderr.decode("utf-8", "replace"))
        elif proc.returncode != res.code:
            detail = proc.stderr.decode("utf-8", "replace").strip()
            raise SystemExit(
                f"{label}: timing run {index + 1} exited {proc.returncode}, "
                f"but run 1 exited {res.code}; stderr: {detail or '(empty)'}"
            )
    return res._replace(seconds=median_seconds(samples))


def edn_diffs(orig_edn, port_edn):
    if orig_edn is None or port_edn is None:
        if orig_edn is port_edn:
            return []
        return [f"snapshot presence: original={orig_edn is not None} port={port_edn is not None}"]
    try:
        a = parse_edn(orig_edn.decode("utf-8"))
        b = parse_edn(port_edn.decode("utf-8"))
    except (ValueError, UnicodeDecodeError) as err:
        return [f"dry.edn does not parse: {err}"]
    if a == b:
        return []
    return ["dry.edn values differ", f"  original: {repr(a)[:400]}", f"  port:     {repr(b)[:400]}"]


def compare(orig, port):
    diffs = []
    if orig.code != port.code:
        diffs.append(f"exit code: original={orig.code} port={port.code}")
    if orig.stdout != port.stdout:
        diffs.append("stdout differs:")
        diff = difflib.unified_diff(orig.stdout.splitlines(), port.stdout.splitlines(),
                                    fromfile="original", tofile="port", lineterm="")
        diffs.extend(list(diff)[:MAX_DIFF_LINES])
    diffs.extend(edn_diffs(orig.edn, port.edn))
    return diffs


def write_expected(directory, res):
    """Writes exit_code, stdout, and dry.edn (when written) of one side."""
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "exit_code").write_text(f"{res.code}\n", encoding="utf-8")
    (directory / "stdout").write_text(res.stdout, encoding="utf-8")
    edn = directory / "dry.edn"
    if res.edn is None:
        edn.unlink(missing_ok=True)
    else:
        edn.write_bytes(res.edn)


def load_expected(directory):
    """Recorded side in `directory`, or None when it holds no recording."""
    code_file = directory / "exit_code"
    if not code_file.is_file():
        return None
    stdout_file = directory / "stdout"
    edn_file = directory / "dry.edn"
    return Result(int(code_file.read_text(encoding="utf-8").strip()),
                  stdout_file.read_text(encoding="utf-8") if stdout_file.is_file() else "",
                  edn_file.read_bytes() if edn_file.is_file() else None, 0.0, "")


def port_expected(name):
    """The directory that holds the port's own recorded output for a fix case."""
    return EXPECTED / name / "port"


def record(cases, expected, run_one, replace):
    """Runs every case before it writes, so a case that fails leaves the old recordings whole.
    With replace, expected/ goes only after every case ran fine. Writes the original's
    recording and, for a fix case, the port's own. Returns the exit code 0.
    """
    recorded = [(case["name"], run_one(case)) for case in cases]
    if replace:
        shutil.rmtree(expected, ignore_errors=True)
    for name, (orig, port) in recorded:
        write_expected(expected / name, orig)
        if port is not None:
            write_expected(expected / name / "port", port)
        print(f"RECORDED {name} exit={orig.code} original={orig.seconds:.2f}s")
    print(f"recorded {len(recorded)} case(s) into {expected}")
    return 0


def run_case(case, original_prefix, original_env, port_prefix, runs=TIMED_RUNS):
    with tempfile.TemporaryDirectory(prefix="dryer-parity-") as tmp:
        tmp = Path(tmp)
        check_path(tmp)
        git_setup = case.get("git_setup")
        orig_proj = make_copy(case["fixture"], git_setup, tmp / "original")
        orig = run_side(original_prefix, orig_proj, case["args"], original_env,
                        f"{case['name']} original", runs)
        if "Traceback" in orig.stderr:
            raise SystemExit(f"original crashed on {case['name']}:\n{orig.stderr}")
        if port_prefix is None:
            return orig, None
        port_proj = make_copy(case["fixture"], git_setup, tmp / "port")
        port = run_side(port_prefix, port_proj, case["args"], None,
                        f"{case['name']} port", runs)
        return orig, port


def check_timing(orig, port):
    """AC-10 diff lines: empty when the median of the port is lower than the median of the
    original, as DR-NFR-2 defines. An equal median fails, because AC-10 asks for lower."""
    if port.seconds < orig.seconds:
        return []
    return [
        f"timing: port median {port.seconds:.3f}s is not lower than "
        f"original median {orig.seconds:.3f}s"
    ]


def check_case(name, case_fixes, orig, port, timing=False):
    """Diff lines and status of one case. A fix case compares the port with its recorded
    output, and it must still differ from the original (DR-PB-3, DR-PB-6). With timing,
    the case also fails when the median of the port is not lower (AC-10)."""
    if not case_fixes:
        diffs = compare(orig, port)
        diffs += check_timing(orig, port) if timing else []
        return diffs, judge(diffs, [], bool(diffs))
    expected = load_expected(port_expected(name))
    if expected is None:
        diffs = [f"no recorded parity/expected/{name}/port/"]
    else:
        diffs = compare(expected, port)
    diffs += check_timing(orig, port) if timing else []
    return diffs, judge(diffs, case_fixes, bool(compare(orig, port)))


def report_case(name, case_fixes, orig, port, timing=False):
    """Prints the verdict and the diff lines of one case, and returns its status."""
    diffs, status = check_case(name, case_fixes, orig, port, timing)
    print(f"{status:<9}{name}  original={orig.seconds:.2f}s port={port.seconds:.2f}s")
    for line in diffs:
        print(f"    {line}")
    if status in ("ACCEPTED", "PENDING"):
        ids = ", ".join(fix["id"] for fix in case_fixes)
        print(f"    {'accepted' if status == 'ACCEPTED' else 'awaiting approval'} by: {ids}")
    return status


def check(cases, fixes, run_one, timing=False):
    """Runs every case as a check, prints one line for each, and returns 1 when one fails."""
    counts = dict.fromkeys(("PASS", "ACCEPTED", "PENDING", "FAIL"), 0)
    for case in cases:
        name = case["name"]
        orig, port = run_one(case)
        counts[report_case(name, fixes.get(name, []), orig, port, timing)] += 1
    print(f"{len(cases)} case(s), {counts['FAIL']} failed, {counts['ACCEPTED']} accepted, "
          f"{counts['PENDING']} awaiting approval")
    return 1 if counts["FAIL"] else 0


def main(argv=None):
    parser = argparse.ArgumentParser(description="Run dryer parity cases.")
    parser.add_argument("--record", action="store_true",
                        help="record results into parity/expected/ (original, and port for fix cases)")
    parser.add_argument("--case", action="append", dest="cases", default=[],
                        help="run only this case (repeatable)")
    parser.add_argument("--original", metavar="DIR",
                        help="original checkout (default: parity/.cache/dryer-original)")
    parser.add_argument("--port", metavar="BIN", help="port binary (default: target/)")
    parser.add_argument("--check-timing", action="store_true",
                        help="fail a case whose port median is not lower than its original median "
                             "(DR-NFR-2, AC-10); run it in the named runner, the Ubuntu 22.04 "
                             "x86_64 guest of ci/linux-vm under qemu with TCG")
    parser.add_argument("--timing-runs", type=positive_int, default=TIMED_RUNS, metavar="N",
                        help=f"runs per side for the median (default {TIMED_RUNS}, as DR-NFR-2 says; "
                             f"--check-timing needs at least {MIN_TIMING_RUNS})")
    args = parser.parse_args(argv)
    # DR-NFR-2, AC-10: a timing verdict needs a median with a middle sample, so
    # --check-timing with 2 runs is an error and not a weaker check.
    if args.check_timing and args.timing_runs < MIN_TIMING_RUNS:
        parser.error(
            f"--check-timing needs --timing-runs of at least {MIN_TIMING_RUNS}, "
            f"because a median of {args.timing_runs} samples has no middle sample"
        )

    pinned = load_toml(PARITY / "upstream.toml")
    original_root = ensure_original(args.original, pinned)
    cases = load_cases(args.cases)
    fixes = load_fixes()

    original_prefix = [os.environ.get("DRYER_ORIGINAL_PYTHON", sys.executable), "-m", "dryer"]
    original_env = dict(os.environ, PYTHONPATH=str(original_root / "src"))
    port_prefix = None

    def run_one(case):
        nonlocal port_prefix
        wants_port = not args.record or bool(fixes.get(case["name"], []))
        if wants_port and port_prefix is None:
            port_prefix = [str(port_binary(args.port))]
        return run_case(case, original_prefix, original_env,
                        port_prefix if wants_port else None, args.timing_runs)

    if args.record:
        return record(cases, EXPECTED, run_one, replace=not args.cases)
    return check(cases, fixes, run_one, args.check_timing)


if __name__ == "__main__":
    sys.exit(main())
