// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/analyze.py
// license: MIT

//! Turn source files and coverage into CRAP entries.

use std::fs;
use std::path::{Path, PathBuf};

use crate::coverage::CoverageBundle;
use crate::crap::{make_entry, sort_entries};
use crate::discover::{language_of, resolve};
use crate::languages::functions_in_file;
use crate::model::{Entry, Function};

/// `None` means coverage was not requested (`--no-coverage`). A function the report
/// does not mention scores 0%, so it sorts with the scored functions.
fn coverage_of(bundle: Option<&CoverageBundle>, function: &Function) -> Option<f64> {
    bundle.map(|bundle| bundle.percent_for(function).unwrap_or(0.0))
}

/// The file relative to the project root as a `/`-joined path, else the file path.
fn source_path(file: &Path, root: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    relative.to_string_lossy().into_owned()
}

/// Python reads in text mode: `\r\n` and a lone `\r` both become `\n`.
fn read_source(file: &Path) -> String {
    let bytes =
        fs::read(file).unwrap_or_else(|err| panic!("cannot read {}: {err}", file.display()));
    String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

pub fn analyze_files(
    files: &[PathBuf],
    project_root: &Path,
    bundle: Option<&mut CoverageBundle>,
) -> Vec<Entry> {
    let root = resolve(project_root);
    let resolved: Vec<PathBuf> = files.iter().map(|file| resolve(file)).collect();
    let mut bundle = bundle;
    if let Some(bundle) = bundle.as_deref_mut() {
        let paths: Vec<String> = resolved
            .iter()
            .map(|file| source_path(file, &root))
            .collect();
        bundle.bind_sources(&paths);
    }
    let coverage = bundle.as_deref();
    let root_text = root.to_string_lossy();
    let mut entries = Vec::new();
    for file in &resolved {
        let Some(language) = language_of(file) else {
            continue;
        };
        let source = read_source(file);
        let relative = source_path(file, &root);
        for function in functions_in_file(language, &source, &relative, &root_text) {
            entries.push(make_entry(&function, coverage_of(coverage, &function)));
        }
    }
    sort_entries(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // CR-4, CR-10
    #[test]
    fn cr_4_cr_10_without_a_bundle_entries_have_no_coverage_and_no_crap() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/app.py"), "def run():\n    return 1\n").unwrap();
        let entries = analyze_files(&[root.join("src/app.py")], root, None);
        assert_eq!(
            entries,
            vec![Entry {
                name: "run".into(),
                namespace: "app".into(),
                complexity: 1,
                coverage: None,
                crap: None,
                path: "src/app.py".into(),
                language: "python".into(),
            }]
        );
    }

    // CR-5
    #[test]
    fn cr_5_a_file_with_an_unknown_extension_is_skipped() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "# notes\n").unwrap();
        let entries = analyze_files(&[dir.path().join("notes.md")], dir.path(), None);
        assert!(entries.is_empty());
    }
}
