//! Shared helpers for the end-to-end tests. Every test runs the built `mutator` binary.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mutator"))
}

pub fn run(dir: &Path, args: &[&str]) -> Out {
    run_env(dir, args, &[])
}

pub fn run_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Out {
    let output = Command::new(bin())
        .args(args)
        .envs(env.iter().copied())
        .current_dir(dir)
        .output()
        .expect("spawn mutator");
    Out {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-c")
        .arg("user.name=mutator-test")
        .arg("-c")
        .arg("user.email=mutator-test@example.com")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// An empty project directory that is a git repository.
pub fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    git(dir.path(), &["init", "-q"]);
    dir
}

pub fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create dirs");
    fs::write(path, text).expect("write file");
}

pub fn git_commit_all(dir: &Path) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "snapshot"]);
}

pub fn snapshot_text(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(".metrics/mutate").join(rel)).expect("read snapshot")
}

/// Every snapshot under `.metrics/mutate`, keyed by relative path, with `:tested-at` removed.
pub fn snapshots(dir: &Path) -> BTreeMap<String, String> {
    let base = dir.join(".metrics/mutate");
    let mut found = BTreeMap::new();
    collect_snapshots(&base, &base, &mut found);
    found
}

fn collect_snapshots(base: &Path, current: &Path, found: &mut BTreeMap<String, String>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_snapshots(base, &path, found);
        } else {
            let key = path
                .strip_prefix(base)
                .expect("under base")
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(&path).expect("read snapshot");
            found.insert(key, strip_tested_at(&text));
        }
    }
}

fn strip_tested_at(text: &str) -> String {
    const KEY: &str = ":tested-at \"";
    let mut out = text.to_string();
    while let Some(start) = out.find(KEY) {
        let value_start = start + KEY.len();
        let Some(close) = out[value_start..].find('"') else {
            break;
        };
        let mut end = value_start + close + 1;
        if out[end..].starts_with(' ') {
            end += 1;
        }
        out.replace_range(start..end, "");
    }
    out
}

/// A copy of `mutator/parity/fixtures/<lang>` in a fresh git repository with one commit.
pub fn copy_fixture(lang: &str) -> tempfile::TempDir {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("parity/fixtures")
        .join(lang);
    let dir = project();
    copy_tree(&from, dir.path());
    git_commit_all(dir.path());
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dir");
    for entry in fs::read_dir(from).expect("read fixture dir") {
        let entry = entry.expect("dir entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}
