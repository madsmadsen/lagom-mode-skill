# Log

## 2026-11-12

- Wrote `skills/feature-loop/SKILL.md`. Reason: SK-4, SK-5, SK-6, SK-7 to SK-19, SK-25. It keeps each ub-mode rule and adds the worktree loop, the gates, and the stop list.
- Copied the ub-mode `LICENSE` to `skills/feature-loop/LICENSE`. Reason: SK-NFR-2.
- Wrote `skills/crapper/SKILL.md`, `skills/dryer/SKILL.md`, and `skills/mutator/SKILL.md`. Reason: SK-3, SK-20 to SK-24.
- Wrote `docs/skill-checklist.md`. Reason: AC-1 needs one item for each of its requirements.
- Added "Leave out files that no longer exist" to the feature files definition. Reason: `git diff --name-only` lists deleted files, and the tools cannot read them.
- Renamed the main skill from `feature-loop` to `lagom-mode`, with the branch prefix `lagom-mode/<short-feature-name>`. Reason: Mads Madsen chose the name. Changed the skill folder, the frontmatter, the three tool skills, `docs/prd/skills.md`, `docs/PRD.md`, and `docs/skill-checklist.md`. Left `docs/.review/` unchanged, because it records past review rounds.
- Added an MIT `LICENSE` to each skill folder, with Mads Madsen as copyright holder and an acknowledgment of Robert C. Martin. `skills/lagom-mode/LICENSE` keeps the ub-mode MIT notice under "Third-party notice". Reason: Mads Madsen asked for MIT everywhere. The MIT License of ub-mode requires its notice in each copy, so the notice stays (SK-NFR-2, updated).
- Removed the repository name from the install line of the tool skills. Reason: AC-2 of `docs/PRD.md` forbids a repository name under `skills/`.
- Added a root `.gitignore`, untracked `tools/crapper/parity/__pycache__/`, hardened `ci/linux-vm/vm.sh` (private `$TMPDIR` work folder, image checksum check, `VM_NET=restricted`), added `CONTRIBUTING.md` with the pull request rules, and removed the "read access" line from `tools/mutator/README.md`. Reason: the pre-publish privacy review (F2, F5, F6, F7). The history rewrite then replaced the Gitea URL, the home path, and two local setup sentences in every commit, removed every `.pyc` file, and signed each commit again (F1, F3, F8).
- Changed the license of this repository from 0BSD to MIT in every commit with `git filter-repo`: each `LICENSE` file, each SPDX header, each `Cargo.toml` `license` field, the crapper license test, and the license sentences in the PRDs, READMEs, and logs. Statements that the originals by Robert C. Martin use 0BSD stay. Reason: Mads Madsen chose MIT for the public release.
