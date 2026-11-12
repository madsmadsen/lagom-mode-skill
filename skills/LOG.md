# Log

## 2026-11-12

- Wrote `skills/feature-loop/SKILL.md`. Reason: SK-4, SK-5, SK-6, SK-7 to SK-19, SK-25. It keeps each ub-mode rule and adds the worktree loop, the gates, and the stop list.
- Copied the ub-mode `LICENSE` to `skills/feature-loop/LICENSE`. Reason: SK-NFR-2.
- Wrote `skills/crapper/SKILL.md`, `skills/dryer/SKILL.md`, and `skills/mutator/SKILL.md`. Reason: SK-3, SK-20 to SK-24.
- Wrote `docs/skill-checklist.md`. Reason: AC-1 needs one item for each of its requirements.
- Added "Leave out files that no longer exist" to the feature files definition. Reason: `git diff --name-only` lists deleted files, and the tools cannot read them.
- Renamed the main skill from `feature-loop` to `lagom-mode`, with the branch prefix `lagom-mode/<short-feature-name>`. Reason: Mads Madsen chose the name. Changed the skill folder, the frontmatter, the three tool skills, `docs/prd/skills.md`, `docs/PRD.md`, and `docs/skill-checklist.md`. Left `docs/.review/` unchanged, because it records past review rounds.
