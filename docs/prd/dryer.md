# PRD: dryer

Status: Draft 1. Owner and approver: Mads Madsen. Date: 2026-11-07.

This document states the requirements for `dryer`, a Rust port of [dryer](https://github.com/unclebob/dryer) by Robert C. Martin. The port is one Cargo crate in the `tools/dryer/` directory. It does not depend on another crate or tool in this repository.

## 1. Problem

dryer finds pairs of code forms that have the same structure, so that a human or an agent can remove duplicated code. The original is written in Python. A project in another language then needs a working Python setup only to run dryer.

## 2. Definitions

| Term | Meaning |
| --- | --- |
| Original | The Python dryer at the commit pinned in `tools/dryer/parity/upstream.toml`. |
| Port | The Rust dryer that this document specifies. |
| Form | A unit of code that dryer compares, for example a function. |
| Candidate pair | Two forms with a similarity score at or above the threshold. |
| Snapshot | The EDN file `.metrics/dry.edn` that dryer writes. |
| EDN | Extensible Data Notation, the Clojure data format. |
| Parity fixture | A small project in one language that the original and the port both analyze. |

## 3. Goals

1. The port is a drop-in replacement for the original: same command name, same flags, same exit codes, same snapshot. The only differences are the entries in `tools/dryer/parity/fixes.toml` (section 7): bug fixes and the other differences that DR-PB-3 lists.
2. The port supports the six languages of the original: Clojure, Java, Go, TypeScript (with JavaScript), Rust, and Python.
3. The port needs no Python at run time.

## 4. Non-goals

1. The port does not add languages, metrics, or flags that the original does not have.
2. The port does not change the scoring or the defaults of the original. A bug fix is not a change of scoring.
3. The port is not published to crates.io. It is published in a public repository on GitHub.

## 5. Functional requirements

- DR-1: The port is a Rust binary named `dryer`.
- DR-2: The port accepts every flag and positional argument of the original, with the same meaning and the same default. It rejects each flag that the original rejects, with the same exit code.
- DR-3: The port returns the same exit code as the original for the same input.
- DR-4: The port writes `.metrics/dry.edn` in the same EDN shape and with the same keys as the original.
- DR-5: The port detects the language of each file by its extension, the same as the original, except for the change in DR-14.
- DR-6: The port skips the same directories as the original. The walk does not enter a directory with one of these 22 names: `.clj-kondo`, `.git`, `.hg`, `.idea`, `.metrics`, `.svn`, `.venv`, `__pycache__`, `__tests__`, `build`, `coverage`, `dist`, `node_modules`, `out`, `spec`, `specs`, `target`, `test`, `testdata`, `tests`, `vendor`, and `venv`. The port leaves out a file when its name is `conftest.py`, when its name starts with `test_` and ends with `.py`, when its name ends with one of these 23 test suffixes, or when a part of its path is one of the five test directory names `test`, `tests`, `spec`, `specs`, and `__tests__`. The 23 test suffixes are `_test.go`, `_test.clj`, `_test.cljc`, `_test.cljs`, `_test.cljd`, `_test.bb`, `.test.ts`, `.spec.ts`, `.test.tsx`, `.spec.tsx`, `.test.mts`, `.spec.mts`, `.test.cts`, `.spec.cts`, `.test.js`, `.spec.js`, `.test.jsx`, `.spec.jsx`, `.test.mjs`, `.spec.mjs`, `.test.cjs`, `.spec.cjs`, and `_test.py`. A source file that the user names stays in the scan, even when its name is a test name or its path holds a test directory. A directory that the user names is walked, so the 22 names stop the walk only below that directory. Each file below a named directory follows the rules above. A named `tests` directory gives no file, because each path below it holds the name `tests`. A named `node_modules` directory gives its source files, because the walk starts inside it. `--changed` applies the same directory names and the same test rules to each changed file. A file that `--changed` selects still belongs to the project root.
- DR-7: The port supports `--changed` with the same Git selection of files as the original.
- DR-8: The port parses source with tree-sitter grammars for Java, Go, TypeScript, JavaScript, Rust, and Python, as the original does with `tree-sitter-language-pack`. For Clojure, the port copies the Clojure reader of the original (`clojure_read.py`).
- DR-9: The port normalizes each form with the rules of the original. It keeps call names, method names, operators, and tree shape. It drops local names, field names, literals, and keywords.
- DR-10: The port scores each pair with the Jaccard similarity of structural fingerprints. It compares forms only within one language.
- DR-11: The port selects the forms for each language with the table in the README of the original, section "What each language compares". DR-15 changes this for Rust test code.
- DR-12: The port supports `--threshold` (default 0.82), `--min-lines` (default 4), `--min-nodes` (default 20), `--format`, `--edn`, and `--text`.
- DR-13: The port exits `0` after a report, `1` on a usage error, and `2` on an unknown `--format`. It also exits `70` when the worker thread does not start or when that thread panics. With `--changed`, a Git failure gives the exit code of Git. A Git command that cannot start gives `1`. A snapshot that cannot be written gives `1`.

### 5.1 Upstream fixes

Each requirement in this section comes from an open issue or pull request of the original, and differs from the original at the pinned commit. Each one has an entry in `tools/dryer/parity/fixes.toml` (DR-PB-3) and a test (DR-PB-4).

- DR-14: The port reads `.js`, `.jsx`, `.mjs`, and `.cjs` files as language `typescript` ([issue 17](https://github.com/unclebob/dryer/issues/17), [PR 16](https://github.com/unclebob/dryer/pull/16)). It parses these files with the JavaScript grammar, which also parses JSX. It treats a file whose name ends in `.test.js`, `.spec.js`, `.test.jsx`, `.spec.jsx`, `.test.mjs`, `.spec.mjs`, `.test.cjs`, or `.spec.cjs` as a test file and does not scan it. A `.d.ts` file stays out of the scan. The help text of the port names JavaScript and lists these four extensions, so a user can see that dryer reads them.

### 5.2 Other differences from the original

Each requirement in this section differs from the original at the pinned commit and has no upstream issue or pull request. Each one has an entry in `tools/dryer/parity/fixes.toml` (DR-PB-3, DR-PB-7) and a test (DR-PB-4).

- DR-15: The port leaves a Rust function out of the scan when it is test code. A function is test code when at least one of these four rules holds. Rule 1: an enclosing `mod` is named `tests`. Rule 2: an enclosing `mod` has the attribute `#[cfg(test)]`. Rule 3: the function has the attribute `#[test]`. Rule 4: the function has the attribute `#[cfg(test)]`. An attribute counts only when it is exactly `cfg` with the single argument `test`, or exactly `test`. It must stand before the item with no item between them. Only other attributes and comments may stand between the attribute and the item. A comment includes both `//` and `/* */`. The port does not match `cfg(all(test, ...))`, `cfg(not(test))`, or `tokio::test`. Code with `#[cfg(not(test))]` stays production code. A `#[cfg(test)]` impl block is not a module, so the port scans its functions. The original skips a Rust function only by rule 1.
- DR-27: The port reads each selected source file. When it cannot read a file, it prints a warning on standard error with the file name and the reason, skips that file, and goes on with the other files. The report and the snapshot cover the files that it could read. The exit code stays `0`. The original stops with a Python traceback instead.

## 6. Non-functional requirements

- DR-NFR-1: Determinism. Two runs on the same files write byte-identical snapshots.
- DR-NFR-2: Speed. On each parity fixture, the median wall-clock time of 5 runs of the port is lower than the median wall-clock time of 5 runs of the original. Both run on one named CI runner: Linux x86_64, Ubuntu 22.04. The time of the original includes Python start-up and excludes the clone and the install.
- DR-NFR-3: Platforms. The port builds and runs on macOS 14 or later (arm64 and x86_64) and on Linux (x86_64 and arm64).
- DR-NFR-4: Install. The repository is public on GitHub. It is not on crates.io. A user installs the port with one `cargo install --locked --git <repo>` command.
- DR-NFR-5: License. The port uses the MIT License. `tools/dryer/LICENSE` names Mads Madsen (`mads@onmadsen.com`) as the copyright holder. It also says that the port is based on dryer by Robert C. Martin, which uses the BSD Zero Clause License. Each `.rs` file starts with the lines `// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>` and `// SPDX-License-Identifier: MIT`. Each file that matches a file of the original names that file.

## 7. Parity and bug fixes

The port copies the behavior of the original at one pinned commit. A parity runner runs the original and the port on each parity fixture and compares the results. The runner gets the original by a clone at the pinned commit and runs it with Python. The port fixes known bugs of the original and does not copy them. It also has two deliberate differences that are not bug fixes: DR-15 leaves out Rust test code, and DR-27 prints a warning for a file that it cannot read and goes on.

- DR-PB-1: `tools/dryer/parity/upstream.toml` records the pinned commit of the original.
- DR-PB-2: A change of the pinned commit needs a new parity run.
- DR-PB-3: Each bug fix, and each other difference from the original, has an entry in `tools/dryer/parity/fixes.toml`. The entry gives the upstream issue or a description, the changed behavior, and the parity fixture that shows the difference.
- DR-PB-4: Each bug fix, and each other difference from the original, has a test that fails on the behavior of the original and passes on the port.
- DR-PB-5: A bug that comes from Python only needs no entry if the port cannot have it.
- DR-PB-6: Mads Madsen approves each entry in `tools/dryer/parity/fixes.toml`.
- DR-PB-7: A difference without an upstream issue or pull request, such as DR-15, goes in section 5.2 and not in section 5.1. Its `fixes.toml` entry names no upstream source. Its `upstream` field says that there is none.

## 8. Acceptance criteria

1. AC-1 (DR-2, DR-3, DR-4): `tools/dryer/parity/coverage.toml` maps every flag of the original and every row of the README table (DR-11) to at least one fixture run. For each entry, the original and the port give the same exit code and the same snapshot after both are parsed as EDN. A difference that `fixes.toml` lists does not count as a failure.
2. AC-2 (DR-5, DR-8): A parity fixture exists for each of the six languages.
3. AC-3 (DR-13): `dryer --format xml` exits `2`.
4. AC-4 (DR-7): On a fixture with a modified, a staged, and an untracked source file, the original and the port, both run with `--changed`, select the same files and write the same snapshot.
5. AC-5 (DR-10, DR-12): On a fixture with two forms that score 0.88, `dryer --threshold 0.8` reports the pair and `dryer --threshold 0.9` does not.
6. AC-6 (DR-2): A flag that the original rejects is also rejected by the port, with the same exit code.
7. AC-7 (DR-NFR-1): Two runs on each fixture write byte-identical snapshots.
8. AC-8 (Goal 3): All fixtures pass with the port on a host that has no Python installed. The expected results come from an earlier parity run.
9. AC-9 (DR-PB-4): Each entry in `fixes.toml` has a test that names the entry and passes on the port.
10. AC-10 (DR-NFR-2): On each parity fixture, the median wall-clock time of the port is lower than the median wall-clock time of the original, measured as DR-NFR-2 defines.
11. AC-11 (DR-14): On a fixture with only `.jsx` source files and two forms that score 0.85, `dryer --threshold 0.8` reports the pair and does not print "No source files to analyze". A file `src/a.test.jsx` in the same fixture is not scanned. On the TypeScript fixture, the port writes the same snapshot as before the change. The output of `dryer --help` names JavaScript and lists the extensions `.js`, `.jsx`, `.mjs`, and `.cjs`.
12. AC-12 (DR-15): On the fixture `rust-test-code`, the functions sit only in a module with `#[cfg(test)]` that is not named `tests`. The original reports a pair there with `--threshold 0.6`. The port prints "No duplicate candidates found." for the same run. With `--threshold 0.8`, two similar functions in such a module give no pair, and the same two functions outside a test module give a pair. Tests show that the port leaves out a function in each of the four rules of DR-15. They also show that a `#[cfg(not(test))]` module, a `#[cfg(all(test, unix))]` module, a `#[tokio::test]` function, and a plain function next to a module named `tests` stay in the scan.
13. AC-13 (DR-27): With one source file that the port cannot read and two source files that hold a pair, the port exits `0`, prints the pair, and prints one warning that names the unreadable file on standard error. The original stops with a traceback and reports nothing.

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
