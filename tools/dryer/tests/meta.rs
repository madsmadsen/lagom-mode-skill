// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Offline checks over the repository files: the CI scripts (DR-NFR-3), the
//! install command (DR-NFR-4), and the license and file headers (DR-NFR-5).
//!
//! Every check reads files that the repository holds. None of them runs a
//! binary, starts a guest, or needs a network. Each check returns the list of
//! problems it found, so a test is one assertion, and its failure names every
//! problem at once.

use std::fs;
use std::path::PathBuf;

const COPYRIGHT: &str = "// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>";
const SPDX: &str = "// SPDX-License-Identifier: MIT";
const INSTALL: &str =
    "cargo install --locked --git https://github.com/madsmadsen/lagom-mode-skill dryer";

/// The images and the architecture rule of the two Linux guest targets.
const VM_NEEDLES: &[&str] = &[
    "jammy-server-cloudimg-arm64.img",
    "jammy-server-cloudimg-amd64.img",
    "ARCH must be arm64 or x86_64",
];

/// The steps that prove each guest target builds, tests, and lints the crate.
const GUEST_NEEDLES: &[&str] = &[
    "cargo build --release",
    "cargo test",
    "cargo clippy --all-targets",
];

/// The license must name the port author, the original author, and the license of the
/// original.
const LICENSE_NEEDLES: &[&str] = &[
    "Copyright (c) 2026 Mads Madsen <mads@onmadsen.com>",
    "based on dryer by Robert C. Martin",
    "BSD Zero Clause License",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// The needles that `text` lacks.
fn missing(text: &str, needles: &[&str]) -> Vec<String> {
    needles
        .iter()
        .filter(|needle| !text.contains(**needle))
        .map(|needle| (*needle).to_string())
        .collect()
}

/// The `.rs` files directly in `dir`, sorted. `src/` and `tests/` have no subdirectory, so
/// a walk would only add a way for the check to find nothing.
fn rs_files(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(root().join(dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();
    files
}

/// A file starts with the two lines of DR-NFR-5 when its first two lines are exactly the
/// copyright line and the SPDX line.
fn starts_with_header(text: &str) -> bool {
    text.lines().take(2).eq([COPYRIGHT, SPDX])
}

/// The paths of the files that do not start with the header.
fn files_without_header(files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .filter(|path| !starts_with_header(&fs::read_to_string(path).unwrap_or_default()))
        .map(|path| {
            path.strip_prefix(root())
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect()
}

/// The CI problems: a target image or a guest step that a script lacks. A check that finds
/// fewer than two `.rs` files is a problem too, because it would pass for the wrong reason.
fn ci_problems() -> Vec<String> {
    let mut problems = missing(&read("../../ci/linux-vm/vm.sh"), VM_NEEDLES);
    problems.extend(missing(
        &read("../../ci/linux-vm/guest-test.sh"),
        GUEST_NEEDLES,
    ));
    problems
}

/// The install problems: the README lacks the locked install command of DR-NFR-4.
fn install_problems() -> Vec<String> {
    missing(&read("README.md"), &[INSTALL])
}

/// The license problems: the MIT title, the two authors, and the license of the original.
fn license_problems() -> Vec<String> {
    let license = read("LICENSE");
    let mut problems = missing(&license, LICENSE_NEEDLES);
    problems.extend((!license.starts_with("MIT License")).then(|| "MIT License title".to_string()));
    problems
}

/// The header problems: each `.rs` file under `src/` or `tests/` must start with the two
/// lines of DR-NFR-5. The fixture projects under `parity/fixtures/` are test data, and
/// DR-NFR-5 leaves them out.
fn header_problems() -> Vec<String> {
    let files: Vec<PathBuf> = ["src", "tests"]
        .iter()
        .flat_map(|dir| rs_files(dir))
        .collect();
    let mut problems = files_without_header(&files);
    problems.extend((files.len() < 2).then(|| format!("found only {} .rs files", files.len())));
    problems
}

#[test]
// AC-14, DR-NFR-3: the Linux guests cover both architectures, and each one builds and tests
fn ac_14_dr_nfr_3_the_linux_vm_scripts_cover_both_architectures() {
    assert_eq!(ci_problems(), Vec::<String>::new());
}

#[test]
// AC-15, DR-NFR-4: the README gives the one install command of DR-NFR-4
fn ac_15_dr_nfr_4_the_readme_has_the_locked_install_command() {
    assert_eq!(install_problems(), Vec::<String>::new());
}

#[test]
// AC-16, DR-NFR-5: the license names the port author and credits the original author
fn ac_16_dr_nfr_5_the_license_names_both_authors() {
    assert_eq!(license_problems(), Vec::<String>::new());
}

#[test]
// AC-16, DR-NFR-5: every .rs file under src/ and tests/ starts with the two header lines
fn ac_16_dr_nfr_5_every_source_file_starts_with_the_header() {
    assert_eq!(header_problems(), Vec::<String>::new());
}

#[test]
// AC-16, DR-NFR-5: the header check is not vacuous. A file without the two lines is a
// problem, a file with the lines is not, and a single blank first line is a problem.
fn ac_16_dr_nfr_5_a_file_without_the_two_lines_is_reported() {
    assert!(starts_with_header(&format!(
        "{COPYRIGHT}\n{SPDX}\nuse std::fs;\n"
    )));
    assert!(!starts_with_header(&format!("{SPDX}\n{COPYRIGHT}\n")));
    assert!(!starts_with_header(&format!("\n{COPYRIGHT}\n{SPDX}\n")));
    assert!(!starts_with_header("fn main() {}\n"));
    assert!(!starts_with_header(""));
}
