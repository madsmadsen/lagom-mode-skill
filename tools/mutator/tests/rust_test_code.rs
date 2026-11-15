// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! End-to-end check of MU-26 (AC-18): a scan of a Rust file lists no site in test code. The test
//! fails on the original, which leaves out only a `mod` named `tests`.

mod common;

use common::{project, run, write};

const LIB_RS: &str = "\
pub fn real(a: i32) -> bool {
    a > 1
}

#[cfg(test)]
mod unit_tests {
    #[test]
    fn in_cfg_module() {
        let _ = 3 > 2;
    }
}

mod plain {
    #[test]
    fn plain_test() {
        let _ = 3 > 2;
    }
}

#[cfg(test)]
fn helper() -> bool {
    3 > 2
}

#[cfg(not(test))]
mod prod_only {
    pub fn kept() -> bool {
        3 > 2
    }
}

mod tests {
    fn hidden() -> bool {
        3 > 2
    }
}

pub fn after() -> bool {
    4 > 3
}
";

/// The distinct function names in the `[defn/<name>]` and `[defn-/<name>]` tags of a scan.
fn scanned_functions(stdout: &str) -> Vec<&str> {
    let mut names: Vec<&str> = stdout
        .lines()
        .filter_map(|line| line.rsplit_once('[')?.1.strip_suffix(']')?.split_once('/'))
        .map(|(_, name)| name)
        .collect();
    names.dedup();
    names
}

// AC-18 (MU-26): only `real`, `kept`, and `after` are production code. The functions in the
// `#[cfg(test)]` module, the `#[test]` fn, the `#[cfg(test)]` fn, and `mod tests` have no sites.
#[test]
fn mu_26_ac18_scan_lists_sites_for_production_rust_only() {
    let dir = project();
    write(dir.path(), "src/lib.rs", LIB_RS);
    let out = run(dir.path(), &["--scan", "--no-coverage", "src/lib.rs"]);
    assert_eq!(out.code, 0, "stderr:\n{}", out.stderr);
    assert_eq!(
        scanned_functions(&out.stdout),
        ["real", "kept", "after"],
        "stdout:\n{}",
        out.stdout
    );
}
