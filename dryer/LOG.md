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
