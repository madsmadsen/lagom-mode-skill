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

The install instructions are in the README of the Rust port of mutator, `tools/mutator/README.md`. Run `mutator --help` to make sure that the command is on the `PATH`.

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

On a loaded host, pass `--max-workers 2`. Never use `--changed` for the gates.

On uncommitted changes (the unit gates), get the lines from `git diff -U0 HEAD -- <file>`. For a new file that git does not track yet, use all its lines.

On the feature files (the feature gates), get the lines from `git diff -U0 <base-commit> HEAD -- <file>`.

Get the changed lines from the hunk headers of the diff. A hunk header looks like `@@ -a,b +c,d @@`. The changed lines are `c` to `c+d-1`. If `,d` is missing, `d` is 1. If `d` is 0, the hunk only deletes lines. Use line `c` for it, unless `c` is 0. A hunk with `c` of 0 and `d` of 0 gives no line. Join the lines of all hunks of the file with commas.

## Delete the file's snapshot first

Before each run on a file, delete each snapshot under `.metrics/mutate/` that holds a form of that file. A snapshot that stays keeps the outcomes of earlier runs for the whole file. `--lines` runs only the mutations on the lines that you give, but the outcome list still holds every mutation of the file, so an exit 3 can come from a line that this run did not change.

The snapshot path follows the namespace: `.metrics/mutate/<namespace>.edn`, and the parts of the namespace become the directories (MU-4). `::` becomes `/`. A form of `crate::foo::Bar` is in `.metrics/mutate/crate/foo/Bar.edn`, and a Go form of `example.com/demo.Widget` is in `.metrics/mutate/example.com/demo.Widget.edn`.

A snapshot holds the forms of one namespace, and those forms can come from more than one file. Read each `.edn` file under `.metrics/mutate/` and look at its `:forms`. Each form has `:file`, the path of its source file relative to the project root. Delete each snapshot in which a form has `:file` equal to the path of your file. A run also prints the path of each snapshot that it writes in a `Wrote <path>` line on stderr, so an earlier run shows you the file to delete.

mutator writes the snapshot again on the next run. The delete is safe: mutator reads the snapshots when it starts and creates the directory when it writes, so a missing file only means that it starts with no earlier outcome for that file. A run that exits `2` writes no snapshot, so a deleted one stays absent until a later run. That later run treats each site in `--lines` as changed and runs it again instead of trusting an old outcome. The run takes longer and is still correct.

Two checked files can share one snapshot. Delete it once, then give each of those files its run after the delete, and do not delete again. The first run writes its own forms, and the second run keeps them and adds its own.

With a fresh snapshot, a survivor comes only from a changed line.

To list the sites without a test run, use `mutator --scan <path>`. A scan does not run mutants and needs no delete. It reads the snapshots that it finds, and it does not write one.

## Read the output

mutator writes one snapshot for each namespace: `.metrics/mutate/<namespace>.edn`. Each form in `:forms` has two counts:

* `:uncovered`: sites that no test runs.
* `:survived`: mutants that no test killed.

The key `:outcomes` maps each mutation to `:killed` or `:survived`.

A checked form is a form whose `:file` is that changed file and whose `:line` to `:end-line` contain a changed line. The gate passes only if the exit code is 0 and every checked form has `:uncovered` 0. `:uncovered` counts every uncovered site of the form, not only the sites on the changed lines. A form whose lines have no changed line is not checked.

## Exit codes

| Code | Meaning | What you do |
| --- | --- | --- |
| 0 | Every executed mutant was killed, or nothing ran. | Read the snapshots. The gate passes only if `:uncovered` is 0 in every checked form. An exit 0 with a `No coverage data` message on stderr is a fail: no report was found, so no site was covered. Fix the coverage command. An exit 0 with a message that holds `will score 0%` is also a fail: a coverage tool failed, so every site scores 0%. Fix the coverage run. Do not pass `--no-coverage`: it sets `:uncovered` to 0, so the snapshot check proves nothing. It also edits a gate command (SK-9). |
| 1 | Usage error. | Fix the command. |
| 2 | The baseline tests failed. | Fix the failing tests first. mutator cannot judge the tests until they pass. |
| 3 | At least one mutant survived. | Find the survivors in `:outcomes` and kill them, as below. You deleted the snapshot of the file before the run, so each survivor comes from a changed line. |
| other code | `--coverage-command` failed and returned its own exit code. | Read the message on stderr. It names the exit code and the last lines of the output. Fix the coverage command. |

A failed `--coverage-command` can also exit with 1, 2, or 3, because its code is not a mutator code. Code 1 looks like a usage error. Check stderr for `Coverage command exited` before you read those rows.

## Act on an outcome

1. Uncovered site: write a test that runs the site and checks its result. Do not add a test that only runs the code.
2. Survivor: read the mutation in `:outcomes`. Write a test that fails when the code has that change. Run the gate again. The snapshot of the file was deleted before the run, so a survivor cannot come from a line that you did not change. If one does, a snapshot of the file was still in place: delete it and run again.
3. Survivor that no test can kill: the code does not change any result that a user can see. It is dead code. Delete it.

Run the mutator gate last. It is slow, and the changes for crapper and dryer change the code that it checks.
