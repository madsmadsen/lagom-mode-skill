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
The venv in `/tmp/mu/venv` is built from those checkouts. The runner clones
nothing.

Before the first run, the runner checks that the original in the venv is at the
commit of `upstream.toml` (MU-PB-1, MU-PB-2). It asks the venv where its
`mutator` package lives and runs `git rev-parse HEAD` in that directory. When
the package is a copy in `site-packages` with no checkout, it reads the commit
that pip records in `direct_url.json` instead. A commit that it cannot read, and
a commit other than the pin, stop the runner with a message that names both.

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

An approval excuses the stdout difference and the snapshot difference of its
run, and only those. A mismatch of the exit code, a git status difference, and an
error stay a `FAIL`, approved or not. An approval is for a difference that the
fix changes on purpose: a fix that changes the snapshot of a run (for example
MU-23, MU-24, or MU-26) excuses that snapshot difference too.

The runner compares fix ids, not fixture paths. One approval therefore
excuses only the runs that name it. `parity/test_status.py` tests this rule
with fake ids, and it checks that `fixes.toml` has no approved entry.
When Mads Madsen approves an entry on purpose, change that test to name the
approved id. Do not delete it. `parity/test_pin.py` tests the pin check with stub
processes and no network. Both files need Python 3.11 or later, and no original.
Run one of:

    python3.12 -m unittest parity/test_status.py
    python3.12 -m unittest discover -s .   # from tools/mutator, as crapper does

`tests/parity_map.rs` runs the second command for you: `cargo test` runs it in
`tools/mutator`. The test skips with a note when the host has no `python3.12`
and no `python3` with `tomllib`, and it kills the run after 120 s. The Linux
guests of `ci/linux-vm/` install `python3.12`.

`parity/__init__.py` marks `parity/` as a package, so discovery finds the tests
and crapper can measure their coverage. The suite covers `test_status.py` (the
approval rule, AC-1, MU-PB-6) and `test_pin.py` (the pin check, MU-PB-1,
MU-PB-2).

## Fixtures

`fixtures/<lang>/` has one small project per language: clojure, java, go,
typescript, rust, and python. Each has a `check.sh` test command that greps a few
pinned tokens, so some mutants die and some survive. Each fixture is a source
tree only; there are no expected outputs. The expected results come from running
the original.
