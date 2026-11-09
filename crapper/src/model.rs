// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/model.py
// license: MIT

/// One scored unit: a function or method.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub namespace: String,
    pub complexity: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub path: String,
    pub language: String,
    pub jacoco_class: Option<String>,
}

/// One row of the CRAP report and of `.metrics/crap.edn`.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub namespace: String,
    pub complexity: u32,
    pub coverage: Option<f64>,
    pub crap: Option<f64>,
    pub path: String,
    pub language: String,
}
