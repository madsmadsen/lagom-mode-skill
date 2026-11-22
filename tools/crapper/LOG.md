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



## 2026-11-11: License holder and credit (CR-NFR-5, AC-20)

- Mads Madsen chose: MIT, copyright Mads Madsen <mads@onmadsen.com>, credit "based on crapper by Robert C. Martin" in LICENSE and README.
- `LICENSE`: copyright line is now Mads Madsen. An acknowledgment paragraph names the original and its license.
- Every `.rs` file in `src/` and `tests/` and every parity `.py` script starts with `Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>` and `SPDX-License-Identifier: MIT`. The old per-file license credit to Robert C. Martin is gone. The reference to the matching upstream file stays, for the parity checks.
- `Cargo.toml` names the author. README names the copyright holder and the original. `tests/meta.rs` (AC-20) and `parity/run.py --check-meta` now require the two new header lines and the new LICENSE text. `--check-meta` reports only the seven unapproved `fixes.toml` entries.
- Proof: `cargo fmt --check` clean, `cargo clippy --all-targets` no issues, `cargo test` 401 passed, 1 ignored. Only comments, metadata and test checks changed, so the crapper, dryer and mutator gates were not rerun.

## 2026-11-12: moved to tools/crapper

- Moved the crate from `crapper/` to `tools/crapper/`. Reason: Mads Madsen asked for one `tools/` folder for the three ports. Paths in the crate are relative to the crate or to the runner file, so no code changed. Updated the docs that name the old path.
- Ran `cargo clean --profile dev`, then `cargo test`, `cargo clippy --all-targets`, and `cargo fmt --check`. Reason: the old test binaries kept the absolute path of the crate in `CARGO_BIN_EXE_crapper`, and cargo did not rebuild them after the move. All tests pass, with 0 clippy warnings.
- Rewrote `LICENSE` from the shared template: MIT, Mads Madsen as copyright holder, the acknowledgment of Robert C. Martin, and a thank-you line. Changed the install command in `README.md` to the new repository URL `https://github.com/madsmadsen/lagom-mode-skill`. Reason: Mads Madsen asked for one license form and the repository name `lagom-mode-skill`.
- Pre-publish privacy review: removed the home Gitea URL and the local home path from the whole history with `git filter-repo`, and removed the tracked `.pyc` files. Reason: the repository goes public on GitHub.
- Changed the license of this repository from 0BSD to MIT in every commit with `git filter-repo`: each `LICENSE` file, each SPDX header, each `Cargo.toml` `license` field, the crapper license test, and the license sentences in the PRDs, READMEs, and logs. Statements that the originals by Robert C. Martin use 0BSD stay. Reason: Mads Madsen chose MIT for the public release.

## 2026-11-15: Rust test code is left out (CR-25, AC-34)

Loop `lagom-mode/rust-test-code-crapper` in `/tmp/lagom-wt/crapper`, source branch `master`, base commit `9089d0ab413270e38cd9f5e0c46e2ffd1e1e32f`.

- Reason: the port skipped a Rust function only when an enclosing `mod` was named `tests`. A `#[cfg(test)] mod unit_tests` or a `#[test]` fn in a plain module was scored as production code. This is a difference from the original, so it has CR-25 in the PRD, the entry `rust-test-code` in `parity/fixes.toml` (not approved), and a test that fails on the recorded output of the original.
- `src/languages/rust.rs`: new `is_test_code` and `has_attribute`. `record_function` calls `is_test_code`. A function is test code when an enclosing mod is named `tests` or has `#[cfg(test)]`, or when the function has `#[test]` or `#[cfg(test)]` (the `attribute_item` siblings right before the item, spaces ignored). `cfg(all(test, ..))`, `cfg(not(test))` and `tokio::test` do not match. The attribute check sits in its own function so that every function stays at CRAP 10 or lower.
- Unit a (`#[cfg(test)] mod unit_tests`): the test failed because all four functions were reported (`["keep","b","helper","deep"]` instead of `["keep"]`: the module name `unit_tests` is not `tests`). Fixed with the module attribute check.
- Units b and c (`#[test]` fn in a plain module, `#[cfg(test)]` helper fn): the tests failed because `check`, `slow` and `helper`, `spaced`, `second` were reported. Fixed with the check on the attributes of the function itself.
- Units d and e (`#[cfg(not(test))]` and look-alike attributes stay production code, `mod tests` still skipped and its neighbor still reported): these tests pass at once, because they pin behavior that must not change. They guard the exact match of the attribute text.
- `tests/cli.rs` `cr_25_ac_34_rust_test_code_is_left_out_of_the_report`: end-to-end check on `--no-coverage`. It passes at once after units a to c.
- Parity: new fixture `parity/fixtures/rust-test-code`. It keeps its Rust source as `lib.rs.txt`, and the run copies it to `src/lib.rs` in its `setup`. Reason: with a real `src/lib.rs` the dryer gate found two similar sample functions, and the mutator gate ran the fixture as a crate and reported uncovered sites and survivors in the sample code. Run `fix-rust-test-code` in `coverage.toml`, original output recorded in `parity/recorded/fix-rust-test-code/` with the runner (`--record --only fix-rust-test-code` on a scratch copy in `/tmp`). The original reports all six functions except `named_tests`. `tests/fixes.rs` `fix_rust_test_code_leaves_out_cfg_test_modules_and_test_functions` asserts that the property is false for that record and true for the port.
- PRD: new CR-25 in section 5.2, AC-34, a note in CR-8 and in section 7.
- Unit gates (crate directory, uncommitted changes), all with the installed older builds: `crapper --changed --threshold 10` exit 0 (worst function `record_function`, CRAP 6.0, new `is_test_code` 4 and `has_attribute` 1 after 100% coverage). `dryer --changed --threshold 0.8` no candidates. `mutator --changed --max-workers 2` exit 0, 0 survivors, 0 uncovered (30 sites killed).
- Feature gates on the feature files: `crapper --threshold 10 src/languages/rust.rs` exit 0. `mutator --max-workers 2 src/languages/rust.rs` exit 0, 0 survivors, 0 uncovered. `dryer --threshold 0.8 src/languages/rust.rs` no candidates. `dryer` on `tests/cli.rs` and `tests/fixes.rs` also lists 45 and 6 pairs at 0.8 or higher. They are in the base commit as well (checked on a copy of both files at the base commit), and none of them contains the new tests (`cli.rs:239`, `fixes.rs:226`). The existing tests were not refactored, because that is outside this feature.
- Full parity run on a scratch copy under `/tmp` (`python3.12 parity/run.py`): 57 runs, 48 pass, 1 skip (`cr13-python`, the `coverage` tool is not installed here), 8 diffs. The 8 diffs are the 7 old fix runs and the new `fix-rust-test-code`, each without approval, so the run reports `fail` by design. `parity/last-run.toml` is not changed (same decision as for the earlier fix entries).
- Rebase: master moved from `9089d0ab413270e38cd9f5e0c46e2ffd1e1e32f` to `0820fbd14bbd4f5d034886b46fffc365b490b6fd` (the dryer loop, files under `tools/dryer` and `docs/prd/dryer.md` only). Ran `git rebase master` in the worktree without conflict. New base commit: `0820fbd14bbd4f5d034886b46fffc365b490b6fd`. Reran all tests and the feature gates (crapper exit 0, dryer no candidates, mutator exit 0 with 0 survivors and 0 uncovered) after the rebase.

## 2026-11-15: crapper --lines (CR-26, AC-35)

Loop `lagom-mode/crapper-lines` in `/tmp/lagom-wt/crapper-lines`, source branch `master`, base commit `ce9b9ff7685949d02ce28fadafe400eb9d3ed6f8`.

- Reason: a review session found that crapper scores every function in each file it gets. An old function that the change did not touch fails the gate (`crapper --changed --threshold 10` on this change reported `run_parity` with CRAP 110 in `parity/run.py`, a function that this change does not touch). `mutator --lines` already limits a run to the listed source lines. crapper needed an option for the same job: it takes the same line list and scores the functions that hold those lines. This is a difference from the original (it rejects `--lines` as an unknown option), so it has CR-26 and AC-35 in the PRD, the entry `lines-option` in `parity/fixes.toml` (not approved), the fixture `parity/fixtures/lines-option`, and a test that fails on the recorded output of the original.
- Red tests first (`tests/cli.rs`, 7 tests named `cr_26_ac_35_*`). All 7 failed with `Unknown option: --lines` and exit code 1, because the option did not exist.
- `src/cli.rs`: `Options.lines`, `parse_lines` (positive whole numbers, Python `int()` rules from `pynum`, empty items skipped, a number above `u32::MAX` becomes `u32::MAX`), help text for `--lines`. `src/analyze.rs`: `holds_a_line` and a `lines` argument of `analyze_files`. The filter runs before the entry is made, so the report, the snapshot, and the `--threshold` status all see only the scored functions. Coverage still runs for all files (the option does not change how coverage runs). A function holds a line from its first to its last line, so the lines of an inner function also belong to the outer function. The 7 tests passed after the change. Unit tests for `parse_lines` (3) and `holds_a_line` (1) pin the edge cases.
- Parity: the new run `fix-lines-option` in `coverage.toml` runs `--no-coverage --lines 9` on the fixture. The fixture keeps its Python source as `app.py.txt` and the run renames it to `app.py` in its `setup`, because a real `app.py` in the repository is scored by `crapper --changed` and has `choose` at CRAP 12. I recorded the original with a scratch venv in `/tmp` (tree-sitter 0.26.1, tree-sitter-language-pack 1.25.0, the original at the pinned commit): exit code 1, empty stdout, no snapshot. `tests/fixes.rs` `fix_lines_option_scores_only_the_functions_that_hold_a_listed_line` asserts that the property is false for that record and true for the port.
- The help text of the port now lists `--lines`, and the original help does not. The parity runner (`parity/run.py` `normalize`) and the replay test (`tests/recorded.rs` `normalize`) remove that exact help block before they compare, so `help-short` and `help-long` stay equal to the original. `parity/test_normalize.py` has a test for the block. Without this, `ac_8_table_equals_recorded_original_table` failed for `help-short` and `help-long`.
- PRD: new CR-26 in section 5.2, AC-35, non-goal 1 and the first paragraph of section 7 name the new flag. README `Use` lists `--lines`.
- Verification: `cargo build --release`, `cargo test` (all suites pass: 338 unit tests, 52 cli, 9, 9, 12 recorded, and the fixes tests), `cargo clippy --all-targets` no issues, `cargo fmt --check` clean, `python3.12 -m unittest parity/test_normalize.py` 4 tests pass.
- Unit gates (crate directory, uncommitted changes) with the installed older builds: `crapper --changed --threshold 10` exit 2 only because of old functions in `parity/run.py` (worst `run_parity` CRAP 110). That is the problem this change fixes. The Rust functions are all 9.0 or lower (worst `apply_option` 9.0, `holds_a_line` 1.0, `parse_lines` 4.0). `normalize` in `parity/run.py`, the one changed Python function, has CRAP 6.0. `dryer --changed --threshold 0.8` no candidates. `mutator --changed --max-workers 2` exit 0, 0 survivors, the Rust files have 0 uncovered sites (42 killed). It reports 197 uncovered sites in `parity/run.py`, because the Python run has no coverage data for that script (its tests run with `python3.12 -m unittest`).
- Install: `cargo install --path tools/crapper --force` fails in this sandbox (`Operation not permitted` on `~/.cargo/.crates.toml`, because the sandbox gives read access only to `~/.cargo`). I installed the build with `--root /tmp/crapper-lines-root`, so `crapper --help` there lists `--lines <n,n,...>`. The binary in `~/.cargo/bin` is still the old build.
- Feature gates, with the new build and the rule that each non-test feature file gets its own `--lines` (lines from `git diff -U0 HEAD -- <file>`): `crapper --threshold 10 --lines <lines> src/analyze.rs` exit 0 (`analyze_files` 6.0, `holds_a_line` 1.0). `src/cli.rs` exit 0 (worst `apply_option` 9.0). `parity/run.py` exit 0 (`normalize` 6.0). `mutator --max-workers 2 --lines <lines> <file>` exit 0 for `src/analyze.rs` (1 killed) and `src/cli.rs` (3 killed, 0 survivors, 0 uncovered). For `parity/run.py` it exits 0 and reports 197 uncovered sites for the whole file (the changed lines have no mutation site). `dryer --threshold 0.8` on the changed `.rs` and `.py` files lists 5 pairs, all in code that is in the base commit (`parity/test_normalize.py` 31-46, `tests/fixes.rs` and `tests/recorded.rs` helper functions); none holds the new tests.
- Parity run on a scratch copy under `/tmp` with the scratch original (`python3.12 parity/run.py --only fix-lines-option help-short help-long fix-rust-test-code`): `help-short` and `help-long` pass. `fix-lines-option` and `fix-rust-test-code` show a DIFF without approval, so the run reports `fail` by design. `parity/last-run.toml` is not changed.
- Rebase: master moved from `ce9b9ff7685949d02ce28fadafe400eb9d3ed6f8` to `a84fd23afd27d8af4d3677893d869712f2b3a691` (the skills commit: `docs/prd/skills.md` and `skills/` only, nothing under `tools/`). Ran `git rebase master` in the worktree without conflict. New base commit: `a84fd23afd27d8af4d3677893d869712f2b3a691`. Reran all tests (all pass, same counts). I did not rerun the gate tools, because `git diff ce9b9ff a84fd23 -- tools` is empty, so the checked files are the same.

## 2026-11-19: review findings (CR-NFR-4, CR-3, AC-20, CR-25, CR-15)

- `README.md` install command: added `--locked`, because `Cargo.toml` uses version ranges for the grammars and a new grammar can change CC (CR-NFR-4, finding 3 of the review).
- `README.md` exit codes: exit code `1` now also names a file that cannot be read (CR-3), and a Git failure under `--changed` returns the exit code of Git. Verified in `src/cli.rs` (`fail` returns 1, `GitError::new(exit_code(...))` carries the Git code).
- `tests/meta.rs`: renamed `ac_20_license_is_0bsd` to `ac_20_license_is_mit`, because the test asserts MIT (AC-20). AC-24 keeps its `#[ignore]`.
- `src/languages/rust.rs` (CR-25): `has_attribute` stopped at the first sibling that is not an attribute, so a comment between `#[test]` and `fn` hid the attribute and the test function was scored. New test `cr_25_a_comment_between_the_attribute_and_the_function_is_skipped` failed with `["keep", "check", "helper"]` before the change. The scan now walks over `line_comment` and `block_comment` siblings and keeps only `attribute_item` nodes.
- `src/runners/typescript_cov.rs` (CR-15): `npx --yes c8` downloads c8 from the network with no announcement. New black-box test `tests/cli.rs` `cr_15_c8_download_is_announced_on_stderr_before_it_runs` puts a fake `npx` on PATH and checks that the notice line comes before the `npx --yes c8` line on stderr and that the fake npx ran. It failed with `no install notice on stderr` before the change. `run_report` now prints `install_notice` for an `npx --yes <package>` command on stderr before it starts. The test file is outside the brief's file list, because the unit test on a new seam would only check the seam, not what the user sees.
- Verification: `cargo build --release` ok, `cargo test` 422 passed and 1 ignored, `cargo clippy --all-targets` no issues, `cargo fmt --check` clean.
- `src/runners/typescript_cov.rs` (CR-15, review finding 1): the install notice had no unit test, so a mutation of the `||` in `install_notice` survived. New unit test `cr_15_install_notice_is_only_for_a_download_command` asserts `None` for `npx vitest run` and for `npx`, and a notice for the c8 command. Reason: the black-box test only covers the c8 path, so nothing pinned the two quiet cases. I swapped `||` for `&&` as a check and the test failed with `Some("Installing vitest ...")`, so the test kills that mutation. The swap was reverted.
- `src/runners/typescript_cov.rs` (CR-15, review finding 2): the notice said `npx downloads it from the network before the run`, which is false when c8 is present. It now says `npx downloads {package} from the network if it is not installed`. The new unit test failed on the old text first and now passes.
- `tests/cli.rs` (CR-15, review finding 3): the c8 notice test named `CR-15, AC-6`, but AC-6 is the cargo-llvm-cov case. It now names CR-15 only. The test checks the notice prefix and the order, not the old wording, so no other change was needed.
- Verification: `cargo build --release` ok, `cargo test` 423 passed and 1 ignored, `cargo clippy --all-targets` no issues, `cargo fmt` applied.

## 2026-11-19: review round 2 (CR-15, CR-25)

- `src/runners/typescript_cov.rs` (CR-15, review finding 3): the unit test `cr_15_install_notice_is_only_for_a_download_command` now also asserts `None` for `pnpm --yes c8` and for `npx --yes`. Reason: in the first case the first word is not `npx`, and in the second case the package is missing. Nothing pinned those two quiet cases. The test passed at once, because `install_notice` already returns `None` for both.
- `src/languages/rust.rs` (CR-25): the doc comment of `has_attribute` had a semicolon. It now has two short sentences, because the house style of this repository forbids a semicolon in prose.
- `src/languages/rust.rs` (CR-25): `has_attribute` dropped `.filter(|n| n.kind() == "attribute_item")`. The `take_while` above it keeps only `attribute_item` and comment nodes, and a comment text never equals `#[test]` or `#[cfg(test)]`, so the filter cannot change the result. The function stays at CC 2 and CRAP 2.0. The CR-25 tests still pass.
- `LOG.md`: put the `422 passed` verification line of the `crapper --lines` change back in front of the later lines. That line is the record of that change. The `423 passed` line is the record of this one. Both stay.
- Verification: `cargo build --release` ok, `cargo test` 423 passed and 1 ignored, `cargo clippy --all-targets` no issues, `cargo fmt --check` clean.
- Unit gates on the changed lines: `dryer --changed --threshold 0.8` exit 0, no candidates. `crapper --threshold 10 --lines 196,197,199,200,201 src/languages/rust.rs` exit 0 (`has_attribute` CC 2, CRAP 2.0). `crapper --threshold 10 --lines 438 src/runners/typescript_cov.rs` exit 0 (`install_notice` CC 4, CRAP 4.0). `crapper --threshold 10 --lines 1191,1192 src/runners/typescript_cov.rs` reports no function, because those lines are test code.
- Mutation gates: `mutator --max-workers 2 --lines 196,197,199,200,201 src/languages/rust.rs` exit 0, `has_attribute` 2 killed, 0 survivors, 0 uncovered. `mutator --max-workers 2 --lines 433 src/runners/typescript_cov.rs` exit 0, `install_notice` 3 killed, 0 survivors, 0 uncovered. `mutator --max-workers 2 --lines 438 src/runners/typescript_cov.rs` exit 0, `install_notice` 0 survivors, 0 uncovered. `mutator --max-workers 2 --lines 1191,1192 src/runners/typescript_cov.rs` exit 0, 0 survivors, 0 uncovered.
- Gate results of commit 2f16fa8, for the record: `cargo test` 423 passed and 1 ignored, `cargo clippy --all-targets` no issues, `cargo fmt` clean, `crapper` 4.0, `dryer` no candidates, `mutator` 0 survivors and 0 uncovered on line 438.

## 2026-11-19: corrections to this log from the review of 9f70152 (CR-15, CR-25)

- Correction to the verification lines above. The `422 passed` line at line 275 belongs to commit 13f41d0, the commit of the first review findings. The `423 passed` line at line 279 is the line that commit 2f16fa8 wrote, because the review of 2f16fa8 states that its diff changed the `422 passed` line of 13f41d0 to `423 passed` in place, and the change of 9f70152 moved that line down by putting the `422 passed` line back in front of it. Line 286 calls the `422 passed` line the record of the `crapper --lines` change, and that names the wrong change and gives no commit. The `--lines` change is the section of 2026-11-15 with 338 unit tests and 52 cli tests. The same line calls the `423 passed` line the record of "this one", and the review of 9f70152 names 2f16fa8 for that line, so the sentence is unclear at best. This session had no `git show` and no `git log`, so I cannot tell from the files whether the second `423 passed` line at line 287 was written by 2f16fa8 or by 9f70152, and this line does not claim which.
- Open owner item (CR-15): c8 stays unpinned in `c8_command` in `src/runners/typescript_cov.rs`. Line 443 runs `npx --yes c8` with no version, so a newer c8 can change the coverage counts of a TypeScript project. CR-15 asks only for the install notice on stderr, so a version pin is an owner decision and Mads Madsen decides. Reason for the record: the review of 2f16fa8 and the review of 9f70152 both name this item, and neither this log nor the open list of `skills/LOG.md` named it before this line.
- The two mutator runs on line 438 and on lines 1191 and 1192 prove only exit 0. Line 438 is the text of a `format!` string and holds no operator, and the mutator makes no site inside a string (MU-13, `tools/mutator/src/sites.rs`). Lines 1191 and 1192 are two assertions of a test function, and both tools skip test code, so the mutator had no site and crapper had no scored function. `src/languages/rust.rs` skips test functions through `is_test_code`, and `tools/mutator/src/functions/rust.rs` skips them through its own `is_test_code`. So the `0 survivors, 0 uncovered` of those runs and the `reports no function` of the crapper run say nothing about behavior. The crapper run on line 438 does score `install_notice` at CRAP 4.0, and the mutator runs on line 433 and on lines 196 to 201 do have kill counts.
- The label `review finding 3` appears on two lines and names two different findings. Line 278 names finding 3 of the review of 13f41d0, which is the wrong acceptance criterion AC-6 on the c8 notice test. Line 283 names finding 3 of the review of 2f16fa8, which is the two quiet cases of `install_notice` with no test. The heading `review round 2` at line 281 means the second review of the crapper fix commits, which is the review of 2f16fa8, and the review of 13f41d0 came before it. `skills/LOG.md:33` calls that same pass the third review, because the skills log counts the first pass of the whole repository, which holds one review for each crate, as one review.

## 2026-11-19: PRD corrections from the confirmed review findings (AC-19, CR-14, CR-15, CR-25, CR-26, CR-NFR-4)

- `docs/prd/crapper.md` AC-19: the command now reads `cargo install --locked --git <repo> crapper`. Reason: `tools/crapper/README.md` and CR-NFR-4 both use `--locked`, and `Cargo.toml` gives the grammars version ranges, so an unlocked install can move a grammar.
- `docs/prd/crapper.md` CR-NFR-4: the requirement now says the port is published in a public repository on GitHub and not on crates.io, and that the install is one `cargo install --locked --git <repo>` command. Reason: the port is public, and the old text said private.
- `docs/prd/crapper.md` non-goal 3: the text now says the port is published in a public repository on GitHub and is not published to crates.io. Reason: same as the CR-NFR-4 line.
- `docs/prd/crapper.md` CR-14: the requirement now states that the port pins no version of `coverage`, of `pytest`, or of `c8`, that `cargo-llvm-cov` is installed with `--locked` (which pins the dependencies of the tool build, not the tool), and that the only pinned install is the Vitest provider of CR-23. Reason: the code in `src/runners/rustpy.rs` (`module_install_steps`, `rust_install_plan`) and in `src/runners/typescript_cov.rs` (`c8_command`) accepts no version, and the review asked for the fact in the requirement.
- `docs/prd/crapper.md` CR-14: the same line names the original commands at the pinned commit. I read them in `src/crapper/runners.py` of `unclebob/crapper` at commit `6fd92c4` over the network, because `parity/.cache` is absent from this worktree. The original runs `<python> -m pip install --disable-pip-version-check <module>`, `npx --yes c8`, `cargo install cargo-llvm-cov --locked`, and `npm install --no-save --no-package-lock @vitest/coverage-v8@<version>`. So the pin behavior of the port matches the original for pip, c8, and cargo-llvm-cov, and the command differences are only the ones CR-18, CR-19, and CR-23 name.
- `docs/prd/crapper.md` CR-26: the requirement now states that each item follows the rules of Python `int()`: an optional `+` sign, an underscore between two digits, and spaces around the item. `--lines 1_0` asks for line 10. A number above `u32::MAX` becomes `u32::MAX`. Reason: `parse_lines` in `src/cli.rs` calls `python_int` from `src/pynum.rs` and clamps with `u32::try_from(...).unwrap_or(u32::MAX)`. The word `positive` stays, because the code rejects a number below 1.
- `docs/prd/crapper.md` CR-25: the text now defines the attributes of an item as the `attribute_item` nodes before it, up to the first node that is neither an attribute nor a comment, and says that a comment between the attribute and the item does not break the attribute. Reason: `has_attribute` in `src/languages/rust.rs` walks over `line_comment` and `block_comment` siblings, and the test `cr_25_a_comment_between_the_attribute_and_the_function_is_skipped` pins that behavior.
- `docs/prd/crapper.md` CR-25: new paragraph with an example that a `#[cfg(test)] impl` block is not a module, so the port scores each function in it as production code. Reason: `is_test_code` looks at enclosing `mod_item` nodes only, so an `impl_item` never counts as a test module. Mads Madsen decided to document this and to leave the code as it is.
- `docs/prd/crapper.md` CR-25 and CR-26: no test is added, because this change touches documents only.
- `tools/crapper/README.md` Install: new paragraph that the port lives in a public repository on GitHub, is not on crates.io, and that the install takes the repository URL and the crate name. Reason: the old text left the repository visibility to the PRD, and the PRD said private.
- `tools/crapper/LOG.md`: this section. Reason: record each action and its reason. The lines at line 165 (`the private repository URL is unknown`) and line 231 (`the repository goes public on GitHub`) stay as they are, because this file never rewrites an old line. Line 165 and line 231 are both out of date: the repository URL is `https://github.com/madsmadsen/lagom-mode-skill` and the repository is public now.
- Check: `cargo test --test meta` passes (9 passed, 1 ignored). A scan of each added line for an em-dash, a semicolon, and the short form of "is not" finds nothing in the PRD and the README. The two PRD hits that remain are old lines.
- CR-26 test check for the number forms: a test exists. `src/cli.rs` unit test `cr_26_lines_parse_to_a_sorted_set_and_skip_empty_items` passes ` 9, 3,,3,+4_0 ` and expects `3, 9, 40`, which pins the spaces, the `+` sign, and the underscore. `cr_26_lines_above_u32_max_become_u32_max` pins the clamp. The end-to-end tests in `tests/cli.rs` use plain numbers only. No code change was made, so no test was added here.

## 2026-11-19: corrections from the review of 1d61e86, and review items CR-26, CR-NFR-4, CR-25, CR-14

- Correction to the heading `2026-11-19: PRD corrections from the confirmed review findings (AC-19, CR-14, CR-15, CR-25, CR-26, CR-NFR-4)`. The heading names CR-15, and no CR-15 text changed in that PRD commit. The CR-15 work of that day is in `src/runners/typescript_cov.rs` and `tests/cli.rs`, not in `docs/prd/crapper.md`. The heading stays as written, because this file never rewrites an old line.
- Correction to the commit message of 1d61e86. The message says that the meta test reads the PRD. It does not: `grep -c prd tools/crapper/tests/meta.rs` returns 0, and the file holds no check against `docs/prd/crapper.md`. The same message says that `cargo test` passes, and the log of that round records only `cargo test --test meta`. The full `cargo test` in `tools/crapper` ran (423 passed, 1 ignored), and the result belongs to that round.
- `docs/prd/crapper.md` CR-26: deleted the last sentence, `Its value has the same form as the value of --lines of mutator`. Reason: `tools/mutator/src/cli.rs` parses with Rust `parse` at lines 253 and 281, so it rejects `1_0` and an overflow, and the CR-26 text allows both.
- `docs/prd/crapper.md` CR-26: deleted the repeated sentence `Spaces around a number do not matter`, because the sentence before it already says that spaces around the item do not change the number. The empty item rule stays, because no other sentence states it.
- `docs/prd/crapper.md` CR-26: `the rules of Python int()` is now `the rules of Python int() for ASCII digits`. Reason: `python_int` in `src/pynum.rs` accepts ASCII digits only and rejects `٣`, which Python accepts.
- `docs/prd/crapper.md` CR-12: the same phrase is now `the rules of Python int() for ASCII digits`. Reason: same function, same limit.
- `docs/prd/crapper.md` CR-NFR-4: the install command now names the crate: `cargo install --locked --git <repo> crapper`. Reason: AC-19 and `README.md` name the crate, and the repository holds three crates with no workspace.
- `docs/prd/crapper.md` CR-14: the list of original commands now ends with the full `npm install --no-save --no-package-lock @vitest/coverage-v8@<version>`. Reason: the list ended with a package spec, and `LOG.md` line 305 names that command and its source (`src/crapper/runners.py` of the original at commit `6fd92c4`).
- `src/languages/rust.rs` (CR-25): new test `cr_25_a_cfg_test_impl_block_keeps_its_functions_as_production_code`. It holds a `#[cfg(test)] impl Board { fn place(&self) {} }` and a `#[cfg(test)] mod unit_tests { fn hidden() {} }` and asserts that only `place` is scored. Reason: the CR-25 paragraph about an `impl` block had no test. `is_test_code` filters on `mod_item` nodes only, so the test passes on the existing code. It pins the documented rule.
- `README.md` (root, install block): added `--locked` to the three `cargo install` lines. Reason: the three crate READMEs and CR-NFR-4 use `--locked`, and no test reads the root README.
- Check: `cargo test cr_25` 8 passed. `cargo build --release` ok. `cargo test` 424 passed and 1 ignored. `cargo clippy --all-targets` no issues. `cargo fmt --check` clean. `dryer --changed --threshold 0.8` no candidates. A scan of each added prose line for an em-dash, a semicolon, and the short form of "is not" finds nothing.

## 2026-11-19: corrections from the review of 25e0075 (CR-12, AC-36)

- Correction to the heading at line 315, `2026-11-19: corrections from the review of 1d61e86, and review items CR-26, CR-NFR-4, CR-25, CR-14`. That heading omits CR-12, and the change under it touched CR-12: line 322 changes the `int()` phrase of CR-12. The commit subject of 25e0075 names CR-12 as well. The heading stays as written, because this file never rewrites an old line.
- Correction to line 318, which says the full `cargo test` in `tools/crapper` ran with `423 passed` for the round of 1d61e86. That line records no time and no command output, so this log cannot prove the run. The main agent ran the full `cargo test` before it made the commit of 1d61e86, and no other record of that run exists. The count of 423 agrees with line 287 and line 290, and the count alone does not prove the run. Read the sentence of line 318 as a report of the main agent, not as a checked result.
- `docs/prd/crapper.md` (CR-25): new AC-36. It names a Rust file with `#[cfg(test)] impl Board { fn place(&self) {} }` and `#[cfg(test)] mod unit_tests { fn hidden() {} }`, and it says that the port scores `place` and does not score `hidden`. AC-36 names the unit test that checks the case. Reason: CR-25 has a paragraph about an `impl` block, and AC-34 does not cover it.
- AC-36 check of the test that proves it. The test `cr_25_a_cfg_test_impl_block_keeps_its_functions_as_production_code` in `src/languages/rust.rs` calls `functions_in`, which wraps `functions_in_source` and reads the function list of the port. It does not run the CLI. No test in `tests/cli.rs` holds an `impl` block: `grep -c "impl " tests/cli.rs` prints 0. So AC-36 words the check on the function list and not on the report text. `cargo test --lib cr_25` passes with 7 tests, and the `impl` test is one of them.
- Check: `cargo test --test meta` passes (9 passed, 1 ignored). A scan of each added line for an em-dash, a semicolon, and the short form of "is not" finds nothing.

## 2026-11-20: approval of the fixes.toml entries (CR-PB-6, AC-24)

- Mads Madsen approved every entry of this crate's `parity/fixes.toml` in the chat of 2026-11-20. The agent set the approval key on each entry as he said. `approved_by = "Mads Madsen"` is now on all 9 entries. `ac_24_fixes_entries_approved` in `tests/meta.rs` lost its `#[ignore]` and runs in `cargo test`. Proof: `python3.12 parity/run.py --check-meta` exits 0, and `cargo test --test meta` passes with 10 tests.

## 2026-11-20: full parity run after the approval (CR-PB-2, CR-PB-6, AC-22)

- Ran the full parity set on a scratch copy of 987d9dd against the original at `6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d` (a clone in a python3.12 venv with tree-sitter 0.26.1 and tree-sitter-language-pack 1.25.0). Command: `python3.12 parity/run.py --port-bin target/release/crapper --original-python <venv python> --record`. Result: runs=58 pass=49 skip=0 diff=9 failed=0, RESULT pass, exit 0. The 9 diffs are the 9 approved entries, and each run reports "exempt: approved fixes.toml entry". `parity/last-run.toml` is the file that the runner wrote, with no hand edit: it now says runs = 58 and lists the 9 diffs, where it said runs = 49 and no diffs before. The run also wrote `parity/recorded/cr13-*` for the six live language runs. `python3.12 parity/run.py --check-meta` exits 0. One earlier attempt stopped at `crash-jacoco-standalone` because another process removed the original venv during the run, and the run was repeated.

## 2026-11-20: fixture coverage for CR-18 and AC-27

- Added `vitest-yarn`, `vitest-bun`, and `vitest-package-manager-field` under `parity/fixtures/`. The last fixture has only the `packageManager` field in `package.json` and no lockfile. Each fixture uses local package manager and Vitest stubs. No real yarn, bun, pnpm, or npm install ran.
- Added `cr_18_ac_27_fixtures_print_install_commands_and_restore_manifest_bytes` in `tests/fixes.rs`. It runs the built port on all four AC-27 fixtures. It checks the exact install command on stderr, coverage `100.0%` for `big`, the bytes of each existing manifest, and the absence of each previously absent lockfile.
- The install stubs deliberately overwrite all seven manifest paths, then install the stub provider. This makes the byte and absence assertions test restoration rather than an install that changes nothing. The pnpm fixture now uses the same deliberate changes. Rejecting stubs for other managers prevent a regression from reaching a real package manager.
- The three new fixtures have Rust tests only and no parity run. The parity runner exempts differences by fixture name, and the approved `vitest-package-manager` entry names only `vitest-pnpm`. Main agreed to omit the three runs because new differences need changes to `fixes.toml`. Neither `fixes.toml` nor `coverage.toml` changed.
- Updated AC-27 in `docs/prd/crapper.md` to name the test file, the fixtures, and the reason for the missing parity runs.
- Failing check before the new fixtures existed: `cargo test --test fixes cr_18_ac_27` exited 101. The test compiled and failed at `copy_dir` with `read dir: No such file or directory`.
- Checks after the fixtures existed: `cargo build --release` exited 0. `cargo test` exited 0 with 426 tests passed. This includes the new AC-27 matrix and the existing pnpm test for coverage `100.0%`. `cargo clippy --all-targets` exited 0 with no issues. `cargo fmt` exited 0.
- Metadata check: `PATH=/tmp/cov-venv/bin:$PATH python3.12 parity/run.py --check-meta` exited 0 and printed `meta ok`. No full parity run ran.
- Test gates: `PATH=/tmp/cov-venv/bin:$PATH dryer --threshold 0.8 tests/fixes.rs` exited 0 with no duplicate candidates. `PATH=/tmp/cov-venv/bin:$PATH crapper --threshold 10 --lines 170,180,214 tests/fixes.rs` exited 0 and scored `install_manifest_bytes` at CRAP 2.0. Formatting moved `assert_fixture_install` to line 181, so that command did not score it. Main must use lines `170,181,215` for the final gate. No production Rust source changed, so no mutation gate applies.

## 2026-11-21: macOS on x86_64 is not a target (crapper)

- Mads Madsen said that macOS on x86_64 is not relevant. The platform requirement of this crate now names macOS 14 or later on arm64 and Linux on x86_64 and arm64, and says that macOS on x86_64 is not a target. Older log lines that say macOS x86_64 was not available stay as written. Proof: `cargo test` for the crate checks that cite the PRD still pass (see the commit message).

## 2026-11-21: review round six, crapper findings (CR-1, CR-13, CR-18, AC-1, AC-17, AC-27, CR-NFR-2)

Corrections first, because this file never rewrites an old line.

- Correction to the entry `2026-11-20: full parity run after the approval (CR-PB-2, CR-PB-6, AC-22)`. It says the run wrote `parity/recorded/cr13-*` for the six live language runs, and it reports `pass=49`. Both statements are true of the run. The sentence leaves the wrong impression about commit c3276b4, which adds only four recording directories: `cr13-clojure`, `cr13-go`, `cr13-java` and `cr13-python`. `cr13-rust` and `cr13-typescript` date from cfda2ab and did not change content. `git show --diff-filter=A c3276b4` lists 12 added files under those four directories, and no file of `cr13-rust` or `cr13-typescript`. The runner rewrites every recording (`parity/run.py`, `record_run`), so the run did write six and the commit added four.
- Correction to the same entry's claim that the six live runs are proof of live coverage. Two of the six held a failed coverage tool. `parity/recorded/cr13-java/stdout.txt` line 1 was `[ERROR] Could not create local repository at ~/.m2/repository -> [Help 1]`. Maven never ran, and `cr13-java/crap.edn` gave coverage `0.0` for both methods, where the stored-report run gives `40.0%` and `100.0%` on the same fixture. `cr13-clojure/crap.edn` gave `0.0` for all three functions, where `clojure-existing` gives `66.7%`, `100.0%` and `0.0%`. Both runs passed only because the original and the port printed the same failure. See the re-run below.
- Correction to the same entry's `--lines` gate line. The line says "Main must use lines `170,181,215` for the final gate". Those numbers held at that commit. After the module comment change of this round the functions are at `175`, `186` and `220`, and the gate ran on those lines (below).
- Correction to the AC-27 entry: the log said `cargo test` ran 426 tests. The count in this crate is 341 in the library unit tests, 0 in the main binary, 53 in `tests/cli.rs`, 10 in `tests/fixes.rs`, 10 in `tests/meta.rs`, 12 in `tests/recorded.rs`, and 0 doc-tests: 426 passed and 0 ignored. A static `grep` of `#[test]` finds 427. The extra hit is `src/languages/rust.rs:324`, which is a `#[test]` line inside a Rust source string that the test feeds to the parser. It is data, not an attribute. The other 426 are real attributes.

### The two failed live runs, re-run with a writable home (CR-13, AC-1)

The root causes were in the fixtures, not in the tools.

- Java: the fixture had no test source, so `mvn ... prepare-agent test ... report` printed `Skipping JaCoCo execution due to missing execution data file` and wrote no `jacoco.xml`. Added `parity/fixtures/java/src/test/java/demo/pkg/BoardTest.java` with three JUnit 5 tests, and the `junit-jupiter` 5.10.2 test dependency in `parity/fixtures/java/pom.xml`.
- Clojure: cloverage 1.2.4 with the old alias printed `No namespaces selected for instrumentation using {:ns-regex (), :ns-exclude-regex ()}` because it defaults `src-ns-path` and `test-ns-path` to empty. The fixture also had no test namespace. Added `parity/fixtures/clojure/test/demo/core_test.clj` and pointed the alias at `-p src -s test -t .*-test`. Cloverage and clojure.test print an elapsed time and a download notice, so the alias now runs the entry point `cov.quiet` in `parity/fixtures/clojure/test/cov/quiet.clj`, which captures that text, prints it on stderr and exits with the cloverage status. `deps.edn` now has `:paths ["src" "test"]`.
- The Maven and `clj` runs need a writable `~/.m2`. The host blocked `~/.m2`. I ran with `_JAVA_OPTIONS=-Duser.home=/tmp/par-home` plus `CLJ_CONFIG`, `CLJ_CACHE` and `GITLIBS` under `/tmp/par-home`, and a pre-created `/tmp/par-home/.m2/repository`. I did not set `HOME`, because the Rust fixture needs `~/.cargo`, and an earlier full run with `HOME=/tmp/par-home` recorded `cr13-rust` at coverage `0.0` instead of `66.6667`. That run is why the recording below has no `HOME` override.
- Command, in `tools/crapper`, once, with the release binary and the original from a python3.12 venv at pinned commit `6fd92c4` with `tree-sitter==0.26.1` and `tree-sitter-language-pack==1.25.0`: `_JAVA_OPTIONS=-Duser.home=/tmp/par-home CLJ_CONFIG=/tmp/par-home/.clojure CLJ_CACHE=/tmp/par-home/.clj-cache GITLIBS=/tmp/par-home/.gitlibs PATH=/tmp/cov-venv/bin:/opt/homebrew/opt/openjdk/bin:$PATH python3.12 parity/run.py --port-bin target/release/crapper --original-python /tmp/par3-venv/bin/python --record`
- Output tail: `SUMMARY runs=58 pass=49 skip=0 diff=9 failed=0`, `RESULT pass`, exit 0. The nine diffs are the nine approved `fixes.toml` entries, each reported as `exempt: approved fixes.toml entry`. The run covers the changed `vitest-pnpm` stub, so `recorded/fix-vitest-package-manager` is current.
- `parity/recorded/cr13-java/crap.edn` now gives `place` `66.6667` and `name` `100.0`. `parity/recorded/cr13-clojure/crap.edn` now gives `grade` `92.3077`, `describe-item` `94.4444` and `tag` `100.0`. `grep -rn "/Users/\|\[ERROR\]\|Picked up" parity/recorded/` finds nothing.
- `parity/recorded/cr13-python` changed in the same run, because I first added `parity/fixtures/python/test_pricing.py` to give the Python fixture a test. The old recording happened to equal `python-existing` and so was a copy of a stored-report run, not of a live run. The new one is live: `discount` `83.3333`, `label` `100.0`, `clamp` `50.0`.
- `parity/last-run.toml` after the run: `commit = "6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d"`, `result = "pass"`, `runs = 58`, `skipped = []`, and the nine `fix-*` ids in `diffs`. The runner wrote it. No hand edit.

### The AC-27 test does fail on a port that does not restore (AC-27, CR-18)

- Scratch copy `/tmp/lagom-norestore` of this crate. In `src/runners/typescript_cov.rs` I replaced the body of `restore_all` with `let _ = snapshot;`, built, and ran `cargo test --test fixes cr_18_ac_27`.
- Result: exit 101, `0 passed; 1 failed`. Message: `assertion left == right failed: vitest-yarn: manifest bytes or lockfile presence changed` at `tests/fixes.rs:211`. `left` held the stub's `changed by the install stub` bytes for the manifests the stub overwrote, and `right` held the fixture bytes. The test checks restoration and not only the install.
- Second variant, same copy with `restore_all` restored: `parity/fixtures/vitest-pnpm/.stubs/pnpm` replaced by `#!/bin/sh` and `exit 0`. Result: exit 101, `0 passed; 1 failed`, `vitest-pnpm: coverage run failed:` at `tests/fixes.rs:206`, and the port printed `Vitest's coverage provider is missing. TypeScript coverage will score 0%`. The no-op stub fails the coverage assertion, as intended.
- Neither scratch change is in the worktree.

### L4: the crapper gate on the changed lines of `tests/fixes.rs`

- After the comment change the functions are at `175` (`install_manifest_bytes`), `186` (`assert_fixture_install`) and `220` (the AC-27 test).
- `PATH=/tmp/cov-venv/bin:$PATH crapper --threshold 10 --lines 175,186,220 tests/fixes.rs` exited 0. Report: `assert_fixture_install` CC 1, coverage 0.0, CRAP 2.0 and `install_manifest_bytes` CC 1, coverage 0.0, CRAP 2.0. Both are below the threshold. The test function is not scored, because a `#[test]` function is test code (CR-25), and the same command with `--lines 220` prints an empty table and exits 0. The earlier gate at `170,180,214` scored only `install_manifest_bytes`, which is why it did not score `assert_fixture_install`.
- `PATH=/tmp/cov-venv/bin:$PATH dryer --threshold 0.8 tests/fixes.rs` exited 0 with no duplicate candidates.

### L5: the module comment of `tests/fixes.rs`

- The comment said "One test for each entry of `parity/fixes.toml`" and "The property is false for the recorded output of the original", and the AC-27 matrix is a second test for the same entry and reads no recording. Added one paragraph that names `cr_18_ac_27_fixtures_print_install_commands_and_restore_manifest_bytes`, says it is the AC-27 matrix, says it reads no recording, and says what it checks (CR-18, AC-27).

### I1: the named runner for CR-NFR-2 and AC-17

- CR-NFR-2 now names the runner: the Ubuntu 22.04 x86_64 guest of `ci/linux-vm`, run in qemu with TCG. The guest is emulated, so the requirement says the check measures only the relation of the two medians and not absolute times. AC-17 names the same runner and says the same. The wording follows DR-NFR-2 and AC-10 of `docs/prd/dryer.md`. `tools/crapper/README.md` has no timing text, so nothing there needed the change.

### I3: the real test count

- `cargo test --no-fail-fast` in `tools/crapper` exited 0. Per suite: `src/lib.rs` 341 passed, `src/main.rs` 0, `tests/cli.rs` 53, `tests/fixes.rs` 10, `tests/meta.rs` 10, `tests/recorded.rs` 12, doc-tests 0. Total 426 passed, 0 ignored, 0 failed. `cargo test --lib -- --list` counts 341, which matches the library total.
- `grep -rc "^\s*#\[test\]" src tests --include=*.rs` sums to 427. The one extra is `src/languages/rust.rs:324`: inside the multi-line `let source = "..."` string of `cr_25_functions_in_a_cfg_test_module_are_skipped`, the text `    #[test]` starts a line. It is fixture text for the parser, not an attribute. So 427 static lines and 426 tests are both right, and the log's earlier 426 was right too.

### Decision 2: the public repository sentence

- `docs/prd/crapper.md` non-goal 3 and CR-NFR-4, and `tools/crapper/README.md`, now attribute the statement to Mads Madsen and say the document does not check it. The repository is not checked here.

### Checks

- `cargo build --release` exited 0. `cargo test` exited 0 with 426 passed. `cargo clippy --all-targets` exited 0 with no issues. `cargo fmt --check` exited 0. `PATH=/tmp/cov-venv/bin:$PATH python3.12 parity/run.py --check-meta` exited 0 and printed `meta ok`.

## 2026-11-21: where lcov.info is read (CR-12, AC-16)

The skill says that crapper reads `lcov.info` only below `coverage/` and `target/coverage/`, and that a report at another path gives each function 0% coverage with no warning. The PRD cites a test for that rule. No test named it, so this entry adds one. The test characterizes the current behavior. The source did not change.

### The new test file

- New file `tests/lcov_paths.rs`, with the CR-NFR-5 header (`upstream-path: none`). Two tests:
  - `cr_12_lcov_is_read_only_below_coverage_and_target_coverage`: a project with one Python function and one LCOV report that says every line ran. (a) the report at `<root>/lcov.info` gives `Cov%` `0.0%` and stderr holds no warning. (b) `<root>/coverage/lcov.info` gives `100.0%`. (c) `<root>/target/coverage/lcov.info` gives `100.0%`. (d) `<root>/target/coverage/rust/src-x/lcov.info` gives `100.0%`.
  - `cr_12_no_report_at_all_gives_zero_coverage_and_no_warning`: `--use-existing-coverage` with no report gives `0.0%` and no warning.
- The `Cov%` cell is compared as its own whitespace token, so a `100.0%` value cannot pass an assertion that expects `0.0%` by substring. The stderr check drops only the `Wrote <path>` snapshot line, which every successful run prints (`src/cli.rs:485`). Any other line fails the test.

### Proof that each case bites (scratch copy `/tmp/crapper-bite`, not in the worktree)

Each run was `cargo test --test lcov_paths` in the copy after one temporary source edit, and the copy was reset from the worktree after each one:

- `merge_lcov` changed to `find_named(root, ...)`, so it reads the root: exit 101, `1 passed; 1 failed`. `CR-12 case (a), a report at the project root is not read, so coverage is 0.0% and not the report value`, left `"100.0%"`, right `"0.0%"`.
- `merge_lcov` changed to read only `target/coverage`: exit 101, `1 passed; 1 failed`. `CR-12 case (b), a report below coverage/ is read, so coverage is 100.0%`, left `"0.0%"`, right `"100.0%"`.
- `merge_lcov` changed to read only `coverage`: exit 101, `1 passed; 1 failed`. `CR-12 case (c), a report below target/coverage/ is read, so coverage is 100.0%`, left `"0.0%"`, right `"100.0%"`.
- `collect_named` changed so it does not descend: exit 101, `1 passed; 1 failed`. `CR-12 case (d), a nested report below target/coverage/rust/src-x/ is read, so coverage is 100.0%`, left `"0.0%"`, right `"100.0%"`.
- A warning added when no LCOV file is found: exit 101, `0 passed; 2 failed`. `CR-12 no report: stderr must hold no warning`, left `["No LCOV coverage report found below coverage/ or target/coverage/."]`, right `[]`, and the same failure for case (a).

### The warning claim, from the source

- With `--use-existing-coverage`, `src/cli.rs` `coverage_bundle` calls `load_bundle` and nothing else. `src/coverage/mod.rs` `merge_lcov` collects files and parses them. No branch prints when none is found, so a missing report gives 0% and no stderr line. The only stderr line of a successful run is `Wrote <root>/.metrics/crap.edn`.
- Without the flag, the run executes the coverage commands. `src/runners/mod.rs` `run_shell` prints `+ (<cwd>) <command>` before each command, and these warnings exist for a failure: `Coverage command exited N. Reports already on disk will not be read.` (custom command, `custom_exit_warning`), `<Language> coverage exited N in <module>. <Language> coverage will score 0%.` (`exit_warning`), `coverage is missing in <package>. Python coverage will score 0%.`, `Neither cargo-llvm-cov nor cargo-tarpaulin is installed...`, `No Cargo.toml; skipping Rust coverage.`, and the Python test and lcov exit warnings of `python_run_warning`. So a successful command that writes its report at a path crapper does not read gives no warning. A failed command or a missing tool does warn, and that warning names the failure, not the path.
- Probe with the release binary in a temp project with one Python file and no report: `--use-existing-coverage` printed only the `Wrote ...` line. `--coverage-command false` printed `+ (...) false` and `Coverage command exited 1. Reports already on disk will not be read.` Default coverage (no flag) ran `python3 -c 'import coverage'`, tried the install, and printed `coverage is missing in ... Python coverage will score 0%.` All three exited 0.

### Checks (in `tools/crapper`, one cargo command at a time)

- `cargo build --release` exited 0.
- `cargo test` exited 0. Suites: `src/lib.rs` 341, `src/main.rs` 0, `tests/cli.rs` 53, `tests/fixes.rs` 10, `tests/lcov_paths.rs` 2, `tests/meta.rs` 10, `tests/recorded.rs` 12, doc-tests 0. Total 428 passed.
- `cargo clippy --all-targets` exited 0 with no issues. `cargo fmt --check` exited 0.
- `PATH=/tmp/cov-venv/bin:$PATH crapper --threshold 10 tests/lcov_paths.rs` exited 0. Report: `fully_hit_record` CC 2, CRAP 6.0, and the six helpers `write_text`, `crapper_in`, `stdout`, `warnings`, `coverage_cell` and `project_with_report` at CRAP 2.0. All below 10. The two `#[test]` functions are not scored, because a `#[test]` function is test code (CR-25).
- `PATH=/tmp/cov-venv/bin:$PATH dryer --threshold 0.8 tests/lcov_paths.rs` exited 0 with `No duplicate candidates found.`

## 2026-11-22: privacy rewrite of the history before the public release

- Rewrote each of the 189 commits of `master`. The rewrite removed the local home path, the name of the sandbox, the folder of the installed skills, the names of the model and of the review agents, and a note about where approvals happened, from the files and from the commit messages. It also removed `docs/.review/` and `docs/prd/.review/`. Each commit hash in a file or in a commit message now names the new commit. Each commit keeps its author, its dates and a GPG signature. Reason: Mads Madsen asked for this before the release. Proof: a search of all files and messages of the new history for the removed words finds only the crate name `iana-time-zone-haiku` in `Cargo.lock`. No path in the history holds `.review`. Each short hash in the files and messages of the new head names a new commit. `git diff` from the old head to the new head shows changes only in the four `LOG.md` files and the removed review files.
