//! The to-do list: parse the arguments, and read and write the store.

pub mod store;
pub mod summary;
pub mod task;

use std::path::Path;

use task::Task;

/// Run one command and return the text to print. An error message is the text for stderr.
pub fn run(args: &[String], path: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("add") => add(&args[1..], path),
        Some("list") => list(path),
        Some(other) => Err(format!("unknown command: {other}")),
        None => Err("usage: rust-todo add <title> | list".to_string()),
    }
}

fn add(rest: &[String], path: &Path) -> Result<String, String> {
    let title = rest.join(" ");
    if title.trim().is_empty() {
        return Err("add needs a title".to_string());
    }
    let mut tasks = store::load(path).map_err(|error| error.to_string())?;
    let task = Task::new(store::next_id(&tasks), &title);
    let id = task.id;
    tasks.push(task);
    store::save(path, &tasks).map_err(|error| error.to_string())?;
    Ok(format!("Added {id}: {title}"))
}

fn list(path: &Path) -> Result<String, String> {
    let tasks = store::load(path).map_err(|error| error.to_string())?;
    let mut text = String::new();
    for task in &tasks {
        text.push_str(&task.format());
        text.push('\n');
    }
    text.push_str(&summary::describe(&tasks));
    text.push('\n');
    Ok(text)
}
