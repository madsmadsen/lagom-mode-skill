# PRD: crapper

Status: Draft 1. Owner and approver: Mads Madsen. Date: 2026-11-07.

This document states the requirements for `crapper`, a Rust port of [crapper](https://github.com/unclebob/crapper) by Robert C. Martin. The port is one Cargo crate in the `tools/crapper/` directory. It does not depend on another crate or tool in this repository.

## 1. Problem

crapper gives a CRAP score to each function, so that a human or an agent can find functions that are complex and not well tested. The original is written in Python. A project in another language then needs a working Python setup only to run crapper.

## 2. Definitions

| Term | Meaning |
| --- | --- |
| Original | The Python crapper at the commit pinned in `tools/crapper/parity/upstream.toml`. |
| Port | The Rust crapper that this document specifies. |
| CC | Cyclomatic complexity: the number of decision points in a function, plus one. |
| CRAP | Change Risk Anti-Patterns score: `CC² × (1 − coverage)³ + CC`. |
| Snapshot | The EDN file `.metrics/crap.edn` that crapper writes. |
| EDN | Extensible Data Notation, the Clojure data format. |
| Parity fixture | A small project in one language that the original and the port both analyze. |

## 3. Goals

1. The port is a drop-in replacement for the original: same command name, same flags, same exit codes, same snapshot. The only differences are the entries in `tools/crapper/parity/fixes.toml` (section 7): bug fixes and the other differences that CR-PB-3 lists.
2. The port supports the six languages of the original: Clojure, Java, Go, TypeScript (with JavaScript), Rust, and Python.
3. The port needs no Python at run time. A Python project still needs Python to run its own tests.

## 4. Non-goals

1. The port does not add languages or metrics. It adds one flag that the original does not have: `--lines` (CR-26).
2. The port does not change the formula or the threshold logic of the original. A bug fix is not a change of formula. A fixes.toml entry is not a change of formula.
3. The port is published in a public repository on GitHub. It is not published to crates.io.
4. The port does not support Windows.

## 5. Functional requirements

- CR-1: The port is a Rust binary named `crapper`.
- CR-2: The port accepts every flag and positional argument of the original, with the same meaning and the same default. It rejects each flag that the original rejects, with the same exit code.
- CR-3: The port returns the same exit code as the original for the same input. When the original stops with a Python traceback (exit code 1), the port exits with code 1, writes one line on stderr, writes nothing on stdout, and writes no snapshot. The text on stderr can differ from the traceback. The port never stops with a Rust panic (exit code 101).
- CR-4: The port writes `.metrics/crap.edn` in the same EDN shape and with the same keys as the original.
- CR-5: The port detects the language of each file by its extension, the same as the original.
- CR-6: The port skips the same directories as the original. The behavior of the original is normative. The list `test`, `spec`, `vendor`, `node_modules`, and `target` is informative.
- CR-7: The port supports `--changed` with the same Git selection of files as the original.
- CR-8: The port names functions and namespaces for each language with the table in the README of the original. The one exception is test code in Rust files (CR-25).
- CR-9: The port parses source with tree-sitter grammars for Java, Go, TypeScript, JavaScript, Rust, and Python, as the original does with `tree-sitter-language-pack`. For Clojure, the port copies the scanner of the original, which comes from crap4clj: it counts decision points with regular expressions and bracket depth, and it does not read EDN.
- CR-10: The port computes `CRAP = CC² × (1 − coverage)³ + CC` for each function. Coverage is the value that the original computes for the function at the pinned commit: a percentage from 0 to 100, measured as LCOV branches when the function has BRDA records and LCOV lines otherwise, JaCoCo instructions, Go statements, and Cloverage forms. The formula uses coverage / 100. Under `--no-coverage`, `:coverage` and `:crap` are nil, as in the original. For a function that the coverage report does not mention, `:coverage` is `0.0` and `:crap` is the score for coverage 0, as in the original.
- CR-11: The port counts decision points for each language with the rules in the README of the original, section "What each language counts".
- CR-12: The port reads coverage in each format of the original. At the pinned commit, the complete list is: Cloverage form counts, LCOV, JaCoCo XML, and the Go coverage profile. Cloverage form counts are the HTML files that Cloverage writes under `target/coverage/` (`target/coverage/**/*.html`). The original reads the per-line counts "N out of M forms covered" from these files. The port reads each number in a Go profile and in a JaCoCo report with the rules of Python `int()` for ASCII digits: an optional sign, underscores between digits, and any number of digits. The port clamps a number above 2^63 − 1 to 2^63 − 1. The port reads JaCoCo XML as the original does, except for the DOCTYPE rule: it deletes the DOCTYPE declaration first with the rule in CR-24, and it rejects a malformed XML declaration. A malformed Go profile, a malformed JaCoCo XML file, and a coverage report without read permission stop the port with exit code 1 (CR-3).
- CR-13: The port runs the default coverage command for each language from the README of the original, section "Coverage commands". It supports `--coverage-command`, `--use-existing-coverage`, and `--no-coverage`.
- CR-14: If a coverage tool is missing, the port installs it the same way as the original, except for the changes in CR-18, CR-19, and CR-23. This covers `cargo-llvm-cov` for Rust, `@vitest/coverage-v8` in `node_modules` for Vitest with no edit to `package.json`, and `coverage` and `pytest` in the interpreter of the project for Python. The port pins no version of `coverage`, of `pytest`, or of `c8`. It installs `cargo-llvm-cov` with `--locked`, which pins the dependencies of the tool build and not the version of the tool. The one install with a pinned version is the Vitest provider, which carries the version of the installed Vitest (CR-23). The original at the pinned commit installs the same way: `<python> -m pip install --disable-pip-version-check <module>`, `npx --yes c8`, `cargo install cargo-llvm-cov --locked`, and `npm install --no-save --no-package-lock @vitest/coverage-v8@<version>`. The install command of the port differs from the original only as CR-18, CR-19, and CR-23 state.
- CR-15: The port prints each tool install on stderr before it starts the install.
- CR-16: The port prints a table on stdout with the worst score first.
- CR-17: `--threshold N` makes the port exit `2` when the worst score is higher than `N`.

### 5.1 Upstream fixes

Each requirement in this section comes from an open issue or pull request of the original, and differs from the original at the pinned commit. Each one has an entry in `tools/crapper/parity/fixes.toml` (CR-PB-3) and a test (CR-PB-4).

- CR-18: The port installs `@vitest/coverage-v8` with the package manager of the project ([issue 25](https://github.com/unclebob/crapper/issues/25), [PR 34](https://github.com/unclebob/crapper/pull/34)). The port selects the first match in this order:
  1. pnpm, if `pnpm-lock.yaml` or `node_modules/.pnpm/` exists. The command is `pnpm add -D --no-lockfile <spec>`.
  2. yarn, if `yarn.lock` exists. The command is `yarn add -D --no-lockfile <spec>`.
  3. bun, if `bun.lockb` or `bun.lock` exists. The command is `bun add -d --no-save <spec>`.
  4. pnpm, yarn, or bun, if the `packageManager` field of `package.json` starts with that name. The command is the one in item 1, 2, or 3.
  5. npm in all other cases, with the command in CR-19.

  After the install, the port restores `package.json`, `package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml`, `yarn.lock`, `bun.lockb`, and `bun.lock` to their bytes from before the install. The port deletes each of these files that did not exist before the install.
- CR-19: The npm command is `npm install --no-save <spec>`, without `--no-package-lock`, so npm keeps every locked dependency at its locked version ([issue 16](https://github.com/unclebob/crapper/issues/16)).
- CR-20: The port removes every environment variable whose name starts with `GIT_` from the environment of each `git` command that it runs. A run inside a Git hook then reads the repository of the project ([PR 34](https://github.com/unclebob/crapper/pull/34)).
- CR-21: In TypeScript and JavaScript, each of these functions is its own entry, when it is not inside another function ([issue 20](https://github.com/unclebob/crapper/issues/20)):
  1. A function in a `var` declaration, with an arrow function or a function expression. The name is the name of the variable, the same as for `const` and `let`.
  2. A method of an object literal that a top-level declaration holds. The name is the name of the method, and the namespace is the module namespace plus the name of the variable, for example `shapes.store`.
  3. A class field that holds an arrow function or a function expression. The name is the name of the field, and the namespace is the namespace of the class, the same as for a method.
  4. An `export default` function with no name. The name is `default`.
- CR-22: In Python, a function inside another function is its own entry when a route call decorates it ([PR 30](https://github.com/unclebob/crapper/pull/30)). The port copies the rules of PR 30:
  1. A route call is a call to `.get`, `.post`, `.put`, `.patch`, `.delete`, `.head`, `.options`, `.trace`, `.websocket`, `.route`, or `.api_route`.
  2. The namespace is the module namespace. The name is the HTTP method in capitals, one space, and the path, for example `GET /users/{user_id}`.
  3. `.route` and `.api_route` take their methods from `methods=[...]`, joined with `|`, for example `GET|POST /sync`. Without `methods`, the method is `GET`.
  4. A name that occurs a second time in a module gets the suffix `#2`, the third time `#3`, and so on.
  5. A string path must be empty or start with `/`. Otherwise the call is not a route call, and the function stays inside its enclosing function. For example, `@mock.patch("pkg.mod.attr")` is not a route call.
  6. An f-string path keeps its fields, for example `GET {PREFIX}/items`. A path that is a name or an attribute keeps its source text, for example `DELETE PATHS.user`.
  7. The decision points of the handler do not count for the enclosing function. The decision points in the arguments of the decorator still count for the enclosing function.
  8. A top-level handler and a method keep their function names.
  9. The coverage range of the enclosing function still includes the lines of the handler.
- CR-23: The port reads the Vitest version from `node_modules/vitest/package.json` only when it is a valid SemVer version: it matches `^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$`. For any other value, for example `file:../untrusted`, the port installs `@vitest/coverage-v8` with no version. Each install command in CR-18 and CR-19 also has the flag `--ignore-scripts`, so no install script of a package runs ([PR 27](https://github.com/unclebob/crapper/pull/27), [issue 26](https://github.com/unclebob/crapper/issues/26)).
- CR-24: Before the port parses JaCoCo XML, it deletes the text that matches `<!DOCTYPE\b(?:[^>\[]|\[[^\]]*\])*>`, in any case and across lines. This also deletes a DOCTYPE declaration with an internal subset (`[ ... ]`), so the XML parser never reads entity declarations ([PR 27](https://github.com/unclebob/crapper/pull/27)). This rule replaces the DOCTYPE rule of the original in CR-12.

### 5.2 Other differences from the original

This section holds a difference that does not come from an upstream issue or pull request. It has an entry in `tools/crapper/parity/fixes.toml` (CR-PB-3) and a test (CR-PB-4).

- CR-25: In Rust files, the port leaves out each function that is test code. Rust best practice puts unit tests in the same file, in a module with any name. A function is test code when at least one of these is true:
  1. An enclosing `mod` is named `tests`. This is the rule of the original.
  2. An enclosing `mod` has the attribute `#[cfg(test)]`. The attribute is `cfg` with the single argument `test`.
  3. The function has the attribute `#[test]`.
  4. The function has the attribute `#[cfg(test)]`.

  The attributes of an item are the `attribute_item` nodes that stand before it, up to the first node that is neither an attribute nor a comment. A comment between an attribute and the item does not break the attribute. Spaces inside an attribute do not matter. No other attribute makes a function test code. For example, `#[cfg(all(test, unix))]`, `#[cfg(not(test))]`, and `#[tokio::test]` do not. Code with `#[cfg(not(test))]` stays production code. The port does not score a function that is test code, and it does not write an entry for it in the snapshot.

  An `impl` block is not a module. A `#[cfg(test)] impl` block is therefore not a `#[cfg(test)]` module, and each function in it is production code. For example, in `#[cfg(test)] impl Board { fn place(&self) {} }`, the port scores `place`. Mads Madsen chose to document this case and to leave the code as it is.

- CR-26: The port has the option `--lines <n,n,...>`. The original does not have it and rejects it as an unknown option. The value is a list of line numbers separated by commas. Each line number is a positive whole number. The port reads each item with the rules of Python `int()` for ASCII digits: an optional `+` sign, an underscore between two digits, and spaces around the item do not change the number, so `--lines 1_0` asks for line 10. An empty item is skipped. A number above `u32::MAX` becomes `u32::MAX`, which no function holds. The port scores only the functions that contain at least one of the listed lines. A function contains a line when the line is the first line of the function, the last line of the function, or between them. The lines of a function inside another function also belong to the outer function. The option applies to every selected file. A function that contains no listed line is not scored. It is not in the report, it is not in the snapshot, and it does not count for `--threshold`. With `--threshold N`, the port exits `2` only when a scored function has a CRAP score higher than `N`. A value with no line number, or with an item that is not a positive whole number, is a bad option value: the port exits `1` and writes a message and the help text on stderr. The option does not change which files are selected or how coverage runs.

## 6. Non-functional requirements

- CR-NFR-1: Determinism. Two runs on the same files with the same coverage report write byte-identical snapshots.
- CR-NFR-2: Speed. On each parity fixture, the median wall-clock time of 5 runs of the port is lower than the median wall-clock time of 5 runs of the original. Both the original and the port run with `--use-existing-coverage` on the same stored coverage report, on one named CI runner: Linux x86_64, Ubuntu 22.04. The time of the original includes Python start-up and excludes the clone and the install.
- CR-NFR-3: Platforms. The port builds and runs on macOS 14 or later (arm64 and x86_64) and on Linux with glibc 2.35 or later (Ubuntu 22.04) (x86_64 and arm64).
- CR-NFR-4: Install. The port is published in a public repository on GitHub and not on crates.io. A user installs it with one `cargo install --locked --git <repo> crapper` command, which builds from the committed `Cargo.lock`.
- CR-NFR-5: License. The port uses the MIT License. `tools/crapper/LICENSE` names Mads Madsen (`mads@onmadsen.com`) as the copyright holder. It also says that the port is based on crapper by Robert C. Martin, which uses the BSD Zero Clause License. Each `.rs` file in `tools/crapper/` starts with a header comment. The first line is `// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>`. The second line is `// SPDX-License-Identifier: MIT`. The next lines name the upstream repository and the pinned commit. A file that ports an upstream file also names that upstream file path. A file with no upstream counterpart says `upstream-path: none`.

## 7. Parity and bug fixes

The port copies the behavior of the original at one pinned commit. A parity runner runs the original and the port on each parity fixture and compares the results. The runner gets the original by a clone at the pinned commit and runs it with Python. The port fixes known bugs of the original and does not copy them. It also has two deliberate differences that are not bug fixes: CR-25 leaves out Rust test code, and CR-26 adds the option `--lines`.

- CR-PB-1: `tools/crapper/parity/upstream.toml` records the pinned commit of the original and the exact versions of `tree-sitter`, `tree-sitter-language-pack`, and each other Python dependency of the original. The parity runner installs these versions.
- CR-PB-2: A change of the pinned commit needs a passing parity run, as AC-1 defines a pass. The parity runner writes `tools/crapper/parity/last-run.toml` with the commit and the result.
- CR-PB-3: Each bug fix, and each other difference from the original, including a difference that comes from Python only, has an entry in `tools/crapper/parity/fixes.toml`. The entry has an `issue` or a `description` field, a `behavior` field that states the changed behavior, and a `fixture` field that names the parity fixture that shows the difference.
- CR-PB-4: Each entry in `tools/crapper/parity/fixes.toml` has a test. The test asserts against the output of the original that the parity run recorded at the pinned commit. It fails on that recorded output and passes on the port.
- CR-PB-6: Mads Madsen approves each entry in `tools/crapper/parity/fixes.toml`. An approved entry has the field `approved_by = "Mads Madsen"`. An entry without this field does not exempt a difference under AC-1.

## 8. Acceptance criteria

1. AC-1 (CR-2, CR-3, CR-4): `tools/crapper/parity/coverage.toml` maps every flag of the original and every row of the README tables (CR-8, CR-11, CR-13) to at least one fixture run. For each entry, the original and the port give the same exit code and the same snapshot after both are parsed as EDN. A difference that `fixes.toml` lists does not count as a failure.
2. AC-2 (CR-5, CR-9): A parity fixture exists for each of the six languages.
3. AC-3 (CR-17): On a fixture with a worst score of 12, `crapper --threshold 10` exits `2` and `crapper --threshold 12` exits `0`. On a fixture with a worst score of exactly 10.0, `crapper --threshold 9.99` exits `2`.
4. AC-4 (CR-7): On a fixture with a modified, a staged, and an untracked source file, the original and the port, both run with `--changed`, select the same files and write the same snapshot.
5. AC-5 (CR-13): On a fixture, the original and the port give the same exit code and snapshot with `--coverage-command <cmd>` and with `--no-coverage`.
6. AC-6 (CR-14, CR-15): On a Rust fixture on a host without `cargo-llvm-cov`, the port installs it, prints the install on stderr, and writes a snapshot with coverage values.
7. AC-7 (CR-14): On a Vitest fixture without `@vitest/coverage-v8`, the port installs it in `node_modules`, and `package.json` is unchanged. On a Python fixture without `coverage` and `pytest`, the port installs them in the interpreter of the project.
8. AC-8 (CR-16): On each fixture, the table that the port prints on stdout equals the table of the original.
9. AC-9 (CR-2): A flag that the original rejects is also rejected by the port, with the same exit code.
10. AC-10 (CR-NFR-1): Two runs on each fixture with `--use-existing-coverage` write byte-identical snapshots.
11. AC-11 (Goal 3): The fixtures for Clojure, Java, Go, TypeScript, and Rust pass with the port on a host that has no Python installed. The expected results come from an earlier parity run.
12. AC-12 (CR-PB-4): For each entry in `fixes.toml`, a test names the entry, fails on the recorded output of the original, and passes on the port. The test needs no live Python.
13. AC-13 (CR-1): `cargo build --release` in `tools/crapper/` produces a binary named `crapper`, and `crapper --help` exits `0`.
14. AC-14 (CR-6): On a fixture with the directories `test/`, `src/test/`, `spec/`, `vendor/`, `node_modules/`, `target/`, and `testing/`, each with a source file, the original and the port select the same files.
15. AC-15 (CR-10): The port gives these known values, with coverage stated as a fraction (coverage / 100): CC=1 and coverage 0 give `2`; CC=5 and coverage 0.5 give `8.125`; CC=5 and coverage 1 give `5`.
16. AC-16 (CR-12): For each of the four coverage formats, a stored coverage report read with `--use-existing-coverage` gives the same coverage for each function in the original and the port.
17. AC-17 (CR-NFR-2): On each parity fixture, the median wall-clock time of the port is lower than the median wall-clock time of the original, measured as CR-NFR-2 defines.
18. AC-18 (CR-NFR-3): A CI matrix of four targets (macOS 14 arm64, macOS 14 x86_64, Linux x86_64 with glibc 2.35 or later on Ubuntu 22.04, and Linux arm64 with glibc 2.35 or later on Ubuntu 22.04) builds the port and passes the parity fixtures with `--use-existing-coverage` on each target.
19. AC-19 (CR-NFR-4): On a clean host with Rust stable and a C compiler, `cargo install --locked --git <repo> crapper` succeeds, and `crapper --help` exits `0`.
20. AC-20 (CR-NFR-5): `tools/crapper/LICENSE` is the MIT License text with the copyright line of Mads Madsen and the credit to Robert C. Martin. A check finds the CR-NFR-5 header comment in every `.rs` file in `tools/crapper/`: each header starts with the copyright line and the SPDX line, and names the upstream repository, the commit, and either an upstream file path or `upstream-path: none`. The commit in each header equals the commit in `tools/crapper/parity/upstream.toml`.
21. AC-21 (CR-PB-1): The parity runner fails when `upstream.toml` lacks the commit or a version. After the install, the Python dependency versions of the original match the versions in `upstream.toml`.
22. AC-22 (CR-PB-2): CI fails when the commit in `tools/crapper/parity/last-run.toml` differs from the commit in `tools/crapper/parity/upstream.toml`, or when the result in `last-run.toml` is not a pass.
23. AC-23 (CR-PB-3): A check confirms that each entry in `fixes.toml` has an `issue` or a `description` field, a `behavior` field, and a `fixture` field that names an existing parity fixture. Every difference that the parity run finds matches an entry.
24. AC-24 (CR-PB-6): A check confirms that every entry in `fixes.toml` has `approved_by = "Mads Madsen"`. A difference whose only matching entry has no such field counts as a failure under AC-1.
25. AC-25 (CR-10): On a fixture with a function that the coverage report does not mention, the original and the port write `:coverage 0.0` and the same `:crap` value (the score for coverage 0) for that function. Under `--no-coverage`, both write nil.
26. AC-26 (CR-3): For each of these inputs, the original and the port exit with code 1, write nothing on stdout, and write no snapshot, and the port writes one line on stderr: a source file that cannot be read (a broken symlink, a symlink loop, a file without read permission); a coverage report without read permission (LCOV, Go profile, JaCoCo, Cloverage HTML); a malformed Go profile (wrong number of fields, a number that is not an integer, a block without a comma, a line without a colon); a malformed JaCoCo XML file (an unclosed tag, a counter that is not an integer, a bad XML declaration); and a source file whose package marker lies outside the project root. The original and the port also give the same result for a Go profile or JaCoCo report that has a number with a sign or underscores between digits, a very large number, or odd DOCTYPE text.
27. AC-27 (CR-18): For each of these fixtures without `@vitest/coverage-v8`, the port runs the install command of CR-18 and prints it on stderr: a pnpm fixture with `pnpm-lock.yaml`, a yarn fixture with `yarn.lock`, a bun fixture with `bun.lock`, and a fixture with only `"packageManager": "pnpm@9.15.9"` in `package.json`. After the run, `package.json` and each lockfile have their bytes from before the run, and no new lockfile exists. On the pnpm fixture of issue 25, the function `big` has coverage `100.0`.
28. AC-28 (CR-19): On an npm fixture with `"semver": "^7.5.0"` locked to `7.5.0` in `package-lock.json`, after the port installs the provider, `node_modules/semver/package.json` has version `7.5.0`.
29. AC-29 (CR-20): With `GIT_DIR` and `GIT_INDEX_FILE` set to a second repository, `crapper --changed` in a fixture selects the same files and writes the same snapshot as without these variables.
30. AC-30 (CR-21): `crapper --no-coverage` on the file `src/shapes.ts` from issue 20 gives seven entries. `plain`, `viaVar`, `legacy`, and `default` are in namespace `shapes`. `objectMethod` is in `shapes.store`. `method` and `field` are in `shapes.Box`. The same file as `src/shapes.js`, without the types, gives the same seven entries.
31. AC-31 (CR-22): On the factory example of PR 30, the port gives the entries `GET /users/{user_id}` and `POST /users`, and the CC of `build_router` does not include their decision points. In the same fixture, a function nested under `@mock.patch("pkg.mod.attr")` has no entry of its own, a top-level handler keeps its function name, and two handlers with the label `GET /users` give `GET /users` and `GET /users#2`.
32. AC-32 (CR-23): With `"version": "4.1.11-beta.1+build.2"` in `node_modules/vitest/package.json`, the port installs `@vitest/coverage-v8@4.1.11-beta.1+build.2`. With `"version": "file:../untrusted"`, the port installs `@vitest/coverage-v8` with no version. On the fixtures of AC-27 and AC-28, each install command has `--ignore-scripts`.
33. AC-33 (CR-24): A JaCoCo report with `<!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd" [ <!ELEMENT report (class*)> ]>` over more than one line, and a method `place` of class `demo/pkg/Board` with 1 missed and 3 covered instructions, gives coverage `75.0` for `demo.pkg.Board#place`.
34. AC-34 (CR-25): On a Rust file with `pub fn keep`, a `#[cfg(test)] mod unit_tests` that holds `#[test] fn unit_check`, a `#[test] fn plain_check`, and a `#[cfg(test)] fn test_helper`, `crapper --no-coverage` reports `keep` and does not report the other three. A `#[cfg(not(test))]` function in the same file is reported. A module named `tests` with no attribute is still left out.
35. AC-35 (CR-26): A Python file has `choose` on lines 1 to 6 (CRAP 12 with 0% coverage) and `ok` on lines 8 and 9. Each of these holds with the same coverage report:
    1. `crapper --lines 9` reports `ok` only, and the snapshot has only `ok`.
    2. `crapper --lines 1` and `crapper --lines 6` report `choose` only.
    3. `crapper --lines 3,8` reports both functions.
    4. `crapper --lines 7` reports no function and writes the snapshot `{:entries []}`.
    5. `crapper --threshold 10 --lines 9` exits `0`. `crapper --threshold 10 --lines 3` exits `2`. `crapper --threshold 10` with no `--lines` exits `2`.
    6. With a second file, the same line list applies to both files.
    7. `--lines 0`, `--lines x`, `--lines 1.5`, `--lines ,`, and `--lines` with no value each exit `1` with a message and the help text on stderr.
    8. `crapper --help` lists `--lines <n,n,...>`.
36. AC-36 (CR-25): On a Rust file with `#[cfg(test)] impl Board { fn place(&self) {} }` and `#[cfg(test)] mod unit_tests { fn hidden() {} }`, the port scores `place` and does not score `hidden`. The unit test `cr_25_a_cfg_test_impl_block_keeps_its_functions_as_production_code` in `tools/crapper/src/languages/rust.rs` checks this case on the function list of the port.

## 9. Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| The original changes often. | The port drifts from the original. | Pin one commit (CR-PB-1). Update on purpose only (CR-PB-2). |
| A tree-sitter grammar version in Rust differs from the one in Python. | Different parse trees give a different CC or different names. | Pin the grammar versions. `upstream.toml` records the exact Python dependency versions of the original, and the parity runner installs them (CR-PB-1). AC-1 detects a difference. |
| The Clojure scanner in the original uses regular expressions and bracket depth, not a reader. | Edge cases of the scanner are hard to copy. | The Clojure fixture covers reader conditionals, syntax-quote, and keyword calls. |
| Floats print in a different way in Rust and Python. | Snapshots differ in the last digits. | AC-1 compares parsed EDN. The port copies the number format of the original. |
| The six coverage tool chains must exist on the CI host. | Parity runs are slow and fragile. | Most runs use `--use-existing-coverage` with stored coverage reports. |
| The port installs tools into the environment of the user. | An install surprises the user. | CR-15 prints each install. |

## 10. Dependencies

1. Rust stable toolchain and Cargo on the build host.
2. A C compiler on the build host (Xcode Command Line Tools on macOS, gcc or clang on Linux).
3. Git 2.30 or later on each host that runs `--changed`.
4. For each language that a user analyzes: the test tools in the README of the original.
5. For parity runs only: Python and the original at the pinned commit.

## 11. Open questions

None.
