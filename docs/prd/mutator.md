# PRD: mutator

Status: Draft 1. Owner and approver: Mads Madsen. Date: 2026-11-07.

This document states the requirements for `mutator`, a Rust port of [mutator](https://github.com/unclebob/mutator) by Robert C. Martin. The port is one Cargo crate in the `tools/mutator/` directory. It does not depend on another crate or tool in this repository.

## 1. Problem

mutator makes small changes to the code and runs the tests after each change. A change that the tests do not detect shows code that the tests do not really check. The original is written in Python, and it needs a `../crapper` checkout next to it to find functions and to make coverage. A project in another language then needs a working Python setup and a second repository only to run mutator.

## 2. Definitions

| Term | Meaning |
| --- | --- |
| Original | The Python mutator at the commit pinned in `tools/mutator/parity/upstream.toml`. |
| Port | The Rust mutator that this document specifies. |
| Mutant | A copy of the code with one small change, for example `<` to `<=`. |
| Site | A place in the code where mutator can make a mutant. |
| Killed | A mutant that makes the tests fail, time out, or not compile. |
| Scan run | A run with `--scan` that lists sites and runs no mutant. |
| Uncovered site | A site on a line that no test runs. |
| Snapshot | An EDN file under `.metrics/mutate/` that mutator writes. |
| EDN | Extensible Data Notation, the Clojure data format. |
| Parity fixture | A small project in one language that the original and the port both analyze. |

## 3. Goals

1. The port is a drop-in replacement for the original: same command name, same flags, same exit codes, same snapshots. The only differences are the bug fixes in `tools/mutator/parity/fixes.toml` (section 7).
2. The port supports the six languages of the original: Clojure, Java, Go, TypeScript (with JavaScript), Rust, and Python.
3. The port needs no Python and no checkout of another repository at run time. A Python project still needs Python to run its own tests.

## 4. Non-goals

1. The port does not add languages, mutation rules, or flags that the original does not have. The upstream fixes in section 5.4 and the `--max-memory-mb` flag of MU-25 are the only exceptions.
2. The port does not change the mutation rules or the outcome rules of the original. A bug fix is not a change of rules. The upstream fixes in section 5.4 and the test code rule of MU-26 are the only exceptions.
3. The port is not published to crates.io. It is published in a public repository on GitHub.

## 5. Functional requirements

### 5.1 Command line and files

- MU-1: The port is a Rust binary named `mutator`.
- MU-2: The port accepts every flag and positional argument of the original, with the same meaning and the same default. It rejects each flag that the original rejects, with the same exit code. There are two exceptions. `--max-memory-mb` is a flag that the original rejects and the port accepts (MU-25). `--max-workers` is a flag that both accept, but the port caps it at half the cores and uses that cap as the default (MU-17).
- MU-3: The port returns the same exit code as the original for the same input.
- MU-4: The port writes one snapshot for each namespace to `.metrics/mutate/<namespace>.edn`, in the same EDN shape and with the same keys as the original.
- MU-5: The port detects the language of each file by its extension, the same as the original.
- MU-6: The port skips each directory with one of these names, at every level below the project root: `.git`, `.hg`, `.svn`, `.idea`, `.venv`, `venv`, `node_modules`, `vendor`, `target`, `dist`, `build`, `out`, `coverage`, `.metrics`, `testdata`, `__pycache__`, `.clj-kondo`, `test`, `tests`, `spec`, `specs`, and `__tests__`. The list is the list of `discover.py` of the crapper commit in `tools/mutator/parity/upstream.toml`. The port also leaves out a source file whose name has a test suffix, for example `_test.go`, `.test.ts`, or `.spec.ts`, and a Python file named `test_<name>.py`, `<name>_test.py`, or `conftest.py`. A file in one of these directories is not selected by `--changed` either. A directory that the user names is walked, even when its name is in the list. The port then drops each file in it as a test file, because the test-code check reads every part of the absolute path of the file, and one of those parts is the directory name. The check reads the whole absolute path, not only the part below the project root. So a project whose own path holds a directory named `test`, `tests`, `spec`, `specs`, or `__tests__` selects no source file, and the run prints `No source files to mutate.` and exits `0`. A file that the user names as a positional argument is taken unless it is a test file. A file that the user gives as the source root, with `--source-root <file>`, is always taken, even when it is a test file. The test-file check runs on the files that the port finds below a directory.
- MU-7: The port supports `--changed` with the same Git selection of files as the original.
- MU-8: The port parses source with tree-sitter grammars for Clojure, Java, Go, TypeScript, JavaScript, Rust, and Python, as the original does through the `tree-sitter-language-pack` code of crapper.

### 5.2 Functions and coverage

The original imports code from a crapper checkout for the work in this section. The port has its own code for it.

- MU-9: The port finds and names functions and namespaces the same as the original. The one exception is MU-26, which leaves out more Rust test code.
- MU-10: The port makes coverage with the same coverage command for each language as the original. It reads the same coverage formats.
- MU-11: If a coverage tool is missing, the port installs it the same way as the original, and prints the install on stderr before it starts the install.
- MU-26: In Rust files, the port leaves out each function that is test code. The port makes no site in such a function, and the function has no snapshot entry. A function is test code when at least one of these is true:
  1. An enclosing `mod` is named `tests`.
  2. An enclosing `mod` has the attribute `#[cfg(test)]`. The attribute has exactly the argument `test`.
  3. The function has the attribute `#[test]`.
  4. The function has the attribute `#[cfg(test)]`.

  The port reads the attributes that stand directly before the item, in the same run of attributes. White space inside an attribute does not matter. The port does not treat `#[cfg(all(test, ...))]`, `#[cfg(not(test))]`, or `#[tokio::test]` as test code. Code under `#[cfg(not(test))]` stays production code. The original leaves out only functions in a `mod` named `tests`. This behavior differs from the original, and `tools/mutator/parity/fixes.toml` lists it.

### 5.3 Mutation

- MU-12: The port applies the mutation rules for each language with the table in the README of the original, section "What each language mutates".
- MU-13: The port does not mutate tokens inside strings, comments, or quoted Clojure forms.
- MU-14: The port runs the baseline test command once before any mutant. If the baseline fails, the port exits `2` and does not change the snapshots.
- MU-15: The port counts a mutant as killed when the tests fail, time out, or do not compile. The timeout is the baseline time multiplied by `--timeout-factor` (default 10), with a minimum of 2 seconds.
- MU-16: The port skips sites on lines that the coverage report does not hit, and counts them as uncovered sites.
- MU-17: The port runs the mutants of one file in parallel worker directories under `target/mutation-workers`, with the overlay rules of the original. The source files of the project do not change. The number of workers is the smallest of three numbers: the number of selected sites, half the number of cores (at least one), and an explicit `--max-workers` value. The default is half the number of cores, at least one. Half the cores is a hard cap. A run with `--max-workers 10000` still uses half the cores, at least one. This behavior differs from the original, and `tools/mutator/parity/fixes.toml` lists it.
- MU-18: Before each run that is not a scan run, the port restores the source files from `target/mutator-backup/`, the same as the original.
- MU-19: If a snapshot exists, the port runs differentially. It reruns the survivors and every site in a function whose text changed. `--mutate-all` reruns all sites.
- MU-20: The port exits `0` when every executed mutant is killed, `2` when the baseline fails, and `3` when a mutant survives. The port also exits `0` when it runs no mutant, for example when no source file is selected. It exits `1` for a usage error, and it prints the help text on stderr. When `--coverage-command` fails, the port exits with the code of that command and names the code on stderr. When `--changed` runs and Git fails, the port exits with the code of Git, for example `128` outside a Git repository, and prints the message of Git on stderr. When Git cannot start at all, for example when `git` is not on `PATH`, the port exits `1` and prints the error of the system. That code is not a code of Git.
- MU-21: When a mutant times out, or its test time is within 10% of its timeout, the port runs that mutant once more with 2 times the timeout. The outcome of this second run is the outcome of the mutant. This behavior differs from the original, and `tools/mutator/parity/fixes.toml` lists it.
- MU-25: The port limits the memory of each mutant test run. The limit is the total resident memory of all processes in the process group of the test command. The default limit is 4096 MiB. The flag `--max-memory-mb N` sets the limit to `N` MiB, and `--max-memory-mb 0` turns the limit off. The port rejects a value that is not a whole number of 0 or more, with exit code `1`. While a mutant test run waits, the port reads the memory of the process group at least once each second. If the memory is over the limit, the port kills the process group with `SIGKILL`, and the mutant counts as killed. The baseline run has no memory limit. MU-21 does not run a mutant again because of the memory limit. This behavior differs from the original, and `tools/mutator/parity/fixes.toml` lists it.

### 5.4 Upstream fixes

Each requirement in this section comes from an open issue of the original, and differs from the original at the pinned commit. Each one has an entry in `tools/mutator/parity/fixes.toml` (MU-PB-3) and a test (MU-PB-4).

- MU-22: In TypeScript and JavaScript, the port treats the import specifiers `'.'` and `'..'` as relative imports ([issue 29](https://github.com/unclebob/mutator/issues/29)). It resolves each one to the `index.<ext>` file in that directory. If that file is the mutated file, the port copies the test file into the worker, so the test loads the mutant.
- MU-23: In Python, the port makes these mutants ([issue 10](https://github.com/unclebob/mutator/issues/10)):
  1. `is` to `is not`, and `is not` to `is`.
  2. `in` to `not in`, and `not in` to `in`.
  3. `//` to `/`, and `/` to `//`.
  4. `%` to `*`.
  5. `**` to `*`.
  6. `@` to `*`.
  7. `+=` to `-=`, and `-=` to `+=`.
  8. `*=` to `/=`, and `/=` to `*=`.
- MU-24: In Python, the coverage line of a site in an `elif` condition, in an `except` clause, or in a `case` guard is the first line of that clause ([issue 9](https://github.com/unclebob/mutator/issues/9)). The port does not use the first line of the enclosing `if`, `try`, or `match` statement. A site on a clause line that no test runs is an uncovered site (MU-16).

## 6. Non-functional requirements

- MU-NFR-1: Determinism. Two runs on the same files give the same outcome for each mutant. One exception is a mutant that the memory limit kills (MU-25). That outcome can depend on timing, because the port reads the memory of the process group at least once each second (MU-25) while the test run waits, and a test run that ends before the next read is not killed. A mutant near its timeout gets a stable outcome through MU-21. A mutant that the memory limit kills does not run a second time (MU-25). The `:tested-at` value of a snapshot changes only when another value in that snapshot changes. When the new content of a snapshot equals the file apart from `:tested-at`, the port keeps the file as it is. The `:tested-at` value is the behavior that differs from the original, and `tools/mutator/parity/fixes.toml` lists it.
- MU-NFR-2: Platforms. The port builds and runs on macOS 14 or later (arm64 and x86_64) and on Linux (x86_64 and arm64).
- MU-NFR-3: Install. The port is published in a public repository on GitHub. It is not published to crates.io. A user installs the port with one command: `cargo install --locked --git https://github.com/madsmadsen/lagom-mode-skill mutator`.
- MU-NFR-4: License. The port uses the MIT License. `tools/mutator/LICENSE` names Mads Madsen (`mads@onmadsen.com`) as the copyright holder. It also says that the port is based on mutator by Robert C. Martin, which uses the BSD Zero Clause License. Each `.rs` file starts with the lines `// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>` and `// SPDX-License-Identifier: MIT`. Each file that matches a file of the original, or of the crapper code that the original imports, names that file.
- MU-NFR-5: Crash safety. If a run stops because of a signal, the next run restores the source files from the backup (MU-18).

## 7. Parity and bug fixes

The port copies the behavior of the original at one pinned commit. A parity runner runs the original and the port on each parity fixture and compares the results. The runner gets the original and the crapper checkout that the original needs by a clone at the pinned commits, and runs the original with Python. The port fixes known bugs of the original and does not copy them.

- MU-PB-1: `tools/mutator/parity/upstream.toml` records the pinned commit of the original and of the crapper checkout that the original needs.
- MU-PB-2: A change of a pinned commit needs a new parity run.
- MU-PB-3: Each bug fix has an entry in `tools/mutator/parity/fixes.toml`. The entry gives the upstream issue or a description, the changed behavior, and the parity fixture that shows the difference.
- MU-PB-4: Each bug fix has a test that fails on the behavior of the original and passes on the port.
- MU-PB-5: A bug that comes from Python only needs no entry if the port cannot have it. An example is `bugs/python-tests-run-with-mutators-virtualenv.md` in the original.
- MU-PB-6: Mads Madsen approves each entry in `tools/mutator/parity/fixes.toml`.

## 8. Acceptance criteria

1. AC-1 (MU-2, MU-3, MU-4): `tools/mutator/parity/coverage.toml` maps every flag of the original and every row of the README table (MU-12) to at least one fixture run. For each entry, the original and the port give the same exit code and the same snapshots after both are parsed as EDN. A difference that `fixes.toml` lists does not count as a failure.
2. AC-2 (MU-5, MU-8): A parity fixture exists for each of the six languages.
3. AC-3 (MU-14): On a fixture with a failing test, the port exits `2`, and each snapshot file keeps its previous bytes.
4. AC-4 (MU-15): On a fixture with a mutant that times out and a mutant that does not compile, the port counts both as killed. With a baseline shorter than 0.2 seconds and `--timeout-factor 10`, the timeout is 2 seconds.
5. AC-5 (MU-17): After a run, `git status` in the fixture shows no change to the source files.
6. AC-6 (MU-18, MU-NFR-5): On a fixture with a pre-seeded `target/mutator-backup/` that differs from the source files, a run that is not a scan run restores the source files from the backup before it runs mutants.
7. AC-7 (MU-19): After a first run, the text of one function changes and the port runs again. The second run reruns only the survivors and the sites in the changed function. A run with `--mutate-all` reruns all sites.
8. AC-8 (MU-7): On a fixture with a modified, a staged, and an untracked source file, the original and the port, both run with `--changed`, select the same files and write the same snapshots.
9. AC-9 (MU-10, MU-11): On a Rust fixture on a host without `cargo-llvm-cov`, the port installs it, prints the install on stderr, and writes snapshots with coverage values.
10. AC-10 (MU-2): A flag that the original rejects is rejected by the port, with the same exit code. The one exception is `--max-memory-mb`, which the original rejects and the port accepts (MU-25).
11. AC-11 (Goal 3): The fixtures for Clojure, Java, Go, TypeScript, and Rust pass with the port on a host that has no Python installed and no crapper checkout. The expected results come from an earlier parity run.
12. AC-12 (MU-PB-4): Each entry in `fixes.toml` has a test that names the entry and passes on the port.
13. AC-13 (MU-21, MU-NFR-1): On a fixture with a mutant whose test time is within 10% of its timeout, the port runs the mutant a second time with 2 times the timeout. Five runs on this fixture give the same outcome for each mutant and byte-identical snapshots, so the `:tested-at` value stays the same.
14. AC-14 (MU-22): On the TypeScript fixture of issue 29, where `src/calc/index.test.ts` imports `big` from `'.'`, the mutant `>` to `>=` on line 2 of `src/calc/index.ts` is killed.
15. AC-15 (MU-23): A scan run on the file `src/demo/ops2.py` from issue 10 lists a site for each of the operators `is`, `in`, `@`, `//`, `%`, `**`, `+=`, and `+`. A scan run on a file with `is not`, `not in`, `-=`, `*=`, and `/=` lists a site for each of them.
16. AC-16 (MU-24): On the fixture of issue 9, where the tests cover line 2 and do not cover lines 4 and 5, the two sites on line 4 are uncovered sites and the site on line 2 runs. The same rule holds for a site in an `except` clause and in a `case` guard whose line no test runs.
17. AC-17 (MU-25): A fixture has a mutant that makes the tests allocate memory without end, and a timeout of more than 60 seconds. The port, run with `--max-memory-mb 256`, counts the mutant as killed within 10 seconds of the start of its test run, and runs it only once. A run with `--max-memory-mb 0` has no memory limit. `--max-memory-mb -1` and `--max-memory-mb abc` exit `1`.
18. AC-18 (MU-26): A Rust file has a production function, a `#[cfg(test)]` module named `unit_tests` with a `#[test]` function, a `#[test]` function in a plain module, and a function with `#[cfg(test)]`. A scan run lists sites for the production function only. A scan run still lists the sites of a function in a `#[cfg(not(test))]` module, and of a normal function next to a `mod tests`.
19. AC-19 (MU-17): A run with `--max-workers 10000` uses half the cores, at least one. The test `tools/mutator/tests/worker_cap.rs` checks the worker count and the help text.

## 9. Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| The original changes often. | The port drifts from the original. | Pin one commit (MU-PB-1). Update on purpose only (MU-PB-2). |
| The port copies the crapper code that the original imports. | The function names or the coverage of the port drift from the crapper code. | AC-1 compares with the original, which uses the pinned crapper code. |
| A tree-sitter grammar version in Rust differs from the one in Python. | Different parse trees give different sites or names. | Pin the grammar versions. AC-1 detects a difference. |
| Two mutator runs at the same time on one host slow each other down. | A survivor can time out and count as killed. | MU-21 runs each mutant near its timeout a second time with 2 times the timeout. AC-13 checks the outcome. |
| A mutant makes the tests allocate memory without end. | The host runs out of memory before the timeout kills the mutant. | MU-25 kills the test run when its memory goes over the limit. AC-17 checks it. |
| A backup from an old run is restored. | Before each run that is not a scan run, the original restores every file under `target/mutator-backup/`. The restore overwrites later edits to the source files. | The port copies the behavior of the original. A fix needs an entry in `fixes.toml`. |
| A run on a large project takes a long time. | The run can hit the command timeout of an agent. | Use `--changed`, `--lines`, or a list of files. |

## 10. Dependencies

1. Rust stable toolchain and Cargo on the build host.
2. Git 2.30 or later on each host that runs `--changed`.
3. For each language that a user analyzes: the test tools in the README of the original.
4. For parity runs only: Python, the original, and the crapper checkout that it needs, at the pinned commits.

## 11. Open questions

None.
