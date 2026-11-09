// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/languages/__init__.py
// license: MIT

pub mod clojure;
pub mod golang;
pub mod java;
pub mod python;
pub mod rust;
pub mod ts;
pub mod typescript;

use crate::model::Function;

/// Functions of one source file, read with the reader for `language`. Unknown languages have none.
pub fn functions_in_file(
    language: &str,
    source: &str,
    path: &str,
    project_root: &str,
) -> Vec<Function> {
    match language {
        "clojure" => clojure::functions_in_source(source, path, project_root),
        "java" => java::functions_in_source(source, path, project_root),
        "go" => golang::functions_in_source(source, path, project_root),
        "typescript" => typescript::functions_in_source(source, path, project_root),
        "python" => python::functions_in_source(source, path, project_root),
        "rust" => rust::functions_in_source(source, path, project_root),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::functions_in_file;

    // CR-8
    #[test]
    fn cr_8_java_source_is_read_by_the_java_reader_and_an_unknown_language_has_no_functions() {
        let source = "class Board { void place() { if (true) return; } }\n";
        let found = functions_in_file("java", source, "Board.java", "/proj");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "place");
        assert_eq!(found[0].language, "java");
        assert!(functions_in_file("ruby", "def x\nend\n", "a.rb", "/proj").is_empty());
    }
}
