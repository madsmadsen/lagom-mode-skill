# Skill checklist

This checklist is for AC-1 of `docs/prd/skills.md`. Mads Madsen inspects the skills in `skills/` and records pass or fail for each item. All items must pass.

| Requirement | Item | File | Result |
| --- | --- | --- | --- |
| SK-1 | Each skill is one `SKILL.md` file. Its YAML frontmatter has `name` and `description`. | `skills/*/SKILL.md` | |
| SK-3 | `feature-loop` names the skills `crapper`, `dryer`, and `mutator`, and says when to load each one. | `skills/feature-loop/SKILL.md` | |
| SK-4 | `feature-loop` has the phases "First Make it Work", "Then Make it Right", and "Practice". It keeps each rule of ub-mode. The only change is the end of the loop: it integrates into the source branch. | `skills/feature-loop/SKILL.md` | |
| SK-5 | `feature-loop` states the loop of section 6 as numbered steps, in that order. | `skills/feature-loop/SKILL.md` | |
| SK-20 | Each tool skill names the install document of its tool. It states how to run the tool on uncommitted changes and on a list of files. It states how to read the output. | `skills/crapper/SKILL.md`, `skills/dryer/SKILL.md`, `skills/mutator/SKILL.md` | |
| SK-21 | The crapper skill maps a high score to an action: split the function, add tests, or both. It states when a `match` or `case` is worth splitting. | `skills/crapper/SKILL.md` | |
| SK-22 | The dryer skill maps a candidate pair to an action: extract the shared shape, or record why the pair stays. | `skills/dryer/SKILL.md` | |
| SK-23 | The mutator skill maps each outcome to an action: cover an uncovered site, or kill a survivor. If no test can kill a survivor, the agent deletes the code. | `skills/mutator/SKILL.md` | |
| SK-24 | Each tool skill states the exit codes of its tool and what the agent does for each code. | `skills/crapper/SKILL.md`, `skills/dryer/SKILL.md`, `skills/mutator/SKILL.md` | |
| SK-25 | `feature-loop` lists every stop condition (SK-9, SK-12, SK-14, SK-15, SK-16) in one numbered list. | `skills/feature-loop/SKILL.md` | |
