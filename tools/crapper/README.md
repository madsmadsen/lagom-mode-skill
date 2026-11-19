# crapper

Rust port of [crapper](https://github.com/unclebob/crapper) by Robert C. Martin. It scores each function with the CRAP metric `CRAP = CC² × (1 − coverage)³ + CC` for Clojure, Java, Go, TypeScript (and JavaScript), Rust, and Python. It needs no Python at run time.

Requirements: `docs/prd/crapper.md` (CR-*). License: MIT (`LICENSE`). Copyright (C) 2026 Mads Madsen. crapper is based on crapper by Robert C. Martin, which uses the BSD Zero Clause License.

## Install

```bash
cargo install --locked --git https://github.com/madsmadsen/lagom-mode-skill crapper
```

The port is published in a public repository on GitHub. It is not published to crates.io, so the install command takes the repository URL and the name `crapper`.

The build needs Rust stable and a C compiler. `--locked` makes Cargo use the committed `Cargo.lock`, so the grammar versions of a release do not move (CR-NFR-4).

## Use

Same command line as the original:

```bash
crapper                              # score every source file under the current directory
crapper --no-coverage                # complexity only; coverage and CRAP are nil
crapper --use-existing-coverage      # read coverage reports already on disk
crapper src/demo/core.clj src/ui     # these files and trees
crapper --changed                    # git additions and edits
crapper combat                       # path fragment filter
crapper --threshold 30               # exit 2 when the worst score is higher
crapper --lines 12,40,41             # score only the functions that hold these lines (CR-26)
crapper --coverage-command '<cmd>'   # run your own coverage command
crapper --root <path> -s <tree>      # project root and source trees
```

It prints a table on stdout (worst score first) and writes `.metrics/crap.edn`. Exit codes: `0` ok, `1` bad arguments or a file that cannot be read, `2` threshold exceeded. A Git failure under `--changed` returns the exit code of Git.

## Parity with the original

`parity/` holds the pin (`upstream.toml`), the fixtures, the runner (`run.py`), the recorded output of the original, and the list of approved differences (`fixes.toml`).

```bash
python3.12 parity/run.py             # clone the original at the pinned commit, run both, compare
python3.12 parity/run.py --check-meta
```
