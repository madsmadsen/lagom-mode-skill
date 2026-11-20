//! The store: one task per line in a plain text file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::task::Task;

pub const DEFAULT_FILE: &str = "todo.txt";
pub const FILE_VAR: &str = "TODO_FILE";

/// The store path comes from `TODO_FILE`. Without that variable it is `todo.txt` in the current
/// directory.
pub fn resolve_path(value: Option<&str>) -> PathBuf {
    match value {
        Some(text) => PathBuf::from(text),
        None => PathBuf::from(DEFAULT_FILE),
    }
}

/// Read the tasks of a file. A missing file holds no task.
pub fn load(path: &Path) -> io::Result<Vec<Task>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(text.lines().filter_map(Task::parse).collect())
}

/// Write the tasks of a file, one line each.
pub fn save(path: &Path, tasks: &[Task]) -> io::Result<()> {
    let mut text = String::new();
    for task in tasks {
        text.push_str(&task.format());
        text.push('\n');
    }
    fs::write(path, text)
}

/// Ids count from 1, so the next id is one above the highest id in use.
pub fn next_id(tasks: &[Task]) -> u32 {
    tasks
        .iter()
        .map(|task| task.id)
        .max()
        .map_or(1, |id| id + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let pid = std::process::id();
        std::env::temp_dir().join(format!("rust-todo-store-{pid}-{name}.txt"))
    }

    /// SK-8: `TODO_FILE` sets the store path, and `todo.txt` is the default.
    #[test]
    fn sk_8_resolve_path_reads_the_variable() {
        assert_eq!(
            resolve_path(Some("/tmp/x.txt")),
            PathBuf::from("/tmp/x.txt")
        );
        assert_eq!(resolve_path(None), PathBuf::from("todo.txt"));
    }

    /// SK-8: a saved list loads back as the same tasks.
    #[test]
    fn sk_8_save_and_load_round_trip() {
        let path = temp_path("round-trip");
        let tasks = vec![Task::new(1, "buy milk"), Task::new(2, "walk dog")];
        save(&path, &tasks).expect("save");
        assert_eq!(load(&path).expect("load"), tasks);
        fs::remove_file(&path).ok();
    }

    /// SK-8: a missing file holds no task.
    #[test]
    fn sk_8_load_of_a_missing_file_is_empty() {
        let path = temp_path("missing");
        fs::remove_file(&path).ok();
        assert_eq!(load(&path).expect("load"), Vec::new());
    }

    /// SK-8: ids count from 1, and the next id is one above the highest id in use.
    #[test]
    fn sk_8_next_id_counts_from_one() {
        assert_eq!(next_id(&[]), 1);
        assert_eq!(next_id(&[Task::new(1, "a")]), 2);
        assert_eq!(
            next_id(&[Task::new(1, "a"), Task::new(2, "b"), Task::new(3, "c")]),
            4
        );
    }
}
