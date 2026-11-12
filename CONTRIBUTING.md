# Contributing

Read `AGENTS.md` first. It states how to make a change, the commands to run, and the commit rules.

## Pull requests run code

A pull request can change more than source code. The tests, the parity runners, and the fixtures run shell commands. Coding agents read `AGENTS.md` and `skills/` as instructions. A change to one of these files can make your computer run code from the author of the pull request.

The maintainer follows these rules for each pull request:

1. Read the full diff on GitHub before you check out the branch.
2. Treat a change to these files as code: `AGENTS.md`, `skills/`, `*.toml`, `*.sh`, `*.py`, `build.rs`, and `Cargo.toml`.
3. Run the code of the pull request only in a Linux guest from `ci/linux-vm/`. Boot the guest with `VM_NET=restricted`, so that it cannot reach your network.
4. Do not start a coding agent on the branch until the diff is reviewed.
5. If you add GitHub Actions, use GitHub-hosted runners only. Require approval for workflows from forks.

## Report a security problem

Do not open a public issue for a security problem. Use "Report a vulnerability" on the Security tab of the GitHub repository.
