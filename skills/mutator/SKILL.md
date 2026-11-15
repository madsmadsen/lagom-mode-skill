---
name: mutator
description: >
  Run mutation tests: change the code in small ways and check that a test
  fails. Cover uncovered sites, kill survivors, delete dead code. Use inside
  lagom-mode at the unit gates and the feature gates, or when the user asks
  whether the tests really check the code.
---

# mutator

mutator makes one small change (a mutant) to the code, then runs the tests. If a test fails, the mutant is killed. If all tests pass, the mutant survived. A survivor means that no test checks that piece of code.

## Install

The install instructions are in the README of the Rust port of mutator. Run `mutator --help` to make sure that the command is on the `PATH`.

## Run

Run `mutator` once for each changed file that is not a test file. Give the changed lines of that file in `--lines`. `mutator` then runs only the mutations on those lines. `--lines` applies one set of lines to every file in a run, so name only one file in each run.

```bash
mutator --lines <n,n,...> <file>
```

Skip a file that has no changed line. Do not use `--changed` for the gates.

On uncommitted changes (the unit gates), get the lines from `git diff -U0 HEAD -- <file>`. For a new file that git does not track yet, use all its lines.

On the feature files (the feature gates), get the lines from `git diff -U0 <base-commit> HEAD -- <file>`.

A hunk header of the diff looks like `@@ -a,b +c,d @@`. The changed lines are `c` to `c+d-1`. If `,d` is missing, `d` is 1. If `d` is 0, the hunk only deletes lines. Use line `c` for it, unless `c` is 0. Join the lines of all hunks of the file with commas.

To list the sites without a test run, use `mutator --scan <path>`.

## Read the output

mutator writes one snapshot for each namespace: `.metrics/mutate/<namespace>.edn`. Each form in `:forms` has two counts:

* `:uncovered`: sites that no test runs.
* `:survived`: mutants that no test killed.

The key `:outcomes` maps each mutation to `:killed` or `:survived`.

The gate passes only if the exit code is 0 and every checked form has `:uncovered` 0.

## Exit codes

| Code | Meaning | What you do |
| --- | --- | --- |
| 0 | Every executed mutant was killed, or nothing ran. | Read the snapshots. The gate passes only if `:uncovered` is 0 in every checked form. |
| 1 | Usage error. | Fix the command. |
| 2 | The baseline tests failed. | Fix the failing tests first. mutator cannot judge the tests until they pass. |
| 3 | At least one mutant survived. | Find the survivors in `:outcomes` and kill them, as below. |

## Act on an outcome

1. Uncovered site: write a test that runs the site and checks its result. Do not add a test that only runs the code.
2. Survivor: read the mutation in `:outcomes`. Write a test that fails when the code has that change. Run the gate again.
3. Survivor that no test can kill: the code does not change any result that a user can see. It is dead code. Delete it.

Run the mutator gate last. It is slow, and the changes for crapper and dryer change the code that it checks.
