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
