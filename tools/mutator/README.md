# mutator

Mutation testing for Clojure, Java, Go, TypeScript (and JavaScript), Rust, and Python. This is a Rust
port of [mutator](https://github.com/unclebob/mutator) by Robert C. Martin. It needs no Python and no
`crapper` checkout. It writes the same `.metrics/mutate/<namespace>.edn` snapshots that
[uml-viewer](https://github.com/unclebob/uml-viewer) reads.

License: MIT License (see `LICENSE`). Copyright (C) 2026 Mads Madsen. mutator is based on
mutator by Robert C. Martin, which uses the BSD Zero Clause License. For the parity checks, each source file
names the file of the original, or of crapper, that it matches.

## Install

```bash
cargo install --git https://github.com/madsmadsen/lagom-mode-skill mutator
```

## Run

```bash
mutator                              # every source file under the current directory
mutator src/demo/core.clj            # one file or one directory
mutator --scan src/demo              # list sites, run no tests
mutator --changed                    # added and modified files from git status
mutator --lines 12,40,41 src/demo.rs # only the mutations on these lines
mutator --mutate-all                 # rerun killed mutants too
mutator --no-coverage                # treat every site as covered
mutator --test-command "pytest -q tests/test_demo.py"
mutator --max-workers 3 src/demo/core.clj
mutator --max-memory-mb 1024         # kill a mutant test run over 1024 MiB (default 4096, 0 = off)
```

`mutator --help` lists every flag. Flags, arguments, exit codes, and snapshots match the original:

| Exit code | Meaning |
| --- | --- |
| `0` | Every executed mutant was killed, or nothing ran. |
| `1` | Usage error. |
| `2` | The baseline tests failed. The snapshots keep their bytes. |
| `3` | At least one mutant survived. |

The tool skips the directories `test`, `spec`, `vendor`, `node_modules`, `target`, and the other
directories that crapper skips.

## Differences from the original

The only differences are in `parity/fixes.toml`:

1. A mutant that times out, or runs within 10% of its timeout, runs once more with 2 times the
   timeout. The second result decides (MU-21).
2. The `:tested-at` line of a snapshot changes only when another value in the snapshot changes, so
   repeated runs give byte-identical files (MU-NFR-1).

## Parity runner

`parity/run.py` runs the original and the port on one fixture project for each language and compares
the exit codes and the snapshots. See `parity/README.md`.

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
