---
name: crapper
description: >
  Score each function with the CRAP metric (complexity and missing test
  coverage) and act on the result. Use inside lagom-mode at the unit gates
  and the feature gates, or when the user asks for function complexity.
---

# crapper

crapper gives each function a CRAP score. A high score means the function is complex, has weak tests, or both. The gate limit is 10. A function at 10.0 passes, and a function above 10.0 fails.

## Install

The install instructions are in `tools/crapper/README.md` of the repository at https://github.com/madsmadsen/lagom-mode-skill. Run `crapper --help` to make sure that the command is on the `PATH`.

## Run

Run `crapper` once for each changed file that is not a test file. Give the changed lines of that file in `--lines`. `crapper` then scores only the functions that contain one of those lines. Old functions that you did not touch do not count. With `--threshold`, the exit status depends only on the scored functions. The snapshot and the table have only those functions.

```bash
crapper --threshold 10 --lines <n,n,...> <file>
```

Skip a file that has no changed line. Skip a file that no longer exists. Do not use `--changed` for the gates.

On uncommitted changes (the unit gates), get the lines from `git diff -U0 HEAD -- <file>`. For a new file that git does not track yet, use all its lines.

On the feature files (the feature gates), get the lines from `git diff -U0 <base-commit> HEAD -- <file>`.

A hunk header of the diff looks like `@@ -a,b +c,d @@`. The changed lines are `c` to `c+d-1`. If `,d` is missing, `d` is 1. If `d` is 0, the hunk only deletes lines. Use line `c` for it, unless `c` is 0. Join the lines of all hunks of the file with commas.

Do not run `crapper` on a test file. It gives the test functions 0% coverage, so each test function with a complexity of 3 or more fails the gate. The `lagom-mode` skill lists the test paths.

crapper runs the coverage command of the project. It prints a table on stdout with the worst score first. It also writes `.metrics/crap.edn`. Each run overwrites that file. crapper can install a missing coverage tool from the network (CR-14).

## Read the output

Each row of the table has one function with its complexity, its coverage, and its CRAP score. Read the rows above 10. If every row is at or below 10, all functions pass.

Exit 0 only proves that the scored functions pass. Confirm that each changed function has a row. `No source files to analyze.` or an empty table means that the line list is wrong. So does a missing function.

## Exit codes

| Code | Meaning | What you do |
| --- | --- | --- |
| 0 | The worst score is 10 or lower. | Confirm that each changed function has a row. Then the crapper gate passes. |
| 1 or any other code | Bad arguments, or a failure to read. | Read stderr. Fix the command or the file. Do not change the threshold. |
| 2 | The worst score is higher than 10. | Read stderr for a coverage warning first. If a line holds `will score 0%`, fix the coverage run, not the code. Otherwise act on each row above 10, as below. Run the gate again. |

Code 1 covers a bad argument, an unreadable file, a bad coverage report, and a metrics write error. A Git error returns its own code.

## Act on a high score

For each function above the limit, do one of these:

1. Split the function into smaller functions, each with one job. This lowers the complexity.
2. Add tests that run the untested branches. This raises the coverage.
3. Do both. With full coverage, the CRAP score equals the complexity. So complexity above 10 cannot pass, even with full coverage. Split first in that case.

Add tests that check behavior. A test that only runs the code to raise the coverage is not a good test. The mutator gate finds it later.

## Case statements

A `match` or `case` has one branch for each arm, so its complexity is high. Split it in these cases:

* An arm holds more than one statement or its own logic. Move the arm body into a named function.
* The score stays above 10 after tests cover every arm. Split the arms into groups with a named function for each group.

If the statement only maps one value to another value, replace it with a lookup table. The table is data and has no branches.

Do not leave a score above 10 in place. The gate has no exception for a `match` or a `case`.
