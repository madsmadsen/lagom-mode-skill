#!/usr/bin/env python3
"""Parity runner for the Rust port of crapper (CR-PB-1..6, AC-1, AC-8, AC-17, AC-20..24).

Usage:
  python3.12 parity/run.py [--port-bin PATH] [--original-python PATH] [--workdir DIR]
                           [--record] [--timing] [--only RUN_ID ...]
  python3.12 parity/run.py --check-meta

Stdlib only. Importing this module does no work.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

PARITY = Path(__file__).resolve().parent
CRATE = PARITY.parent
APPROVER = "Mads Madsen"
LICENSE_TITLE = "MIT License"
LANGUAGE_FIXTURES = ["clojure", "java", "go", "typescript", "rust", "python"]
TIMING_RUNS = 5
HEADER_LINES = 10
RUN_ID = re.compile(r"[a-z0-9-]+")
FIXTURE_NAME = re.compile(r"[a-z0-9][a-z0-9_-]*")
GIT_ENV = {
    "GIT_AUTHOR_NAME": "parity",
    "GIT_AUTHOR_EMAIL": "parity@example.invalid",
    "GIT_COMMITTER_NAME": "parity",
    "GIT_COMMITTER_EMAIL": "parity@example.invalid",
    "GIT_AUTHOR_DATE": "2000-01-01T00:00:00+0000",
    "GIT_COMMITTER_DATE": "2000-01-01T00:00:00+0000",
}
VERSION_SCRIPT = (
    "import importlib.metadata as m, json, sys\n"
    "def v(n):\n"
    "    try:\n"
    "        return m.version(n)\n"
    "    except m.PackageNotFoundError:\n"
    "        return None\n"
    "print(json.dumps({n: v(n) for n in sys.argv[1:]}))\n"
)


class ParityError(Exception):
    """A failure that stops the runner with a message."""


# ---------------------------------------------------------------- EDN subset


class EdnError(Exception):
    """The snapshot is not in the EDN subset this runner reads."""


@dataclasses.dataclass(frozen=True)
class Keyword:
    name: str


TOKEN = re.compile(r'[\s,]*(?:([{}\[\]])|("(?:[^"\\]|\\.)*")|([^\s,{}\[\]"]+))')
TRAILING = re.compile(r"[\s,]*\Z")
INTEGER = re.compile(r"[+-]?\d+")
FLOAT = re.compile(r"[+-]?\d+(?:\.\d+(?:[eE][+-]?\d+)?|[eE][+-]?\d+)")
ATOMS = {"nil": None, "true": True, "false": False}


def tokenize(text: str) -> list[str]:
    tokens = []
    pos = 0
    while match := TOKEN.match(text, pos):
        tokens.append(match.group(1) or match.group(2) or match.group(3))
        pos = match.end()
    if not TRAILING.match(text, pos):
        raise EdnError(f"cannot read EDN at offset {pos}")
    return tokens


def read_string(token: str) -> str:
    try:
        return json.loads(token)
    except json.JSONDecodeError as err:
        raise EdnError(f"bad string {token}") from err


def read_atom(token: str):
    if token.startswith('"'):
        return read_string(token)
    if token.startswith(":"):
        return Keyword(token[1:])
    if token in ATOMS:
        return ATOMS[token]
    if INTEGER.fullmatch(token):
        return int(token)
    if FLOAT.fullmatch(token):
        return float(token)
    raise EdnError(f"unknown token {token!r}")


def read_items(tokens: list[str], pos: int, closer: str) -> tuple[list, int]:
    items = []
    while True:
        if pos >= len(tokens):
            raise EdnError(f"missing {closer}")
        if tokens[pos] == closer:
            return items, pos + 1
        value, pos = read_value(tokens, pos)
        items.append(value)


def to_map(items: list) -> dict:
    if len(items) % 2:
        raise EdnError("map with a key and no value")
    try:
        return dict(zip(items[::2], items[1::2]))
    except TypeError as err:
        raise EdnError(f"unhashable map key: {err}") from err


def read_value(tokens: list[str], pos: int):
    if pos >= len(tokens):
        raise EdnError("unexpected end of EDN")
    token = tokens[pos]
    if token == "{":
        items, pos = read_items(tokens, pos + 1, "}")
        return to_map(items), pos
    if token == "[":
        return read_items(tokens, pos + 1, "]")
    return read_atom(token), pos + 1


def parse_edn(text: str):
    tokens = tokenize(text)
    value, pos = read_value(tokens, 0)
    if pos != len(tokens):
        raise EdnError("extra data after the top-level value")
    return value


def canon(value):
    """Make parsed EDN comparable: a float keeps its exact repr, so 1 and 1.0 differ."""
    if isinstance(value, dict):
        return {key: canon(item) for key, item in value.items()}
    if isinstance(value, list):
        return [canon(item) for item in value]
    if isinstance(value, float):
        return ("float", repr(value))
    return value


# ---------------------------------------------------------------- TOML inputs


def load_toml(path: Path) -> dict:
    if not path.is_file():
        raise ParityError(f"missing file: {path}")
    with path.open("rb") as handle:
        return tomllib.load(handle)


def upstream_problems(upstream: dict) -> list[str]:
    problems = []
    for key in ("repository", "commit"):
        if not str(upstream.get(key, "")).strip():
            problems.append(f"upstream.toml: {key} missing or empty (AC-21)")
    deps = upstream.get("python-dependencies") or {}
    if not deps:
        problems.append("upstream.toml: [python-dependencies] missing or empty (AC-21)")
    for name, version in deps.items():
        if not str(version).strip():
            problems.append(f"upstream.toml: version of {name} missing or empty (AC-21)")
    return problems


def is_fixture(parity: Path, name) -> bool:
    return (
        isinstance(name, str)
        and FIXTURE_NAME.fullmatch(name) is not None
        and (parity / "fixtures" / name).is_dir()
    )


def run_problems(runs: list[dict], parity: Path) -> list[str]:
    problems = []
    seen: set[str] = set()
    for run in runs:
        rid = run.get("id", "")
        if not RUN_ID.fullmatch(rid):
            problems.append(f"coverage.toml: bad run id {rid!r}")
        elif rid in seen:
            problems.append(f"coverage.toml: duplicate run id {rid}")
        seen.add(rid)
        if not is_fixture(parity, run.get("fixture")):
            problems.append(f"run {rid}: fixture {run.get('fixture')!r} is not an existing parity fixture")
    return problems


def coverage_gaps(coverage: dict) -> list[str]:
    """AC-1: every required item is covered by at least one run."""
    covered = {item for run in coverage.get("run", []) for item in run.get("covers", [])}
    items = coverage.get("required", {}).get("items", [])
    return [f"AC-1: no run covers {item}" for item in items if item not in covered]


def load_fixes(parity: Path) -> list[dict]:
    return load_toml(parity / "fixes.toml").get("fix", [])


def is_approved(entry: dict) -> bool:
    return entry.get("approved_by") == APPROVER


def fix_label(entry: dict) -> str:
    return entry.get("issue") or entry.get("description") or "<no issue>"


def validate_fix(entry: dict, parity: Path) -> list[str]:
    problems = []
    if not (entry.get("issue") or entry.get("description")):
        problems.append("needs an issue or a description (AC-23)")
    if not entry.get("behavior"):
        problems.append("needs a behavior (AC-23)")
    if not is_fixture(parity, entry.get("fixture")):
        problems.append(f"fixture {entry.get('fixture')!r} is not an existing parity fixture (AC-23)")
    return problems


def fix_problems(fixes: list[dict], parity: Path) -> list[str]:
    return [
        f"fixes.toml fix #{number}: {problem}"
        for number, entry in enumerate(fixes, 1)
        for problem in validate_fix(entry, parity)
    ]


def unapproved_problems(fixes: list[dict]) -> list[str]:
    return [
        f'AC-24: fixes.toml fix {fix_label(entry)} lacks approved_by = "{APPROVER}"'
        for entry in fixes
        if not is_approved(entry)
    ]


def exemption(fixes: list[dict], fixture: str) -> tuple[bool, list[str]]:
    """A difference is exempt when an approved entry names its fixture (AC-1, AC-24)."""
    entries = [entry for entry in fixes if entry.get("fixture") == fixture]
    approved = any(is_approved(entry) for entry in entries)
    return approved, [fix_label(entry) for entry in entries if not is_approved(entry)]


# ---------------------------------------------------------------- running


@dataclasses.dataclass(frozen=True)
class Outcome:
    code: int
    stdout: bytes
    edn: bytes | None
    snapshot: object


@dataclasses.dataclass(frozen=True)
class Context:
    original: list[str]
    port: list[str]
    parity: Path
    record: bool
    fixes: list[dict]


@dataclasses.dataclass
class RunResult:
    run_id: str
    status: str  # PASS | SKIP | DIFF
    diffs: list[str] = dataclasses.field(default_factory=list)
    exempt: bool = False
    unapproved: list[str] = dataclasses.field(default_factory=list)

    @property
    def failed(self) -> bool:
        return self.status == "DIFF" and not self.exempt


def make_copy(fixture_dir: Path) -> Path:
    root = Path(tempfile.mkdtemp(prefix="crapper-parity-"))
    project = root / "project"
    shutil.copytree(fixture_dir, project)
    return project


def drop_copy(project: Path) -> None:
    shutil.rmtree(project.parent, ignore_errors=True)


def run_setup(commands: list[str], project: Path) -> None:
    env = {**os.environ, **GIT_ENV}
    for command in commands:
        subprocess.run(["sh", "-c", command], cwd=project, env=env, check=True)


GO_TEST_TIME = re.compile(rb"(\t)\d+\.\d+s(\t)")


def normalize(stdout: bytes, project: Path) -> bytes:
    for variant in sorted({str(project), str(project.resolve())}, key=len, reverse=True):
        stdout = stdout.replace(variant.encode(), b"<ROOT>")
    return GO_TEST_TIME.sub(rb"\1<TIME>s\2", stdout)


def metrics_root(project: Path, args: list[str]) -> Path:
    """The original writes .metrics/crap.edn under --root (default: project)."""
    if "--root" in args:
        index = args.index("--root")
        if index + 1 < len(args):
            return project / args[index + 1]
    return project


def read_edn_bytes(project: Path, args: list[str]) -> bytes | None:
    path = metrics_root(project, args) / ".metrics" / "crap.edn"
    return path.read_bytes() if path.is_file() else None


def snapshot_of(edn: bytes | None):
    if edn is None:
        return None
    try:
        return canon(parse_edn(edn.decode("utf-8")))
    except (EdnError, UnicodeDecodeError) as err:
        return ("invalid", str(err))


def run_env(run: dict, project: Path) -> dict[str, str]:
    """Environment of the tool under test: a `.stubs` directory in the fixture goes first on PATH,
    and each `env` entry of the run (`NAME=VALUE`, `{project}` is the project path) is set."""
    env = dict(os.environ)
    stubs = project / ".stubs"
    if stubs.is_dir():
        env["PATH"] = f"{stubs}{os.pathsep}{env.get('PATH', '')}"
    for entry in run.get("env", []):
        name, _, value = entry.partition("=")
        env[name] = value.replace("{project}", str(project))
    return env


def execute(prefix: list[str], args: list[str], project: Path, env: dict[str, str]) -> Outcome:
    done = subprocess.run([*prefix, *args], cwd=project, capture_output=True, env=env)
    edn = read_edn_bytes(project, args)
    return Outcome(done.returncode, normalize(done.stdout, project), edn, snapshot_of(edn))


def run_pair(run: dict, ctx: Context) -> tuple[Outcome, Outcome]:
    fixture_dir = ctx.parity / "fixtures" / run["fixture"]
    original_copy = make_copy(fixture_dir)
    port_copy = make_copy(fixture_dir)
    try:
        for project in (original_copy, port_copy):
            run_setup(run.get("setup", []), project)
        args = run.get("args", [])
        original = execute(ctx.original, args, original_copy, run_env(run, original_copy))
        return original, execute(ctx.port, args, port_copy, run_env(run, port_copy))
    finally:
        drop_copy(original_copy)
        drop_copy(port_copy)


def diff_outcomes(original: Outcome, port: Outcome) -> list[str]:
    diffs = []
    if original.code != port.code:
        diffs.append(f"exit {original.code} vs {port.code}")
    if original.stdout != port.stdout:
        diffs.append("stdout differs")
    if original.snapshot != port.snapshot:
        diffs.append("snapshot differs")
    return diffs


def missing_tools(names: list[str]) -> list[str]:
    return [name for name in names if shutil.which(name) is None]


def record_run(run_id: str, outcome: Outcome, parity: Path) -> None:
    target = parity / "recorded" / run_id
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    (target / "exit.txt").write_text(f"{outcome.code}\n", encoding="utf-8")
    (target / "stdout.txt").write_bytes(outcome.stdout)
    if outcome.edn is not None:
        (target / "crap.edn").write_bytes(outcome.edn)


def describe_diff(result: RunResult) -> str:
    line = f"DIFF {result.run_id}: {'; '.join(result.diffs)}"
    if result.exempt:
        return f"{line} (exempt: approved fixes.toml entry)"
    detail = f"; lacking approval: {', '.join(result.unapproved)}" if result.unapproved else ""
    return f"{line} (no approved fixes.toml entry{detail})"


def run_one(run: dict, ctx: Context) -> RunResult:
    rid = run["id"]
    missing = missing_tools(run.get("requires", []))
    if missing:
        print(f"SKIP {rid} (missing: {', '.join(missing)})")
        return RunResult(rid, "SKIP")
    original, port = run_pair(run, ctx)
    if ctx.record:
        record_run(rid, original, ctx.parity)
    diffs = diff_outcomes(original, port)
    if not diffs:
        print(f"PASS {rid}")
        return RunResult(rid, "PASS")
    exempt, unapproved = exemption(ctx.fixes, run["fixture"])
    result = RunResult(rid, "DIFF", diffs, exempt, unapproved)
    print(describe_diff(result))
    return result


def select_runs(coverage: dict, only: list[str] | None) -> list[dict]:
    runs = coverage.get("run", [])
    if not only:
        return runs
    known = {run["id"] for run in runs}
    unknown = [rid for rid in only if rid not in known]
    if unknown:
        raise ParityError(f"--only: unknown run ids: {', '.join(unknown)}")
    return [run for run in runs if run["id"] in set(only)]


# ---------------------------------------------------------------- original install (AC-21)


def fresh_dir(path: Path) -> Path:
    if path.exists():
        shutil.rmtree(path)
    return path


def clone_upstream(repository: str, commit: str, clone: Path) -> None:
    subprocess.run(["git", "clone", repository, str(clone)], check=True)
    subprocess.run(["git", "-C", str(clone), "checkout", commit], check=True)


def installed_versions(python: Path, names: list[str]) -> dict:
    done = subprocess.run(
        [str(python), "-c", VERSION_SCRIPT, *names], capture_output=True, text=True, check=True
    )
    return json.loads(done.stdout)


def git_head(clone: Path) -> str:
    done = subprocess.run(
        ["git", "-C", str(clone), "rev-parse", "HEAD"], capture_output=True, text=True, check=True
    )
    return done.stdout.strip()


def verify_install(python: Path, deps: dict, clone: Path, commit: str) -> list[str]:
    installed = installed_versions(python, list(deps))
    problems = [
        f"{name}: installed {installed.get(name)}, pinned {version}"
        for name, version in deps.items()
        if installed.get(name) != version
    ]
    head = git_head(clone)
    if head != commit:
        problems.append(f"clone HEAD {head} != commit {commit}")
    return problems


def setup_original(workdir: Path, upstream: dict) -> str:
    workdir.mkdir(parents=True, exist_ok=True)
    clone = fresh_dir(workdir / "upstream")
    venv = fresh_dir(workdir / "venv")
    deps = upstream["python-dependencies"]
    clone_upstream(upstream["repository"], upstream["commit"], clone)
    subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)
    python = venv / "bin" / "python"
    pins = [f"{name}=={version}" for name, version in deps.items()]
    subprocess.run([str(python), "-m", "pip", "install", *pins], check=True)
    subprocess.run([str(python), "-m", "pip", "install", "-e", str(clone)], check=True)
    problems = verify_install(python, deps, clone, upstream["commit"])
    if problems:
        raise ParityError("installed original does not match upstream.toml:\n" + "\n".join(problems))
    return str(python)


def build_port(crate: Path) -> Path:
    subprocess.run(["cargo", "build", "--release"], cwd=crate, check=True)
    binary = crate / "target" / "release" / "crapper"
    if not binary.is_file():
        raise ParityError(f"cargo build did not produce {binary}")
    return binary


def absolute_tool(name: str) -> str:
    found = shutil.which(name)
    if found is None:
        raise ParityError(f"not an executable: {name}")
    return os.path.abspath(found)


# ---------------------------------------------------------------- timing (AC-17)


def timed(argv: list[str], cwd: Path) -> float:
    start = time.perf_counter()
    subprocess.run(argv, cwd=cwd, capture_output=True)
    return time.perf_counter() - start


def time_fixture(fixture: str, ctx: Context) -> tuple[float, float]:
    project = make_copy(ctx.parity / "fixtures" / fixture)
    tools = {"original": ctx.original, "port": ctx.port}
    samples: dict[str, list[float]] = {"original": [], "port": []}
    try:
        for index in range(TIMING_RUNS):
            order = ["original", "port"] if index % 2 == 0 else ["port", "original"]
            for tool in order:
                argv = [*tools[tool], "--use-existing-coverage"]
                samples[tool].append(timed(argv, project))
    finally:
        drop_copy(project)
    return statistics.median(samples["original"]), statistics.median(samples["port"])


def timing_problems(ctx: Context) -> list[str]:
    problems = []
    for fixture in LANGUAGE_FIXTURES:
        original, port = time_fixture(fixture, ctx)
        print(f"TIMING {fixture}: original median {original:.4f}s, port median {port:.4f}s")
        if port >= original:
            problems.append(
                f"AC-17: {fixture}: port median {port:.4f}s is not lower than original {original:.4f}s"
            )
    return problems


# ---------------------------------------------------------------- last-run.toml (CR-PB-2)


def toml_list(items: list[str]) -> str:
    return "[" + ", ".join(json.dumps(item) for item in items) + "]"


def render_last_run(commit: str, result: str, runs: int, skipped: list[str], diffs: list[str]) -> str:
    return (
        f"commit = {json.dumps(commit)}\n"
        f'result = "{result}"\n'
        f"runs = {runs}\n"
        f"skipped = {toml_list(skipped)}\n"
        f"diffs = {toml_list(diffs)}\n"
    )


def write_last_run(parity: Path, commit: str, results: list[RunResult]) -> None:
    result = "fail" if any(r.failed for r in results) else "pass"
    text = render_last_run(
        commit,
        result,
        len(results),
        [r.run_id for r in results if r.status == "SKIP"],
        [r.run_id for r in results if r.status == "DIFF"],
    )
    (parity / "last-run.toml").write_text(text, encoding="utf-8")


def print_summary(results: list[RunResult]) -> None:
    count = {status: sum(r.status == status for r in results) for status in ("PASS", "SKIP", "DIFF")}
    failed = sum(r.failed for r in results)
    print(
        f"SUMMARY runs={len(results)} pass={count['PASS']} skip={count['SKIP']} "
        f"diff={count['DIFF']} failed={failed}"
    )
    print(f"RESULT {'fail' if failed else 'pass'}")


# ---------------------------------------------------------------- --check-meta (AC-20, AC-22..24)


def last_run_problems(parity: Path, commit: str) -> list[str]:
    path = parity / "last-run.toml"
    if not path.is_file():
        return ["AC-22: last-run.toml is missing"]
    last = load_toml(path)
    problems = []
    if last.get("commit") != commit:
        problems.append(f"AC-22: last-run.toml commit {last.get('commit')!r} != upstream.toml commit {commit!r}")
    if last.get("result") != "pass":
        problems.append(f"AC-22: last-run.toml result is {last.get('result')!r}, not 'pass'")
    return problems


def license_problems(crate: Path) -> list[str]:
    path = crate / "LICENSE"
    if not path.is_file() or not path.read_text(encoding="utf-8").startswith(LICENSE_TITLE):
        return [f"AC-20: LICENSE does not begin with '{LICENSE_TITLE}'"]
    return []


def parse_header(lines: list[str]) -> dict[str, str]:
    header: dict[str, str] = {}
    for line in lines:
        if line.startswith("// ") and ":" in line:
            key, _, value = line[3:].partition(":")
            header.setdefault(key.strip(), value.strip())
    return header


def rs_header_problems(path: Path, crate: Path, upstream: dict) -> list[str]:
    rel = path.relative_to(crate)
    lines = path.read_text(encoding="utf-8").splitlines()[:HEADER_LINES]
    header = parse_header(lines)
    problems = []
    if header.get("upstream") != upstream["repository"]:
        problems.append(f"AC-20: {rel}: header does not name {upstream['repository']}")
    if header.get("upstream-commit") != upstream["commit"]:
        problems.append(f"AC-20: {rel}: header upstream-commit is not {upstream['commit']}")
    if not header.get("upstream-path"):
        problems.append(f"AC-20: {rel}: header lacks upstream-path (use 'none' if no counterpart)")
    return problems


def rust_sources(crate: Path) -> list[Path]:
    return sorted(
        path
        for sub in ("src", "tests")
        if (crate / sub).is_dir()
        for path in (crate / sub).rglob("*.rs")
    )


def check_meta(parity: Path = PARITY, crate: Path = CRATE) -> list[str]:
    upstream = load_toml(parity / "upstream.toml")
    commit = str(upstream.get("commit", ""))
    fixes = load_fixes(parity)
    problems = upstream_problems(upstream) + last_run_problems(parity, commit)
    problems += fix_problems(fixes, parity) + unapproved_problems(fixes)
    problems += license_problems(crate)
    for path in rust_sources(crate):
        problems += rs_header_problems(path, crate, upstream)
    return problems


def report_meta(problems: list[str]) -> int:
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1
    print("meta ok")
    return 0


# ---------------------------------------------------------------- main


def run_parity(args: argparse.Namespace) -> int:
    upstream = load_toml(PARITY / "upstream.toml")
    coverage = load_toml(PARITY / "coverage.toml")
    fixes = load_fixes(PARITY)
    problems = (
        upstream_problems(upstream)
        + run_problems(coverage.get("run", []), PARITY)
        + coverage_gaps(coverage)
        + fix_problems(fixes, PARITY)
    )
    if problems:
        return report_meta(problems)
    runs = select_runs(coverage, args.only)
    original_python = (
        absolute_tool(args.original_python)
        if args.original_python
        else setup_original((args.workdir or PARITY / ".work").absolute(), upstream)
    )
    port = absolute_tool(str(args.port_bin)) if args.port_bin else str(build_port(CRATE))
    ctx = Context([original_python, "-m", "crapper"], [port], PARITY, args.record, fixes)
    results = [run_one(run, ctx) for run in runs]
    print_summary(results)
    timing = timing_problems(ctx) if args.timing else []
    if args.only:
        print("last-run.toml not written: --only is a partial run")
    else:
        write_last_run(PARITY, upstream["commit"], results)
    for problem in timing:
        print(problem, file=sys.stderr)
    failed = any(r.failed for r in results)
    return 1 if failed or timing else 0


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Parity runner: original Python crapper vs the Rust port.")
    parser.add_argument("--port-bin", help="port binary (default: cargo build --release in the crate)")
    parser.add_argument("--original-python", help="interpreter with the pinned original installed (default: install into --workdir)")
    parser.add_argument("--workdir", type=Path, help="clone and venv directory (default: parity/.work)")
    parser.add_argument("--record", action="store_true", help="write the original's outputs to parity/recorded/")
    parser.add_argument("--timing", action="store_true", help="AC-17 timing on the six language fixtures")
    parser.add_argument("--only", nargs="+", metavar="RUN_ID", help="run only these run ids")
    parser.add_argument("--check-meta", action="store_true", help="check last-run.toml, fixes.toml, LICENSE and .rs headers; no original needed")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    if sys.version_info < (3, 11):
        print("parity: Python 3.11 or later is required", file=sys.stderr)
        return 1
    args = parse_args(argv)
    try:
        if args.check_meta:
            return report_meta(check_meta(PARITY, CRATE))
        return run_parity(args)
    except (ParityError, subprocess.CalledProcessError, OSError, tomllib.TOMLDecodeError) as err:
        print(f"parity: {err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
