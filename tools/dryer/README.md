# dryer

dryer finds code that repeats in a similar shape. It compares each function or method with the others in the same language. It prints a report of the pairs that score at or above a threshold. It also writes a snapshot to `.metrics/dry.edn`. This crate is a Rust port of [dryer](https://github.com/unclebob/dryer) by Robert C. Martin. The port uses the MIT License. The port copies the original at commit `66ff6d21a42c04afcad89c78a80066176d1294b0` (see `parity/upstream.toml`). The port needs no Python at run time.

## Install

```sh
cargo install --git https://github.com/madsmadsen/lagom-mode-skill dryer
```

You must have:

- The Rust stable toolchain and Cargo.
- Git 2.30 or later, for `--changed`.
- Network access during the build. The build fetches the Python grammar from Git. The crate pins `tree-sitter-python` to commit `26855eabccb19c6abf499fbc5b8dc7cc9ab8bc64`. The original's grammar bundle (`tree-sitter-language-pack` 1.25.0) uses this grammar shape (checked on 35 snippets and six real projects). The crates.io release 0.25.0 gives a different parse tree.

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

- `path`: A file or directory to compare. Test paths are included when you name them.
- `filter`: When the argument is not a path, dryer compares only the source files whose path contains this text.

Languages: Clojure (`.clj .cljc .cljs .cljd .bb`), Java (`.java`), Go (`.go`), TypeScript (`.ts .tsx .mts .cts`), Rust (`.rs`), and Python (`.py`). Clojure compares every top-level form except `ns`. The other languages compare functions and methods. A form compares only with forms in the same language.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | dryer wrote the report. |
| 1 | Usage error. |
| 2 | Unknown output format. |

The snapshot is `.metrics/dry.edn`. It uses EDN, the Clojure data format. The same files give the same snapshot on each run.

## Differences from the original and the PRD

- The port follows the original. It does not support `.js` files. The PRD (`docs/prd/dryer.md`, DR-5 and DR-8) names JavaScript. This is a wording mismatch in the PRD. [ASSUMPTION] The owner must decide whether to keep the original's behavior or change the PRD.
- The port follows the original's skipped directory list. The PRD (DR-6) names a shorter list: `test`, `spec`, `vendor`, `node_modules`, and `target`. The port skips these directories: `.clj-kondo`, `.git`, `.hg`, `.idea`, `.metrics`, `.svn`, `.venv`, `__pycache__`, `__tests__`, `build`, `coverage`, `dist`, `node_modules`, `out`, `spec`, `specs`, `target`, `test`, `testdata`, `tests`, `vendor`, and `venv`. The port also skips `*_test.go`, `*.test.cts`, `*.spec.cts`, `*.spec.ts`, `test_*.py`, and `*_test.py` files. The PRD wording must match this list. [ASSUMPTION] The owner must confirm this.
- The port needs no Python at run time. Python runs only in the parity runner (see below).
- The file `parity/fixes.toml` has no entries. The port has no approved behavior change from the original.

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
- `--record`: Run only the original. Write the results to `parity/expected/<case>/`. Without `--case`, this removes the old results first.

`cargo test` replays the recorded results in `parity/expected/`. It needs no Python. The test is `tests/parity_expected.rs`.

## License

dryer uses the MIT License. See [LICENSE](LICENSE). Copyright (C) 2026 Mads Madsen. dryer is based on dryer by Robert C. Martin, which uses the BSD Zero Clause License.
