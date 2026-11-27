# Skill checklist

This checklist is for AC-1 of `docs/prd/skills.md`. Mads Madsen inspects the skills in `skills/` and records pass or fail for each item. All items must pass.

| Requirement | Item | File | Result |
| --- | --- | --- | --- |
| SK-1 | Each skill is one `SKILL.md` file. Its YAML frontmatter has `name` and `description`. | `skills/*/SKILL.md` | |
| SK-3 | `lagom-mode` names the skills `crapper`, `dryer`, and `mutator`, and says when to load each one. | `skills/lagom-mode/SKILL.md` | |
| SK-4 | `lagom-mode` has the phases "First Make it Work", "Then Make it Right", and "Practice". It keeps each rule of ub-mode. Only two rules change: the end of the loop integrates into the source branch, and each function must have a CRAP score of 10 or lower. | `skills/lagom-mode/SKILL.md` | |
| SK-5 | `lagom-mode` states the loop of section 6 as numbered steps, in that order. | `skills/lagom-mode/SKILL.md` | |
| SK-20 | Each tool skill names the install document of its tool. It states how to run the tool on uncommitted changes and on a list of files. It states how to read the output. | `skills/crapper/SKILL.md`, `skills/dryer/SKILL.md`, `skills/mutator/SKILL.md` | |
| SK-21 | The crapper skill maps a high score to an action: split the function, add tests, or both. It states when a `match` or `case` is worth splitting. | `skills/crapper/SKILL.md` | |
| SK-22 | The dryer skill maps a candidate pair to an action: the agent extracts the shared shape. A written reason alone does not pass the gate. A pair counts only when the `:start-line` to `:end-line` range of one side holds a changed line of that side's `:file`, and a pair of two old forms does not count. If a counted pair stays at 0.8 or more, the gate fails and the retry limit applies. | `skills/dryer/SKILL.md` | |
| SK-23 | The mutator skill maps each outcome to an action: cover an uncovered site, or kill a survivor. If no test can kill a survivor, the agent deletes the code. | `skills/mutator/SKILL.md` | |
| SK-24 | Each tool skill states the exit codes of its tool and what the agent does for each code. | `skills/crapper/SKILL.md`, `skills/dryer/SKILL.md`, `skills/mutator/SKILL.md` | |
| SK-25 | `lagom-mode` lists every stop condition in one numbered list: the retry limit of SK-9, the detached HEAD of SK-12, the failed fast-forward and the return limit of SK-14, the rebase conflict of SK-15, and the rejected push of SK-16. A stop report names its stop condition. | `skills/lagom-mode/SKILL.md` | |
| SK-26 | Each tool skill names the languages that its tool supports, and it says that JavaScript files (`.js`, `.jsx`, `.mjs`, `.cjs`) are read as TypeScript. | `skills/crapper/SKILL.md`, `skills/dryer/SKILL.md`, `skills/mutator/SKILL.md` | |
