# AGENTS.md

This repository holds Rust ports of three tools by Robert C. Martin, and the requirements for agent skills that use them. Read this file before you change anything.

## Layout

| Path | Content |
| --- | --- |
| `crapper/` | Rust port of [crapper](https://github.com/unclebob/crapper). It gives each function a CRAP score. |
| `dryer/` | Rust port of [dryer](https://github.com/unclebob/dryer). It finds duplicated code structure. |
| `mutator/` | Rust port of [mutator](https://github.com/unclebob/mutator). It runs mutation tests. |
| `docs/prd/` | One requirements document (PRD) for each crate, and `skills.md` for the agent skills. |
| `ci/linux-vm/` | Scripts that build and test the ports in Ubuntu 22.04 guests (arm64 and x86_64) with qemu. |

Each crate is a separate Cargo project. There is no Cargo workspace. A crate must not depend on another crate or tool in this repository.

## Requirements

1. The PRD of a crate is `docs/prd/<crate>.md`. Each requirement has an ID: `CR-*` for crapper, `DR-*` for dryer, `MU-*` for mutator, `SK-*` for the skills. Each acceptance criterion is `AC-<n>` and names its requirement IDs.
2. Name the requirement ID in each test and in each commit message.
3. Write the PRDs in plain English: short sentences, active voice, no contractions, no semicolons, no em-dashes. A PRD has no open assumptions. If you cannot answer an assumption from a source, ask Mads Madsen, or delete it.
4. Mads Madsen is the owner and approver of each PRD.

## Parity with the original

1. Each port copies the behavior of the original at the commit in `<crate>/parity/upstream.toml`. If the port and the original differ, the original is correct, unless `<crate>/parity/fixes.toml` lists the difference.
2. Each difference from the original needs a requirement in the PRD, an entry in `fixes.toml`, and a test that fails on the behavior of the original. A difference that comes from an upstream issue or pull request goes in the "Upstream fixes" section of the PRD and links its source.
3. Do not approve a `fixes.toml` entry. Leave it in the unapproved state of that file. Mads Madsen approves each entry.
4. Each crate has its own parity runner. Read the README or the header of the runner before you use it:
   - crapper: `python3.12 crapper/parity/run.py`
   - dryer: `dryer/parity/run_parity.py`
   - mutator: `mutator/parity/run.py`, with the Python of the venv of the original (`mutator/parity/README.md`).
5. Each source file keeps the license header style of its crate. For crapper, CR-NFR-5 gives the exact header.

## How to make a change

1. Follow ub-mode: make it work, then make it right, then follow the practice rules.
2. Write the failing test first. Run it and record why it fails. Then make the smallest change that makes it pass.
3. For implementation work, give each small unit to the `implementer` agent with a complete brief. That agent does not build or test. The agent that gives the brief runs every build, test, and gate.
4. Run these commands in the crate directory before each commit:

   ```bash
   cargo build --release
   cargo test
   cargo clippy --all-targets
   cargo fmt
   ```

5. Run the quality gates on the changed files. Each function must have a CRAP score below 10 (`crapper --threshold 9.99`). No pair of forms can have a dryer score of 0.8 or more (`dryer --threshold 0.8`). The mutation run must have no survivors and no uncovered sites.
6. Add each action and its reason to `<crate>/LOG.md`.

## Load on the host

Parallel builds and mutation runs can overload the host. Obey these limits:

1. Run at most one cargo command at a time for each crate.
2. Run `mutator` with `--max-workers 2`.
3. Run mutation and parity work on scratch copies under `/tmp`, not in the repository.
4. Before you finish, stop each test binary and each tool process that you started.

## Commits

1. Make small commits. Each commit message says why, what changed, and how you proved it.
2. Start the subject with the crate name, for example `crapper: ...`, or with `docs:` for a change to the PRDs only.
3. Commit only the files that you changed: `git commit -- <paths>`. Other agents can work in this repository at the same time.
