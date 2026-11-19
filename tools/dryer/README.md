# dryer

dryer finds code that repeats in a similar shape. It compares each function or method with the others in the same language. It prints a report of the pairs that score at or above a threshold. It also writes a snapshot to `.metrics/dry.edn`. This crate is a Rust port of [dryer](https://github.com/unclebob/dryer) by Robert C. Martin. The port uses the MIT License. The port copies the original at commit `66ff6d21a42c04afcad89c78a80066176d1294b0` (see `parity/upstream.toml`). The port needs no Python at run time.

## Install

```sh
cargo install --locked --git https://github.com/madsmadsen/lagom-mode-skill dryer
```

`--locked` uses the versions in `Cargo.lock`. Without it, Cargo may take a newer grammar, and the scores can change.

You must have:

- The Rust stable toolchain and Cargo.
- Git 2.30 or later, for `--changed`.
- Network access during the build. The build fetches the Python grammar from Git. The crate pins `tree-sitter-python` to commit `26855eabccb19c6abf499fbc5b8dc7cc9ab8bc64`. The original's grammar bundle (`tree-sitter-language-pack` 1.25.0) uses this grammar shape (checked on 35 snippets and six real projects). The crates.io release 0.25.0 gives a different parse tree.

A checkout below a directory named `test`, `tests`, or `spec` gives the report `No source files to analyze.`. dryer skips those directory names at every level of the path, not only inside the project. Put the checkout in a directory with another name.

## Run

```sh
dryer
```

With no paths, dryer compares the source files under the project root. It prints the report. It writes `.metrics/dry.edn` under the project root.

Usage:

```text
dryer [options] [path-or-filter ...]
```

Options:

| Flag | Meaning |
| --- | --- |
| `-h`, `--help` | Print the help and exit. |
| `--root <path>` | Set the project root. dryer writes the metrics here. Default: the current directory. |
| `-s`, `--source-root <path>` | Walk this tree instead of the project root. You can repeat this flag. |
| `--changed` | Compare the added and modified source files from `git status`. |
| `--threshold N` | Set the minimum similarity score. Default: 0.82. |
| `--min-lines N` | Set the minimum source lines in a compared form. Default: 4. |
| `--min-nodes N` | Set the minimum normalized syntax nodes in a compared form. Default: 20. |
| `--format F` | Set the output format: `text` or `edn`. Default: `text`. |
| `--edn` | Same as `--format edn`. |
| `--text` | Same as `--format text`. |

Arguments:

- `path`: A file or directory to compare. A test file that you name stays in the scan. A test directory that you name yields no file.
- `filter`: When the argument is not a path, dryer compares only the source files whose path contains this text.

Languages: Clojure (`.clj .cljc .cljs .cljd .bb`), Java (`.java`), Go (`.go`), TypeScript (`.ts .tsx .mts .cts`), JavaScript (`.js .jsx .mjs .cjs`), Rust (`.rs`), and Python (`.py`). Dryer reads JavaScript files as language `typescript`. Clojure compares every top-level form except `ns`. The other languages compare functions and methods. A form compares only with forms in the same language.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | dryer wrote the report. |
| 1 | Usage error, or the snapshot could not be written. |
| 2 | Unknown output format. |
| 70 | The worker thread did not start, or it panicked. |
| Git code | With `--changed`, a Git command failed. |

A Git command that cannot start gives `1`, and so does a Git process that a signal kills. A file that dryer cannot read gives a warning on standard error, and the run goes on (DR-16).

The snapshot is `.metrics/dry.edn`. It uses EDN, the Clojure data format. The same files give the same snapshot on each run.

## Differences from the original and the PRD

- The port needs no Python at run time. Python runs only in the parity runner (see below).
- The file `parity/fixes.toml` lists each difference from the original. The port leaves a Rust function out when it is test code (DR-15). A function is test code when an enclosing `mod` is named `tests`, when an enclosing `mod` has `#[cfg(test)]`, or when the function has `#[test]` or `#[cfg(test)]`. The original skips a function only in a `mod` named `tests`.
- When the port cannot read a source file, it prints a warning and goes on (DR-16). The original stops with a Python traceback and exit code `1`.

## Parity

The parity runner compares the port with the pinned original on small fixture projects. The runner needs:

- Python 3.11 or later, with `tree-sitter` and `tree-sitter-language-pack` installed in one environment.
- The original dryer at the pinned commit. Without `--original`, the runner clones it into `parity/.cache/dryer-original`.
- A built `dryer` binary.

Set `DRYER_ORIGINAL_PYTHON` to the Python in the environment that has the original's dependencies:

```sh
DRYER_ORIGINAL_PYTHON=/path/to/venv/bin/python python3 parity/run_parity.py --port /path/to/dryer
```

Options for the runner:

- `--case NAME`: Run only this case. You can repeat this option.
- `--original DIR`: Use this checkout of the original.
- `--port BIN`: Use this `dryer` binary.
- `--record`: Write results to `parity/expected/<case>/`. Without `--case`, this removes the old results first. A case with a `fix` also records the port's own output in `parity/expected/<case>/port/`.

`--record` rewrites `parity/expected/<case>/port/` from the current binary. A regression in a fix case becomes the expected output. The owner must review each `port/` diff before the commit.

The runner runs each side five times and prints the median wall-clock time of each side (DR-NFR-2, AC-10). Run 1 gives the compared result. Runs 2 to 5 reuse the copy only for timing, and the runner removes `.metrics` before each of them. A timing run that exits unlike run 1 stops the runner, so a crashed run cannot lower the median. The error names the case, the side, and the stderr of the failed run. `--record` runs every case before it deletes `parity/expected/`, so a failed case keeps the old recordings.

`parity/test_run_parity.py` tests `run_side`, `record`, and `check` with fake programs and stubs (DR-NFR-2). It covers the result of run 1, the median of 5 samples, the cleanup of `.metrics`, a side without a snapshot, a side that always exits 1, a timing run that crashes, the error text of that crash, and the `--record` delete. Run it from `tools/dryer/`:

```sh
python3 -m unittest discover -s .
```

`cargo test` also runs it, with `python3.12`, in `tests/parity_python.rs`. That test skips with a note on a host that has no `python3.12`.

crapper runs `python3 -m unittest discover -s .` from the crate root. mutator runs `python3 -m unittest discover` there, with no `-s`, which finds the same tests from the top-level directory. `parity/__init__.py` makes the tests visible to both, because discovery descends only into packages.

A case with a `fix` in `coverage.toml` differs from the original on purpose. For such a case the runner compares the port with `parity/expected/<case>/port/`, not with the original. It also fails the case when the port no longer differs from the original. No entry in `parity/fixes.toml` is approved until Mads Madsen adds `approved_by`.

`cargo test` replays the recorded results in `parity/expected/`. It needs no Python. The test is `tests/parity_expected.rs`. It replays every case, and a fix case is checked against its `port/` recording.

## License

dryer uses the MIT License. See [LICENSE](LICENSE). Copyright (C) 2026 Mads Madsen. dryer is based on dryer by Robert C. Martin, which uses the BSD Zero Clause License.
