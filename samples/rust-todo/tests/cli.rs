//! The command line, run as a user runs it: the binary reads `TODO_FILE` and the arguments.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rust_todo::task::Task;

fn todo_file(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "rust-todo-cli-{}-{nanos}-{name}.txt",
        std::process::id()
    ));
    fs::remove_file(&path).ok();
    path
}

fn run(path: &PathBuf, args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-todo"))
        .args(args)
        .env("TODO_FILE", path)
        .output()
        .expect("run the binary");
    let mut text = String::from_utf8(output.stdout).expect("utf8");
    text.push_str(&String::from_utf8(output.stderr).expect("utf8"));
    (output.status.code().unwrap_or(-1), text)
}

/// SK-8: `add` stores a task, and `list` shows it with the id that counts from 1.
#[test]
fn sk_8_add_then_list_shows_the_task() {
    let path = todo_file("add");
    let (code, out) = run(&path, &["add", "buy milk"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1"), "{out}");
    let (code, out) = run(&path, &["list"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1\tbuy milk"), "{out}");
    fs::remove_file(&path).ok();
}

/// SK-8: the store is a plain text file with one tab separated task per line.
#[test]
fn sk_8_the_store_is_a_text_file() {
    let path = todo_file("file");
    run(&path, &["add", "walk dog"]);
    run(&path, &["add", "pay bill"]);
    let text = fs::read_to_string(&path).expect("read the store");
    let tasks: Vec<Task> = text.lines().filter_map(Task::parse).collect();
    assert_eq!(
        tasks,
        vec![Task::new(1, "walk dog"), Task::new(2, "pay bill")]
    );
    fs::remove_file(&path).ok();
}

/// SK-8: an unknown command fails with a message and exit code 1.
#[test]
fn sk_8_unknown_command_fails() {
    let path = todo_file("unknown");
    let (code, out) = run(&path, &["frobnicate"]);
    assert_eq!(code, 1);
    assert!(out.contains("unknown command: frobnicate"), "{out}");
    fs::remove_file(&path).ok();
}

/// SK-8: `add` without a title fails and writes no task.
#[test]
fn sk_8_add_without_a_title_fails() {
    let path = todo_file("no-title");
    let (code, out) = run(&path, &["add"]);
    assert_eq!(code, 1);
    assert!(out.contains("add needs a title"), "{out}");
    assert!(!path.exists());
}
