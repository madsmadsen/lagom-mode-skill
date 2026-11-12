---
name: mutator
description: >
  Run mutation tests: change the code in small ways and check that a test
  fails. Cover uncovered sites, kill survivors, delete dead code. Use inside
  feature-loop at the unit gates and the feature gates, or when the user asks
  whether the tests really check the code.
---

# mutator

mutator makes one small change (a mutant) to the code, then runs the tests. If a test fails, the mutant is killed. If all tests pass, the mutant survived. A survivor means that no test checks that piece of code.

## Install

The install instructions are in `mutator/README.md` in the `deterministic-coding` repository. Run `mutator --help` to make sure that the command is on the `PATH`.

## Run

On uncommitted changes (the unit gates):

```bash
mutator --changed
```

On a list of files (the feature gates):

```bash
mutator <feature-files>
```

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
