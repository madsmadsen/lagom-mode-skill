# crapper

Rust port of [crapper](https://github.com/unclebob/crapper) by Robert C. Martin. It scores each function with the CRAP metric `CRAP = CC² × (1 − coverage)³ + CC` for Clojure, Java, Go, TypeScript (and JavaScript), Rust, and Python. It needs no Python at run time.

Requirements: `docs/prd/crapper.md` (CR-*). License: MIT (`LICENSE`).

## Install

```bash
cargo install --git <repository-url> crapper
```

`<repository-url>` is the private repository of `deterministic-coding`. The build needs Rust stable and a C compiler.

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
crapper --coverage-command '<cmd>'   # run your own coverage command
crapper --root <path> -s <tree>      # project root and source trees
```

It prints a table on stdout (worst score first) and writes `.metrics/crap.edn`. Exit codes: `0` ok, `1` bad arguments, `2` threshold exceeded.

## Parity with the original

`parity/` holds the pin (`upstream.toml`), the fixtures, the runner (`run.py`), the recorded output of the original, and the list of approved differences (`fixes.toml`).

```bash
python3.12 parity/run.py             # clone the original at the pinned commit, run both, compare
python3.12 parity/run.py --check-meta
```
