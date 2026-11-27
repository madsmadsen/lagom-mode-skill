// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
mod common;

use common::{project, run, write};
use std::fs;

const SRC: &str = "def f(a):\n    return a > 1 and a < 9\n";

/// MU-30 / AC-24: a worker holds a real copy of each JavaScript or TypeScript file of the project
/// root. A test command that fails when `vite.config.ts` is a link passes in every worker, so every
/// mutant survives. The run exits 3 and prints no baseline failure.
#[test]
fn mu_30_ac_24_a_worker_holds_real_copies_of_root_config_modules() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    write(dir.path(), "vite.config.ts", "export default {}\n");
    write(dir.path(), "notes.txt", "notes\n");
    let scratch = tempfile::tempdir().expect("temp dir");
    let script = scratch.path().join("config.sh");
    fs::write(
        &script,
        "#!/bin/sh\ncase \"$(basename \"$PWD\")\" in worker-*) if test -L vite.config.ts; then exit 1; fi ;; esac\nexit 0\n",
    )
    .expect("write script");
    let cmd = format!("sh {}", script.display());
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--max-workers",
            "2",
            "--test-command",
            &cmd,
        ],
    );
    assert_eq!(
        out.code, 3,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        !out.stderr.contains("Baseline failed"),
        "stderr: {}",
        out.stderr
    );
}

/// MU-30 / AC-25: a worker holds real copies of the files that a configuration file imports through
/// relative imports, directly or through other files. A helper in a linked directory builds its
/// paths from its own location, so the test command fails in a worker where that directory is a
/// link.
#[test]
fn mu_30_ac_25_a_worker_holds_real_copies_of_the_files_that_a_config_imports() {
    let dir = project();
    write(dir.path(), "src/m.py", SRC);
    write(
        dir.path(),
        "vite.config.ts",
        "import { entries } from './scripts/entries.ts'\nexport default {}\n",
    );
    write(
        dir.path(),
        "scripts/entries.ts",
        "import { base } from '../shared/paths.ts'\nexport const entries = base\n",
    );
    write(dir.path(), "shared/paths.ts", "export const base = 1\n");
    write(dir.path(), "scripts/unused.ts", "export const unused = 2\n");
    let scratch = tempfile::tempdir().expect("temp dir");
    let script = scratch.path().join("config.sh");
    fs::write(
        &script,
        "#!/bin/sh\n\
         case \"$(basename \"$PWD\")\" in\n\
         worker-*)\n\
         here=$(pwd -P)\n\
         if [ \"$(cd scripts && pwd -P)\" != \"$here/scripts\" ]; then exit 1; fi\n\
         if [ \"$(cd shared && pwd -P)\" != \"$here/shared\" ]; then exit 1; fi\n\
         ;;\n\
         esac\n\
         exit 0\n",
    )
    .expect("write script");
    let cmd = format!("sh {}", script.display());
    let out = run(
        dir.path(),
        &[
            "--no-coverage",
            "--max-workers",
            "2",
            "--test-command",
            &cmd,
        ],
    );
    assert_eq!(
        out.code, 3,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        !out.stderr.contains("Baseline failed"),
        "stderr: {}",
        out.stderr
    );
}
