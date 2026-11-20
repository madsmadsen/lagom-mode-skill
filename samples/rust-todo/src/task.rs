//! One task and its line format. A line holds the id and the title, separated by a tab.
//!
//! The file also holds the summary sentence of the list, `describe`. That function has many
//! branches and one test (the skills PRD, AC-14). The task functions are small and tested.

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: u32,
    pub title: String,
}

impl Task {
    pub fn new(id: u32, title: &str) -> Self {
        Task {
            id,
            title: title.to_string(),
        }
    }

    /// Read one line of the store. Ids count from 1, so an id of 0 is not a task.
    pub fn parse(line: &str) -> Option<Self> {
        let mut parts = line.split('\t');
        let id = parts.next()?.parse().ok()?;
        if id == 0 {
            return None;
        }
        let title = parts.next()?;
        Some(Task::new(id, title))
    }

    /// Write the line that `parse` reads back.
    pub fn format(&self) -> String {
        format!("{}\t{}", self.id, self.title)
    }
}

/// A sentence about the list. The function grew branch on branch, and its one test takes one case.
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

    /// Skills PRD section 9: a line parses to the task with its id and its title.
    #[test]
    fn sample_parse_reads_the_id_and_the_title() {
        assert_eq!(Task::parse("3\tbuy milk"), Some(Task::new(3, "buy milk")));
    }

    /// Skills PRD section 9: an id of 0 is not a task, because ids count from 1.
    #[test]
    fn sample_parse_rejects_id_zero() {
        assert_eq!(Task::parse("0\tbuy milk"), None);
    }

    /// Skills PRD section 9: a line of the wrong shape does not parse.
    #[test]
    fn sample_parse_rejects_a_line_of_the_wrong_shape() {
        assert_eq!(Task::parse("buy milk"), None);
        assert_eq!(Task::parse("one\tbuy milk"), None);
        assert_eq!(Task::parse("2"), None);
    }

    /// Skills PRD section 9: format writes the line that parse reads.
    #[test]
    fn sample_format_round_trips() {
        let task = Task::new(7, "walk dog");
        assert_eq!(task.format(), "7\twalk dog");
        assert_eq!(Task::parse(&task.format()), Some(task));
    }

    /// Skills PRD AC-14: the summary names the number of tasks and their ids.
    #[test]
    fn sample_describe_names_the_list() {
        let tasks = vec![Task::new(1, "a"), Task::new(2, "b")];
        assert_eq!(
            describe(&tasks),
            "A few tasks (2), ids 1 and 2. All in order."
        );
    }
}
