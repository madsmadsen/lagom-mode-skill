//! The summary line of the list. This module holds one old complex function that the sample
//! features do not touch (PRD for the skills, AC-14). Leave it as it is.

use crate::task::Task;

/// A sentence about the list. This function grew branch on branch, and its test is weak. That is
/// on purpose: it is the old complex function of AC-14.
pub fn describe(tasks: &[Task]) -> String {
    let count = tasks.len();
    if count == 0 {
        return "The list is empty.".to_string();
    }
    let head = match count {
        1 => "One task".to_string(),
        2..=4 => format!("A few tasks ({count})"),
        5..=9 => format!("Several tasks ({count})"),
        10..=49 => format!("Many tasks ({count})"),
        _ => format!("A long list ({count})"),
    };
    let first = tasks.first().map_or(0, |task| task.id);
    let last = tasks.last().map_or(0, |task| task.id);
    let span = if first == last {
        format!("id {first}")
    } else if last == first + 1 {
        format!("ids {first} and {last}")
    } else {
        format!("ids {first} to {last}")
    };
    let tail = if count > 20 {
        "The list is getting long."
    } else if count > 10 {
        "Keep an eye on it."
    } else {
        "All in order."
    };
    format!("{head}, {span}. {tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SK-8: the summary names the number of tasks and their ids.
    #[test]
    fn sk_8_describe_names_the_list() {
        let tasks = vec![Task::new(1, "a"), Task::new(2, "b")];
        assert_eq!(
            describe(&tasks),
            "A few tasks (2), ids 1 and 2. All in order."
        );
    }
}
