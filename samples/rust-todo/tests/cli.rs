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

/// Skills PRD section 9: `add` stores a task, and `list` shows it with the id that counts from 1.
#[test]
fn sample_add_then_list_shows_the_task() {
    let path = todo_file("add");
    let (code, out) = run(&path, &["add", "buy milk"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "Added 1: buy milk");
    let (code, out) = run(&path, &["list"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "1\tbuy milk\nOne task, id 1. All in order.\n");
    fs::remove_file(&path).ok();
}

/// Skills PRD section 9: `list` prints every task, then the summary sentence of the list.
#[test]
fn sample_list_prints_the_summary_line() {
    let path = todo_file("summary");
    run(&path, &["add", "walk dog"]);
    run(&path, &["add", "pay bill"]);
    let (code, out) = run(&path, &["list"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "1\twalk dog\n2\tpay bill\nA few tasks (2), ids 1 and 2. All in order.\n"
    );
    fs::remove_file(&path).ok();
}

/// Skills PRD section 9: the store is a plain text file with one tab separated task per line.
#[test]
fn sample_the_store_is_a_text_file() {
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

/// Skills PRD section 9: ids count from 1. A task keeps its id when the file is written again,
/// and when another task goes. The next id is one above the highest id in use.
#[test]
fn sample_ids_stay_after_a_rewrite_and_a_removal() {
    let path = todo_file("ids");
    run(&path, &["add", "walk dog"]);
    run(&path, &["add", "pay bill"]);
    run(&path, &["add", "buy milk"]);

    let text = fs::read_to_string(&path).expect("read the store");
    let rewritten: String = text
        .lines()
        .filter_map(Task::parse)
        .map(|task| format!("{}\n", task.format()))
        .collect();
    fs::write(&path, rewritten).expect("rewrite the store");

    let (_, out) = run(&path, &["list"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        &lines[..3],
        ["1\twalk dog", "2\tpay bill", "3\tbuy milk"],
        "{out}"
    );

    let kept: String = text
        .lines()
        .filter(|line| !line.starts_with("2\t"))
        .map(|line| format!("{line}\n"))
        .collect();
    fs::write(&path, kept).expect("remove the second task");

    let (_, out) = run(&path, &["list"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(&lines[..2], ["1\twalk dog", "3\tbuy milk"], "{out}");

    let (code, out) = run(&path, &["add", "pay bill"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "Added 4: pay bill");
    fs::remove_file(&path).ok();
}

/// Skills PRD section 9: an unknown command fails with a message and exit code 1.
#[test]
fn sample_unknown_command_fails() {
    let path = todo_file("unknown");
    let (code, out) = run(&path, &["frobnicate"]);
    assert_eq!(code, 1);
    assert_eq!(out, "unknown command: frobnicate\n");
    fs::remove_file(&path).ok();
}

/// Skills PRD section 9: a run without an argument prints the usage and exits 1.
#[test]
fn sample_no_argument_prints_the_usage() {
    let path = todo_file("usage");
    let (code, out) = run(&path, &[]);
    assert_eq!(code, 1);
    assert_eq!(out, "usage: rust-todo add <title> | list\n");
    fs::remove_file(&path).ok();
}

/// Skills PRD section 9: `add` without a title fails and writes no task.
#[test]
fn sample_add_without_a_title_fails() {
    let path = todo_file("no-title");
    let (code, out) = run(&path, &["add"]);
    assert_eq!(code, 1);
    assert_eq!(out, "add needs a title\n");
    assert!(!path.exists());
}
