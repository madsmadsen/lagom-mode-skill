#!/usr/bin/env python3
"""Parity runner: the Rust port of dryer against the pinned Python original.

Usage:
  run_parity.py [--case NAME]... [--original DIR] [--port BIN]
  run_parity.py --record [--case NAME]... [--original DIR]

Default mode runs the original and the port on each case and compares exit
code, normalized stdout, and .metrics/dry.edn. --record runs only the original
and writes parity/expected/<case>/ (exit_code, stdout, dry.edn if written).

Each case copies fixtures/<fixture>/ into a fresh temp directory for each
program, applies git_setup if set, and runs `<program> --root <copy> ARGS`
with cwd set to the copy. Copy paths in stdout are replaced with <ROOT>.

Stdlib only; Python >= 3.9. Set DRYER_ORIGINAL_PYTHON to choose the interpreter
that has the original's dependencies (for example the venv python).
"""

import argparse
import difflib
import json
import os
import re
import shutil
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
    by_case = {}
    for fix in load_toml(PARITY / "fixes.toml").get("fix", []):
        for key in ("id", "fixture", "approved_by"):
            if key not in fix:
                raise SystemExit(f"fixes.toml: fix missing {key}: {fix}")
        by_case.setdefault(fix["fixture"], []).append(fix["id"])
    return by_case


# ---------------------------------------------------------------- running
def normalize_stdout(raw, root):
    text = raw.decode("utf-8", "replace")
    for path in sorted({str(root), os.path.realpath(root)}, key=len, reverse=True):
        text = text.replace(path, "<ROOT>")
    return text


def run_side(prefix, root, args, env):
    start = time.perf_counter()
    proc = subprocess.run(prefix + ["--root", str(root)] + args, cwd=root, env=env,
                          capture_output=True)
    seconds = time.perf_counter() - start
    snap = root / ".metrics" / "dry.edn"
    edn = snap.read_bytes() if snap.exists() else None
    return Result(proc.returncode, normalize_stdout(proc.stdout, root), edn, seconds,
                  proc.stderr.decode("utf-8", "replace"))


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


def write_expected(name, res):
    d = EXPECTED / name
    d.mkdir(parents=True, exist_ok=True)
    (d / "exit_code").write_text(f"{res.code}\n", encoding="utf-8")
    (d / "stdout").write_text(res.stdout, encoding="utf-8")
    if res.edn is not None:
        (d / "dry.edn").write_bytes(res.edn)


def run_case(case, original_prefix, original_env, port_prefix, record):
    with tempfile.TemporaryDirectory(prefix="dryer-parity-") as tmp:
        tmp = Path(tmp)
        check_path(tmp)
        git_setup = case.get("git_setup")
        orig_proj = make_copy(case["fixture"], git_setup, tmp / "original")
        orig = run_side(original_prefix, orig_proj, case["args"], original_env)
        if "Traceback" in orig.stderr:
            raise SystemExit(f"original crashed on {case['name']}:\n{orig.stderr}")
        if record:
            return orig, None
        port_proj = make_copy(case["fixture"], git_setup, tmp / "port")
        port = run_side(port_prefix, port_proj, case["args"], None)
        return orig, port


def main(argv=None):
    parser = argparse.ArgumentParser(description="Run dryer parity cases.")
    parser.add_argument("--record", action="store_true",
                        help="run the original only and write parity/expected/")
    parser.add_argument("--case", action="append", dest="cases", default=[],
                        help="run only this case (repeatable)")
    parser.add_argument("--original", metavar="DIR",
                        help="original checkout (default: parity/.cache/dryer-original)")
    parser.add_argument("--port", metavar="BIN", help="port binary (default: target/)")
    args = parser.parse_args(argv)

    pinned = load_toml(PARITY / "upstream.toml")
    original_root = ensure_original(args.original, pinned)
    cases = load_cases(args.cases)
    fixes = load_fixes()

    original_prefix = [os.environ.get("DRYER_ORIGINAL_PYTHON", sys.executable), "-m", "dryer"]
    original_env = dict(os.environ, PYTHONPATH=str(original_root / "src"))
    port_prefix = None if args.record else [str(port_binary(args.port))]

    if args.record and not args.cases:
        shutil.rmtree(EXPECTED, ignore_errors=True)

    failures = 0
    accepted = 0
    for case in cases:
        name = case["name"]
        orig, port = run_case(case, original_prefix, original_env, port_prefix, args.record)
        if args.record:
            write_expected(name, orig)
            print(f"RECORDED {name} exit={orig.code} original={orig.seconds:.2f}s")
            continue
        diffs = compare(orig, port)
        if not diffs:
            status = "PASS"
        elif name in fixes:
            status = "ACCEPTED"
            accepted += 1
        else:
            status = "FAIL"
            failures += 1
        print(f"{status:<9}{name}  original={orig.seconds:.2f}s port={port.seconds:.2f}s")
        for line in diffs:
            print(f"    {line}")
        if status == "ACCEPTED":
            print(f"    accepted by: {', '.join(fixes[name])}")

    if args.record:
        print(f"recorded {len(cases)} case(s) into {EXPECTED}")
        return 0
    print(f"{len(cases)} case(s), {failures} failed, {accepted} accepted")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
