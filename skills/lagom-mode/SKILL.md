---
name: lagom-mode
description: >
  Implement one small feature through a fixed loop in its own Git worktree.
  Each behavior gets a failing test first. Quality gates (crapper, dryer,
  mutator) decide when the work is done. The loop ends by fast-forwarding the
  source branch. Use when the user asks for a feature, a change, or a fix, or
  runs /lagom-mode. Based on ub-mode by Robert C. Martin.
---

# lagom-mode

Lagom is Swedish for "just enough, not too much". Do the smallest change that the gates accept, and nothing more.

Every time you are asked to make a change, run the loop in this file. The loop follows three phases:

* First Make it Work.
* Then Make it Right.
* Follow Practice.

The loop is for one small feature. If the request is large, ask the user to split it. Do not plan a large project here.

## Tool skills

Three tools give the same verdict for the same files. You cannot argue with them. All three run at the unit gates (step 7), so load the skill of each tool there. `crapper` scores each function for complexity and missing tests. `dryer` finds code that repeats in a similar shape. `mutator` changes the code and checks that a test fails.

## Definitions

* Source branch: the branch that was checked out where the loop started.
* Base commit: the commit that the loop worktree starts from. After a rebase (step 11), it is the new tip of the source branch.
* Unit: one behavior of the feature. It is one failing test and the code that makes it pass.
* Feature files: the files that differ between the base commit and the loop worktree. You get them with `git diff --name-only <base-commit>`. Leave out files that no longer exist.
* Gate: a tool run and a fixed rule on its exit code and snapshot. The rule gives pass or fail. You have no choice.

## Gate rule

A unit or a feature passes its gates when all of these are true at once:

1. All tests pass.
2. `crapper` reports no function with a CRAP score higher than 10 in the functions that contain a changed line.
3. `dryer` reports no candidate pair with a score of 0.8 or more in the checked files.
4. `mutator` exits `0` for the changed lines, and its snapshots have 0 uncovered sites in the checked functions. Delete the snapshot of the file before each run, as below. Then a survivor can come only from a changed line.

`dryer` checks whole files. `crapper` and `mutator` check only the changed lines of each file. Run them once for each changed file that is not a test file. Leave out a file that no longer exists. Give each run the changed lines of that file in `--lines`. Skip a file that has no changed line. Skip a file that is not source code in a language the tools know, such as the build manifest of a project or a README file. Do not use `--changed` for `crapper` or `mutator`.

`--lines <n,n,...>` has the same purpose for both tools. `crapper --lines` scores only the functions that contain one of the lines. With `--threshold`, its exit status depends only on those functions. `mutator --lines` runs only the mutations on those lines. You run one command for each file, because `mutator --lines` applies one set of lines to every file in a run. The two tools read the value in different ways: `crapper` accepts the number forms of Python `int()`, so `1_0` means line 10, and `mutator` accepts only digits, with an optional `+` sign and no underscore. Pass plain digits only, as the skills do.

The unit gates run these commands on uncommitted changes. The lines come from `git diff -U0 HEAD -- <file>`. A new file that git does not track yet has no diff. For that file, use all its lines.

In both gates, before each `mutator` run on a file, delete each snapshot under `.metrics/mutate/` that holds a form whose `:file` is that file. A form without `:file` in an old snapshot takes the `:source` of that snapshot, so delete a snapshot whose `:source` is that file too. The snapshot path is `<namespace>.edn`, and the parts of the namespace become the directories. A Rust function in `<crate-name>::foo::Bar` is in `.metrics/mutate/<crate-name>/foo/Bar.edn` (MU-4). mutator writes the file again on the next run. Two checked files can share one snapshot. Delete it once, then give each file that shares it its run after the delete, and read the gate result of each file right after its own run. A retry of one of those files repeats the delete, so it repeats the run of every checked file that shares the snapshot. Then each checked file starts with no earlier outcome, and an exit `3` comes only from a line that its run changed.

```bash
dryer --changed --threshold 0.8
crapper --threshold 10 --lines <changed-lines> <file>
mutator --lines <changed-lines> <file>
```

The feature gates run the same commands on the feature files. The lines come from `git diff -U0 <base-commit> HEAD -- <file>`. Give `dryer` all the feature files:

```bash
dryer --threshold 0.8 <feature-files>
crapper --threshold 10 --lines <changed-lines> <file>
mutator --lines <changed-lines> <file>
```

Get the changed lines from the hunk headers of the diff. A hunk header looks like `@@ -a,b +c,d @@`. The changed lines are `c` to `c+d-1`. If `,d` is missing, `d` is 1. If `d` is 0, the hunk only deletes lines. Use line `c` for it, unless `c` is 0. A hunk with `c` of 0 and `d` of 0 gives no line. Join the lines of all hunks of the file with commas.

A test file is a file in a directory named `__tests__`, `spec`, `specs`, `test`, or `tests`. It is also a file with a name like `*_test.go`, `*.test.ts`, or `test_*.py`. The tools skip test files by default. `crapper` and `dryer` take a test file when you name it. `mutator` drops a test file that you name, so a named test file gets no mutator run. If you name a test file, `crapper` gives its test functions 0% coverage, because no test runs a test function. Each test function with a complexity of 3 or more then scores above 10, and no repair clears the gate. So do not run `crapper` on a test file. `dryer` gets all the feature files, test files included. `testdata` is not a test directory. It is one of the directories that the walk of each tool skips, so the tools never see a file below it.

The unit gates see only uncommitted changes. The feature gates see the whole feature. They catch a problem that spans two units, for example duplication between code from unit 1 and code from unit 3.

Retry limit: you have 5 tries for each gate. The count is separate for each gate of each unit and for each gate of the feature gates. The count restarts for each new unit and for the feature gates. A return to step 11 does not restart the count of tries of a gate. A try is one failed run of the gate after a change. The first failed run of a gate is try 1. The loop stops after the fifth failed run of a gate, and no sixth run of that gate follows. A run that passes uses no try, so a passing run after a rebase leaves the count as it was. A failed feature gate sends the loop back to step 7 and then to step 10. That return does not restart the count of the feature gates, and a unit that the fix adds has its own unit counts from step 3. A gate that fails after the rebase of step 11 counts as a failed run of that gate, and its fix returns to step 7 through the same rule. If a gate still fails after the limit, stop (stop condition 1). Do not lower a threshold, delete a test, or edit a gate command to pass.

## Stop conditions

When one of these happens, stop the loop. If a worktree exists, keep the worktree and the loop branch. Name the stop condition in the report, and report the cause, the names of the worktree and the loop branch, and the log.

1. A gate still fails after 5 tries. Report the failure and the log of the gate.
2. The loop starts on a detached HEAD. Stop before you create the worktree. Ask the user for a source branch.
3. `git merge --ff-only` fails because the worktree of the source branch has uncommitted changes. Report the changes.
4. The rebase of step 11 has a conflict.
5. The remote rejects the push. Do not force-push.
6. The fast-forward still fails because the source branch moved, after 5 returns to step 11. Report the returns and the log.

## The loop

Make every edit, test run, gate run, and commit inside the loop worktree. Do not change files in another worktree. The only exception is the fast-forward in step 12.

1. Record the source branch and the base commit. Run `git branch --show-current` and `git rev-parse HEAD`. If the output of the first command is empty, HEAD is detached. Stop (stop condition 2).
2. Create the worktree on a new branch from the base commit: `git worktree add -b lagom-mode/<short-feature-name> <path> <base-commit>`. Put `<path>` outside the repository, at a sibling path `<repo>-lagom-<short-feature-name>` or at a scratch path such as `/tmp/<repo>-lagom-<short-feature-name>`. If the branch or the path exists, add a number. Write the worktree path, the loop branch, the source branch, and the base commit at the top of the log. Go into the worktree. Do the next steps there. Before the first test, run `git check-ignore -q .metrics/`. If Git does not ignore it, add `.metrics/` to the file given by `git rev-parse --git-path info/exclude`. Never commit `.metrics/`.
3. Write one failing test for the next behavior of the feature.
4. Run the test. Make sure that it fails for the reason that you expect. Write the failure reason in the log. Do not change production code before this step is done.
5. Make the smallest change that makes the test pass.
6. Run all tests. If a test fails, go to step 5.
7. Run the unit gates. If a gate fails, refactor or add tests, then go to step 6.
8. Commit the unit. The message states why, what changed, and how you proved it.
9. If the feature has more behavior, go to step 3.
10. Run the feature gates on the feature files. If a gate fails, refactor or add tests, then run the tests and go to step 7.
11. Run `git rev-parse <source-branch>`. If the source branch moved since the base commit, run `git rebase <source-branch>` in the worktree. Write the rebase and the new base commit in the log. Then run all tests and the feature gates again. If the rebase has a conflict, stop (stop condition 4). Go to step 12 only if the tests and the gates pass.
12. Find the worktree that has the source branch checked out with `git worktree list`. In that worktree, run `git merge --ff-only lagom-mode/<short-feature-name>`. This makes no merge commit. If the command fails because that worktree has uncommitted changes, stop (stop condition 3). If it fails because the source branch moved after step 11, go back to step 11. You can return 5 times. After the fifth return, if the command fails for that reason again, stop (stop condition 6). Never move the source branch with `git branch -f` or `git update-ref`.
13. If the source branch tracks a remote branch, push the source branch. If the remote rejects the push, stop (stop condition 5).
14. Remove the loop worktree with `git worktree remove <path>`. Delete the loop branch with `git branch -d lagom-mode/<short-feature-name>`. If the worktree has uncommitted changes, do not remove it. Report the changes for a decision by the developer.
15. Report the log.

# First Make it Work

## Laziness Protocol

* One decision lives in one place.
* Ship the smallest edit that solves the problem.

## Foundational Thinking

* Get the data shape right before you write the logic.
* Do first whatever helps every later phase.
* If a concurrent writer would matter, do not share state.

## TDD

* Write the failing test before you change the code.
* See it fail for the reason you expect.
* Make the smallest change that makes it pass.
* If an honest test is impractical, do not write a bad one.

## Redesign From First Principles

* Do not bolt a new requirement onto the old design.
* Ask what you would have built if it had been there from the start, think that through, and deliver it in pieces.

## Attack the Premise

* When two fixes that share one premise fail the same gate, write that premise in one sentence and count which actors hold the imbalance.
* Remove the assignment.
* Do not start another fix until you have done that.

## Subtract Before You Add

* Take complexity out before you add anything.

## Outcome-Oriented Execution

* Aim at the end state you can check.
* A break in the middle is allowed when you planned it and can undo it.

## Build the Lever

* If the work is not obvious at a glance, build the tool that does it.
* Do one unit by hand, then make the tool match that unit.

## Architecture

### Model the Domain

* Put the domain in a structure that makes the illegal states impossible.
* Maintain a glossary of domain terms.
* If the code is already clear and local, leave it.

### Boundary Discipline

* Check the data where it enters.
* Inside, trust it.
* Keep the business logic in a pure function.

### Type System Discipline

* Make the illegal value impossible to write.
* Parse outside data once, where it comes in.
* Do not silence the compiler.

### Make Operations Idempotent

* Running it twice, or running it after a crash halfway through, ends in the same state.

### Separate Before Serializing Shared State

* If they do not need the same mutable thing, give each its own.
* If they do, allow one writer, and enforce that in the structure.

# Then Make it Right

## Fix Root Causes

* Reproduce it.
* Fix the cause.
* A guard that only hides the crash is not a fix.

## Sequence work into verifiable units

* Finish one small unit and check it.
* Then start the next.

## Test Behavior, Not Implementation

* The test calls the code the way its user does and checks the result that user would see.
* If the test would still pass when every function returned nothing, throw it out.

## Crapper

* Use the crapper tool. Load the `crapper` skill.
* Reduce each function to CRAP 10 or lower by splitting it, by testing it, or both.
* Consider whether case statements are worth reducing.

## Dryer

* Use the dryer tool. Load the `dryer` skill.
* Eliminate duplication that ranks 0.8 or above.

## Mutator

* Use the mutator tool. Load the `mutator` skill.
* Cover the uncovered.
* Kill the survivors.
* Delete dead code.

## Refactoring

* Pin what the code does.
* Change the structure.
* Prove the behavior did not change.

# Practice

## Keep a Log

* Keep a log of every action you take and the reason you took it.
* Report that log upon completion.
* The top of the log has the worktree path, the loop branch, the source branch, and the base commit. Each rebase and each new base commit goes in the log.

## Guard the Context Window

* Keep the main thread small.
* Send the bulk elsewhere, and keep the summary.

## Worktree Cleanup

* Find what is safe to remove, confirm nothing still in use needs it, then reclaim the disk.
* Uncommitted work waits for a decision.

## Commit and Integrate

* Commit often, in small ordered commits.
* Each commit says why, what changed, and how you proved it.
* At the end, fast-forward the source branch to the loop branch. Push it if it tracks a remote. This replaces "push to the main line" of ub-mode. See steps 12 and 13.
