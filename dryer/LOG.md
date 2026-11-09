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

- DR-5 and DR-8 mention JavaScript. The original has no `.js` extension and only TypeScript grammars. The port has none.
- DR-6 lists five skipped directories. The original skips more (`.git`, `dist`, `build`, `tests`, `specs`, `__tests__`, ...). The port copies the original list.
- DR-12 and the help text of the original also have `--root` and `-s/--source-root`, which the PRD list omits. The port has them (DR-2).
- Suspected upstream quirk, copied for parity and NOT added to `fixes.toml` (needs owner approval, DR-PB-6): `is_test_file` looks at
  every component of the absolute path, so a project that sits under a directory named `test`, `tests`, `spec`, ... finds no files.
- Python `int()` accepts non-ASCII digits (for example Arabic-Indic). The port accepts ASCII digits only. Not reachable from tests of the original.
- Original crashes with a traceback when a metrics file cannot be written or git is missing. The port prints `dryer: <error>` and
  exits 1 (the original also exits 1 in these cases).

## Blockers and open items

- DR-NFR-3 (macOS 14 arm64 and x86_64, Linux x86_64 and arm64): only macOS arm64 was exercised. Nothing in the crate is platform specific
  except the tree-sitter C grammars, which build with the default C toolchain.
- DR-NFR-4: the README shows `cargo install --git https://github.com/<owner>/deterministic-coding dryer`. The real URL is not known. `[ASSUMPTION]`.
  Because `tree-sitter-python` is a git dependency, the build needs network access to github.com for that crate.
- DR-PB-3, DR-PB-6, AC-9: `parity/fixes.toml` has no entries because no fix is approved. AC-9 has a test that parses the file and
  checks that each entry id appears in a test (it passes with zero entries).
- AC-8 is met by `tests/parity_expected.rs`, which replays `parity/expected/` (recorded from the original) with no Python.
- uml-viewer check (PRD AC-5): no `fixes.toml` entry changes snapshot keys, so none is needed.
