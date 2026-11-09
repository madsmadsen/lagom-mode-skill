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
