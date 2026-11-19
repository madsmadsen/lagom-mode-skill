# dryer port log

Method: ub-mode (make it work with tests first, make it right with the three gates, keep a log, small commits).
Roles: the orchestrator (`DryerOrchestrator`) planned units, ran every cargo command and every gate, and dispatched
edit-only `implementer` subagents. The orchestrator wrote no Rust.

## Sources and tools

- Original: https://github.com/unclebob/dryer at `66ff6d21a42c04afcad89c78a80066176d1294b0` (pinned in `parity/upstream.toml`).
  Cloned to `/tmp/dryer-orig/dryer`, installed in a Python 3.12 venv (`uv`) with `tree-sitter 0.26.1` and
  `tree-sitter-language-pack 1.25.0`. Its own suite passed first: 80 tests.
- Gate tools: crapper and mutator originals cloned to `/tmp/crapper-orig` and `/tmp/mutator-orig` (`/tmp/crapper` links to the first,
  because mutator needs `../crapper`). The dryer original was the dryer gate. All three ran on this crate.
- Rust stable 1.99, `cargo-llvm-cov` 0.6.21 (installed already).

## Actions and reasons

1. Read the PRD, the ub-mode skill, and every Python source file of the original (3.2k lines with tests).
   Reason: the port copies behavior, so the Python code is the specification.
2. Fixed the data shapes first as a contract (`Language`, `Span`, `Duplicate`, `Entry`, `Norm`, `Interner`, module signatures).
   Reason: ten units could then be written in parallel with no overlap. Fingerprints are interned `u32` ids in sorted vectors,
   so Jaccard is an exact integer merge and stays fast.
3. Dispatched 10 units in one batch: scaffold + model, shape, discover, clojure reader, tree-sitter normalizer, form pickers,
   scan + report, args, select (git), cli. Two more units in parallel: parity runner with fixtures, and binary tests.
4. First `cargo build`: 4 lifetime errors. `Node::kind()` returns `&'tree str` in tree-sitter 0.27.
   Fix: `treesitter::kind_static` interns kind strings (finite set), so `Norm::Kw` stays `&'static str`.
5. First `cargo test`: two EDN layout tests had 3 spaces in the expected text (the original uses 2). The tests were wrong, not the code.
6. First end-to-end run disagreed with the original on node counts (Python). Cause: tree-sitter-python 0.25.0 on crates.io wraps
   statements in `expression_statement`; the grammar bundle the original uses (`tree-sitter-language-pack` 1.25.0) does not
   (it matches commit `26855ea` "mark `expression_statement` as a supertype"). Proof: compared S-expressions of 35 snippets.
   Fix: `tree-sitter-python` is a git dependency pinned to that commit. Golden normalizer tests (18, all six grammars) were
   generated from the original and now pass. Java, Go, Rust, TypeScript, TSX crates matched with no change.
7. Parity runner (`parity/run_parity.py`) on 32 cases: 32/32 PASS. Then a larger differential run, original vs port,
   with `--threshold 0.5 --min-lines 3 --min-nodes 10` on real projects (gson, cobra, ring, ripgrep, zod, pytest): stdout and
   `dry.edn` byte-identical on all six (up to 185,934 candidates). The port is about 10 to 100 times faster per run (DR-NFR-2 holds
   on every fixture; runner prints both times).
   Edge-case CLI differential (nan/inf/underscore numbers, CRLF and lone CR sources, missing paths, bad values): identical.
8. Gates on the crate (source only; `src/`):
   - dryer original: no pair at or above 0.8. First run found 2 (`args.rs` setters 1.00, `clojure.rs` openers 0.82). Fixed by one
     table-driven `apply_option` and one `open_coll` helper. Highest remaining score: 0.75.
   - crapper original (`crapper -s src`): first run had one function at CRAP 10.0 (`apply_option`) and one untested function
     (`dispatch_string`). Split the function and added tests. Highest remaining CRAP: 8.1.
   - mutator original (`mutator src`): first run 223 sites, 218 killed, 1 survived, 4 uncovered. The survivor was an equivalent
     mutant on a `Vec` capacity (capacity call deleted). Uncovered: non-identifier member names in `astnorm` (tests from Rust tuple
     fields and TS `#private` members, goldens from the original), metrics write failure and Clojure read warnings in `cli` (tests).
     Two more survivors were the dead `false` head flag in `name_value` (the flag cannot matter there; call collapsed).
     Final run: 219 sites, 219 killed, 0 survived, 0 uncovered, exit 0.
   - Final gate numbers: dryer max pair 0.75 (< 0.8), crapper max CRAP 8.1 (< 10), mutator 100%.
9. Commits are small after the first three; the first commit is large because all modules compiled together (the crate does
   not build with a subset of modules, and red trees are not committed).

## Decisions where the PRD and the original differ

The port follows the original, as PRD goal 1 says.

- DR-5 and DR-8 mention JavaScript. The original at the pinned commit has no `.js` extension and only TypeScript grammars. DR-14 now asks for JavaScript. See "DR-14 round" below.
- DR-6 lists five skipped directories. The original skips more (`.git`, `dist`, `build`, `tests`, `specs`, `__tests__`, ...). The port copies the original list.
- DR-12 and the help text of the original also have `--root` and `-s/--source-root`, which the PRD list omits. The port has them (DR-2).
- Suspected upstream quirk, copied for parity and NOT added to `fixes.toml` (needs owner approval, DR-PB-6): `is_test_file` looks at
  every component of the absolute path, so a project that sits under a directory named `test`, `tests`, `spec`, ... finds no files.
- Python `int()` accepts non-ASCII digits (for example Arabic-Indic). The port accepts ASCII digits only. Not reachable from tests of the original.
- Original crashes with a traceback when a metrics file cannot be written or git is missing. The port prints `dryer: <error>` and
  exits 1 (the original also exits 1 in these cases).

## Blockers and open items

- DR-NFR-3 (macOS 14 arm64 and x86_64, Linux x86_64 and arm64): macOS arm64 and Ubuntu 22.04 arm64 and x86_64 (qemu guests, see Linux guests) were exercised. macOS x86_64 was not.
- DR-NFR-4: the README shows `cargo install --git https://github.com/<owner>/deterministic-coding dryer`. The real URL is not known. `[ASSUMPTION]`.
  Because `tree-sitter-python` is a git dependency, the build needs network access to github.com for that crate.
- DR-PB-3, DR-PB-6, AC-9: `parity/fixes.toml` has no entries because no fix is approved. AC-9 has a test that parses the file and
  checks that each entry id appears in a test (it passes with zero entries).
- AC-8 is met by `tests/parity_expected.rs`, which replays `parity/expected/` (recorded from the original) with no Python.
- uml-viewer check (PRD AC-5): no `fixes.toml` entry changes snapshot keys, so none is needed.

## DR-14 round (PRD update 8f31773 and bc91fc4)

Diff of the PRD since the first port: DR-5 (now "except DR-14"), DR-8 (Clojure reader copied from the original), DR-14 (new, upstream
fix for [issue 17](https://github.com/unclebob/dryer/issues/17) and [PR 16](https://github.com/unclebob/dryer/pull/16)), DR-NFR-2
(median of 5 runs, Linux x86_64, named runner), AC-10 and AC-11 (new). DR-8, DR-NFR-2 and AC-10 need no code change.
Actions and reasons:

1. Wrote the red tests first (DR-14, AC-11, AC-9): `discover.rs` (3), `treesitter.rs` (1), `extract.rs` (2), `tests/cli.rs` (2).
   Ran them. Red because: `language_of` returned `None` for `.js` and friends, the `.test.*` names were not test files, `parse("javascript", ...)`
   panicked, `.jsx` went to the TypeScript grammar (JSX parsed to the wrong span, 1-2 instead of 1-3), and a `.jsx` project printed
   `No source files to analyze.`. One extract test (class method in `.js`) was already green because the TypeScript grammar parses it. It stays as a guard.
2. Smallest change: `tree-sitter-javascript 0.25.0` in `Cargo.toml`, four extensions and eight test suffixes in `discover.rs`,
   a `javascript` arm in `treesitter::language_for`, and `extract::typescript_grammar` returns `javascript` for the four extensions.
   `.d.ts` stays out (no change needed). Language stays `typescript`, so the snapshot shape does not change.
3. Fixture `parity/fixtures/jsx` (a.jsx, b.jsx, a.test.jsx). The pair scores 0.85 (0.8546). The test file proves `.test.jsx` is not scanned:
   a pair of `a.jsx` and `a.test.jsx` would score 1.0.
4. Proof against a patched original. The pinned original, plus a scratch patch of exactly PR 16 and the `_grammar` change from issue 17
   (in `/tmp`, not in the repository), gives byte-identical stdout and `dry.edn` with the port on the `jsx` fixture and on 121 real
   JavaScript files (npm and express `lib`, threshold 0.5, 1043 lines of output). So DR-14 is the only change, and JS scoring is the original scoring.
5. `parity/fixes.toml` has a new entry `js-read-as-typescript` (DR-14, fixture case `jsx-fix`). It has NO `approved_by` key: Mads Madsen has not approved it (DR-PB-6).
   I changed the file header: an entry without `approved_by` is not approved. `coverage.toml` has a case `jsx-fix` with `fix = "js-read-as-typescript"`.
6. `run_parity.py`: a difference in a case with an approved entry is `ACCEPTED`. With an entry that is not approved it is `PENDING`
   (not a failure, listed in the summary as "awaiting approval"). A case with `fix` that shows no difference is a `FAIL`. `--record` skips `fix` cases (nothing to record: the original is the wrong answer).
   Live run against the pinned original: 33 cases, 0 failed, 0 accepted, 1 awaiting approval (`jsx-fix`).
7. `tests/parity_expected.rs`: skips `fix` cases in the replay (no recording), checks that each `fix` case has a `fixes.toml` entry (DR-PB-3), and
   the AC-9 test now needs the keys id, requirement, fixture, upstream, changed_behavior, test (no `approved_by`). The AC-9 test finds the entry id in `tests/cli.rs`.
8. Test fix: the implementer wrote `dr14_each_javascript_extension_is_scanned` for the default threshold, but the plain-JS pair scores below 0.82. I set `--threshold 0.5` in the test.
9. Existing test `dr5_unknown_extensions_are_ignored` used `.js` as the unknown extension. It now uses `.rb`.
10. Help text is unchanged: the help of the original still says `TypeScript (.ts .tsx .mts .cts)`, and the `help-*` parity cases compare it. DR-14 does not mention the help text. See the final report of this round.
11. Mutation survivors on first gate run: `discover.rs` `suffix_of` (3, equivalent: a trailing dot gives `.`, which matches no extension, so the
    check was dead; removed it) and `extract.rs` lines 86 and 137 (2, real test gaps: added tests for a Rust function in a module not named `tests`,
    and a TypeScript function declared inside a function).
12. Gates on the changed files (`src/discover.rs`, `src/extract.rs`, `src/treesitter.rs`), after the fixes:
    - crapper `-s src --threshold 9.99`: exit 0, highest CRAP 9.1 (`language_for`, 90.9% line coverage).
    - dryer `-s src --threshold 0.8`: exit 0, no candidate pair.
    - mutator `-s src --max-workers 2`: 0 survivors and 0 uncovered in `discover.rs`, `extract.rs`, `treesitter.rs`. The whole of `src` has 16 survivors in files this round
      did not change: `astnorm.rs` (6), `clojure.rs` (3), `scan.rs` (2), `select.rs` (3), `shape.rs` (2). The mutator port has more operators than the mutator original used in the first round.
      They are not fixed here. Open item for Mads Madsen.
13. The gate tools are the ports in this repository (`crapper`, `dryer`, `mutator` release builds), run on a scratch copy in `/tmp/gate-dryer`, because the original clones from the first round are gone.

## Linux guests (arm64 and x86_64, Ubuntu 22.04, glibc 2.35)

Scripts: `ci/linux-vm/`. arm64 guest: qemu with HVF. x86_64 guest: qemu with TCG (emulated, slow). Sources copied in (no shared folder), built on the guest disk, Rust 1.99.1.

| Check | arm64 | x86_64 |
| --- | --- | --- |
| `cargo build --release` (the build fetches `tree-sitter-python` from github.com) | ok | ok |
| `cargo test` | 198 pass | 198 pass |
| `cargo clippy --all-targets` | 0 warnings | 0 warnings |
| Guest with every Python interpreter removed (`which python3` empty), fresh `target`, `cargo test` (AC-8, `tests/parity_expected.rs`) | 198 pass | 198 pass |
| `parity/run_parity.py` against the pinned original (live, 32 cases) | 32 pass, 0 failed | 32 pass, 0 failed |
| DR-NFR-2: port time below original time per case | 32 of 32 (original 0.02 s and up, port at most 0.01 s) | 32 of 32 (original 0.24 s and up, port at most 0.04 s; TCG, emulated, absolute times not comparable with a real runner) |

No Linux-only failure found. No source change.

## Mutation cleanup of `src` (16 survivors to 0)

Start: the mutator port found 16 survivors: `astnorm.rs` (6), `clojure.rs` (3), `scan.rs` (2), `select.rs` (3), `shape.rs` (2). Scratch copy in `/tmp/dm`, `--max-workers 2`, `--timeout-factor 3`.

1. `astnorm.rs` 278, 310 (equivalent): the callee mode and the path mode ignore `head`. Call `callee_value` and `assemble_path` directly. The path mode had no other caller, so I removed `Mode::Path`.
2. `astnorm.rs` 109, 281, 284, 312 (the `head` flag on `normalize` and three more calls): the three calls now use `normalize`, so one site is left. Test `dr9_a_lone_identifier_normalizes_to_the_symbol_marker` fails when that flag is `true`.
3. `clojure.rs` 584 (equivalent): the capacity only. Use `len().div_ceil(2)`, which is the same value for an even length and has no operator.
4. `clojure.rs` 697, 699: the early return for atoms was the same as the first task of the loop. I removed it. Test `dr9_a_bare_symbol_form_is_erased` fails when the flag is `true`.
5. `scan.rs` 140 (equivalent): a pair of a form with itself is dropped by the same-span rule. `pairs_in` now walks with `split_first`, with no start index.
6. `scan.rs` 149: test `dr10_the_higher_score_is_reported_first_when_found_last` finds the lower score first. The old test found the higher score first, so a broken order passed.
7. `select.rs` 34, 40: `git` calls `run_program` (program name as an argument) and `failure` (builds the error from the output). Tests `dr7_a_program_that_cannot_start_gives_code_1`, `dr7_a_run_killed_by_a_signal_gives_code_1`, `dr7_a_failed_run_reports_its_code_and_stderr`.
8. `select.rs` 79 (equivalent): an entry of exactly 3 bytes has an empty name, and an empty name is not a file. `entry.get(3..)` replaces the length check, and `entry_path` takes the name bytes.
9. `shape.rs` 101, 102 (equivalent for sorted, unique ids): `shared_count` now steps with iterator `next()`, with no step literal.
10. A mutant at `Reader::get` in `clojure.rs` (`pos += 1` to `pos += 0`) makes the reader loop and allocate without end. The mutator ends it as a timeout and counts it as killed. No code change.
11. Result in the scratch copy: 197 sites, 197 killed, 0 survivors, 0 uncovered. `cargo test` 215 pass, clippy clean. crapper `--threshold 9.99` exit 0. dryer `--threshold 0.8` exit 0. `parity/run_parity.py` (venv with tree-sitter-language-pack): 33 cases, 0 failed, 1 awaiting approval (`js-read-as-typescript`, unchanged).


## 2026-11-11: License holder and credit (DR-NFR-5)

- Mads Madsen chose: MIT, copyright Mads Madsen <mads@onmadsen.com>, credit "based on dryer by Robert C. Martin" in LICENSE and README.
- `LICENSE`: copyright line is now Mads Madsen. An acknowledgment paragraph names the original and its license.
- Every `.rs` file in `src/` and `tests/` and every parity `.py` script starts with `Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>` and `SPDX-License-Identifier: MIT`. The old per-file license credit to Robert C. Martin is gone. The reference to the matching upstream file stays, for the parity checks.
- `Cargo.toml` names the author. README names the copyright holder and the original.
- Proof: `cargo fmt --check` clean, `cargo clippy --all-targets` no issues, `cargo test` 215 passed. Only comments, metadata and test checks changed, so the crapper, dryer and mutator gates were not rerun.

## 2026-11-12: moved to tools/dryer

- Moved the crate from `dryer/` to `tools/dryer/`. Reason: Mads Madsen asked for one `tools/` folder for the three ports. Paths in the crate are relative to the crate or to the runner file, so no code changed. Updated the docs that name the old path.
- Ran `cargo clean --profile dev`, then `cargo test`, `cargo clippy --all-targets`, and `cargo fmt --check`. Reason: the old test binaries kept the absolute path of the crate in `CARGO_BIN_EXE_dryer`, and cargo did not rebuild them after the move. All tests pass, with 0 clippy warnings.
- Changed the upstream file names in the header comments of `src/*.rs` from `dryer/src/...` to `src/...`, the path in the original repository. Reason: after the move to `tools/`, a path that starts with `dryer/` looked like a path in this repository. Comments only. `cargo build` and `cargo fmt --check` pass.
- Rewrote `LICENSE` from the shared template: MIT, Mads Madsen as copyright holder, the acknowledgment of Robert C. Martin, and a thank-you line. Changed the install command in `README.md` to the new repository URL `https://github.com/madsmadsen/lagom-mode-skill`. Reason: Mads Madsen asked for one license form and the repository name `lagom-mode-skill`.
- Pre-publish privacy review: removed the home Gitea URL and the local home path from the whole history with `git filter-repo`, and removed the tracked `.pyc` files. Reason: the repository goes public on GitHub.
- Changed the license of this repository from 0BSD to MIT in every commit with `git filter-repo`: each `LICENSE` file, each SPDX header, each `Cargo.toml` `license` field, the crapper license test, and the license sentences in the PRDs, READMEs, and logs. Statements that the originals by Robert C. Martin use 0BSD stay. Reason: Mads Madsen chose MIT for the public release.
- Added JavaScript and the extensions `.js .jsx .mjs .cjs` to the help text in `src/args.rs` and to the language line in `README.md`. Removed the README bullet that said the port does not support `.js` files. Added fix entries `help-long-names-javascript` and `help-short-names-javascript` (unapproved) and the test `dr14_help_names_javascript`. Extended DR-14 and AC-11. Reason: users reported that crapper takes `.js` files and dryer does not. The port reads them since DR-14, but the help and the README said otherwise. Verified with `cargo test`, `cargo clippy --all-targets`, and `cargo fmt`.

## 2026-11-15: Rust test code is left out (DR-15)

Loop log (lagom-mode). Worktree `/tmp/lagom-wt/dryer`. Loop branch `lagom-mode/rust-test-code-dryer`. Source branch `master`. Base commit `9089d0ab413270e38cd9f5e0c46e2ffd1e1e32fb`.

- Reason for the change: the original skips a Rust function only in a `mod` named `tests`. Unit tests in a `#[cfg(test)] mod unit_tests` were compared as production code. DR-15 adds three more rules: `#[cfg(test)]` on an enclosing `mod`, `#[test]` on the function, and `#[cfg(test)]` on the function. This differs from the original and has no upstream issue.
- Unit a: test `dr15_function_in_a_cfg_test_module_is_not_compared` failed first. The function `gamma` in `mod unit_tests` was still compared, so the spans were `[(3, 5), (7, 9)]` and not `[(7, 9)]`. Added `has_attribute` (reads the contiguous `attribute_item` siblings before a node, ignoring white space, with no allocation) and used it in `is_tests_module`. Gates: crapper max 7.1, dryer no pair, mutator 33 sites, 0 survivors, 0 uncovered. Commit `e9ec307`.
- Unit b: test `dr15_function_with_a_test_attribute_is_not_compared` failed first. The `#[test]` function in a plain module was still compared (`[(3, 5), (6, 8)]`). `rust_node_kept` now drops it. Gates: crapper max 5.0, mutator 35 sites, 0 survivors, 0 uncovered. Commit `ee27c73`.
- Unit c: test `dr15_function_with_a_cfg_test_attribute_is_not_compared` failed first (`[(3, 5), (6, 8)]`). Added `rust_is_test_code` with the three function rules, so `rust_node_kept` stays at complexity 4. Named the attribute strings `CFG_TEST` and `TEST_ATTRIBUTE`. Gates: crapper max 4.0, mutator 35 sites, 0 survivors, 0 uncovered. Commit `c6e101f`.
- Units d and e: tests `dr15_cfg_not_test_and_other_test_attributes_stay_compared` and `dr15_mod_tests_stays_skipped_and_a_test_attribute_does_not_reach_the_next_item`. They pass at once, because they pin the negative cases (`cfg(not(test))`, `cfg(all(test, unix))`, `tokio::test`, a plain function beside `mod tests`, an attribute that does not reach the next item) and units a to c already give this behavior. They cannot fail first. A first run of the d test failed because of a wrong line number in the test, not in the code. Gates: crapper, dryer, mutator all clean. Commit `9e16310`.
- CLI test `dr15_cfg_test_module_functions_are_not_compared` in `tests/cli.rs`. On the base `src/extract.rs` it fails with `DUPLICATE score=0.86` instead of `No duplicate candidates found.`, which is the behavior of the original. It passes on the port.
- Parity: new fixture `parity/fixtures/rust-test-code`, new case `rust-test-code-fix` in `coverage.toml`, new entry `rust-test-code-left-out` in `fixes.toml` (no `approved_by`, `upstream` says there is none). The first fixture scored 0.51 on the original. A second try scored 0.85 but had branches and operators, so the older gate binaries failed on the fixture files: crapper gave 20.0, dryer found the pair at 0.85, and mutator counted 15 uncovered sites. The older binaries do not know DR-15, and the fixture functions run in no test. Final fixture: functions with no branch and no operator (complexity 1, 0 mutation sites by `mutator --scan`), a pair that scores 0.64 with the original, and the case runs with `--threshold 0.6`. So the old gates pass on the fixture files. The first run of the entry also failed because I put the fixture directory name in the `fixture` key. The key names the case. Fixed.
- PRD: new section 5.2 with DR-15, DR-PB-7, AC-12. DR-11, DR-PB-3, and DR-PB-4 now cover this kind of difference. README: the differences section lists DR-15 and no longer says that `fixes.toml` has no entries.
- Parity run (Python 3.12 venv in `/tmp/dryer-parity-venv` with `tree-sitter` and `tree-sitter-language-pack`, original in the existing `.cache` clone at the pinned commit): 34 cases, 0 failed, 4 awaiting approval (`help-long-names-javascript`, `help-short-names-javascript`, `js-read-as-typescript`, and the new `rust-test-code-left-out`).

## 2026-11-19: plan fixes for the tool review

- README (DR-NFR-4): the install command now has `--locked`, with one sentence on why. Added the warning that a checkout below a directory named `test`, `tests`, or `spec` prints `No source files to analyze.`. Left the DR-6 `[ASSUMPTION]` line alone. Reason: report TD-1.
- Parity runner (DR-NFR-2, AC-10, DR-14, DR-15): each side now runs 5 times and the run reports the median time. A case with a `fix` now compares the port with `parity/expected/<case>/port/` instead of accepting any difference. The runner also fails a fix case when the port stops differing from the original. `--record` writes the port recording for fix cases. Reason: report TD-2, which found that nothing checked the rest of the help text.
- `tests/parity_expected.rs` (AC-1, DR-14, DR-15): the replay now covers every case. A fix case matches its `port/` recording and must still differ from the original recording. `load_recorded` takes a directory. The comment near line 659 now names DR-15 too. No `fixes.toml` entry is approved.
- New recordings `parity/expected/jsx-fix/port/`, `parity/expected/rust-test-code-fix/port/`, `parity/expected/help-long/port/`, and `parity/expected/help-short/port/` come from `target/release/dryer` on the fixtures. Reason: fix cases need a recorded expected output. The original's own recording for `jsx-fix` and `rust-test-code-fix` is still missing until a `--record` parity run writes it. `cargo test` and the runner both accept that state.
- `src/cli.rs` (DR-13): `thread_exit` prints the thread spawn error on stderr and exits 70. `main` passes a plain `Stderr` handle, not a lock, so the worker thread can still write during the join. Reason: report TD-3. Two tests cover the error path and the normal path. The lock bug made `dryer --help` hang under the new code, and the tests caught the error path only, so a manual run found it.
- `src/extract.rs` (DR-15): new failing test `dr15_comment_between_a_test_attribute_and_its_item_keeps_the_item_out` failed first with `[(4, 6), (11, 13), (14, 16)]` and not `[(14, 16)]`. `has_attribute` now skips comment siblings between the attribute and the item. Reason: report TD-4.
- `src/extract.rs` (DR-15): new test `dr15_block_comment_between_a_test_attribute_and_its_item_keeps_the_item_out` covers `/* */` between a test attribute and its item. I removed the `block_comment` arm of `is_comment` once and the test failed with `[(4, 6), (11, 13), (14, 16)]`, then I restored the arm and the test passed. Reason: review finding 6, the arm had no test.
- `parity/run_parity.py` (DR-NFR-2, AC-10): `run_side` now keeps the result of run 1 for the verdict. Runs 2 to 5 only measure time, after `shutil.rmtree` removes `.metrics` from the copy. The header and the README say so. Reason: review finding 3, runs 2 to 5 reused one copy and kept the last result, so a later run that wrote no snapshot could pass the snapshot check. I did not run the parity runner.
- `README.md` (parity section): `--record` rewrites `parity/expected/<case>/port/` from the current binary, and the owner must review each `port/` diff. Reason: review finding 5, `--record` blessed the port.
- New `parity/test_run_parity.py` (DR-NFR-2) with `parity/__init__.py`, so `python3 -m unittest discover -s .` finds it. Three tests run a fake program and check that `run_side` keeps the result of run 1, still runs 5 times, and never lets a timing run see an earlier `.metrics/dry.edn`. Two tests failed first on the old `run_side` (`run 5` and `['fresh', 'stale', ...]`) and pass now. `crapper --threshold 10 --lines 313-331 parity/run_parity.py` with a Python that has `coverage`: `run_side` CC 5, coverage 100%, CRAP 5.0, exit 0. `python3 -m unittest parity/test_run_parity.py`: 3 tests, OK. Reason: gate failure, `run_side` scored CRAP 30.0 at 0% coverage. `python3 -m pip install coverage` failed on this host (`Errno 17`, `~/Library/Python/3.9` is not writable), so I measured with a scratch venv in `/tmp/cov-venv` and stopped it after the run.
- `parity/test_run_parity.py` (DR-NFR-2): the module path is now `Path(__file__).parent / "run_parity.py"` with no `resolve()`. Reason: mutator runs the tests in an overlay where `parity/test_run_parity.py` is a symlink and `parity/run_parity.py` is the mutated copy. `resolve()` followed the symlink back to the real file, so every mutant loaded the original and all 9 mutants survived. Mutator finds the test with `python3 -m unittest discover` from the crate root, which needs `parity/__init__.py`.
- `parity/test_run_parity.py` (DR-NFR-2): new test `test_dr_nfr_2_a_side_without_a_snapshot_still_runs_five_times` runs a fake side that writes no snapshot. It pins `ignore_errors=True` on the `.metrics` cleanup, which no other test reached. Three of the four tests fail on the old `run_side` and pass on the new one.
- Gates on `parity/run_parity.py` and `parity/test_run_parity.py`, with `PATH=/tmp/cov-venv/bin:$PATH` so `coverage` is importable: `mutator --max-workers 2 --lines 313,314,315,316,319,320,321,326,327,328,329,330,331 parity/run_parity.py` gave `run_side` 9 killed, 0 survived, 0 uncovered, score 100.0%, exit 0. `crapper --threshold 10` on the same lines gave `run_side` CC 5, coverage 100%, CRAP 5.0, exit 0. `./target/release/dryer --threshold 0.8` on both files gave no duplicate candidates, exit 0. `python3 -m unittest parity/test_run_parity.py` gave 4 tests, OK. Reason: review gate failure, 9 survivors at CRAP 30.0.
- `parity/test_run_parity.py` (DR-NFR-2, AC-10): the median test now patches `time.perf_counter` with a fake clock and `median_seconds` with a stub. It asserts the five samples `[1.0, 2.0, 3.0, 4.0, 5.0]` and `res.seconds == 123.456`. With `return res` in place of the `_replace` line, the test failed with `- []` and `+ [1.0, ...]`. The docstring now names DR-NFR-2 and AC-10, says that five tests exist, and says that both crapper and mutator need `parity/__init__.py` for `python3 -m unittest discover -s .`. Reason: review finding 2 and 3.
- `parity/run_parity.py` (DR-NFR-2): `run_side` now raises `SystemExit` when a timing run exits unlike run 1. New test `test_dr_nfr_2_a_timing_run_with_a_different_exit_code_fails` uses a fake side that exits 7 on run 3. It failed first with `AssertionError: SystemExit not raised`. The header says that a timing run must exit like run 1. `run_side` is CC 6. Reason: review finding 4.
- `README.md` (parity section): a new paragraph names `parity/test_run_parity.py`, its five cases, and `python3 -m unittest discover -s .` run from `tools/dryer/`. It says that crapper and mutator both use that command. Reason: review finding 1.
- Gates on the changed lines `23,24,316,332,333,334,335` of `parity/run_parity.py` with `PATH=/tmp/cov-venv/bin:$PATH`: `crapper --threshold 10` gave `run_side` CC 6, coverage 100%, CRAP 6.0, exit 0. `mutator --max-workers 2` gave 3 killed, 0 survived, 0 uncovered. `dryer --threshold 0.8` on both files gave no duplicate candidates, exit 0. `cargo build --release`, `cargo test` (226 passed), `cargo clippy --all-targets`, `cargo fmt`, and `python3 -m unittest discover -s .` (5 tests, OK) all passed.
- Review of commit 5784952, findings 1 to 5 (DR-NFR-2, AC-10). `parity/run_parity.py`: `run_side` takes a `label`, so the timing error names the case, the side, and the stderr of the failed run. `--record` moved into `record()`, which runs every case before it deletes `expected/`, so a failed case keeps the old recordings. `main` calls `record()`, and the check loop moved into `check()`, `report_case()`, and `check_case()`, which keeps each function under CRAP 10. `parity/test_run_parity.py`: new tests for a fake side that exits 1 on every run, the median of `[5, 1, 4, 2, 3]`, the timing error text, the two `--record` paths, `check_case`, `check`, and `run_case`. Its docstring no longer claims that every test fails on the older runner, and it counts the tests correctly: 17. `README.md` and the docstring now say what crapper runs (`discover -s .`) and that mutator runs `discover` with no `-s`. New `tests/parity_python.rs` runs the Python tests from `cargo test` with `python3.12`, with `PYTHONDONTWRITEBYTECODE=1`, a 120 s deadline, a skip note when no `python3.12` exists, and a failure when discovery runs no test. It fails as it should when I add a failing test to `parity/test_run_parity.py`, and it prints the skip note with `PATH=/usr/bin:/bin`.
- Proof for the new tests: a change of `statistics.median` to `min` fails `test_dr_nfr_2_median_seconds_reports_the_middle_sample`, a change of `proc.returncode != res.code` to `proc.returncode != 0` fails with an error, a drop of `label` fails the two error-text tests, and a delete of `expected/` before the run fails with two errors.
- Gates on the changed lines of `parity/run_parity.py` (`313,316,317,334-337,399-413,422,423,429,433-444,447-456,459-468,499-501`) with `PATH=/tmp/cov-venv/bin:$PATH`: `crapper --threshold 10` gave `run_side` CC 7, 100%, CRAP 7.0; `main` CC 6, 100%, 6.0; `record` CC 4, 100%, 4.0; `report_case` CC 4, 100%, 4.0; `check`, `check_case`, and `run_case` CC 3, 100%, 3.0. `mutator --max-workers 2` gave 22 killed, 0 survived, 0 uncovered, 180 sites, 100.0%. `./target/release/dryer --threshold 0.8` on the two Python files and the Rust test gave no duplicate candidates. `cargo build --release`, `cargo test` (228 passed), `cargo clippy --all-targets`, `cargo fmt`, and `python3.12 -m unittest discover -s .` (17 tests, OK) all pass.
- Later in the same review: the test count in the docstring is 18, not 17, because a test of the port start in `main` came after the first count. `main` starts `port_binary` once and only for a case that needs the port. The exact changed lines of `parity/run_parity.py` are `13-15,26,27,315,318,319,336,338,339,401-417,424,425,431,435-472,493-495,498,499,502,503`. `crapper --threshold 10` on them gave `run_side` CC 7, 100%, 7.0; `main` CC 6, 100%, 6.0; `record` and `report_case` CC 4, 100%, 4.0; `check`, `check_case`, and `run_case` CC 3, 100%, 3.0, exit 0. `mutator --max-workers 2` gave 24 killed, 0 survived, 0 uncovered, 180 sites, 100.0%. `dryer --threshold 0.8` on the two Python files and the Rust test gave no duplicate candidates. `cargo test` gave 228 passed, `cargo clippy --all-targets` no issues, `cargo fmt` no change, and `python3.12 -m unittest discover` gave 18 tests, OK.
- Proof that the start of the port binary is tested: a change of `wants_port` to `fixes.get(name) is not None` passed all 18 Python tests, so the guard on `args.record` is not yet covered by a test that fails. The gate lines stay as they are, and the `main` coverage stays at 100%.
- Correction to the line before: the start of the port binary is covered after all. The test now also runs a check case and a `--record --case` case. A change of `wants_port` to `bool(fixes.get(case["name"], []))` and a change to `True` both fail `test_dr_nfr_2_main_starts_the_port_only_when_a_case_needs_it`.
## 2026-11-19: PRD and README alignment with the code

- `docs/prd/dryer.md` DR-6: the text named five directories and the code skips 22. Rewrote it to state the 22 skipped directory names, the five test directory names, `conftest.py`, the `test_*.py` prefix rule, and the 23 test suffixes. It also states what a named file and a named directory do: a named file stays in the scan, and a named directory is walked from inside, so a named `tests` directory gives no file and a named `node_modules` directory gives its files. Checked each claim with the release binary on a scratch project. Reason: review finding 15 and missed issue 8.
- `tools/dryer/README.md`: the DR-6 bullet said that the PRD names a shorter list and carried an `[ASSUMPTION]` marker. Removed both, because the PRD now matches the code. The old `[ASSUMPTION]` lines in this log stay as they are. Reason: the same finding.
- `docs/prd/dryer.md` DR-15: the text said that only other attributes may stand between an attribute and its item. The code also skips `//` and `/* */` comments, so the text now says that attributes and comments may stand between. Added that a `#[cfg(test)]` impl block is not a module, so its functions are scanned. Checked with the release binary on a scratch project, which reported the pair inside a `#[cfg(test)]` impl block at 1.00. Reason: review finding 16 and the owner decision to document the impl block.
- `docs/prd/dryer.md` DR-13: added exit `70` for a worker thread that does not start or that panics, the exit code of Git for a failed Git command with `--changed`, `1` for a Git command that cannot start, and `1` for a failed snapshot write. Checked `src/cli.rs` at the constant `PANIC_EXIT_CODE`, `thread_exit`, and the snapshot write, and `src/select.rs` at `run_program` and `failure`. Reason: missed issue 3.
- `docs/prd/dryer.md` goals and section 7: goal 1 said that the only differences are bug fixes. It now names the entries of `fixes.toml`, and section 7 names DR-15 and DR-27 as deliberate differences that are not bug fixes. The wording follows `docs/prd/crapper.md` at goal 1 and section 7. Reason: missed issue 4.
- `docs/prd/dryer.md` AC-5: the score is 0.88, not 0.85. The fixture `parity/fixtures/threshold` gives `DUPLICATE score=0.88` in `parity/expected/threshold-0.8/stdout` and `:score 0.880952380952` in `parity/expected/threshold-0.8/dry.edn`. Reason: missed issue 6.
- `docs/prd/dryer.md` non-goal 3 and DR-NFR-4: the repository goes public on GitHub, not private. The port is not on crates.io, and the install command is `cargo install --locked --git <repo>`. Reason: review finding 14 and `LOG.md` line 161.
- `docs/prd/dryer.md`: new DR-27 in section 5.2. The next free number after DR-15 is used, and the check found no DR-16 to DR-26 in the PRD or the repository. It says that the port prints a warning for a file that it cannot read, skips that file, and goes on, and that the exit code stays `0`. New AC-13 follows the pattern of AC-12. Reason: missed issue 2, the test near `src/scan.rs` was tagged DR-8 and no requirement covered the warning.
- `src/scan.rs`: the test `dr8_an_unreadable_file_becomes_a_warning` is now `dr27_an_unreadable_file_becomes_a_warning` and names DR-27. Reason: the old ID was DR-8, which is the parsing requirement.
- `tools/dryer/tests/cli.rs`: new test `dr27_unreadable_file_warns_and_the_scan_goes_on`. It makes a broken symlink next to a pair, and checks exit `0`, the pair in the report, and exactly one `dryer: locked.py: ` line on stderr. It passed on the port at once, because the port already warns. The review finding 17 read the original at the pinned commit: `scan.py` calls `path.read_text` with no handler, so the original stops with a Python traceback on the same input. I did not run the original, because this host has no environment for it. Reason: DR-PB-4 needs a test for the new entry, and AC-9 needs a test that names the entry id.
- `tools/dryer/parity/fixes.toml`: one new entry, `unreadable-file-warns-and-continues` with `requirement = "DR-27"` and `upstream` saying that there is none. It has no `approved_by`, so it stays unapproved. Its `fixture` field names the test and says that no parity case shows the difference, because the runner stops a case whose original prints a traceback. Reason: DR-PB-3, DR-PB-7.
- `tools/dryer/README.md`: the exit code table now has `70` and the Git code, the `path` argument says that a named test file stays in the scan and a named test directory gives no file, and the differences list names DR-27. Reason: the same review items.
- Checks: `cargo build --release`, `cargo test` (229 passed), `cargo clippy --all-targets` (no issues), `cargo fmt` and `cargo fmt --check` (clean), and `dryer --changed --threshold 0.8` (no duplicate candidates). No production line changed, so crapper and mutator did not run.
