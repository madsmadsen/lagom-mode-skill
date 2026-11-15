# PRD: lagom-mode skills

Status: Draft 1. Owner and approver: Mads Madsen. Date: 2026-11-07.

This document states the requirements for the agent skills in the `skills/` directory. The main skill, `lagom-mode`, makes an agent implement one small feature through a fixed loop with quality gates. It is based on [ub-mode](https://github.com/unclebob/ub-mode) by Robert C. Martin.

## 1. Problem

An AI coding agent decides by itself when its work is done. Its idea of "done" changes from run to run. A human must read the full diff to find complex functions, duplicated code, and code that the tests do not really check.

ub-mode answers this with a set of rules and three tools: crapper, dryer, and mutator. The tools give the same verdict for the same files, so the agent cannot argue with them.

Agents that work in parallel in one checkout also see the changes of each other. A gate can then fail or pass because of a file that another agent changed.

## 2. Definitions

| Term | Meaning |
| --- | --- |
| Skill | A `SKILL.md` file that tells an agent how to do one kind of work. |
| Loop | The fixed sequence of steps that `lagom-mode` runs for one feature. |
| Finish | The successful end of a loop: SK-14, SK-16, and SK-18 succeed. |
| Stop | The end of a loop after a failure: SK-9, SK-12, SK-14, SK-15, or SK-16. |
| Unit | One behavior of the feature: one failing test and the code that makes it pass. |
| Gate | A tool run and a fixed rule (SK-7) on its exit code and snapshot. The rule gives pass or fail and leaves no choice to the agent. |
| Worktree | A Git working tree on its own branch, made with `git worktree add`. |
| Source branch | The branch that was checked out where the loop started. |
| Base commit | The commit that the loop worktree starts from. After a rebase (SK-15), the new tip of the source branch. |
| Feature files | The files that differ between the base commit and the loop worktree. |

## 3. Goals

1. An agent that loads `lagom-mode` implements one small feature through a fixed loop. It finishes only when all gates pass. Otherwise it stops and reports.
2. Each loop runs in its own worktree, so changes by other agents do not affect its gates.
3. The skills work in any agent that reads `SKILL.md` files, for example Claude Code, omp, Cursor, and Grok.

## 4. Non-goals

1. The skills do not depend on features of one agent, for example subagents or plan tools.
2. The loop is for one small feature. It does not plan large projects or split epics.
3. The skills do not specify, build, or install the tools. They use the tools as commands.
4. The skills are private. They are not published to a public skill registry.

## 5. The tools

The skills use three commands from the `PATH`: `crapper`, `dryer`, and `mutator`. A skill works the same with the Python version or the Rust version of each tool. The skills depend only on this interface:

| Command | Used interface |
| --- | --- |
| `crapper` | `--lines <n,n,...>`, a file, `--threshold N`. With `--lines`, `crapper` scores only the functions that contain one of the lines, and the exit status, the table, and the snapshot have only those functions (CR-26). Exit `0` when the worst CRAP score is `N` or lower, and `2` when it is higher than `N` (CR-17). A table on stdout with the worst score first (CR-16). |
| `dryer` | `--changed`, a list of files, `--threshold N`, a report of candidate pairs under `:candidates` in the snapshot `.metrics/dry.edn`. Each pair has its two forms in `:left` and `:right`, each with `:file`, `:start-line`, and `:end-line`, and its similarity score in `:score` (DR-10). Exit `0` after a report, `1` on a usage error, and `2` on an unknown `--format` (DR-13). |
| `mutator` | `--lines <n,n,...>`, a file. With `--lines`, `mutator` runs only the mutations on those lines, and one set of lines applies to every file in a run (MU-2). Exit `0` when every executed mutant is killed, `2` when the baseline fails, and `3` when a mutant survives (MU-20). Uncovered sites and survivors in `.metrics/mutate/<namespace>.edn`: each form in `:forms` has the counts `:uncovered` and `:survived`, and `:outcomes` maps each mutation to `:killed` or `:survived`. |

## 6. The loop

The steps come from ub-mode. `lagom-mode` adds the worktree, the order, and the stop conditions.

```mermaid
flowchart TD
  S[Feature request] --> W[Create worktree from base commit]
  W --> B[Write one failing test]
  B --> C[See it fail for the expected reason]
  C --> D[Smallest change to pass]
  D --> E{All tests pass?}
  E -->|no| D
  E -->|yes| G[Unit gates on uncommitted changes]
  G -->|fail| R[Refactor or add tests]
  R --> E
  G -->|pass| K[Commit the unit]
  K --> I{More behavior in the feature?}
  I -->|yes| B
  I -->|no| F[Feature gates on all feature files]
  F -->|fail| R
  F -->|pass| P[Fast-forward source branch, push, remove worktree]
```

The loop runs the gates two times. The unit gates see only uncommitted changes, so they run before each commit. The feature gates run at the end on the feature files. In both, `crapper` and `mutator` check only the changed lines of each file (SK-8). This catches a problem that spans two units, for example duplication between code from unit 1 and code from unit 3.

## 7. Functional requirements

### 7.1 Format

- SK-1: Each skill is one `SKILL.md` file with YAML frontmatter that has `name` and `description`, the same format as ub-mode.
- SK-2: A skill must not need a tool or a feature that only one agent has.
- SK-3: `lagom-mode` names the three tool skills and tells the agent when to load each one.

### 7.2 Main skill: lagom-mode

- SK-4: `lagom-mode` keeps the three phases of ub-mode, "First Make it Work", "Then Make it Right", and "Practice", and keeps each rule of ub-mode. Two rules change. First, ub-mode pushes to the main line at the end, and `lagom-mode` integrates into the source branch (SK-14 to SK-16). Second, ub-mode reduces each function under CRAP 10, and `lagom-mode` reduces each function to CRAP 10 or lower (SK-7).
- SK-5: `lagom-mode` states the loop of section 6 as numbered steps, in that order.
- SK-6: `lagom-mode` tells the agent to write one failing test, run it, and record the failure reason before it changes production code.
- SK-7: A unit or a feature passes its gates when all of these are true at once:
  1. All tests pass.
  2. crapper reports no function with a CRAP score higher than 10 in the checked files.
  3. dryer reports no candidate pair with a score of 0.8 or more in the checked files.
  4. mutator exits `0` for the checked files, and the snapshots have 0 uncovered sites in the checked functions.
- SK-8: `dryer` checks whole files. The unit gates run `dryer --changed --threshold 0.8`. The feature gates run `dryer --threshold 0.8` with the feature files as positional arguments, test files included. The agent gets the feature files with `git diff --name-only <base-commit>`. `crapper` and `mutator` check only the changed lines. In both gates, the agent runs `crapper --threshold 10 --lines <changed-lines> <file>` and `mutator --lines <changed-lines> <file>` once for each changed file that is not a test file. The agent skips a file that has no changed line, and a file that no longer exists. The agent does not use `--changed` for `crapper` or `mutator`. The agent runs one command for each file, because `mutator --lines` applies one set of lines to every file in a run. The agent does not run `crapper` on a test file, because `crapper` gives the test functions in a named test file 0% coverage and the gate then fails for a reason that no repair clears. In the unit gates, the agent takes the changed lines of a file from `git diff -U0 HEAD -- <file>`, and uses all lines of a new file that git does not track yet. In the feature gates, the agent takes them from `git diff -U0 <base-commit> HEAD -- <file>`.
- SK-9: `lagom-mode` sets a retry limit of 5 tries for each gate. If a gate still fails after the limit, the agent stops and reports the failure and the log. It does not lower a threshold, delete a test, or edit a gate command to pass.
- SK-10: `lagom-mode` tells the agent to keep a log of each action and its reason, and to report the log at the end.
- SK-11: `lagom-mode` tells the agent to commit each unit after its unit gates pass. Each commit message states why, what changed, and how the agent proved it.
- SK-25: `lagom-mode` lists every stop condition (SK-9, SK-12, SK-14, SK-15, SK-16) in one numbered list.

### 7.3 Worktree

- SK-12: At the start of a loop, the agent records the source branch and the base commit. It creates a new worktree on a new branch `lagom-mode/<short-feature-name>` from the base commit. If the loop starts on a detached HEAD, the agent stops before it creates the worktree and asks for a source branch.
- SK-13: The agent makes every edit, test run, gate run, and commit inside the loop worktree. It does not change files in another worktree, except the fast-forward in SK-14.
- SK-14: After the feature gates pass, the agent fast-forwards the source branch to the loop branch with `git merge --ff-only`. It runs the command in the worktree that has the source branch checked out. It makes no merge commit. If the command fails because that worktree has uncommitted changes, the agent stops and reports.
- SK-15: If the source branch moved since the base commit, the agent rebases the loop branch onto the new tip of the source branch. Then it runs all tests and the feature gates again, and does SK-14 only if they pass. If the rebase has a conflict, the agent stops and reports.
- SK-16: If the source branch tracks a remote branch, the agent pushes the source branch after the fast-forward. If the remote rejects the push, the agent stops and reports. It does not force-push.
- SK-17: The agent records the worktree path, the loop branch, the source branch, and the base commit at the top of the log. It records each rebase and the new base commit.
- SK-18: After SK-14 and SK-16 succeed, the agent removes the loop worktree and deletes the loop branch. If the worktree has uncommitted changes, the agent does not remove it, and it reports the changes for a decision by the developer.
- SK-19: If the loop stops, the agent keeps the worktree and the loop branch and reports their names.

### 7.4 Tool skills

- SK-20: Each tool skill names the document with the install instructions of its tool: the README of its port. It also states how to run the tool on uncommitted changes and on a list of files, and how to read its output.
- SK-21: The crapper skill maps a high score to an action: split the function, add tests, or both. It states when a `match` or `case` is worth splitting.
- SK-22: The dryer skill maps a candidate pair to an action: extract the shared shape, or record why the pair stays.
- SK-23: The mutator skill maps each outcome to an action: cover an uncovered site, or kill a survivor with a new test. If no test can kill a survivor, the code is dead, and the agent deletes it.
- SK-24: Each tool skill states the exit codes of its tool and what the agent does for each code.

## 8. Non-functional requirements

- SK-NFR-1: Isolation. Two loops that run at the same time in two worktrees of one repository do not change the gate results of each other.
- SK-NFR-2: License. Each skill folder has a `LICENSE` file with the MIT License, Mads Madsen as the copyright holder, and an acknowledgment of Robert C. Martin. The `LICENSE` of `lagom-mode` also keeps the MIT License notice of ub-mode.

## 9. Acceptance criteria

The sample features run on the sample Rust project `samples/rust-todo`, a command-line to-do list, with the three tools on the `PATH`. The 5 sample feature requests are:

1. Add a `done <id>` command that marks a task as done.
2. Add a `--due <date>` option to `add`, and list overdue tasks first.
3. Add an `export --csv` command that writes all tasks as CSV.
4. Add a `delete <id>` command that removes a task.
5. Add a `--tag <name>` option to `add`, and a `--tag <name>` filter to `list`.

1. AC-1 (SK-1, SK-3, SK-4, SK-5, SK-20 to SK-25): `docs/skill-checklist.md` has one item for each of these requirements. Mads Madsen inspects the skills and records pass or fail for each item. All items pass.
2. AC-2 (SK-7, SK-9): An agent loads `lagom-mode` and gets a small feature request. The agent ends with all four conditions of SK-7 true for the feature files, or with a stop report after the retry limit. Mads Madsen reviews the log for the 5 sample features.
3. AC-3 (SK-6): In the log of each sample feature, every unit has a run of its failing test, with the failure reason recorded, before the first change to production code in that unit.
4. AC-4 (SK-8): In one sample feature, unit 1 and unit 3 add two functions that dryer scores 0.8 or more. The unit gates pass, the feature gates fail, and the agent removes the duplication before it finishes.
5. AC-5 (SK-9): For every sample feature, a script finds no change to a gate command or a threshold under `skills/`, and no deleted test file in the diff of the loop branch.
6. AC-6 (SK-12): A loop that starts on a detached HEAD stops before it runs `git worktree add`, and asks for a source branch.
7. AC-7 (SK-12, SK-13, SK-NFR-1): Two agents run two loops in one repository at the same time. Each loop log shows its own worktree, and no commit in one loop branch contains a file change from the other loop.
8. AC-8 (SK-14, SK-18): After a loop that finishes, the source branch points to the last commit of the loop, and the history has no merge commit. `git worktree list` does not show the loop worktree, and `git branch --list lagom-mode/*` does not show the loop branch.
9. AC-9 (SK-14, SK-19): The worktree that has the source branch checked out has uncommitted changes when the feature gates pass. The agent stops and reports, and the loop worktree and the loop branch still exist.
10. AC-10 (SK-15): Another commit lands on the source branch while a loop runs. At the end, the log shows a rebase, a second run of the feature gates, and then the fast-forward.
11. AC-11 (SK-15, SK-19): The rebase has a conflict. The agent stops and reports, and the loop worktree and the loop branch still exist.
12. AC-12 (SK-16): The remote rejects the push. The agent stops, the log names the rejection, and the remote branch is unchanged.
13. AC-13 (SK-18): The loop worktree has uncommitted changes after SK-14 and SK-16 succeed. The agent does not remove the worktree, and it reports the changes.
14. AC-14 (SK-8): In one sample feature, the agent changes one old file that has a complex function that it does not touch, and adds one new file. In the log of the unit gates, `crapper` and `mutator` run once for each of the two files, with `--lines` set to the changed lines of that file, or to all lines of the new file. The old complex function does not make the gate fail. No run uses `--changed` for `crapper` or `mutator`. A file with no changed line has no run. No run of `crapper` names a test file.

## 10. Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| The agent lowers a threshold or deletes a test to pass a gate. | False pass. | SK-9 forbids it. AC-5 detects it. |
| ub-mode and dryer use different defaults: 0.8 against 0.82. | The agent uses the wrong threshold. | SK-8 states `--threshold 0.8` in the command. |
| The source branch is checked out in another worktree that has uncommitted work. | A fast-forward there can fail, or can mix the loop changes with that work. | SK-14 uses `git merge --ff-only` and stops if Git refuses. The agent never moves the branch with `git branch -f` or `git update-ref`. |
| Each worktree has its own build output and no installed dependencies. | The first build and the first coverage run in a new worktree are slow. | Accept the cost. |
| A tool has a bug that blocks the loop. | The loop stops at the retry limit for a reason outside the feature. | The agent stops and reports (SK-9). |

## 11. Dependencies

1. Git 2.30 or later on each host that runs a loop.
2. `crapper`, `dryer`, and `mutator` on the `PATH`, with the interface in section 5.
3. The test tools of the project that the agent works on.
4. Until a Rust port replaces its original, one wrapper script for each tool puts the Python original on the `PATH` under the command name `crapper`, `dryer`, or `mutator`. The mutator original needs a crapper checkout at `../crapper`, next to the mutator checkout.

## 12. Open questions

None.
