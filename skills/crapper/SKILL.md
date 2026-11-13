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

The install instructions are in the README of the Rust port of crapper. Run `crapper --help` to make sure that the command is on the `PATH`.

## Run

On uncommitted changes (the unit gates):

```bash
crapper --changed --threshold 10
```

On a list of files (the feature gates):

```bash
crapper --threshold 10 <feature-files>
```

crapper runs the coverage command of the project. It prints a table on stdout with the worst score first. It also writes `.metrics/crap.edn`.

## Read the output

Each row of the table has one function with its complexity, its coverage, and its CRAP score. Read the first rows. If the first row is at or below 10, all functions pass.

## Exit codes

| Code | Meaning | What you do |
| --- | --- | --- |
| 0 | The worst score is 10 or lower. | The crapper gate passes. |
| 1 | Bad arguments. | Fix the command. Do not change the threshold. |
| 2 | The worst score is higher than 10. | Act on the first row of the table, as below. Run the gate again. |

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
