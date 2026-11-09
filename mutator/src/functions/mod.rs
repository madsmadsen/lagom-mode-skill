//! Port of crapper languages/__init__.py (BSD Zero Clause License, Robert C. Martin).

pub mod clojure;
pub mod golang;
pub mod java;
pub mod python;
pub mod rust;
pub mod typescript;

use crate::model::{Function, Language};

/// Functions of `source` for `language`, read with `project_root` for path resolution.
pub fn functions_in_file(
    language: Language,
    source: &str,
    path: &str,
    project_root: &str,
) -> Vec<Function> {
    let root = Some(project_root);
    match language {
        Language::Clojure => clojure::functions_in_source(source, path, root),
        Language::Java => java::functions_in_source(source, path, root),
        Language::Go => golang::functions_in_source(source, path, root),
        Language::TypeScript => typescript::functions_in_source(source, path, root),
        Language::Rust => rust::functions_in_source(source, path, root),
        Language::Python => python::functions_in_source(source, path, root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MU-9: the Java reader is selected for Language::Java.
    #[test]
    fn mu_9_dispatches_java_source_to_java_reader() {
        let source = "class Sample { int run() { return 1; } }";
        let found = functions_in_file(Language::Java, source, "Sample.java", "/nonexistent-mu9");
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str()))
            .collect();
        assert_eq!(pairs, vec![("Sample", "run")]);
    }

    /// MU-9: the Go reader is selected for Language::Go and resolves the package name.
    #[test]
    fn mu_9_dispatches_go_source_to_go_reader() {
        let source = "package sample\n\nfunc Run() {}\n";
        let found = functions_in_file(Language::Go, source, "sample.go", "/nonexistent-mu9");
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f.namespace.as_str(), f.name.as_str()))
            .collect();
        assert_eq!(pairs, vec![("sample", "Run")]);
    }
}
