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
