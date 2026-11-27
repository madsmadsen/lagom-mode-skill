---
name: dryer
description: >
  Find code that repeats in a similar shape and remove the duplication. Use
  inside lagom-mode at the unit gates and the feature gates, or when the
  user asks for duplicate code.
---

# dryer

dryer compares each function, method, or top-level form with the others in the same language. It gives each pair a similarity score from 0 to 1. The gate limit is 0.8. A pair with a score of 0.8 or more fails.

dryer works on Clojure, Java, Go, TypeScript, JavaScript, Rust, and Python. It reads a `.js`, `.jsx`, `.mjs`, or `.cjs` file as TypeScript, so a JavaScript file takes part in the comparison like a TypeScript file.

## Install

The install instructions are in `tools/dryer/README.md`. Run `dryer --help` to make sure that the command is on the `PATH`.

## Run

Run dryer in the root of the loop worktree. `--changed` and the snapshot use the project root. The project root is the current directory unless you give `--root`.

Always give the threshold. The default threshold of the Rust port is 0.82, and the gate uses 0.8.

On uncommitted changes (the unit gates):

```bash
dryer --changed --threshold 0.8
```

On a list of files (the feature gates):

```bash
dryer --threshold 0.8 <feature-files>
```

Leave out a file that no longer exists. dryer treats a path that is missing as a substring filter. That filter can remove every file from the run. Skip the run when the list is empty.

Test paths are included only when you name them. Rust test code stays out of the scan even when you name it (DR-15).

## Read the output

dryer prints the pairs that score at or above the threshold. It also writes `.metrics/dry.edn`. The key `:candidates` holds the pairs. Each pair has these keys:

* `:left` and `:right`: the two forms. Each has `:file`, `:start-line`, and `:end-line`.
* `:score`: the similarity score.

The dryer gate passes when no pair in `:candidates` has a side whose `:start-line` to `:end-line` holds a changed line of its `:file`. A pair of two old forms does not count.

Get the changed lines of each file from the hunk headers of its diff, as `lagom-mode` says. For a unit gate, run `git diff -U0 HEAD -- <file>`. For a feature gate, run `git diff -U0 <base-commit> HEAD -- <file>`. A new file that git does not track yet has no diff, so all its lines count. You do not pass these lines to dryer. dryer still checks the whole file, and the command stays the same.

dryer reports no source file when it finds none. That message is a pass. dryer writes no snapshot in that run. Do not read an old `.metrics/dry.edn`.

## Exit codes

| Code | Meaning | What you do |
| --- | --- | --- |
| 0 | dryer wrote the report. | Read `:candidates`. The gate passes only if no pair has a side whose `:start-line` to `:end-line` holds a changed line of its `:file`. |
| 1 | Usage error, or dryer did not write the snapshot. | Fix the command. Do not change the threshold. If the write failed, fix its cause, for example a file in the way of `.metrics`. |
| 2 | Unknown output format. | Remove the `--format` flag or use `text` or `edn`. |
| 70 | The worker thread did not start, or it panicked. | Read stderr. It names the reason. |
| other code | Git failed during `--changed`. | Read stderr. |

Exit code 0 does not mean that the gate passes. dryer exits with 0 even when it finds pairs.

## Act on a pair

Act on each pair that counts. A pair does not count when both of its forms lie on old lines, as above. For each counted pair, open the two forms at the lines in the report. Then do one of these:

1. Extract the shared shape into one function. Give the differences to it as arguments. Call it from both places.
2. If the two forms look alike but mean different things in the domain, change one of them so that its shape differs. Write the reason in the log.

Writing the reason alone does not pass the gate. The pair stays in `:candidates` until its score is below 0.8, so the next run reports it again. A counted pair stays counted while one of its forms holds a changed line. If you cannot lower the score of a counted pair within 5 tries, stop and report the pair and your reason (stop condition 1 of lagom-mode).

Run the tests after each change. Pin the behavior before you extract, as in the Refactoring rule of lagom-mode.
