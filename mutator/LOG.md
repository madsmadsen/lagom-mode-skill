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
