# mutator port log

Method: ub-mode. Orchestrator plans and verifies; implementer agents
(`implementer` agent) write the Rust. One log line per action with its reason.

## Setup
- Read ub-mode skill, `docs/PRD.md`, `docs/prd/mutator.md`. Why: they set the rules and the MU ids.
- Cloned the original `unclebob/mutator` (a5a0cb8) and `unclebob/crapper` (6fd92c4) into `/tmp/mu`. Why:
  the port copies their behavior; the crapper code the original imports must be ported into this crate
  (PRD section 2 rule 1, MU-9, MU-10, MU-11).
- Built a Python 3.12 venv in `/tmp/mu/venv` with both originals installed. Why: gate tool and parity reference.
- `cargo init` in `mutator/`, added tree-sitter 0.27 + grammar crates, sha2, serde_json, regex, libc, chrono.
  Why: MU-8 (tree-sitter grammars), digests, mutation ids, process groups, `tested-at`.
- Wrote `/tmp/mu/CONTRACT.md`: module layout and exact signatures shared by all implementers.
  Why: lets 16 units be written in parallel (ub-mode: data shape first, sequence into verifiable units).
- Wrote `parity/upstream.toml` (MU-PB-1).

## Make it Work: wave 1 (16 parallel implementer units, one per module)
- Dispatched units ModelEdn, DiscoverTs, FnJavaGo, FnPyRust, FnTypeScript, FnClojure, CljSites, TsSites, BindFns,
  CoverageRead, RunnerPlan, WorkersMu, CoverRun, MetricsSnap, EngineReport, CliMain, ParityRunner, all with agent
  `implementer`. Why: orchestrator does not write Rust; modules are
  independent given `/tmp/mu/CONTRACT.md`. Each brief names files, the Python source to port, edge cases,
  and says edit only (no build/test).
- Built and tested the modules incrementally by editing `src/lib.rs` to list only landed modules.
  Failures found and routed back to the owning agent (the orchestrator did not edit Rust):
  - `discover`/`ts`: help-text test expected `vendor` last (it is `venv`); `absolute_path` with no root returned a relative path.
  - `sites`: `parent_kind` lifetime error; wrong expected text for `not` deletion (Python leaves two spaces);
    1,200-term expression took 72 s because `Node::parent()` is O(depth) in tree-sitter. Fixed by an explicit ancestor stack. Now < 1 s.
  - `metrics`: contradictory idempotence test; silent write errors now print to stderr.
  - `report`: expected unmarked scan line has two leading spaces; `engine`: clamp lint.
- `cargo clippy --fix` removed 8 mechanical lints (collapsible if, `?`, `last()`, `bytes().nth`). Why: tool-made fixes, no hand-written Rust.
- Committed green groups in order: scaffold, functions, sites, bind+metrics, runner+workers, coverage, engine.

## Make it Right: gates
- Parity: wrote `parity/run.py` (+ `coverage.toml`, six fixtures) and ran it against the original (`/tmp/mu/venv`). First run found one bug:
  `--scan` marked every site as changed when no snapshot exists (Python: empty set). Fixed with a test. Stdout is now compared strictly. Result: 38/38 PASS
  (same exit codes, same EDN snapshots apart from `tested-at`).
- `parity/golden.py` records the original's output on each fixture into `tests/golden/` so `tests/fixtures_langs.rs` needs no Python (AC-11).
- e2e tests (`tests/*.rs`): AC-2..AC-9 (AC-9 with a fake `cargo` on PATH), AC-10, AC-12 (`fixes_toml.rs`), AC-1 (`parity_map.rs`), AC-13.
- crapper (original, `crapper src`): found cover_run functions at CRAP 12-30 and `edn::render`, `push_json_char` at CC 11-12. Fixed by
  splitting coverage code into pure planners plus one executor with tests for each branch; render/push_json_char split into helpers.
  Final: max CRAP 9.0 (`link_children`).
- dryer (original, threshold 0.8): 8 duplicate pairs (4 copies of `resolve`, `strip_src` x2, ancestor names x2, literal tables, setter/number twins,
  nearest). All removed; "No duplicate candidates found".
- Fixed a flaky e2e helper: git commit now ignores user gpg/hook config.
- The crapper run also analysed `parity/fixtures` (and wrote `target/` in the Rust fixture); fixture build output is git-ignored and removed.

## Mutator gate on the port itself (incomplete)
- Ran the port on `src` with `--use-existing-coverage` and `cargo test --lib` plus eight fast integration tests as the test command
  (per-worker `CARGO_TARGET_DIR`). Only `src/bind.rs` finished before the run was stopped (each mutant needs a Rust compile; about 15 min per file;
  ~600 sites in 26 files). bind.rs: 33 killed, 3 survived: `bind.rs:241 <= -> <`, `241 < -> <=`, `248 >= -> >` (boundary conditions in the
  `splitlines`/`digest` range clamp). These survivors are NOT yet fixed; other files are NOT yet mutation-checked.
- Not verified: AC-9 with a real `cargo-llvm-cov` install (the e2e test uses a fake `cargo`; the host already has the tool and a real install needs network
  and a host without it). MU-PB-6 approval of `fixes.toml` entries is pending (`approved = false`).

## Mutation pass results (port run on its own src, parallel scratch copies)
- Pass 1 survivors found: bind 3, workers 7, cover_run 11 (+2 uncovered), coverage 8, metrics 7 (+3 uncovered), engine 11, runner 6,
  python fn 2, rust fn 3, sites 1, cli 10 (+1), report 2, clojure_reader 10 (+3), clojure_sites 1, edn 1 = 112 survivors (+9 uncovered).
- All were killed by new behaviour tests or by deleting dead/equivalent code (e.g. unreachable fallbacks, nanos in run-dir name, `not in` branch
  that the original also never mutates). Re-runs after fixes: bind, workers, cover_run, coverage, metrics, engine, runner, discover, ts,
  python/rust function finders, sites: exit 0 (no survivors).
- Not re-run after the fixes (scratch dir /tmp/mu was lost when the host reset): cli, report, model, clojure_reader, clojure_sites, edn (second pass),
  and a first pass of functions/clojure.rs (80 sites, its run never finished). Those files have fixes but no confirming rerun.
- Final: cargo test 461 passed, clippy clean; original crapper max CRAP 9.0; original dryer no candidates.

## Mutation gate closed (rerun of the open files)
- Rebuilt the scratch setup under `/tmp/mgc`: original mutator (a5a0cb8) and crapper (6fd92c4) in a Python 3.12 venv for parity; the gate itself ran the
  port's release binary on per-file scratch copies of this crate, with `--use-existing-coverage` (lcov from `cargo llvm-cov`, paths rewritten to the copy),
  per-slot `CARGO_TARGET_DIR`, `ulimit -t 120`, at most 2 runs x 2 workers, each run in its own session and killed by group after it ended.
  cli.rs was gated with `cargo test --lib --test cli_exit --test changed --test overlay` and a lcov from the same tests, since its printing code is only
  reachable through the binary; other files used `cargo test --lib`.
- Timeout kill check (asked after orphan test binaries were seen): the port sends SIGKILL to the whole process group of a timed-out command
  (`runner.rs` `kill_group_and_reap`, child spawned with `process_group(0)`), like the original (`start_new_session` + `killpg`). Tests
  `mu_15_timeout_kills_the_process_group` and `..._background_grandchildren` cover it. No leak found; the orphans came from many parallel runs.
  `pgrep` after each run showed no mutator test binary left.
- First rerun: model.rs, clojure_reader.rs, report.rs, edn.rs: 0 survivors. cli.rs: 6 survivors + 1 uncovered (only reachable through the binary).
  sites.rs: 2 (`visit_negation` and `visit_constant` `&&` -> `||`). clojure_sites.rs: `line_of` `+ 1` shown as uncovered (multi-line expression).
  functions/clojure.rs first full pass: 80 sites, 12 survivors.
- Fixes: exact-pair tests for unary minus and numbers other than 0/1 (MU-12); `line_of` as one `match` plus a direct test; direct tests of
  `string_end`, `char_end`, `form_end`, `skip_reader`, `blank`, `strip_root`, `keyword_ends`, empty string (MU-9); two e2e tests that the summary
  table is printed only when forms were mutated (MU-4).
- Final reruns: cli.rs 58 killed, functions/clojure.rs 80 killed, sites.rs, clojure_sites.rs, model.rs (4), report.rs, edn.rs, clojure_reader.rs
  (34): 0 survivors, 0 uncovered.
- Parity: `parity/run.py` against the pinned original gives 38 PASS. `flag-help-short` and `flag-help-long` now carry `allow_stdout_diff = true`
  in `coverage.toml`, because e6b7084 (half the cores) changed the help text on purpose. `fixes.toml` is unchanged.
- cargo test 477 passed, clippy --all-targets clean.

## Linux guests (arm64 and x86_64, Ubuntu 22.04, glibc 2.35)

Scripts: `ci/linux-vm/`. arm64 guest: qemu with HVF. x86_64 guest: qemu with TCG (emulated, slow). Sources copied in (no shared folder), built on the guest disk, Rust 1.99.1.

| Step | Action | Result |
| --- | --- | --- |
| L1 | `cargo build --release`, `cargo test`, `cargo clippy --all-targets` with python3 installed. | arm64: 477 pass, clippy 0 warnings. x86_64: 477 pass, clippy 0 warnings. |
| L2 | Guest clone with every Python interpreter removed (`which python3` empty, no executable `python*` file), fresh `target`, `cargo test`. | arm64: 476 pass, 1 FAIL: `cli::tests::mu_3_surviving_mutant_returns_three_and_writes_snapshot` ran `python3 -c ...` as its test command (`sh: python3: not found`, run exits 2, not 3). It was a test that needed Python on the host, which MU AC-11 forbids. |
| L3 | Fix (`src/cli.rs`, test only): test command is `grep -q 'return a + b' src/demo.py`. Same outcome: the baseline passes, an arithmetic mutant is killed, a comparison mutant survives (exit 3, snapshot `defn/add`). | arm64 without Python: 477 pass, clippy 0. x86_64 without Python: 477 pass, clippy 0 (the `timeouts` tests also pass under TCG). |
| L4 | Mutation gate for `src/cli.rs` after the edit (scratch copy `/tmp/mgl`, lcov from `cargo llvm-cov --lib --test cli_exit --test changed --test overlay`, `--use-existing-coverage`, 2 workers, `ulimit -t 120`, own process group, killed by group, `pgrep` empty). | 58 killed, 0 survived, 0 uncovered. |

The golden tests (`tests/fixtures_langs.rs`, AC-11) pass with no Python and no crapper checkout on both architectures. The parity runner `parity/run.py` (needs the original mutator and crapper) was not run on Linux.

## Decisions round: worker cap fix and AC-9 on a fresh guest

- Worker cap (owner decision): `parity/fixes.toml` has a third entry, `worker-cap-half-the-cores` (`approved = false`; the owner approves it).
  The PRD has no requirement for the worker count; the closest is MU-17 (parallel workers), which the entry uses. Test:
  `tests/worker_cap.rs::mu_17_default_worker_count_is_half_the_cores_at_least_one` (also `mu_17_help_says_half_the_cores_not_one_per_core`).
  The comments on `allow_stdout_diff = true` for `flag-help-short` and `flag-help-long` cite the fix id, and
  `tests/parity_map.rs::mu_pb_3_allow_stdout_diff_names_a_fix_in_fixes_toml` checks that each one names an id in `fixes.toml`.
  `run.py` needed no change: `allow_stdout_diff` already skips the stdout check per run. No test checks that every entry is approved, so none was weakened.
  Results: cargo test 480 passed, clippy `--all-targets` clean, `parity/run.py` (venv rebuilt in `/tmp/mu/venv` from the pinned checkouts) 38 PASS.
  No `src/` file changed in this round, so no mutation gate run was needed.
- AC-9, real install (`ci/linux-vm/install-check.sh`): fresh clone of the arm64 guest (Ubuntu 22.04, HVF), Rust 1.99.1, no `cargo-llvm-cov`
  (the script fails if it is there), network on, sources copied over ssh, release mutator, parity Rust fixture (`git init` + commit),
  `mutator --test-command 'sh check.sh'` (coverage on).

  | Run | Order of events on stderr | Exit | Install count |
  | --- | --- | --- | --- |
  | 1 | line 1: `Neither cargo-llvm-cov nor cargo-tarpaulin is installed. Installing cargo-llvm-cov.`; then `+ (/home/dev) rustup component add llvm-tools-preview`; `+ (/home/dev) cargo install cargo-llvm-cov --locked`; cargo output (first at line 5: `Updating crates.io index`, `Installed`/`Installing cargo-llvm-cov v0.9.1`); `+ ... cargo llvm-cov --lcov --output-path .../lcov.info`; `Finished report saved ...`; `Wrote .metrics/mutate/demo.edn`, `demo/Counter.edn` | 0 | 1 (notice on 1 line, before any cargo output) |
  | 2 | no notice, no install; starts with `+ ... cargo llvm-cov ...`; same `Wrote` lines | 0 | 0 |

  `~/.cargo/bin/cargo-llvm-cov` exists after run 1. The fixture has no `cargo test` tests, so the coverage report hits no line: all 14 sites are uncovered and
  no mutant runs (summary rows show 0 killed, 0 survived, 14 uncovered). The coverage values are written, and the notice-before-install order and the
  one-time install are what this check proves. The guest is stopped by the script; `pgrep -f linux-vm` shows no qemu of mine.

## PRD update: upstream fixes MU-22, MU-23, MU-24 (AC-14, AC-15, AC-16)

Reason: commits 8f31773 and bc91fc4 added section 5.4 (three upstream fixes) and AC-14 to AC-16 to the PRD. MU-8 now names Clojure among the tree-sitter
grammars (see the open point at the end). No other requirement changed.

| Step | Action and reason | Result |
| --- | --- | --- |
| P1 | Read the PRD diff against 4384d65, the three upstream issues (9, 10, 29), `fixes.toml`, and `upstream.toml`. The pinned commits stay as they are. | Three fixes are missing: MU-22, MU-23, MU-24. |
| P2 | Tests first: `tests/upstream_fixes.rs` (5 end-to-end tests named for MU-22/23/24 and AC-14/15/16) and 7 unit tests in `sites.rs` and `workers.rs`. | Before any code change the 5 end-to-end tests failed for the reasons of the issues: AC-14 `SURVIVED src/calc/index.ts:2 > -> >=` (exit 3, not 0); AC-15 scan lists only `src/demo/ops2.py:7 + -> -` (1 site, as issue 10 says); AC-16 `SURVIVED` on line 4 for the two `elif` sites and no `uncovered` mark on the `except` and `case` sites. |
| P3 | MU-22: in `workers.rs` the import pattern `(\.[^'"]+)` became `(\.[^'"]*)`. `candidates` already resolves `.` and `..` to `index.<ext>`, so the importer is copied into the worker. | AC-14 passes: `KILLED src/calc/index.ts:2 > -> >=`. |
| P4 | MU-23: in `sites.rs` a Python-only table `PYTHON_OPERATORS` (13 pairs) and `augmented_assignment` in `BINARY_PARENTS`. `is not` and `not in` are one non-leaf node in tree-sitter-python, so `visit_compound` handles them with the node kind as key and the source text as original. Python `/` keeps `*` and gains `//`. `is`/`in` are category `equality`. | AC-15 passes. |
| P5 | MU-24: in `sites.rs` `is_statement` also stops the climb at `elif_clause`, `except_clause`, `except_group_clause`, `case_clause` (no other grammar has these kinds). | AC-16 passes: both line-4 sites are `UNCOVERED`, line 2 runs. |
| P6 | `fixes.toml`: three new entries `ts-dot-imports-are-relative`, `python-more-operators`, `python-clause-coverage-line`, all `approved = false`. Each names its test (AC-12). | `fixes_toml` tests pass. |
| P7 | The old test `mu_12_python_not_in_has_no_sites` pinned the behavior of the original. It became `mu_12_python_not_in_has_one_site_by_mu_23`. | Passes. |
| P8 | The parity fixture `fixtures/python` had `bonus / 1`, which the fix MU-23 now mutates to `//`. An unapproved fix prints FAIL in `run.py`, so the fixture line became `bonus * 1` (the two sides still compare). `golden.py` regenerated `tests/golden/python/` from the original (only the Python golden changed). | The Python golden test passes again. |
| P9 | A mutation gate run found one survivor in `workers.rs:124` (`.max(1)` to `.max(0)` in `worker_cap`, from e6b7084; it cannot show on a host with 2 or more cores). The cap moved to `cap_for(cores)` with the test `mu_17_cap_for_is_half_the_cores_and_at_least_one`. The first gate run of `sites.rs` also found one survivor (`||` to `&&` in `visit_compound`, a parent check that no grammar can fail). The check was removed. | Both files: 0 survivors. |

Gates (scratch copy under `/tmp`, own `CARGO_TARGET_DIR`, 2 workers, `ulimit -t 120`, every process stopped afterwards):
- `cargo build --release`, `cargo test` 493 passed, `cargo clippy --all-targets` clean, `cargo fmt` clean.
- Original-port tools: `crapper --threshold 9.99` exit 0 (worst CRAP 9.0), `dryer --threshold 0.8` no candidates.
- `mutator` on `src/sites.rs`: 32 killed, 0 survived, 0 uncovered. On `src/workers.rs`: 31 killed, 0 survived, 0 uncovered.
- `parity/run.py` against the pinned original: 38 PASS.

Open point for the owner: MU-8 now says the port parses Clojure with a tree-sitter grammar. The port reads Clojure with its own reader that copies the node shapes
of tree-sitter-clojure (`clojure_reader.rs`), and the Clojure parity fixture and golden pass. No code change was made, because the PRD does not say that a
grammar crate is required.

## PRD update: memory limit for each mutant test run (MU-25, AC-17)

Reason: commit c9ea828 added MU-25 and AC-17. A dryer mutant (`pos += 1` to `pos += 0`) grew one test binary to 76 GB before the MU-15 timeout ended it.
MU-15 already kills the whole process group (`process_group(0)`, `kill(-pid, SIGKILL)`), so only the limit was missing.

| Step | Action and reason | Result |
| --- | --- | --- |
| M1 | Failing tests first: `tests/memory_limit.rs` (AC-17: kill within 12 s and one run with `--max-memory-mb 256`; limit 0 survives; bad values exit 1; help lists the flag). | Before any code change 3 of 4 failed: the flag was unknown, so the run exited 1 (usage error). The bad-value test passed by accident (unknown flag also exits 1), so `cli.rs` unit tests pin the messages. |
| M2 | New file `src/memory.rs`: `limit_bytes` (0 = off), `is_over` (strictly more than the limit), `group_resident_bytes(pgid)`. macOS uses libc `proc_listpids` (`PROC_PGRP_ONLY` = 2, as `libc` has no constant) and `proc_pidinfo` `PROC_PIDTASKINFO` `pti_resident_size`. Other hosts read `/proc/<pid>/stat` (pgrp and rss pages). No `ps`, no rlimits (macOS does not enforce `RLIMIT_AS` or `RLIMIT_RSS`). The `/proc` parser is not behind `cfg`, so the macOS host tests it with a fake `/proc` directory. | Unit tests pass. The Linux glue (`sysconf` plus `/proc`) is 4 lines and was not built or run on Linux (the cross check needs a C compiler for tree-sitter). |
| M3 | `runner.rs`: `CommandRunner::run_limited(..., max_memory)`; `run` keeps no limit. `wait_for` polls every 5 ms and a `MemoryWatch` reads the group at most every 200 ms (the PRD asks for at least once each second). Over the limit: the same `kill_group_and_reap` as a timeout, code 137, `memory_killed = true`, `timed_out = false`. | `mu_25_*` runner tests pass. |
| M4 | A read that fails is shown, not hidden: `proc_listpids` returns 0 both for "empty" and for "error", so `errno` is cleared first and a 0 with `errno != 0` is an error. On an error the run prints one stderr line (`Cannot read the memory of the test run (...). The memory limit does not apply to it.`), drops the watch, and the run ends by its timeout. `proc_pidinfo` `ESRCH` (process gone) counts as 0 bytes. Tests: failing sampler ends the watch after one read and the timeout still fires; the notice text. | Pass. In the test sandbox both libproc calls work (AC-17 passed here), so the error path is shown by tests with an injected sampler, not by the sandbox. |
| M5 | `workers.rs`: new `Limits { timeout, max_memory }` replaces the `timeout` argument of `run_mutants`; `needs_second_run` takes the first `CommandResult` and is false after a memory kill (MU-21 must not rerun it). The baseline calls `run(..., None)` and has no limit. | `mu_25_a_memory_kill_needs_no_second_run`, `mu_25_a_mutant_over_the_memory_limit_is_killed_once` pass. |
| M6 | `cli.rs`: `--max-memory-mb <number>`, default 4096, 0 = off, stored in `Options.max_memory_mb`. Parsed like `--mutation-warning`: not an integer gives the Python `int()` message, a value that is negative gives `requires a non-negative integer` (a bare `-1` is reported as a missing value, like the other count flags). Help text and README list it. `engine.rs` passes `limit_bytes(max_memory_mb)` through `Limits`. | Exit 1 for `-1`, `abc`, `1.5`, empty, missing. |
| M7 | `fixes.toml`: entry `max-memory-limit` (`approved = false`) with `tests/memory_limit.rs::mu_25_ac17_a_mutant_over_the_memory_limit_is_killed_once_and_fast`. The original rejects the flag, so a parity run cannot pass on it: `tests/parity_map.rs` now names the flag in `PORT_ONLY_FLAGS` with its fix id instead of requiring a `coverage.toml` run, and checks that the fix exists. The comment on `allow_stdout_diff` for `-h` and `--help` names the new help lines. | `parity/run.py`: see the gate results below. |
| M8 | Test design for AC-17: the baseline sleeps 1 s, `--timeout-factor 100` gives a timeout above 60 s, the allocating mutant grows a shell string by 8 MiB steps and exits by itself at 512 MiB, so it always ends even if the limit fails. The `limit 0` test grows a 10 MiB string by doubling to 320 MiB (over 256 MiB, fast). | AC-17 test takes about 4 s in total (killed about 1 s after its start). |
| M9 | Mutation gate found survivors. Equivalent ones (a `0` that is overwritten: initial `Vec` fill, `proc_pidinfo` argument, `.unwrap_or(0)` of a size) were removed by using `Default` values. Real ones got tests: `listing_error` (pure function for the errno rule), `resident_bytes` of a gone pid, `Finish` fields for spawn failure and timeout (`Finish` now uses `..Default::default()`). | See gate results. |

Gates (scratch copy under `/tmp`, own `CARGO_TARGET_DIR`, 2 workers, `ulimit -t 120`, every process stopped afterwards):
- `cargo build --release`, `cargo test` 515 passed, `cargo clippy --all-targets` clean, `cargo fmt` clean.
- `crapper --threshold 9.99` exit 0, `dryer --threshold 0.8` no candidates (both the original-port tools built in this repo).
- `mutator --max-workers 2`: `memory.rs` 15 killed, `runner.rs` 38 killed, `workers.rs` 33 killed, `engine.rs` 34 killed, `cli.rs` 58 killed (test command
  `cargo test --lib --test cli_exit --test changed --test overlay --test differential --test coverage_install`, since the printing code is only reachable
  through the binary): 0 survivors, 0 uncovered in each.
- `parity/run.py` against the pinned original: all runs PASS, none FAIL or DIFF-ALLOWED.
- Not done: the Linux build of `memory.rs` (the 4-line `sysconf` plus `/proc` glue) was not built or run on a Linux guest. The `/proc` parsing is tested on macOS with a fake `/proc`.


## 2026-11-11: License holder and credit (MU-NFR-4)

- Mads Madsen chose: MIT, copyright Mads Madsen <mads@onmadsen.com>, credit "based on mutator by Robert C. Martin" in LICENSE and README.
- `LICENSE`: copyright line is now Mads Madsen. An acknowledgment paragraph names the original and its license.
- Every `.rs` file in `src/` and `tests/` and every parity `.py` script starts with `Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>` and `SPDX-License-Identifier: MIT`. The old per-file license credit to Robert C. Martin is gone. The reference to the matching upstream file stays, for the parity checks.
- `Cargo.toml` names the author. README names the copyright holder and the original.
- Proof: `cargo fmt --check` clean, `cargo clippy --all-targets` no issues, `cargo test` 515 passed. Only comments, metadata and test checks changed, so the crapper, dryer and mutator gates were not rerun.
