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
use std::path::{Path, PathBuf};

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

/// The default crate list of the guest test script (AC-14).
const GUEST_DEFAULT_CRATES: &[&str] = &["crapper", "dryer", "mutator"];

/// The license must name the port author, the original author, and the license of the
/// original.
const LICENSE_NEEDLES: &[&str] = &[
    "Copyright (c) 2026 Mads Madsen <mads@onmadsen.com>",
    "based on dryer by Robert C. Martin",
    "BSD Zero Clause License",
    "Permission is hereby granted",
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

/// The default crate list of `guest-test.sh`: the words inside the `crates=(...)` that the
/// script uses when the caller names no crate. The script has two such lists, and the
/// default one follows `||`.
fn guest_default_crates(script: &str) -> Vec<String> {
    script
        .lines()
        .find(|line| line.contains("|| crates=("))
        .and_then(|line| line.split_once("|| crates=(").map(|(_, rest)| rest))
        .and_then(|rest| rest.split_once(')').map(|(inner, _)| inner))
        .map(|inner| inner.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

/// The names of `wanted` that `have` lacks, each as one line of text.
fn missing_names(have: &[String], wanted: &[&str]) -> Vec<String> {
    wanted
        .iter()
        .filter(|name| !have.iter().any(|got| got == *name))
        .map(|name| format!("guest-test.sh default crates lack {name}"))
        .collect()
}

/// The CI problems: a target image, a guest step, or a default crate that a script lacks.
fn ci_problems() -> Vec<String> {
    let mut problems = missing(&read("../../ci/linux-vm/vm.sh"), VM_NEEDLES);
    let guest = read("../../ci/linux-vm/guest-test.sh");
    problems.extend(missing(&guest, GUEST_NEEDLES));
    problems.extend(missing_names(
        &guest_default_crates(&guest),
        GUEST_DEFAULT_CRATES,
    ));
    problems
}

/// The install problems of one README text: it needs the command as a whole line of its
/// own, and the command may appear once.
fn install_text_problems(readme: &str) -> Vec<String> {
    let mut problems: Vec<String> = (!readme.lines().any(|line| line.trim() == INSTALL))
        .then(|| format!("README.md has no line {INSTALL}"))
        .into_iter()
        .collect();
    if readme.matches(INSTALL).count() > 1 {
        problems.push("README.md names the install command more than once".to_string());
    }
    problems
}

/// The install problems: the README lacks the locked install command of DR-NFR-4 as a
/// whole line of its own.
fn install_problems() -> Vec<String> {
    install_text_problems(&read("README.md"))
}

/// The license problems: the MIT title, the MIT permission sentence, the two authors, and
/// the license of the original.
fn license_problems() -> Vec<String> {
    let license = read("LICENSE");
    let mut problems = missing(&license, LICENSE_NEEDLES);
    problems.extend((!license.starts_with("MIT License")).then(|| "MIT License title".to_string()));
    problems.extend(
        (!license.contains("copies of the Software")).then(|| "MIT license body".to_string()),
    );
    problems
}

/// The header problems: each `.rs` file under `src/` or `tests/` must start with the two
/// lines of DR-NFR-5. The fixture projects under `parity/fixtures/` are test data, and
/// DR-NFR-5 leaves them out. A check that finds fewer than two `.rs` files is a problem
/// too, because it would pass for the wrong reason.
fn header_problems() -> Vec<String> {
    let files: Vec<PathBuf> = ["src", "tests"]
        .iter()
        .flat_map(|dir| rs_files(dir))
        .collect();
    let mut problems = files_without_header(&files);
    problems.extend((files.len() < 2).then(|| format!("found only {} .rs files", files.len())));
    problems
}

/// The needles of a named original file: its directory and its extension.
const ORIGINAL_NEEDLES: &[&str] = &["src/dryer/", ".py"];

/// True when `line` names one `src/dryer/<name>.py` path. A wildcard names no file.
fn names_one_original(line: &str) -> bool {
    ORIGINAL_NEEDLES.iter().all(|needle| line.contains(needle)) && !line.contains('*')
}

/// The third line of one file, or "" when the file is shorter or cannot be read.
fn third_line(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .nth(2)
        .unwrap_or_default()
        .to_string()
}

/// True for a `src/` file that matches a file of the original. `src/lib.rs` is the crate
/// root, and the original is a package with no root module, so it matches no file.
fn matches_original(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name != "lib.rs")
}

/// The original-file problems: DR-NFR-5 says that each file which matches a file of the
/// original names that file. The check cannot open the original, because a CI run has no
/// checkout of it, so it requires the third line of each `src/` file to name one
/// `src/dryer/<name>.py` path, with no wildcard.
fn original_file_problems() -> Vec<String> {
    rs_files("src")
        .iter()
        .filter(|path| matches_original(path))
        .filter(|path| !names_one_original(&third_line(path)))
        .map(|path| {
            format!(
                "third line names no single .py path of the original: {}",
                path.display()
            )
        })
        .collect()
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

#[test]
// AC-16, DR-NFR-5: each src/ file that matches a file of the original names a .py path
// on its third line. The check cannot open the original, so it asks for the path shape.
fn ac_16_dr_nfr_5_each_source_file_names_its_original() {
    assert_eq!(original_file_problems(), Vec::<String>::new());
}

#[test]
// AC-14, DR-NFR-3: the default crate list of guest-test.sh is read from the script and
// not from a substring, so a script that dropped dryer is a problem.
fn ac_14_dr_nfr_3_the_guest_default_crates_hold_dryer() {
    let line = "crates=(\"$@\"); [ ${#crates[@]} -gt 0 ] || crates=(crapper dryer mutator)\n";
    assert_eq!(
        guest_default_crates(line),
        vec!["crapper", "dryer", "mutator"]
    );
    let dropped = "crates=(\"$@\"); [ ${#crates[@]} -gt 0 ] || crates=(crapper mutator)\n";
    assert!(!guest_default_crates(dropped).iter().any(|c| c == "dryer"));
    assert!(guest_default_crates("no crate line\n").is_empty());
}

#[test]
// AC-15, DR-NFR-4: the install command must be a whole line of the README, so a line that
// only mentions it in passing does not count.
fn ac_15_dr_nfr_4_the_install_command_needs_a_line_of_its_own() {
    assert_eq!(install_problems(), Vec::<String>::new());
    assert_eq!(
        install_text_problems(&format!("```sh\n{INSTALL}\n```\n")),
        Vec::<String>::new()
    );
    assert_eq!(
        install_text_problems(&format!("Run {INSTALL} on a clean host.\n")).len(),
        1
    );
    assert_eq!(
        install_text_problems(&format!("{INSTALL}\n{INSTALL}\n")).len(),
        1
    );
}
