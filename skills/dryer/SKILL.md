---
name: dryer
description: >
  Find code that repeats in a similar shape and remove the duplication. Use
  inside lagom-mode at the unit gates and the feature gates, or when the
  user asks for duplicate code.
---

# dryer

dryer compares each function or method with the others in the same language. It gives each pair a similarity score from 0 to 1. The gate limit is 0.8. A pair with a score of 0.8 or more fails.

## Install

The install instructions are in `tools/dryer/README.md` in the `deterministic-coding` repository. Run `dryer --help` to make sure that the command is on the `PATH`.

## Run

Always give the threshold. The default of dryer is 0.82, and the gate uses 0.8.

On uncommitted changes (the unit gates):

```bash
dryer --changed --threshold 0.8
```

On a list of files (the feature gates):

```bash
dryer --threshold 0.8 <feature-files>
```

Test paths are included only when you name them.

## Read the output

dryer prints the pairs that score at or above the threshold. It also writes `.metrics/dry.edn`. The key `:candidates` holds the pairs. Each pair has these keys:

* `:left` and `:right`: the two forms. Each has `:file`, `:start-line`, and `:end-line`.
* `:score`: the similarity score.

If `:candidates` is empty, the dryer gate passes.

## Exit codes

| Code | Meaning | What you do |
| --- | --- | --- |
| 0 | dryer wrote the report. | Read `:candidates`. The gate passes only if it is empty. |
| 1 | Usage error. | Fix the command. Do not change the threshold. |
| 2 | Unknown output format. | Remove the `--format` flag or use `text` or `edn`. |

Exit code 0 does not mean that the gate passes. dryer exits with 0 even when it finds pairs.

## Act on a pair

For each pair, open the two forms at the lines in the report. Then do one of these:

1. Extract the shared shape into one function. Give the differences to it as arguments. Call it from both places.
2. If the two forms look alike but mean different things in the domain, change one of them so that its shape differs. Write the reason in the log.

Writing the reason alone does not pass the gate. The pair stays in `:candidates` until its score is below 0.8. If you cannot lower the score within 5 tries, stop and report the pair and your reason (stop condition 1 of lagom-mode).

Run the tests after each change. Pin the behavior before you extract, as in the Refactoring rule of lagom-mode.
