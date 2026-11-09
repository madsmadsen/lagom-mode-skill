//! AC-2, AC-11, AC-12 (base); MU-5, MU-8, MU-9, MU-12.
//!
//! The golden files in `tests/golden/<lang>/` come from the ORIGINAL Python mutator, run on each
//! fixture in `parity/fixtures/<lang>` by `parity/golden.py`. This test needs no Python and no
//! crapper checkout: it runs the port and compares its stdout, exit code, and snapshots with them.

mod common;

use common::{copy_fixture, run, snapshots};
use mutator::edn;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn golden_dir(lang: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(lang)
}

fn golden_text(lang: &str, file: &str) -> String {
    fs::read_to_string(golden_dir(lang).join(file)).expect("golden file")
}

/// Relative path (with `/`) -> text, for every file under `dir`.
fn read_tree(dir: &Path) -> BTreeMap<String, String> {
    fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                collect(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).expect("under root");
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(rel, fs::read_to_string(&path).expect("read file"));
            }
        }
    }
    let mut out = BTreeMap::new();
    collect(dir, dir, &mut out);
    out
}

/// Replaces the fixture's temp dir (as given and canonicalized) with `<ROOT>`.
fn normalize(text: &str, dir: &Path) -> String {
    let canonical = dir.canonicalize().expect("canonical fixture dir");
    let canonical = canonical.to_str().expect("utf-8 path");
    let given = dir.to_str().expect("utf-8 path");
    text.replace(canonical, "<ROOT>").replace(given, "<ROOT>")
}

fn check_fixture(lang: &str) {
    let dir = copy_fixture(lang);
    let root = dir.path();

    let scan = run(root, &["--scan", "--no-coverage"]);
    assert_eq!(
        normalize(&scan.stdout, root),
        golden_text(lang, "scan.out"),
        "scan output"
    );

    let mutate = run(
        root,
        &[
            "--no-coverage",
            "--test-command",
            "sh check.sh",
            "--max-workers",
            "2",
        ],
    );
    assert_eq!(
        normalize(&mutate.stdout, root),
        golden_text(lang, "run.out"),
        "run output"
    );
    let want_code: i32 = golden_text(lang, "run.exit")
        .trim()
        .parse()
        .expect("golden exit code");
    assert_eq!(mutate.code, want_code, "exit code");

    let want = read_tree(&golden_dir(lang).join("snapshots"));
    let got = snapshots(root);
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        want.keys().collect::<Vec<_>>(),
        "snapshot files"
    );
    // `snapshots` already drops the `:tested-at` pair from the port's side.
    for (rel, text) in &want {
        let want_value = edn::parse(text).expect("golden snapshot parses");
        let got_value = edn::parse(&got[rel]).expect("port snapshot parses");
        assert_eq!(got_value, want_value, "snapshot {rel}");
    }
}

// MU-5, MU-8, MU-9, MU-12; AC-2, AC-11, AC-12 (base).
#[test]
fn mu_5_fixture_clojure_matches_the_original() {
    check_fixture("clojure");
}

#[test]
fn mu_5_fixture_java_matches_the_original() {
    check_fixture("java");
}

#[test]
fn mu_5_fixture_go_matches_the_original() {
    check_fixture("go");
}

#[test]
fn mu_5_fixture_typescript_matches_the_original() {
    check_fixture("typescript");
}

#[test]
fn mu_5_fixture_rust_matches_the_original() {
    check_fixture("rust");
}

#[test]
fn mu_5_fixture_python_matches_the_original() {
    check_fixture("python");
}
