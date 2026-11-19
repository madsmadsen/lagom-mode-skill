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

The install instructions are in the README of the Rust port of mutator, `tools/mutator/README.md`. The repository is at https://github.com/madsmadsen/lagom-mode-skill. Run `mutator --help` to make sure that the command is on the `PATH`.

## Side effects

Before it judges anything, mutator changes the host in two ways that you must know about:

1. If a coverage tool is missing, mutator installs it from the network (MU-11). It prints the install on stderr before it starts. A run can therefore install packages in the project, or on the host.
2. Before each run that is not a scan run, mutator restores every file under `target/mutator-backup/` (MU-18). The restore overwrites later edits to the source files. Delete that directory when it holds no copy that you need, or keep your edits out of the restored files until the run ends.

## Run

Run `mutator` from the project root. It writes `.metrics/mutate/` below the root.

Run `mutator` once for each changed file that is not a test file. Give the changed lines of that file in `--lines`. `mutator` then runs only the mutations on those lines. `--lines` applies one set of lines to every file in a run, so name only one file in each run. Skip a file that has no changed line, and a file that no longer exists.

```bash
mutator --lines <n,n,...> <file>
```

To run on a list of files, name each one as an argument: `mutator <file>...`. `--lines` applies to every file, so pass `--lines` with one file only.

On a loaded host, pass `--max-workers 2` (AGENTS.md, "Load on the host"). Never use `--changed` for the gates.

On uncommitted changes (the unit gates), get the lines from `git diff -U0 HEAD -- <file>`. For a new file that git does not track yet, use all its lines.

On the feature files (the feature gates), get the lines from `git diff -U0 <base-commit> HEAD -- <file>`.

A hunk header of the diff looks like `@@ -a,b +c,d @@`. The changed lines are `c` to `c+d-1`. If `,d` is missing, `d` is 1. If `d` is 0, the hunk only deletes lines. Use line `c` for it, unless `c` is 0. If `c` is 0 and `d` is above 0, the hunk adds lines at the start of the file. Use lines 1 to `d` for it. If `c` is 0 and `d` is 0, the hunk has no line to check. Join the lines of all hunks of the file with commas.

To list the sites without a test run, use `mutator --scan <path>`.

## Read the output

mutator writes one snapshot for each namespace: `.metrics/mutate/<namespace>.edn`. Each form in `:forms` has two counts:

* `:uncovered`: sites that no test runs.
* `:survived`: mutants that no test killed.

The key `:outcomes` maps each mutation to `:killed` or `:survived`.

A checked form is a form whose `:file` is that changed file and whose `:line` to `:end-line` contain a changed line. The gate passes only if the exit code is 0 and every checked form has `:uncovered` 0. `:uncovered` counts every uncovered site of the form, not only the sites on the changed lines. A form whose lines have no changed line is not checked.

## Exit codes

| Code | Meaning | What you do |
| --- | --- | --- |
| 0 | Every executed mutant was killed, or nothing ran. | Read the snapshots. The gate passes only if `:uncovered` is 0 in every checked form. An exit 0 with a `No coverage data` message on stderr is a fail: no report was found, so no site was covered. Fix the coverage command, or pass `--no-coverage` only if the gate may run without coverage. |
| 1 | Usage error. | Fix the command. |
| 2 | The baseline tests failed. | Fix the failing tests first. mutator cannot judge the tests until they pass. |
| 3 | At least one mutant survived. | Confirm that the survivor is on a changed line, because an old snapshot carries outcomes forward. Then find the survivors in `:outcomes` and kill them, as below. |
| other code | The coverage command failed and returned its own exit code. | Read the message on stderr. It names the exit code and the last lines of the output. Fix the coverage command, or pass `--no-coverage` only if the gate may run without coverage. |

A coverage failure can also exit with 2 or 3, because its code is not a mutator code. Check stderr for `Coverage command exited` before you read those rows.

## Act on an outcome

1. Uncovered site: write a test that runs the site and checks its result. Do not add a test that only runs the code.
2. Survivor: confirm that the survivor is on a changed line. An old snapshot carries its outcomes forward for the whole file, and `--lines` runs only the changed lines. Exit 3 can then come from a line that you did not change. If the survivor is on an unchanged line, delete the snapshot `.metrics/mutate/<namespace>.edn` that holds the form and run the gate again. If it is on a changed line, read the mutation in `:outcomes`. Write a test that fails when the code has that change. Run the gate again.
3. Survivor that no test can kill: judge it first. An equivalent mutant changes the code and no result, for example `<` to `<=` where both give the same answer. It is not dead code, and deleting it breaks the program. Delete the code only when the survivor shows that the code changes no result that a user can see.

Run the mutator gate last. It is slow, and the changes for crapper and dryer change the code that it checks.
