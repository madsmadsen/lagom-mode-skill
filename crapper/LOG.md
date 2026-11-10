# crapper port: log of actions and reasons

Orchestrated by `CrapperOrchestrator`. Every Rust file was written by an implementer agent (`implementer`, about 30 tasks in all). The orchestrator only wrote Cargo.toml, README, LICENSE, .gitignore, parity/upstream.toml and this log, and ran every build, test, format and lint step once per phase.

## Method (ub-mode)

1. Make it work: data shape first (`Function`, `Entry`), tests ported from the Python tests before code, smallest change.
2. Make it right: behaviour tests with CR/AC IDs, crapper gate (CRAP < 10), dryer gate (< 0.8), mutator gate, kill survivors.
3. Practice: this log, small ordered commits that say why, what, proof.

## Actions and reasons

| Step | Action | Reason |
| --- | --- | --- |
| 1 | Read the skill, `docs/PRD.md`, `docs/prd/crapper.md`. | Rules and IDs. |
| 2 | Cloned `unclebob/crapper` to `/tmp/crapper-work/upstream`. HEAD is `6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d`. | Pin the original (CR-PB-1). |
| 3 | Installed the original in a venv (Python 3.12, tree-sitter 0.26.1, tree-sitter-language-pack 1.25.0). Its own tests: 145 pass, 1 fails because `bb` is not installed. | The original works as a gate and parity oracle. No need to hand-apply thresholds. |
| 4 | Chose crates: tree-sitter 0.25 with per-language grammars (java 0.23, go 0.25, typescript 0.23, javascript 0.25, rust 0.24, python 0.25), roxmltree, regex, serde_json. Dev: tempfile, toml. | CR-9. Static grammars need no run-time download. |
| 5 | Wrote `/tmp/crapper-work/CONTEXT.md`: module contract, header rule (CR-NFR-5), test-naming rule. | One shared contract so 9 implementers could work in parallel. |
| 6 | Batch 1 (9 implementers): core (model, crap, metrics, report), discover/analyze/lib/main, cli, language base (ts helpers, java, go), rust+python, typescript, clojure, coverage, runners. | Independent files, one owner each. |
| 7 | The runners implementer stalled (3 requests in 8 minutes). Killed it, split into three units (mod, typescript, rust+python) with a fixed contract. | Smaller units finish. |
| 8 | The coverage implementer wrote to `<repo>/src/coverage/` (wrong root). Moved the files into `crapper/src/coverage/`. | Stay inside the tool directory. |
| 9 | First build in a scratch copy found 4 compile errors (roxmltree lifetime, a missing `is_empty`, two temporary borrows in tests). Dispatched one fix-up. | One compile loop per phase. |
| 10 | Fixtures and parity runner written by two more implementers (`parity/`). Recorded the original's output for 29 runs. | AC-1, AC-2, AC-8, AC-16, AC-25, CR-PB-*. |
| 11 | Ran `parity/run.py`: 33 runs, 29 pass, 4 skip, 0 diffs. | Language readers matched the original on the first run. |
| 12 | Extra differential runs (not in the repo): original vs port with `--no-coverage` on tokio (3395 lines of EDN), syn, clap_builder, npm (JS), upstream's own Python, crap4clj, crap4java, crap4go, uml-viewer, concurrent-ruby Java. All identical. | The fixtures are small. Real code finds reader differences. |
| 13 | Added replay tests (`tests/recorded.rs`) and meta tests (`tests/meta.rs`). `flags-root` exposed that both the runner and the replay read `.metrics` under cwd, not under `--root`. Fixed both. | AC-11, AC-20..24. |
| 14 | MISTAKE: ran the original crapper inside the crate directory. It deleted stored coverage reports in the Go, Java and Clojure fixtures (its default clean step) and tried a `pip install` into user Python. Recreated the three reports from the recorded output (verified against the original in scratch copies) and moved all later gate runs to a scratch copy (`/tmp/gate`). | Never run a coverage-cleaning tool on the fixtures in place. |
| 15 | Live install checks (AC-6, AC-7). Rust: no `cargo-llvm-cov` on PATH, port printed `Installing cargo-llvm-cov` and the install command on stderr, installed it, and wrote coverage (66.7% for `classify`). Vitest: port installed `@vitest/coverage-v8@2.1.9` with `npm install --no-save --no-package-lock`, `package.json` unchanged. Python: port installed `coverage` and `pytest` into the project `.venv`. | CR-14, CR-15. |
| 16 | Timing: median of 5 runs, `--use-existing-coverage`, macOS arm64: original about 0.17 s, port about 0.006 s on all six fixtures. | AC-17 (macOS only). |
| 17 | Compared stdout, stderr and exit code for 14 flag cases. Found one Python-only difference: `--threshold 1_0` (Python `float` accepts underscores). Copied the Python rule into `python_float` instead of adding a `fixes.toml` entry. | Drop-in behaviour needs no approval when the port matches. |
| 18 | Crapper gate (original, on a scratch copy, `-s src`, real `cargo llvm-cov`): 11 functions at CRAP 12-72 (all `cover_*` in `runners`, 0% covered). Dispatched 3 refactors: inject an executor, split, test with a recording executor. | ub-mode: CRAP < 10. |
| 19 | Dryer gate (original): 3 pairs at 1.00, 1.00, 0.91. Dispatched one refactor (shared `ts::ancestor_names`, `ts::resolve`, `format_optional`, `begin_form`). Dryer now reports none. | ub-mode: dryer < 0.8. |
| 20 | Mutator gate: see section below. | ub-mode. |

## Gate results

- `cargo build --release`, `cargo test`, `cargo clippy --all-targets`, `cargo fmt`: clean.
- Crapper (original) on `src`: no function with CRAP >= 10 after step 18.
- Dryer (original) on `src`: `No duplicate candidates found.`
- Parity runner: 33 runs, 29 pass, 4 skipped (`clj`, `mvn`, `go` not installed), 0 diffs.

## Mutation gate

Tool: the original mutator at its HEAD, run per file in scratch copies under /tmp (never in `crapper/`), `--no-coverage --mutate-all --timeout-factor 30`, several copies in parallel.

Workarounds for the original mutator (not port bugs): `src/coverage/` is not copied into its workers, so in the scratch copy only it is renamed (`src/cover` + `#[path]`); timeouts count as KILLED, so the timeout factor is raised so compile time does not hide survivors; the test command skips the git-setup replay tests (`ac_4`, `ac_5`) that fail inside a worker, except for `cli.rs` which runs `--test cli`.

Round results (survivors found -> killed):

| File | Found | Result |
| --- | --- | --- |
| `crap.rs`, `golang.rs`, `java.rs`, `metrics.rs` | 0 | clean |
| `cli.rs` | 7 + 4 + 2 | killed (tests for `--changed` filters, exit codes, git failure, unwritable `.metrics`, `--coverage-command` status) |
| `discover.rs`, `report.rs` | 5 | killed or made equivalent |
| `languages/python.rs` | 5 | killed or made equivalent |
| `languages/rust.rs`, `typescript.rs` | 14 | killed or made equivalent |
| `runners/*` | 22 | killed (pure warning helpers, injected executor); equivalent literals removed |
| `coverage/*` | 15 | killed; redundant clause and DOCTYPE regex removed |
| `languages/clojure.rs` | 25 | killed or made equivalent (17 new tests, 6 dead helpers removed) |
| `languages/ts.rs` | 0 | clean |

Total survivors found across rounds: about 112. All were killed with a behaviour test or removed as equivalent code.

### Gate closure (re-mutation of every file after the fixes)

Same tool and flags as above, one scratch copy per group under /tmp, at most 4 runs at once. Test command: `cargo test -- --skip ac_4_ --skip ac_5_` (`cargo test --lib --test cli` for `cli.rs`), `ulimit -t 120`, git `commit.gpgsign` forced off through `GIT_CONFIG_*` env.

| Step | Action | Reason |
| --- | --- | --- |
| G1 | `tests/cli.rs` `git` helper now passes `-c commit.gpgsign=false`. | `git commit` in the test repos failed once in a few runs because the user's global config signs commits (flaky red test). |
| G2 | Re-mutated `cli.rs` (52 sites), `discover.rs` (21), `report.rs` (1), `analyze.rs` (0), `languages/python.rs` (11), `rust.rs` (25), `typescript.rs` (44), `coverage/formats.rs` (49), `coverage/mod.rs` (7), `coverage/paths.rs` (46), `runners/mod.rs` (23), `runners/rustpy.rs` (38), `runners/typescript_cov.rs` (20). | Every fixed file was never re-mutated. Result: 0 survivors in all of them. |
| G3 | Re-mutated `languages/clojure.rs`: 94 sites, 1 survivor (`blank_span` `end - start` -> `end + start`: no test compared the blanked text exactly). | Added exact-output tests for `without_strings_and_comments`. |
| G4 | Found the runaway mutant. Diagnostic test script: build under `ulimit -t 120`, run tests under `ulimit -t 15`, and log the mutated diff when the test binary dies with a signal. 12 mutants hung (`string_end` `start + 2 + offset` -> `start - 2 + offset` is the one that spun with no memory growth: `string_end` returns `start`, so `without_strings_and_comments` never advances; others: l.68/69/84/111 `skip_while` operators, l.146 and l.460 `index += 1` -> `+= 0`, l.171, l.217, l.360, l.370). | A hang is only "killed" by the mutator timeout. |
| G5 | Replaced the five hand-advanced `while`/`loop` scans (`balanced_end`, `without_strings_and_comments`/`blank_or_copy` (now `Option<usize>`), `extract_top_level_defns`, `skip_ignorable`, `skip_leading_metadata`) by bounded `for` loops with a `resume` index; deleted `skip_space_and_comment`. 9 new `cr_11_*` tests with exact values. | A wrong skip index now gives wrong output (fast assertion failure), never an endless loop. Behaviour on valid input is identical. |
| G6 | Re-ran `clojure.rs`: 1 survivor (`0..form.len()` bound -> `1..`), fixed with `for _ in form`. Final: 95 killed, 0 survived, no hung mutant (hang detector found none). | Equivalent bound removed, not kept. |

Files with no mutation sites: `lib.rs`, `main.rs`, `model.rs`, `languages/mod.rs`. `crap.rs`, `golang.rs`, `java.rs`, `ts.rs`, `metrics.rs` were already clean and were not changed.

Final: every `src` file has 0 survivors. `cargo test`: 321 pass. `cargo clippy --all-targets`: clean. `parity/run.py` (scratch copy, release binary): 33 runs, 29 pass, 4 skip (`clj`, `mvn`, `go`), 0 diffs.

Lint: `cargo clippy --all-targets` reports no issues (fixed `cloned_ref_to_slice_refs` in `rustpy.rs` and `unnecessary_map_or` in `clojure.rs`).

## Differences from the PRD text (need an owner decision)

- CR-10 and AC-25 say a function with no coverage data has `:coverage nil`. The original at the pinned commit writes `0.0` (its README says: "A function the coverage report does not mention scores 0%"). The port copies the original. Parity run `missing-function` records this.
- `parity/fixes.toml` has no entries. The port found no bug in the original to fix, and the only Python-only difference found (`1_0` in `--threshold`) is copied, not fixed. Candidate differences that are not copied (a decision for Mads Madsen):
  - A broken symlink or an unreadable source file stops the original with a Python traceback (exit 1) and the port with a Rust panic (exit 101).
  - A malformed Go profile or JaCoCo XML does the same.
  - `float()` accepts non-ASCII digits in `--threshold`; the port does not.

## Not reachable here (blockers)

- Mutation gate: closed (see Gate closure).
- AC-18 CI matrix (macOS x86_64, Linux x86_64 and arm64): only macOS arm64 is available. No CI file was written because CI lives outside `crapper/`.
- AC-17 on the named CI runner (Ubuntu 22.04): timing was measured on macOS arm64 only.
- AC-19 `cargo install --git <repo>`: the private repository URL is unknown. `cargo install --path crapper` succeeded and the installed binary printed `--help` with exit 0.
- AC-22 CI check: `python3.12 parity/run.py --check-meta` and `tests/meta.rs` implement it; no CI job runs it.
- `cr13-clojure`, `cr13-java`, `cr13-go`: default coverage commands need `clj`, `mvn` and `go`, which are not installed. Their command construction is tested with a recording executor (CR-13) but no live parity run exists. `cr13-python` passed with `coverage` and `pytest` installed in a scratch venv on PATH.
- AC-11 "host with no Python": the replay tests (`tests/recorded.rs`) need no Python; they were not run on a host that lacks Python.
- `uml_viewer_commit` checks (AC-5 of the main PRD): no `fixes.toml` entry changes snapshot shape, so none is needed.
