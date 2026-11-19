# Parity runner

`run.py` checks the Rust port against the original Python mutator. It runs
every entry in `coverage.toml` on both, then compares the results.

## Run

From the repository root, with the original's venv (it provides the original
mutator and its EDN reader):

    /tmp/mu/venv/bin/python tools/mutator/parity/run.py

Options:

- `--original-only`: run only the original side. The port does not need to be built.
- `--only NAME`: run one entry. Repeatable.
- `--show`: print the diffs of each run.

Environment:

- `ORIGINAL_PYTHON`: interpreter for the original (default `/tmp/mu/venv/bin/python`).
- `MUTATOR_BIN`: the port binary (default `tools/mutator/target/debug/mutator`).

## Pinned sources

`upstream.toml` pins the original mutator (`a5a0cb8`) and crapper (`6fd92c4`).
The venv in `/tmp/mu/venv` is built from those checkouts.

## What is compared

Each run gets a fresh copy of its fixture, plus a git repository with one commit.
Both sides then run the same setup steps and the same arguments.

- Exit codes must match each other. If the entry sets `expect_exit`, both must match it.
- Every snapshot under `.metrics/mutate/**/*.edn` must exist on both sides with equal
  content. The original's reader parses each file, and the top-level `tested-at`
  key is dropped before the comparison.
- stdout must be equal on both sides after `<ROOT>` normalization. No run skips
  this check: a stdout difference needs an approved fix (see below).
- `git status --porcelain` of both trees must be equal.
- stderr is informational. Its line differences are printed, not compared.
- `--show` prints the diffs.

Each output line is `PASS`, `FAIL`, or `DIFF-ALLOWED`. The runner exits 1 if any run is `FAIL`.

## Approved differences

`fixes.toml` lists the intentional differences. A run names the difference it
shows with `fix = "<id>"`, or with a list of ids when more than one entry
explains it. When every named id has `approved = true` in `fixes.toml`, a
`DIFF-ALLOWED` replaces `FAIL`. Mads Madsen approves each entry (MU-PB-6). A run
that names no fix, or an id that is not approved, prints `FAIL`.

An approval excuses the stdout difference of its run, and only that. A
mismatch of the exit code, a metrics difference, a git status difference, and an
error stay a `FAIL`, approved or not.

The runner compares fix ids, not fixture paths. One approval therefore
excuses only the runs that name it. `parity/test_status.py` tests this rule
with fake ids, and it checks that `fixes.toml` has no approved entry. When Mads
Madsen approves an entry on purpose, change that test to name the approved id.
Do not delete it. It needs Python 3.11 or later, and no original. Run one of:

    python3.12 -m unittest parity/test_status.py
    python3.12 -m unittest discover -s .   # from tools/mutator, as crapper does

`tests/parity_map.rs` runs the second command for you: `cargo test` runs it in
`tools/mutator`. The test skips with a note when the host has no `python3.12`
and no `python3` with `tomllib`.

`parity/__init__.py` marks `parity/` as a package, so discovery finds the tests
and crapper can measure their coverage.

## Fixtures

`fixtures/<lang>/` has one small project per language: clojure, java, go,
typescript, rust, and python. Each has a `check.sh` test command that greps a few
pinned tokens, so some mutants die and some survive. Each fixture is a source
tree only; there are no expected outputs. The expected results come from running
the original.
