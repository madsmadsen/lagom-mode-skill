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
cargo install --locked --git https://github.com/madsmadsen/lagom-mode-skill mutator
```

`--locked` uses the versions in `Cargo.lock`. Without it, Cargo may take a newer grammar, and the
sites can change. The repository is public on GitHub, and the crate is not published to crates.io.

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

`mutator --help` lists every flag. Flags, arguments, exit codes, and snapshots match the original,
apart from two flags. `--max-memory-mb` is a flag that only the port has (MU-25). `--max-workers`
is a flag that both have, but the port caps it at half the cores and uses that cap as the default
(MU-17):

| Exit code | Meaning |
| --- | --- |
| `0` | Every executed mutant was killed, or nothing ran. |
| `1` | Usage error. |
| `2` | The baseline tests failed. The snapshots keep their bytes. |
| `3` | At least one mutant survived. |
| other | The `--coverage-command` failed and returned its own code. stderr names the code. Also, `--changed` ran and Git failed: the port returns the code of Git, for example `128` outside a Git repository, and prints the message of Git on stderr. When Git cannot start at all, the port exits `1`, and that code is not a code of Git. |

The tool skips 22 directory names, the list of crapper's `discover.py`. The list holds `test`,
`tests`, `spec`, `specs`, `__tests__`, `vendor`, `node_modules`, `target`, and build output
directories such as `dist`, `build`, and `out`. MU-6 gives the whole list.

The port also drops any source file whose full path has a part named `test`, `tests`, `spec`,
`specs`, or `__tests__`. That rule reads the whole absolute path, not only the part below the
project root. So a directory that you name is walked, but the port drops every file in it. A
project whose own path holds one of these names selects no file: the run prints
`No source files to mutate.` and exits `0`. Work around it by moving or copying the project to a
path with no such part. A file that you name as the source root, with `--source-root <file>`, is
always taken, even when it is a test file.

## Differences from the original

The only differences are the entries of `parity/fixes.toml`. Each entry names its requirement
and its test. Mads Madsen approves each entry (MU-PB-6).

1. `second-run-near-timeout` (MU-21). A mutant that times out, or runs within 10% of its timeout,
   runs once more with 2 times the timeout. The second result decides.
2. `idempotent-tested-at` (MU-NFR-1). The `:tested-at` line of a snapshot changes only when another
   value in the snapshot changes, so repeated runs give byte-identical files.
3. `worker-cap-half-the-cores` (MU-17). The default worker count is half the cores, at least one,
   and `--help` says so. The original runs one mutant per core. No run uses more workers than
   half the cores, even when `--max-workers` asks for more.
4. `ts-dot-imports-are-relative` (MU-22, upstream issue 29). A TypeScript or JavaScript test that
   imports its module from `'.'` or `'..'` runs against the mutant. The original reports a killed
   mutant as SURVIVED.
5. `python-more-operators` (MU-23, upstream issue 10). Python files get sites for `is`, `is not`,
   `in`, `not in`, `//`, `%`, `**`, `@`, `+=`, `-=`, `*=`, and `/=`. The file holds more mutants.
6. `python-clause-coverage-line` (MU-24, upstream issue 9). A Python site in an `elif` condition,
   an `except` clause, or a `case` guard takes the first line of that clause. A site on a line
   that no test runs is an uncovered site.
7. `max-memory-limit` (MU-25). The port kills a mutant test run whose process group uses more than
   the limit, and counts the mutant as killed. The flag `--max-memory-mb N` sets the limit in MiB.
   The default is 4096, and 0 turns the limit off. The original has no limit and rejects the flag.
8. `rust-test-code-by-attribute` (MU-26). The port leaves out a Rust function in a `#[cfg(test)]`
   module, and a function with `#[test]` or `#[cfg(test)]`. The original leaves out only the
   functions of a `mod` named `tests`.

## Coverage installs and the network

MU-11: when a coverage tool is missing, the port installs it the same way as the original, and it
prints the install on stderr first. These commands use the network and write outside the project
tree:

| Language | Command | Writes |
| --- | --- | --- |
| Python | `<python> -m pip install --disable-pip-version-check coverage` | Site packages of the project interpreter. |
| Python | `<python> -m pip install --disable-pip-version-check pytest` | Site packages of the project interpreter, only when a pytest config exists and pytest does not import. |
| TypeScript | `npm install --no-save --no-package-lock @vitest/coverage-v8[@<version>]` | `node_modules`. The port restores `package.json` and the lockfile from a copy it takes first. |
| TypeScript | `npx --yes c8 ...` | The npm cache, on the first run. |
| Rust | `rustup component add llvm-tools-preview` | The toolchain, when `rustup` exists. |
| Rust | `cargo install cargo-llvm-cov --locked` | `~/.cargo/bin`. |

The port installs nothing else. In particular it installs no Python package into the project
requirements, and it changes no `Cargo.toml`.

## Restore from the backup

MU-18: before a run that is not a scan run, the port copies every file under
`target/mutator-backup/` back over the source file at the same relative path, and then removes the
backup file. This puts back a source file that an interrupted older run left mutated (MU-NFR-5). A
file under that directory overwrites its source file, so keep it out of Git and check it before a
run if another tool wrote it.

## Parity runner

`parity/run.py` runs the original and the port on one fixture project for each language and compares
the exit codes and the snapshots. See `parity/README.md`.

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

`cargo test` also runs `python3.12 -m unittest discover -s . -p 'test_*.py'` for
the parity runner through `tests/parity_map.rs`. That test skips with a note when
the host has no `python3.12` and no `python3` with `tomllib`. It kills the run
after 120 s, and it writes no bytecode into the crate. The Linux guests of
`ci/linux-vm/` install `python3.12`, so the parity tests run there. The results
line of `guest-test.sh` names the interpreter path that was found.

