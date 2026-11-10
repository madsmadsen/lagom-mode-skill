# PRD: dryer

Status: Draft 1. Owner and approver: Mads Madsen. Date: 2026-11-07.

This document states the requirements for `dryer`, a Rust port of [dryer](https://github.com/unclebob/dryer) by Robert C. Martin. The port is one Cargo crate in the `dryer/` directory. It does not depend on another crate or tool in this repository.

## 1. Problem

dryer finds pairs of code forms that have the same structure, so that a human or an agent can remove duplicated code. The original is written in Python. A project in another language then needs a working Python setup only to run dryer.

## 2. Definitions

| Term | Meaning |
| --- | --- |
| Original | The Python dryer at the commit pinned in `dryer/parity/upstream.toml`. |
| Port | The Rust dryer that this document specifies. |
| Form | A unit of code that dryer compares, for example a function. |
| Candidate pair | Two forms with a similarity score at or above the threshold. |
| Snapshot | The EDN file `.metrics/dry.edn` that dryer writes. |
| EDN | Extensible Data Notation, the Clojure data format. |
| Parity fixture | A small project in one language that the original and the port both analyze. |

## 3. Goals

1. The port is a drop-in replacement for the original: same command name, same flags, same exit codes, same snapshot. The only differences are the bug fixes in `dryer/parity/fixes.toml` (section 7).
2. The port supports the six languages of the original: Clojure, Java, Go, TypeScript (with JavaScript), Rust, and Python.
3. The port needs no Python at run time.

## 4. Non-goals

1. The port does not add languages, metrics, or flags that the original does not have.
2. The port does not change the scoring or the defaults of the original. A bug fix is not a change of scoring.
3. The port is private. It is not published to crates.io.

## 5. Functional requirements

- DR-1: The port is a Rust binary named `dryer`.
- DR-2: The port accepts every flag and positional argument of the original, with the same meaning and the same default. It rejects each flag that the original rejects, with the same exit code.
- DR-3: The port returns the same exit code as the original for the same input.
- DR-4: The port writes `.metrics/dry.edn` in the same EDN shape and with the same keys as the original.
- DR-5: The port detects the language of each file by its extension, the same as the original.
- DR-6: The port skips the directories `test`, `spec`, `vendor`, `node_modules`, and `target`, the same as the original.
- DR-7: The port supports `--changed` with the same Git selection of files as the original.
- DR-8: The port parses source with tree-sitter grammars for Java, Go, TypeScript, JavaScript, Rust, and Python, as the original does with `tree-sitter-language-pack`. For Clojure, the port copies the Clojure reader of the original (`clojure_read.py`).
- DR-9: The port normalizes each form with the rules of the original. It keeps call names, method names, operators, and tree shape. It drops local names, field names, literals, and keywords.
- DR-10: The port scores each pair with the Jaccard similarity of structural fingerprints. It compares forms only within one language.
- DR-11: The port selects the forms for each language with the table in the README of the original, section "What each language compares".
- DR-12: The port supports `--threshold` (default 0.82), `--min-lines` (default 4), `--min-nodes` (default 20), `--format`, `--edn`, and `--text`.
- DR-13: The port exits `0` after a report, `1` on a usage error, and `2` on an unknown `--format`.

## 6. Non-functional requirements

- DR-NFR-1: Determinism. Two runs on the same files write byte-identical snapshots.
- DR-NFR-2: Speed. On each parity fixture, the median wall-clock time of 5 runs of the port is lower than the median wall-clock time of 5 runs of the original. Both run on one named CI runner: Linux x86_64, Ubuntu 22.04. The time of the original includes Python start-up and excludes the clone and the install.
- DR-NFR-3: Platforms. The port builds and runs on macOS 14 or later (arm64 and x86_64) and on Linux (x86_64 and arm64).
- DR-NFR-4: Install. A user with read access to the private repository installs the port with one `cargo install --git` command.
- DR-NFR-5: License. The port uses the MIT License. Each file that derives from the original names its source.

## 7. Parity and bug fixes

The port copies the behavior of the original at one pinned commit. A parity runner runs the original and the port on each parity fixture and compares the results. The runner gets the original by a clone at the pinned commit and runs it with Python. The port fixes known bugs of the original and does not copy them.

- DR-PB-1: `dryer/parity/upstream.toml` records the pinned commit of the original.
- DR-PB-2: A change of the pinned commit needs a new parity run.
- DR-PB-3: Each bug fix has an entry in `dryer/parity/fixes.toml`. The entry gives the upstream issue or a description, the changed behavior, and the parity fixture that shows the difference.
- DR-PB-4: Each bug fix has a test that fails on the behavior of the original and passes on the port.
- DR-PB-5: A bug that comes from Python only needs no entry if the port cannot have it.
- DR-PB-6: Mads Madsen approves each entry in `dryer/parity/fixes.toml`.

## 8. Acceptance criteria

1. AC-1 (DR-2, DR-3, DR-4): `dryer/parity/coverage.toml` maps every flag of the original and every row of the README table (DR-11) to at least one fixture run. For each entry, the original and the port give the same exit code and the same snapshot after both are parsed as EDN. A difference that `fixes.toml` lists does not count as a failure.
2. AC-2 (DR-5, DR-8): A parity fixture exists for each of the six languages.
3. AC-3 (DR-13): `dryer --format xml` exits `2`.
4. AC-4 (DR-7): On a fixture with a modified, a staged, and an untracked source file, the original and the port, both run with `--changed`, select the same files and write the same snapshot.
5. AC-5 (DR-10, DR-12): On a fixture with two forms that score 0.85, `dryer --threshold 0.8` reports the pair and `dryer --threshold 0.9` does not.
6. AC-6 (DR-2): A flag that the original rejects is also rejected by the port, with the same exit code.
7. AC-7 (DR-NFR-1): Two runs on each fixture write byte-identical snapshots.
8. AC-8 (Goal 3): All fixtures pass with the port on a host that has no Python installed. The expected results come from an earlier parity run.
9. AC-9 (DR-PB-4): Each entry in `fixes.toml` has a test that names the entry and passes on the port.
10. AC-10 (DR-NFR-2): On each parity fixture, the median wall-clock time of the port is lower than the median wall-clock time of the original, measured as DR-NFR-2 defines.

## 9. Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| The original changes often. | The port drifts from the original. | Pin one commit (DR-PB-1). Update on purpose only (DR-PB-2). |
| A tree-sitter grammar version in Rust differs from the one in Python. | Different parse trees give different fingerprints. | Pin the grammar versions. AC-1 detects a difference. |
| The Clojure reader in the original is custom Python code. | Edge cases of the reader are hard to copy. | The Clojure fixture covers reader conditionals, syntax-quote, and keyword calls. |
| Floats print in a different way in Rust and Python. | Snapshots differ in the last digits, for example `0.890909090909`. | AC-1 compares parsed EDN. The port copies the number format of the original. |

## 10. Dependencies

1. Rust stable toolchain and Cargo on the build host.
2. Git 2.30 or later on each host that runs `--changed`.
3. For parity runs only: Python and the original at the pinned commit.

## 11. Open questions

None.
