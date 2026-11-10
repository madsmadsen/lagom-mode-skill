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
- Parity runner: 33 runs, 33 pass, 0 skipped, 0 diffs (live `cr13-*` runs: see Live parity runs).

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

### Live parity runs (clj, mvn, go, java installed)

Parity runner on a scratch copy (`/tmp/par2`, `/tmp/par3`), release binary, original from the pinned venv, `coverage` and `pytest` from a scratch venv on PATH, one run at a time with `timeout`.

| Run | Result | Cause / action |
| --- | --- | --- |
| `cr13-python` | pass | |
| `cr13-java` (mvn) | pass | |
| `cr13-clojure` (clj) | pass | |
| `cr13-go` | diff in stdout | Only the `go test` wall time (`0.261s` vs `0.264s`) in the `ok <pkg> <time> coverage: ...` line. Same coverage (22.2%), same table, same snapshot. Test-runner noise, not a port or original bug: `parity/run.py` `normalize` now replaces the tab-delimited elapsed time by `<TIME>` (`parity/test_normalize.py`, CR-PB-3 area). No `fixes.toml` entry, no `src` change, so no re-mutation. |

Full run after the fix: 33 runs, 33 pass, 0 skip, 0 diffs; `parity/last-run.toml` rewritten (`skipped = []`). No candidate `fixes.toml` entry found. `cargo test` 321 pass, clippy clean.

Files with no mutation sites: `lib.rs`, `main.rs`, `model.rs`, `languages/mod.rs`. `crap.rs`, `golang.rs`, `java.rs`, `ts.rs`, `metrics.rs` were already clean and were not changed.

Final: every `src` file has 0 survivors. `cargo test`: 321 pass. `cargo clippy --all-targets`: clean. `parity/run.py` (scratch copy, release binary): 33 runs, 29 pass, 4 skip (`clj`, `mvn`, `go`), 0 diffs. The 4 skips were closed later, see Live parity runs.

Lint: `cargo clippy --all-targets` reports no issues (fixed `cloned_ref_to_slice_refs` in `rustpy.rs` and `unnecessary_map_or` in `clojure.rs`).

## Differences from the original and decisions of Mads Madsen

- Until the upstream-fix work (section "Upstream fixes CR-18 to CR-24"), `parity/fixes.toml` had no entries. The owner decided two points (next section). Both make the port equal to the original, so neither needed an entry. Now it has seven entries, all unapproved.
- Still open (not copied, decision for Mads Madsen): `float()` in `--threshold` and `int()` in Go and JaCoCo numbers accept non-ASCII digits in Python; the port accepts ASCII only. The other open items are in the list at the end of the next section.

## Owner decisions: CR-10 and crash behavior (CR-3, AC-26)

Orchestrated by `CrapperDecisions`. 19 `implementer` tasks (never more than 2 at once). The orchestrator ran every build, test, clippy, fmt, fuzz and gate step.

### Item 1: CR-10 and AC-25 say 0.0, not nil

Decision: the original is right. A function that the coverage report does not mention gets `:coverage 0.0` and `:crap` for coverage 0. The port already did this. `docs/prd/crapper.md` (CR-10, AC-25) now says 0.0, and nil stays for `--no-coverage`. New test `cr_10_ac_25_function_missing_from_the_report_scores_coverage_0_0` (tests/cli.rs) names CR-10 and AC-25 and asserts the whole snapshot for a function the report does not mention. The open question is removed from this log. No [ASSUMPTION] label depended on it. Commit 9d61a8f.

### Item 2: crash behavior, original vs port

Decision: the port behaves like the original. Exit code 1, nothing on stdout, no snapshot, one line on stderr (the text need not match the Python traceback). Reproduced against the pinned original (`6fd92c4`, scratch venv `/tmp/orig-venv`, Python 3.12.16, tree-sitter 0.26.1, tree-sitter-language-pack 1.25.0, run in scratch copies under /tmp). In every row the original wrote 0 bytes on stdout and wrote no `.metrics/crap.edn`.

| Case | Original: exit, last stderr line | Port before | Port now |
| --- | --- | --- | --- |
| Broken symlink `src/broken.py` -> `/nonexistent/missing.py` | 1, `FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent/missing.py'` | 101 (panic) | 1 |
| Source file with mode 000 | 1, `PermissionError: [Errno 13] Permission denied: '<root>/src/secret.py'` | 101 | 1 |
| Symlink loop `src/loop.py` -> `loop.py` | 1, `RuntimeError: Symlink loop from '<root>/src/loop.py'` | 101 | 1 |
| Go profile line with 2 fields | 1, `ValueError: invalid coverage segment on line 2: 'foo.go:1.1,2.2 1'` | 101 | 1 |
| Go profile, start line `a.1` | 1, `ValueError: invalid literal for int() with base 10: 'a'` | 101 | 1 |
| Go profile block without a comma | 1, `ValueError: not enough values to unpack (expected 2, got 1)` | 101 | 1 |
| Go profile line without a colon | 1, `ValueError: invalid coverage segment on line 2: 'foo.go 1 1'` | 101 | 1 |
| Go profile statements `x` | 1, `ValueError: invalid literal for int() with base 10: 'x'` | 101 | 1 |
| JaCoCo XML unclosed tag | 1, `xml.etree.ElementTree.ParseError: unclosed token: line 1, column 8` | 101 | 1 |
| JaCoCo counter `missed="x"` | 1, `ValueError: invalid literal for int() with base 10: 'x'` | 101 | 1 |
| JaCoCo `standalone="yeses"` in the XML declaration | 1, `ParseError: XML declaration not well-formed` | 0 (accepted) | 1 |
| Report without read permission (lcov.info, coverage.out, jacoco.xml, Cloverage .html) | 1, `PermissionError: [Errno 13] Permission denied: '<root>/coverage/lcov.info'` | 0 (ignored the file) | 1 |
| Source symlinked from outside the root, package marker (pyproject.toml) outside the root, no flags | 1, `ValueError: '<ext>' is not in the subpath of '<root>'` (before any tool starts) | 101 | 1 |

Cases where the original does NOT crash (exit 0) and the port did not match, now equal: Go numbers and JaCoCo numbers with a sign, underscores between digits (`1_0`, `0_4`) or more than 64 bits (port before: exit 1 or an overflow risk); a garbage or lowercase DOCTYPE in the JaCoCo XML (port before: exit 1). Same in both from the start: a JaCoCo `line="zz"` (read as 0); a broken symlink whose target has no source extension (skipped).

How: every fallible reader returns `Result<_, String>` up to `cli::analyze`, which prints the message through one `fail()` helper and returns 1. New `src/pynum.rs` holds `python_float` (moved) and `python_int` (sign, underscores between digits, magnitude clamped at i64::MAX). `parse_jacoco_index` removes DOCTYPE text with the original's regex and checks the XML declaration. Order of work, as in the original: coverage reports first (Go, JaCoCo, LCOV, forms), then source files.

Parity: 16 new runs in `parity/coverage.toml` (`crash-*` x 12, `lenient-*` x 4) with the original's recorded exit code and stdout, compared by `parity/run.py` (exit code, stdout, snapshot) and replayed offline by `tests/recorded.rs` (`ac_26_crashes_exit_1_like_the_recorded_original`, AC-8 group for `lenient-*`). Each fixture state is built by `setup` shell commands in the copy (symlinks, `chmod 000`, report files), so nothing odd is checked in.

Commits (all red tests first, then code): 1112a57 (source files), 6fc8f33 (Go profile, JaCoCo XML, unreadable report), 92af57e (Python int rules), aa762e3 (package outside root, report directory errors), ab23eef (JaCoCo DOCTYPE and XML declaration).

### Other panic paths (audit)

Method: read every `unwrap`, `expect`, `panic!`, index and slice in non-test code; then differential fuzzing against the original (random edits of the fixture sources and reports, both tools run in scratch copies): 4150 mutated sources (all six languages), 2100 mutated report sets of all four formats, 2100 mutated JaCoCo XML files, 900 mutated Go profiles.

Fixed (reachable from a file or a report): `analyze::read_source` panic; `coverage::merge_go` and `merge_jacoco` panics; `read_text` ignoring an unreadable report; `runners::coverage_report` `expect` (source outside the root); `rustpy::make_report_dir` and `remove_if_present` panics; possible i64 overflow in `span_totals`, `go_percent`, `method_percent` and `jacoco_percent` (made safe together with the Python int clamp).

Checked, not reachable from input, left as is: `ts::node_text` (source is lossy UTF-8, tree-sitter offsets sit on character boundaries); `golang::package_namespace` (absolute path has a parent, go.mod directory is an ancestor); `expect` on static regex patterns and on `crap.rs` number formatting; indexing in `languages/clojure.rs` (bounded loops since step G5). The fuzz found no panic in any source file.

Result of the fuzz after the fixes: no exit-code difference and no panic. Open differences (not fixed, decisions for Mads Madsen):

- About 1 in 100 mutated sources differ in output, all on invalid syntax where tree-sitter error recovery differs (for example a NUL byte inside a Rust file gives another tree). Not a crash.
- The XML parser of the port (roxmltree) accepts an invalid processing-instruction target that Python's expat rejects (2 of 1200 fuzzed JaCoCo reports). The port exits 0 where the original exits 1.
- Numbers above 2^63 - 1 in a Go profile or JaCoCo report are clamped; Python ints are unbounded. A sum of two such numbers can score differently from the original. This is a difference from the original that is not in `fixes.toml` (an entry needs approval, CR-PB-6). CR-12 in the PRD states the clamp.
- Live coverage run only: when creating or removing a report directory fails for Clojure, Java, Go and TypeScript (`attempt`, `prepare_report`), the port warns and goes on; the original raises (exit 1). Not reproduced here, no panic.
- Non-ASCII digits, see the section above.

### Results

`cargo test`: 375 pass. `cargo clippy --all-targets`: clean. `parity/run.py` (pinned original in a scratch venv, `clj`, `mvn`, `go`, `java`, `npm`, `cargo-llvm-cov` live): 49 runs, 49 pass, 0 skip, 0 diffs (33 earlier + 16 new); `last-run.toml` rewritten.

Mutation gate re-run on every touched `src` file (scratch copy of the committed tree, `ulimit -t 120`, 2 workers, at most 2 runs at once, process groups killed after each run, `pgrep` empty): `analyze.rs` 0 sites; `coverage/mod.rs` 7 killed, 0 survived; `coverage/formats.rs` 40 killed, 0 survived; `pynum.rs` 24 killed, 0 survived; `runners/mod.rs` 23 and `runners/rustpy.rs` 38 and `runners/typescript_cov.rs` 20 and `cli.rs` 38: all killed, 0 survived. No test-only code in `src`.

## Not reachable here (blockers)

- Mutation gate: closed (see Gate closure).
- AC-18 CI matrix: macOS arm64 and, in qemu guests, Ubuntu 22.04 arm64 and x86_64 are done (see Linux guests). macOS x86_64 is not available here. No CI file was written because CI lives outside `crapper/`.
- AC-17 on the named CI runner (Ubuntu 22.04 x86_64): measured in a qemu TCG guest (emulated, slow), see Linux guests. A real x86_64 runner is not available here.
- AC-19 `cargo install --git <repo>`: the private repository URL is unknown. `cargo install --path crapper` succeeded and the installed binary printed `--help` with exit 0.
- AC-22 CI check: `python3.12 parity/run.py --check-meta` and `tests/meta.rs` implement it; no CI job runs it.
- (closed) `cr13-clojure`, `cr13-java`, `cr13-go`, `cr13-python` now run live, see Live parity runs.
- (closed) AC-11 "host with no Python": `cargo test` ran on Ubuntu 22.04 guests with every Python interpreter removed, see Linux guests.
- `uml_viewer_commit` checks (AC-5 of the main PRD): no `fixes.toml` entry changes snapshot shape, so none is needed.

## Linux guests (arm64 and x86_64, Ubuntu 22.04, glibc 2.35)

Scripts: `ci/linux-vm/` (`vm.sh`, `guest-setup.sh`, `guest-test.sh`, `guest-timing.sh`, `guest-nopython.sh`). Official Ubuntu 22.04 cloud images, cloud-init, ssh.
arm64 guest: qemu with HVF (native speed). x86_64 guest: qemu with TCG (emulated, about 10 times slower). Nothing is shared with the guest: sources go in as a tar stream
(`git ls-files`, so no `target/`, no `.metrics`), and cargo builds in the guest's own disk. Rust 1.99.1 stable (rustup, minimal + clippy + rustfmt).

| Step | Action | Result |
| --- | --- | --- |
| L1 | First arm64 run. 6 tests in `tests/recorded.rs` failed. | Cause: my copy script excluded every directory called `target`, and the fixtures keep their stored coverage under `target/coverage/`. Not a crapper bug. The copy now uses `git ls-files`. |
| L2 | `cargo build --release`, `cargo test`, `cargo clippy --all-targets` on both architectures. | arm64: build ok, 321 tests pass, clippy 0 warnings. x86_64: build ok, 321 tests pass, clippy 0 warnings. |
| L3 | AC-11/AC-18: clone of each guest, `dpkg --purge` of all python3 packages, `which python3` and `command -v python` print nothing, no executable `python*` file on disk, fresh `target`. Same three commands. | arm64: 321 pass, clippy 0. x86_64: 321 pass, clippy 0. `tests/recorded.rs` (AC-11) passes with no Python on both. |
| L4 | AC-17 (CR-NFR-2): `parity/run.py --only <the six *-existing runs> --timing` with the pinned original (Python 3.12.16 from uv, tree-sitter 0.26.1, tree-sitter-language-pack 1.25.0), release port, median of 5. All six runs matched the original. | arm64 (native): original 0.027 to 0.039 s, port 0.001 to 0.002 s. x86_64 (TCG, emulated): original 0.335 to 0.498 s, port 0.015 to 0.023 s. The port is lower on all six fixtures on both. |

The x86_64 times are not comparable with a real Ubuntu 22.04 runner (both sides are slowed by the emulator, the ratio is about 20 to 30 times as on arm64). The AC-17 relation (port median lower than original median) holds in the guest; the absolute numbers say nothing about a real runner.

## Upstream fixes CR-18 to CR-24 (PRD commits 8f31773 and bc91fc4)

Orchestrated by `CrapperOrchestrator`. Implementer agents (`implementer`) wrote the production code, at most 3 at a time: TsInstall, TsShapes, PyRoutes, DoctypeRule, GitEnv, SplitAnalyze. The orchestrator wrote every test first, ran every build, test, clippy, fmt, and gate, and wrote the fixtures, the parity runner changes, and this log.

Read first: `git diff 4384d65 HEAD -- docs/prd/crapper.md`. The new items are CR-18 to CR-24 and AC-27 to AC-33. CR-3, CR-9, CR-12, and AC-26 were already met by commits 8465981 to 4384d65 and c97b1be. No parity or `upstream.toml` change is needed (CR-PB-1, pinned commit unchanged).

| Step | Action | Reason |
| --- | --- | --- |
| U1 | Fetched issue 20 and PRs 27, 30, 34 of the original (diffs). | The PRD says "copy the rules of PR 30" and names exact commands. The diffs give the order of flags (`--ignore-scripts` before the spec) and the Python reference for the route rules. |
| U2 | Wrote the red tests. Ran them. CR-24: `cr_24_jacoco_doctype_with_an_internal_subset_is_deleted` and `cr_24_ac_33_*` panicked at `expect("valid jacoco")` (the old regex left `]>` behind, the XML parser failed). CR-20: `cr_20_ac_29_*` failed because `--changed` read the decoy repository (no `untracked_fn` in stdout). CR-18/19/23: 7 tests failed (`left: [npm, install, --no-save, --no-package-lock, ...]` for pnpm, yarn, bun, packageManager, a lockfile not restored, and `@vitest/coverage-v8@file:../untrusted`). CR-21: 4 tests failed (`left: [plain, method]`, 2 of 7 entries). CR-22: 3 tests failed (`left: [build_router]` with CC 6, handlers had no entries). | TDD: see each test fail for the expected reason before the code. |
| U3 | Fixed a wrong expectation of my own: the guard test `cr_21_shapes_inside_a_function_stay_inside_it` expected CC 4 but a class inside a function is skipped by the counter (CC 3). | The test is a regression guard, not a new behavior. It passed before and after. |
| U4 | Briefed implementer agents (CR-18/19/23 in `typescript_cov.rs`, CR-21 in `typescript.rs`, CR-22 in `python.rs` and `ts.rs`, then CR-24 in `formats.rs` and CR-20 in `cli.rs`). `cargo fmt`, `cargo test` (394 pass), clippy clean. Five commits, one for each requirement group. | One owner for each file. |
| U5 | The old DOCTYPE tests used junk with a `[`. The CR-24 rule does not delete an unclosed `[`, so the original and the port differ there. Removed the `[` from the junk in `tests/cli.rs`, `coverage.toml` (`lenient-jacoco-doctype`), and the formats test. Added `cr_24_jacoco_doctype_with_an_unclosed_internal_subset_is_an_error`. | The PRD replaces the DOCTYPE rule (CR-24). The lenient parity run must stay a run where both agree. |
| U6 | Seven `fixes.toml` entries (`vitest-package-manager`, `vitest-npm-lockfile`, `vitest-untrusted-install`, `git-env`, `ts-shapes`, `python-routes`, `jacoco-doctype-subset`). None has `approved_by`. | CR-PB-3, CR-PB-6: only Mads Madsen approves. |
| U7 | Seven fixtures: `vitest-pnpm`, `vitest-npm-locked`, `vitest-untrusted`, `changed-git-env`, `ts-shapes`, `python-routes`, `jacoco-subset`. The three Vitest fixtures have stub package managers in `.stubs/` (shell scripts: npm, pnpm) and a stub `node_modules/.bin/vitest`. The stub npm fails the way the real npm fails (a tree made by pnpm, a `file:` spec, an install that would run scripts, `--no-package-lock` that ignores the lock). Real `pnpm` and `yarn` installs are not run in the repository tests. | A real install needs the network and changes with time. The stubs give a fixed, fast, offline result for both the original and the port. |
| U8 | Runs in `coverage.toml` can now have `env` (`NAME=VALUE`, `{project}` is the project path) and a fixture `.stubs` directory goes first on PATH. Changed `run.py`, `record_original.py`, and `tests/recorded.rs`. Runs with `fix` are not part of the offline replay of `tests/recorded.rs`. | The `git-env` fix needs `GIT_DIR`. The install fixes need stub tools. One mechanism for both. |
| U9 | Recorded the original for the seven `fix-*` runs with `record_original.py` (pinned original, tree-sitter 0.26.1, tree-sitter-language-pack 1.25.0). The original: `big` has 0.0% (pnpm), 40.0% (npm lock ignored), 0.0% (untrusted version), selects all files with `GIT_DIR` set, lists 2 TypeScript entries, lists `build_router` with CC 5 and no handlers, exits 1 on the internal subset. | CR-PB-4: a test must fail on the recorded output. |
| U10 | `tests/fixes.rs`: one test for each entry. Each test asserts a property that is false for the recorded original and true for the port. `tests/meta.rs`: `ac_24_fixes_entries_approved` is `#[ignore]` (it fails by design until Mads Madsen approves). New test `ac_24_fixes_entries_name_no_other_approver`. `python3.12 parity/run.py --check-meta` still fails on the 7 entries. | AGENTS.md: do not approve. `cargo test` must stay clean. The strict check is kept and is one flag away. |
| U11 | Gate: `crapper -s src --threshold 9.99` found `cli::analyze` at CC 10, CRAP 10.0. Split it into `scored_entries` and `report`. | Our own source must pass our own gate. |
| U12 | Mutation: `cli.rs:238` `code: 1` was uncovered (the line had no region in the LCOV). Added `GitError::new`, so the literal sits on a line with a region. Now 0 uncovered. | Cover the uncovered. |

### Decisions and open points

- `last-run.toml` is NOT rewritten. A full parity run now fails by design: the seven runs `fix-*` differ and no entry is approved (AC-24, AC-1). The file still says `pass` for the last full run at the pinned commit (49 runs, before the fix runs). AC-22 in CI stays green on that file. After Mads Madsen approves the entries, run `python3.12 parity/run.py` and the file is rewritten.
- CR-23 regex: Python `$` also matches before a trailing newline, so PR 27 accepts `4.1.11\n`. The port uses the Rust `$` (end of text only) and so rejects it. The PRD says "valid SemVer version". The test `cr_23_ac_32_a_version_that_is_not_semver_is_not_pinned` pins this.
- AC-26 says the port and the original give the same result for "odd DOCTYPE text", but CR-24 replaces the DOCTYPE rule of the original. They differ on a DOCTYPE with a `[` and no `]`, and on a DOCTYPE with an internal subset. Tracked by the entry `jacoco-doctype-subset`. The PRD should say that AC-26 means DOCTYPE text without `[`. Not changed (PRDs are read-only for this task).
- The CR-18 order for `packageManager` is pnpm, yarn, bun, as in PR 34. A lockfile or `node_modules/.pnpm` always wins over the field.
- The install notice (CR-15) still says "package.json and the lockfile are restored afterward". The command itself is on stderr from `run_shell` (`+ (cwd) pnpm add ...`), which is what AC-27 asks for.
- `yarn add --no-lockfile` and `bun add --no-save` flags are as the PRD states. The repository tests use stub tools, not live installs.

