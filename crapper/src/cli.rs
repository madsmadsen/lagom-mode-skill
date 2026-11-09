// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/cli.py
// license: MIT

//! Command line for the multi-language CRAP tool.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

use crate::analyze::analyze_files;
use crate::coverage::{load_bundle, CoverageBundle};
use crate::discover::{
    is_test_file, iter_source_files, language_of, CLOJURE_TEST_SUFFIXES, GO_TEST_SUFFIX,
    JS_TEST_SUFFIXES, SKIP_DIRS, TEST_DIRS,
};
use crate::metrics::write_metrics;
use crate::model::Entry;
use crate::report::format_report;
use crate::runners::run_coverage;

/// Full `--help` text. Its skipped-directory and test-pattern lists come from `discover`.
fn help_text() -> String {
    let skipped = skipped_dirs();
    let patterns = test_patterns();
    format!(
        "Usage: crapper [options] [path-or-filter ...]

Detect the language of each source file and score it with the CRAP metric
CRAP = CC² × (1 − coverage)³ + CC. Writes .metrics/crap.edn for uml-viewer
and prints a report sorted worst first.

Languages: Clojure (.clj .cljc .cljs .bb), Java (.java), Go (.go),
TypeScript and JavaScript (.ts .tsx .mts .cts .js .jsx .mjs .cjs),
Rust (.rs), Python (.py).

Options:
  -h, --help                    Print this help and exit.
  --root <path>                 Project root. Metrics are written here.
                                Default: the current directory.
  -s, --source-root <path>      Walk this tree instead of the project root.
                                May be repeated.
  --changed                     Analyze added and modified source files from
                                git status.
  --no-coverage                 Score complexity only. Coverage and CRAP are N/A.
  --use-existing-coverage       Read coverage already on disk. Do not rerun tests.
  --coverage-command <cmd>      Run this command instead of the per-language
                                coverage tools, then read the reports it wrote.
  --threshold <number>          Exit 2 when the worst CRAP score is above this.

Arguments:
  path              File or directory to analyze. Test paths are included when
                    you name them explicitly.
  filter            When the argument is not a path, only source files whose
                    path contains this text are analyzed.

With no paths, source files under the project root are analyzed. Directories
named {skipped} are skipped, as are {patterns}.

Coverage, when it is produced:
  Clojure      clj -M:cov --lcov, then Cloverage form counts or LCOV
  Java         Maven JaCoCo instruction coverage, for each pom.xml module
  Go           go test ./... -coverprofile=...
  TypeScript   npm run coverage, Vitest's own LCOV, or c8 around npm test
  Rust         cargo llvm-cov, or cargo tarpaulin, in the nearest Cargo.toml
  Python       coverage.py LCOV, via pytest or unittest, per project

uml-viewer joins .metrics/crap.edn on :namespace and :name.
  Clojure      the ns, and the defn name
  Java         package.Class, and the method name
  Go           import path, or import.path.Receiver for methods
  TypeScript   dotted module path, or module.Class for methods
  Rust         crate::module, or crate::module::Type for methods
  Python       dotted module path, or module.Class for methods
"
    )
}

fn skipped_dirs() -> String {
    let names: BTreeSet<&str> = SKIP_DIRS.iter().chain(TEST_DIRS.iter()).copied().collect();
    names.into_iter().collect::<Vec<_>>().join(", ")
}

fn test_patterns() -> String {
    let mut patterns = vec![format!("*{GO_TEST_SUFFIX}")];
    patterns.extend(
        CLOJURE_TEST_SUFFIXES
            .iter()
            .map(|suffix| format!("*{suffix}")),
    );
    patterns.extend(JS_TEST_SUFFIXES.iter().map(|suffix| format!("*{suffix}")));
    patterns.extend(
        ["test_*.py", "*_test.py", "conftest.py"]
            .iter()
            .map(|s| s.to_string()),
    );
    patterns.join(", ")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub project_root: PathBuf,
    pub source_roots: Vec<String>,
    pub positionals: Vec<String>,
    pub no_coverage: bool,
    pub use_existing_coverage: bool,
    pub coverage_command: Option<String>,
    pub threshold: Option<f64>,
    pub changed: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            project_root: PathBuf::from("."),
            source_roots: Vec::new(),
            positionals: Vec::new(),
            no_coverage: false,
            use_existing_coverage: false,
            coverage_command: None,
            threshold: None,
            changed: false,
        }
    }
}

/// What the command line asked for: an analysis, or a help/usage message.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Analyze(Options),
    Help { message: String, exit_code: i32 },
}

/// Parses the arguments that follow the program name.
pub fn parse_args(args: &[String]) -> Parsed {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Parsed::Help {
            message: help_text(),
            exit_code: 0,
        };
    }
    match parse_options(args).and_then(check_conflicts) {
        Ok(options) => Parsed::Analyze(options),
        Err(message) => Parsed::Help {
            message: format!("{message}\n\n{}", help_text()),
            exit_code: 1,
        },
    }
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        index += apply_option(&mut options, args, index)?;
    }
    Ok(options)
}

/// Applies the option at `index` and returns how many arguments it used.
fn apply_option(options: &mut Options, args: &[String], index: usize) -> Result<usize, String> {
    let arg = args[index].as_str();
    match arg {
        "--no-coverage" => options.no_coverage = true,
        "--use-existing-coverage" => options.use_existing_coverage = true,
        "--changed" => options.changed = true,
        "-s" | "--source-root" | "--root" | "--coverage-command" | "--threshold" => {
            let value = take(args, index, arg)?;
            apply_value(options, arg, value)?;
            return Ok(2);
        }
        _ if arg.starts_with('-') => return Err(format!("Unknown option: {arg}")),
        _ => options.positionals.push(arg.to_string()),
    }
    Ok(1)
}

/// Stores the value of an option that takes one. `--threshold` is the only other case.
fn apply_value(options: &mut Options, option: &str, value: String) -> Result<(), String> {
    match option {
        "-s" | "--source-root" => options.source_roots.push(value),
        "--root" => options.project_root = PathBuf::from(value),
        "--coverage-command" => options.coverage_command = Some(value),
        _ => options.threshold = Some(parse_threshold(&value)?),
    }
    Ok(())
}

/// The value after `option`. An empty value or one that starts with `-` is missing.
fn take(args: &[String], index: usize, option: &str) -> Result<String, String> {
    match args.get(index + 1) {
        Some(value) if !value.is_empty() && !value.starts_with('-') => Ok(value.clone()),
        _ => Err(format!("{option} requires a value")),
    }
}

fn parse_threshold(raw: &str) -> Result<f64, String> {
    python_float(raw).ok_or_else(|| "--threshold requires a number".to_string())
}

/// Python's `float()` for ASCII text: surrounding whitespace is ignored, and
/// an underscore is allowed only directly between two digits.
fn python_float(raw: &str) -> Option<f64> {
    let text = raw.trim();
    let bytes = text.as_bytes();
    let underscores_between_digits = bytes.iter().enumerate().all(|(index, &byte)| {
        byte != b'_'
            || (index > 0
                && index + 1 < bytes.len()
                && bytes[index - 1].is_ascii_digit()
                && bytes[index + 1].is_ascii_digit())
    });
    if underscores_between_digits {
        text.replace('_', "").parse().ok()
    } else {
        None
    }
}

fn check_conflicts(options: Options) -> Result<Options, String> {
    if options.no_coverage && options.coverage_command.is_some() {
        return Err("--no-coverage cannot be combined with --coverage-command".to_string());
    }
    Ok(options)
}

/// A git call that failed. `code` is the status the process exits with.
#[derive(Debug)]
struct GitError {
    code: i32,
    message: String,
}

fn git(root: &Path, args: &[&str]) -> Result<Output, GitError> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| GitError {
            code: 1,
            message: error.to_string(),
        })
}

fn git_stdout(output: Output) -> Result<String, GitError> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if stderr.is_empty() {
        "git status failed".to_string()
    } else {
        stderr
    };
    Err(GitError {
        code: exit_code(output.status),
        message,
    })
}

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| -signal))
        .unwrap_or(1)
}

/// Added and modified files from git, including files in new directories.
///
/// Paths come from `git status -z`, which does not quote non-ASCII names.
/// They are resolved from the repository root, then limited to `root`, so
/// `--root` can be a subdirectory. Deleted files are left out.
fn changed_files(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    let top_text = git_stdout(git(root, &["rev-parse", "--show-toplevel"])?)?;
    let top = PathBuf::from(top_text.trim());
    let status = git_stdout(git(
        root,
        &[
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--",
            ".",
        ],
    )?)?;
    let limit = resolve(root);
    Ok(status
        .split('\0')
        .filter_map(|entry| entry.get(3..).filter(|rel| !rel.is_empty()))
        .map(|rel| resolve(&top.join(rel)))
        .filter(|path| path.is_file() && path.starts_with(&limit))
        .collect())
}

fn changed_source(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    Ok(changed_files(root)?
        .into_iter()
        .filter(|path| language_of(path).is_some() && !is_test_file(path))
        .collect())
}

/// Splits positional arguments into existing paths and text filters.
fn positionals(root: &Path, args: &[String]) -> (Vec<PathBuf>, Vec<String>) {
    let mut existing = Vec::new();
    let mut filters = Vec::new();
    for arg in args {
        let candidate = root.join(arg);
        if candidate.exists() {
            existing.push(resolve(&candidate));
        } else {
            filters.push(arg.clone());
        }
    }
    (existing, filters)
}

fn explicit_files(existing: &[PathBuf]) -> Vec<PathBuf> {
    existing
        .iter()
        .flat_map(|path| {
            if path.is_dir() {
                iter_source_files(std::slice::from_ref(path))
            } else if language_of(path).is_some() {
                vec![path.clone()]
            } else {
                Vec::new()
            }
        })
        .collect()
}

fn selected(
    options: &Options,
    root: &Path,
    existing: &[PathBuf],
) -> Result<Vec<PathBuf>, GitError> {
    if options.changed {
        return changed_source(root);
    }
    if !options.source_roots.is_empty() {
        let roots: Vec<PathBuf> = options
            .source_roots
            .iter()
            .map(|path| resolve(&root.join(path)))
            .collect();
        return Ok(iter_source_files(&roots));
    }
    if !existing.is_empty() {
        return Ok(explicit_files(existing));
    }
    Ok(iter_source_files(&[root.to_path_buf()]))
}

fn apply_filters(files: Vec<PathBuf>, filters: &[String]) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter(|path| {
            let text = path.to_string_lossy();
            filters.is_empty() || filters.iter().any(|item| text.contains(item.as_str()))
        })
        .collect()
}

/// Resolves each path, then sorts by path text and removes duplicates.
fn sorted_unique(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut resolved: Vec<PathBuf> = files.iter().map(|path| resolve(path)).collect();
    resolved.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    resolved.dedup();
    resolved
}

fn select_files(options: &Options) -> Result<Vec<PathBuf>, GitError> {
    let root = resolve(&options.project_root);
    let (existing, filters) = positionals(&root, &options.positionals);
    let files = apply_filters(selected(options, &root, &existing)?, &filters);
    Ok(sorted_unique(files))
}

fn resolve(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn coverage_bundle(options: &Options, root: &Path, files: &[PathBuf]) -> Option<CoverageBundle> {
    if options.no_coverage {
        return None;
    }
    if options.use_existing_coverage {
        return Some(load_bundle(root));
    }
    let status = run_coverage(root, files, options.coverage_command.as_deref());
    if options.coverage_command.is_some() && matches!(status, Some(code) if code != 0) {
        return Some(CoverageBundle::default());
    }
    Some(load_bundle(root))
}

fn threshold_status(threshold: Option<f64>, entries: &[Entry]) -> i32 {
    let worst = entries
        .iter()
        .filter_map(|entry| entry.crap)
        .reduce(f64::max);
    match (threshold, worst) {
        (Some(limit), Some(worst)) if worst > limit => {
            eprintln!("CRAP threshold exceeded: {worst:.1} > {limit:.1}");
            2
        }
        _ => 0,
    }
}

fn show_help(message: &str, exit_code: i32) -> i32 {
    if exit_code == 0 {
        print!("{message}");
    } else {
        eprint!("{message}");
    }
    exit_code
}

fn analyze(options: &Options) -> i32 {
    let root = resolve(&options.project_root);
    let files = match select_files(options) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("{}", error.message);
            return error.code;
        }
    };
    if files.is_empty() {
        println!("No source files to analyze.");
        return 0;
    }
    let mut bundle = coverage_bundle(options, &root, &files);
    let entries = analyze_files(&files, &root, bundle.as_mut());
    let metrics = match write_metrics(&entries, &root) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    print!("{}", format_report(&entries));
    eprintln!("Wrote {}", metrics.display());
    threshold_status(options.threshold, &entries)
}

/// Runs the CLI with the arguments that follow the program name. Returns the exit code.
pub fn run(args: &[String]) -> i32 {
    match parse_args(args) {
        Parsed::Help { message, exit_code } => show_help(&message, exit_code),
        Parsed::Analyze(options) => analyze(&options),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    // CR-1
    #[test]
    fn cr_1_help_text_names_the_skipped_dirs_and_test_patterns() {
        let help = help_text();
        assert!(help.starts_with("Usage: crapper"));
        assert!(help.contains("uml-viewer"));
        assert!(help.contains(".clj-kondo"));
        assert!(help.contains("*.test.cts"));
        assert!(help.contains("conftest.py"));
    }

    // CR-2
    #[test]
    fn cr_2_help_anywhere_wins_and_exits_0() {
        let parsed = parse_args(&args(&["--root", "x", "--bogus", "-h"]));
        assert_eq!(
            parsed,
            Parsed::Help {
                message: help_text(),
                exit_code: 0
            }
        );
    }

    // CR-2
    #[test]
    fn cr_2_unknown_option_exits_1_with_message_and_help() {
        let parsed = parse_args(&args(&["--bogus"]));
        assert_eq!(
            parsed,
            Parsed::Help {
                message: format!("Unknown option: --bogus\n\n{}", help_text()),
                exit_code: 1,
            }
        );
    }

    // CR-2
    #[test]
    fn cr_2_take_rejects_missing_empty_and_dash_values() {
        assert!(take(&args(&["ok", "--opt", ""]), 1, "--opt").is_err());
        assert!(take(&args(&["--opt", "-1"]), 0, "--opt").is_err());
        assert!(take(&args(&["--opt"]), 0, "--opt").is_err());
        assert_eq!(
            take(&args(&["--opt", "value"]), 0, "--opt"),
            Ok("value".to_string())
        );
    }

    // CR-2
    #[test]
    fn cr_2_missing_value_exits_1_with_message() {
        let parsed = parse_args(&args(&["--root"]));
        assert_eq!(
            parsed,
            Parsed::Help {
                message: format!("--root requires a value\n\n{}", help_text()),
                exit_code: 1,
            }
        );
    }

    // CR-2
    #[test]
    fn cr_2_threshold_must_be_a_number() {
        let parsed = parse_args(&args(&["--threshold", "nope"]));
        assert_eq!(
            parsed,
            Parsed::Help {
                message: format!("--threshold requires a number\n\n{}", help_text()),
                exit_code: 1,
            }
        );
        let Parsed::Analyze(options) = parse_args(&args(&["--threshold", "5"])) else {
            panic!("expected an analysis");
        };
        assert_eq!(options.threshold, Some(5.0));
    }

    // CR-2
    #[test]
    fn cr_2_no_coverage_conflicts_with_coverage_command() {
        let parsed = parse_args(&args(&["--no-coverage", "--coverage-command", "true"]));
        assert_eq!(
            parsed,
            Parsed::Help {
                message: format!(
                    "--no-coverage cannot be combined with --coverage-command\n\n{}",
                    help_text()
                ),
                exit_code: 1,
            }
        );
    }

    // CR-2
    #[test]
    fn cr_2_parse_reads_every_flag_value_and_positional() {
        let parsed = parse_args(&args(&[
            "--root",
            "proj",
            "-s",
            "src",
            "--source-root",
            "lib",
            "--changed",
            "--use-existing-coverage",
            "--coverage-command",
            "make cov",
            "--threshold",
            "7.5",
            "pkg/mod.py",
        ]));
        let Parsed::Analyze(options) = parsed else {
            panic!("expected an analysis");
        };
        assert_eq!(options.project_root, PathBuf::from("proj"));
        assert_eq!(options.source_roots, args(&["src", "lib"]));
        assert!(options.changed);
        assert!(options.use_existing_coverage);
        assert!(!options.no_coverage);
        assert_eq!(options.coverage_command.as_deref(), Some("make cov"));
        assert_eq!(options.threshold, Some(7.5));
        assert_eq!(options.positionals, args(&["pkg/mod.py"]));
    }

    // CR-2
    #[test]
    fn cr_2_defaults_to_the_current_directory_with_no_flags() {
        let Parsed::Analyze(options) = parse_args(&[]) else {
            panic!("expected an analysis");
        };
        assert_eq!(options, Options::default());
        assert_eq!(options.project_root, PathBuf::from("."));
    }

    // CR-2
    #[test]
    fn cr_2_skipped_dirs_are_sorted_and_joined() {
        let names = skipped_dirs();
        assert!(names.starts_with(".clj-kondo, .git, "));
        assert!(names.contains("__tests__"));
        assert!(names.ends_with("venv"));
    }

    // CR-17
    #[test]
    fn cr_17_worst_score_comparison_is_strict() {
        let entry = |crap: f64| Entry {
            name: "f".into(),
            namespace: "m".into(),
            complexity: 1,
            coverage: Some(0.0),
            crap: Some(crap),
            path: "m.py".into(),
            language: "python".into(),
        };
        assert_eq!(threshold_status(Some(10.0), &[entry(10.0)]), 0);
        assert_eq!(threshold_status(Some(9.99), &[entry(10.0)]), 2);
        assert_eq!(threshold_status(Some(1.0), &[]), 0);
        assert_eq!(threshold_status(None, &[entry(10.0)]), 0);
    }

    // CR-2
    #[test]
    fn cr_2_python_float_accepts_what_python_accepts() {
        assert_eq!(python_float("1_0"), Some(10.0));
        assert_eq!(python_float("1_0.5_0"), Some(10.5));
        assert_eq!(python_float("1e1_0"), Some(1e10));
        assert_eq!(python_float(" 12 "), Some(12.0));
    }

    // CR-2
    #[test]
    fn cr_2_python_float_rejects_misplaced_underscores() {
        for raw in [
            "_1", "1_", "1__0", "1_.5", "1._5", "1e_5", "1_e5", "-_1", "abc", "",
        ] {
            assert_eq!(python_float(raw), None, "{raw:?} should be rejected");
        }
    }

    // CR-2
    #[test]
    fn cr_2_threshold_accepts_single_underscores_between_digits() {
        let Parsed::Analyze(options) = parse_args(&args(&["--threshold", "1_0"])) else {
            panic!("expected an analysis");
        };
        assert_eq!(options.threshold, Some(10.0));
    }

    // CR-2, CR-13
    #[test]
    fn cr_2_cr_13_default_does_not_read_existing_coverage() {
        let Parsed::Analyze(options) = parse_args(&[]) else {
            panic!("expected an analysis");
        };
        assert!(!options.use_existing_coverage);
    }

    // CR-3
    #[test]
    fn cr_3_exit_code_is_the_status_or_the_negated_signal() {
        assert_eq!(exit_code(ExitStatus::from_raw(0x0100)), 1);
        assert_eq!(exit_code(ExitStatus::from_raw(0x0200)), 2);
        assert_eq!(exit_code(ExitStatus::from_raw(9)), -9);
    }

    // CR-3
    #[test]
    fn cr_3_exit_code_without_status_or_signal_is_1() {
        assert_eq!(exit_code(ExitStatus::from_raw(0x7f)), 1);
    }
}
