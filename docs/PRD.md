# PRD: lagom-mode-skill

Status: Draft 4. Owner and approver: Mads Madsen. Date: 2026-11-07.

This document states the overall goal of the `lagom-mode-skill` repository and the preferences for its development. Four PRDs state the requirements of the parts. Each statement that comes from a guess and not from a source carries the label `[ASSUMPTION]`.

## 1. Goal

An AI coding agent decides by itself when its work is done. Its idea of "done" changes from run to run. A human must then read the full diff to find complex functions, duplicated code, and code that the tests do not really check.

This repository gives the agent a fixed loop with quality gates. A gate is a tool run and a fixed rule (skills SK-7) on its exit code and snapshot. The rule gives pass or fail and leaves no choice to the agent. On the same input files, the same project tests, and the same coverage tools (after any install that the tool makes on its first run), each tool gives the same exit code and a byte-identical snapshot, except for the accepted risks in section 9. So the agent cannot argue with a gate. The agent finishes a feature only when all tests pass and all gates pass. If a gate still fails after its retry limit (skills SK-9), the agent stops and reports.

The loop is based on [ub-mode](https://github.com/unclebob/ub-mode) by Robert C. Martin. The gates are his three tools: [crapper](https://github.com/unclebob/crapper), [dryer](https://github.com/unclebob/dryer), and [mutator](https://github.com/unclebob/mutator). The tools are written in Python. This repository ports each tool to Rust, so that the crapper, dryer, and mutator binaries need no Python at run time.

## 2. Parts

| Part | Directory | PRD | Content |
| --- | --- | --- | --- |
| Skills | `skills/` | [skills.md](prd/skills.md) | The `lagom-mode` skill and one skill for each tool. |
| crapper | `tools/crapper/` | [crapper.md](prd/crapper.md) | Rust port of crapper: CRAP score for each function. |
| dryer | `tools/dryer/` | [dryer.md](prd/dryer.md) | Rust port of dryer: pairs of code with the same structure. |
| mutator | `tools/mutator/` | [mutator.md](prd/mutator.md) | Rust port of mutator: mutation testing. |

The parts are independent:

1. A tool does not know about another tool. Each tool is one Cargo package with no dependency on another package in this repository.
2. The skills do not know how a tool is built. They use `crapper`, `dryer`, and `mutator` as commands on the `PATH`, with the interface in section 5 of the skills PRD.
3. A change to one part needs no change to another part, if the command interface of the tools stays the same. The command interface is the set of commands, positional file arguments, flags, exit codes, and outputs that a skill reads. Skills PRD section 5 lists all of them. A change to a tool outside this set needs no change to the skills.

## 3. Users

| User | Need |
| --- | --- |
| Developer | Asks an agent for a small feature and gets a change that passed every gate. |
| Agent | Gets numbered steps (SK-5), a written list of stop conditions (SK-25), and a stated next action for each tool exit code (SK-24). |
| Reviewer | Reads a short log and a few small commits, not a large unchecked diff. |
| [uml-viewer](https://github.com/unclebob/uml-viewer) user | Reads the same `.metrics/` snapshots as before, now written by the ports, except for approved `fixes.toml` entries. Before Mads Madsen approves a `fixes.toml` entry that changes snapshot keys or shape, he loads the port's fixture snapshot in uml-viewer at a pinned commit, and it renders with no error. The entry records `uml_viewer_commit` and `uml_viewer_checked_on`. |

## 4. Non-goals

1. The repository does not port uml-viewer.
2. The repository does not add tools, metrics, or languages that the originals do not have.
3. The parts are private. They are not published to crates.io or to a public skill registry.

## 5. Development preferences

### 5.1 Order of delivery

1. Skills work and port work can overlap. The skills use the Rust port of each tool, because the gates need `crapper --lines` and `mutator --lines` (skills SK-8), and the Python crapper rejects `--lines` (CR-26). The skills PRD states this interface in section 5.
2. A port replaces its original only after it passes its parity criteria and Mads Madsen approves.
3. The ports can come in any order, because no port depends on another. `[ASSUMPTION]` crapper comes first, because the skills need it in every gate.
4. The development of this repository uses `lagom-mode` with the tools that are installed at that time.

### 5.2 Ports

1. Each port is a drop-in replacement for its original at one pinned commit: same command name, same flags and positional arguments, same exit codes, and the same snapshot after both are parsed as EDN. The only allowed differences are the entries in the port's `parity/fixes.toml`.
2. A port fixes known bugs of its original and does not copy them. Each fix is listed in the `parity/fixes.toml` of the port, and Mads Madsen approves each entry.
3. Each port has its own parity runner. The runner compares the port with its original on small fixture projects.
4. A port does not share code with another port. A duplicated piece of code is accepted as the cost of independence.
5. The ports build with the Rust stable toolchain. They run on macOS 14 or later and on Linux, on arm64 and x86_64.
6. A user installs each port with one `cargo install --git` command. The README of each port states this command. The skills do not.
7. Each `LICENSE` file in this repository is the MIT License, with Mads Madsen as the copyright holder and an acknowledgment of Robert C. Martin. The `LICENSE` of `lagom-mode` and the `LICENSE` at the root also keep the MIT License notice of ub-mode, because the MIT License requires it in each copy.

### 5.3 Documents and traceability

1. Each requirement has an ID. The ID prefix names its part: `SK` for skills, `CR` for crapper, `DR` for dryer, and `MU` for mutator.
2. Tests, specifications, and commits refer to these IDs.
3. Documents use plain English: short sentences, active voice, and the modals "must", "can", and "will".
4. Each statement that comes from a guess carries the label `[ASSUMPTION]` until the owner accepts or replaces it.

## 6. Success

The repository reaches its goal when all of these are true:

1. Each of the 3 sample features (skills PRD, section 9) ends with a Finish (skills SK-14, SK-16, SK-18) and all four SK-7 conditions true, with no human edit to code during the loop. This holds in both agent harnesses named in the skills PRD, with no change to the skills (skills PRD, AC-14), so all 6 of 6 runs pass. A stop report counts as a failure for this criterion.
2. All three ports have replaced their originals (section 5.1).
3. crapper, dryer, and mutator run on a host with no Python installed on the Clojure, Java, Go, TypeScript, and Rust fixtures (crapper PRD AC-11, dryer PRD AC-8, mutator PRD AC-11).
4. For each sample feature, Mads Madsen reviews only the loop log and the loop commits, with one commit per unit and each message stating why, what and how it was proved (SK-11), and records that he did not need to read the full diff.

## 7. Open questions

Each `[ASSUMPTION]` label in this document and in the four part PRDs is an open question for the owner.

## 8. Acceptance criteria

1. AC-1 (section 2, item 1): For each of `tools/crapper/`, `tools/dryer/`, and `tools/mutator/`, `cargo metadata` lists no path dependency and no git dependency on another package in this repository.
2. AC-2 (section 2, item 2): No file under `skills/` mentions `cargo` or this repository. A search under `skills/` for `cargo` and for `lagom-mode-skill` finds no match.
3. AC-3 (section 2, item 3): Each command, positional file argument, flag, exit code, and output that a skill under `skills/` names, including `lagom-mode`, appears in section 5 of the skills PRD.
4. AC-4 (section 1, determinism): crapper PRD AC-10, dryer PRD AC-7, and mutator PRD AC-13 pass. On each port's fixtures, after any tool install, two runs of each SK-8 gate command give the same exit code and byte-identical snapshots.
5. AC-5 (section 3, uml-viewer user): Each `fixes.toml` entry that changes snapshot keys or shape has the fields `uml_viewer_commit` and `uml_viewer_checked_on`, and the fixture snapshot of that entry renders with no error in uml-viewer at that commit.

## 9. Accepted risks

1. A flaky test in the project can change the result of skills SK-7 item 1 between runs on the same files. The tools do not control this.
2. A mutant whose second run under mutator MU-21 takes about 2 times its timeout can still change its outcome between runs.
3. The first gate run in a new worktree can install a coverage tool. It needs network access to the package registry, and a failed install fails the gate.
