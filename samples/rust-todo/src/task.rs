//! One task and its line format. A line holds the id and the title, separated by a tab.

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

#[cfg(test)]
mod tests {
    use super::*;

    /// SK-8: a line parses to the task with its id and its title.
    #[test]
    fn sk_8_parse_reads_the_id_and_the_title() {
        assert_eq!(Task::parse("3\tbuy milk"), Some(Task::new(3, "buy milk")));
    }

    /// SK-8: an id of 0 is not a task, because ids count from 1.
    #[test]
    fn sk_8_parse_rejects_id_zero() {
        assert_eq!(Task::parse("0\tbuy milk"), None);
    }

    /// SK-8: a line of the wrong shape does not parse.
    #[test]
    fn sk_8_parse_rejects_a_line_of_the_wrong_shape() {
        assert_eq!(Task::parse("buy milk"), None);
        assert_eq!(Task::parse("one\tbuy milk"), None);
        assert_eq!(Task::parse("2"), None);
    }

    /// SK-8: format writes the line that parse reads.
    #[test]
    fn sk_8_format_round_trips() {
        let task = Task::new(7, "walk dog");
        assert_eq!(task.format(), "7\twalk dog");
        assert_eq!(Task::parse(&task.format()), Some(task));
    }
}
