# PRD: mutator

Status: Draft 1. Owner and approver: Mads Madsen. Date: 2026-11-07.

This document states the requirements for `mutator`, a Rust port of [mutator](https://github.com/unclebob/mutator) by Robert C. Martin. The port is one Cargo crate in the `mutator/` directory. It does not depend on another crate or tool in this repository. Each statement that comes from a guess and not from a source carries the label `[ASSUMPTION]`.

## 1. Problem

mutator makes small changes to the code and runs the tests after each change. A change that the tests do not detect shows code that the tests do not really check. The original is written in Python, and it needs a `../crapper` checkout next to it to find functions and to make coverage. A project in another language then needs a working Python setup and a second repository only to run mutator.

## 2. Definitions

| Term | Meaning |
| --- | --- |
| Original | The Python mutator at the commit pinned in `mutator/parity/upstream.toml`. |
| Port | The Rust mutator that this document specifies. |
| Mutant | A copy of the code with one small change, for example `<` to `<=`. |
| Site | A place in the code where mutator can make a mutant. |
| Killed | A mutant that makes the tests fail, time out, or not compile. |
| Scan run | A run that lists sites and runs no mutant. `[ASSUMPTION]` The original has such a mode. |
| Uncovered site | A site on a line that no test runs. |
| Snapshot | An EDN file under `.metrics/mutate/` that mutator writes. |
| EDN | Extensible Data Notation, the Clojure data format. |
| Parity fixture | A small project in one language that the original and the port both analyze. |

## 3. Goals

1. The port is a drop-in replacement for the original: same command name, same flags, same exit codes, same snapshots. The only differences are the bug fixes in `mutator/parity/fixes.toml` (section 7).
2. The port supports the six languages of the original: Clojure, Java, Go, TypeScript (with JavaScript), Rust, and Python.
3. The port needs no Python and no checkout of another repository at run time. A Python project still needs Python to run its own tests.

## 4. Non-goals

1. The port does not add languages, mutation rules, or flags that the original does not have.
2. The port does not change the mutation rules or the outcome rules of the original. A bug fix is not a change of rules.
3. The port is private. It is not published to crates.io.

## 5. Functional requirements

### 5.1 Command line and files

- MU-1: The port is a Rust binary named `mutator`.
- MU-2: The port accepts every flag and positional argument of the original, with the same meaning and the same default. It rejects each flag that the original rejects, with the same exit code.
- MU-3: The port returns the same exit code as the original for the same input.
- MU-4: The port writes one snapshot for each namespace to `.metrics/mutate/<namespace>.edn`, in the same EDN shape and with the same keys as the original.
- MU-5: The port detects the language of each file by its extension, the same as the original.
- MU-6: The port skips the directories `test`, `spec`, `vendor`, `node_modules`, and `target`, the same as the original.
- MU-7: The port supports `--changed` with the same Git selection of files as the original.
- MU-8: The port parses source with tree-sitter grammars for Java, Go, TypeScript, JavaScript, Rust, and Python, and with an EDN-aware reader for Clojure. `[ASSUMPTION]` The original uses tree-sitter for the same grammars.

### 5.2 Functions and coverage

The original imports code from a crapper checkout for the work in this section. The port has its own code for it.

- MU-9: The port finds and names functions and namespaces the same as the original.
- MU-10: The port makes coverage with the same coverage command for each language as the original. It reads the same coverage formats.
- MU-11: If a coverage tool is missing, the port installs it the same way as the original, and prints the install on stderr before it starts the install.

### 5.3 Mutation

- MU-12: The port applies the mutation rules for each language with the table in the README of the original, section "What each language mutates".
- MU-13: The port does not mutate tokens inside strings, comments, or quoted Clojure forms.
- MU-14: The port runs the baseline test command once before any mutant. If the baseline fails, the port exits `2` and does not change the snapshots.
- MU-15: The port counts a mutant as killed when the tests fail, time out, or do not compile. The timeout is the baseline time multiplied by `--timeout-factor` (default 10), with a minimum of 2 seconds.
- MU-16: The port skips sites on lines that the coverage report does not hit, and counts them as uncovered sites.
- MU-17: The port runs the mutants of one file in parallel worker directories under `target/mutation-workers`, with the overlay rules of the original. The source files of the project do not change.
- MU-18: Before each run that is not a scan run, the port restores the source files from `target/mutator-backup/`, the same as the original.
- MU-19: If a snapshot exists, the port runs differentially. It reruns the survivors and every site in a function whose text changed. `--mutate-all` reruns all sites.
- MU-20: The port exits `0` when every executed mutant is killed, `2` when the baseline fails, and `3` when a mutant survives.
- MU-21: When a mutant times out, or its test time is within 10% of its timeout, the port runs that mutant once more with 2 times the timeout. The outcome of this second run is the outcome of the mutant. This behavior differs from the original, and `mutator/parity/fixes.toml` lists it. `[ASSUMPTION: 10% and 2 times]`

## 6. Non-functional requirements

- MU-NFR-1: Determinism. Two runs on the same files give the same outcome for each mutant. A mutant near its timeout gets a stable outcome through MU-21.
- MU-NFR-2: Platforms. The port builds and runs on macOS 14 or later (arm64 and x86_64) and on Linux (x86_64 and arm64).
- MU-NFR-3: Install. A user with read access to the private repository installs the port with one `cargo install --git` command.
- MU-NFR-4: License. The port uses the MIT License. Each file that derives from the original or from the crapper code that the original imports names its source.
- MU-NFR-5: Crash safety. If a run stops because of a signal, the next run restores the source files from the backup (MU-18).

## 7. Parity and bug fixes

The port copies the behavior of the original at one pinned commit. A parity runner runs the original and the port on each parity fixture and compares the results. The runner gets the original and the crapper checkout that the original needs by a clone at the pinned commits, and runs the original with Python. The port fixes known bugs of the original and does not copy them.

- MU-PB-1: `mutator/parity/upstream.toml` records the pinned commit of the original and of the crapper checkout that the original needs.
- MU-PB-2: A change of a pinned commit needs a new parity run.
- MU-PB-3: Each bug fix has an entry in `mutator/parity/fixes.toml`. The entry gives the upstream issue or a description, the changed behavior, and the parity fixture that shows the difference.
- MU-PB-4: Each bug fix has a test that fails on the behavior of the original and passes on the port.
- MU-PB-5: A bug that comes from Python only needs no entry if the port cannot have it. An example is `bugs/python-tests-run-with-mutators-virtualenv.md` in the original.
- MU-PB-6: Mads Madsen approves each entry in `mutator/parity/fixes.toml`.

## 8. Acceptance criteria

1. AC-1 (MU-2, MU-3, MU-4): `mutator/parity/coverage.toml` maps every flag of the original and every row of the README table (MU-12) to at least one fixture run. For each entry, the original and the port give the same exit code and the same snapshots after both are parsed as EDN. A difference that `fixes.toml` lists does not count as a failure.
2. AC-2 (MU-5, MU-8): A parity fixture exists for each of the six languages.
3. AC-3 (MU-14): On a fixture with a failing test, the port exits `2`, and each snapshot file keeps its previous bytes.
4. AC-4 (MU-15): On a fixture with a mutant that times out and a mutant that does not compile, the port counts both as killed. With a baseline shorter than 0.2 seconds and `--timeout-factor 10`, the timeout is 2 seconds.
5. AC-5 (MU-17): After a run, `git status` in the fixture shows no change to the source files.
6. AC-6 (MU-18, MU-NFR-5): On a fixture with a pre-seeded `target/mutator-backup/` that differs from the source files, a run that is not a scan run restores the source files from the backup before it runs mutants.
7. AC-7 (MU-19): After a first run, the text of one function changes and the port runs again. The second run reruns only the survivors and the sites in the changed function. A run with `--mutate-all` reruns all sites.
8. AC-8 (MU-7): On a fixture with a modified, a staged, and an untracked source file, the original and the port, both run with `--changed`, select the same files and write the same snapshots.
9. AC-9 (MU-10, MU-11): On a Rust fixture on a host without `cargo-llvm-cov`, the port installs it, prints the install on stderr, and writes snapshots with coverage values.
10. AC-10 (MU-2): A flag that the original rejects is also rejected by the port, with the same exit code.
11. AC-11 (Goal 3): The fixtures for Clojure, Java, Go, TypeScript, and Rust pass with the port on a host that has no Python installed and no crapper checkout. The expected results come from an earlier parity run.
12. AC-12 (MU-PB-4): Each entry in `fixes.toml` has a test that names the entry and passes on the port.
13. AC-13 (MU-21, MU-NFR-1): On a fixture with a mutant whose test time is within 10% of its timeout, the port runs the mutant a second time with 2 times the timeout. Five runs on this fixture give the same outcome for each mutant and byte-identical snapshots.

## 9. Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| The original changes often. | The port drifts from the original. | Pin one commit (MU-PB-1). Update on purpose only (MU-PB-2). |
| The port copies the crapper code that the original imports. | The function names or the coverage of the port drift from the crapper code. | AC-1 compares with the original, which uses the pinned crapper code. |
| A tree-sitter grammar version in Rust differs from the one in Python. | Different parse trees give different sites or names. | Pin the grammar versions. AC-1 detects a difference. |
| Two mutator runs at the same time on one host slow each other down. | A survivor can time out and count as killed. | MU-21 runs each mutant near its timeout a second time with 2 times the timeout. AC-13 checks the outcome. |
| A backup from an old run is restored. | The restore overwrites later edits to the source files. | `[ASSUMPTION]` The port copies the behavior of the original. A fix needs an entry in `fixes.toml`. |
| A run on a large project takes a long time. | The run can hit the command timeout of an agent. | Use `--changed` or a list of files. |

## 10. Dependencies

1. Rust stable toolchain and Cargo on the build host.
2. Git 2.30 or later on each host that runs `--changed`.
3. For each language that a user analyzes: the test tools in the README of the original.
4. For parity runs only: Python, the original, and the crapper checkout that it needs, at the pinned commits.

## 11. Open questions

Each `[ASSUMPTION]` label in this document is an open question for the owner.
