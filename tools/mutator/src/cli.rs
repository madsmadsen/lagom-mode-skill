// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Port of src/mutator/cli.py.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use crate::bind::splitlines;
use crate::cover_run;
use crate::coverage::covered_lines;
use crate::discover::{
    SKIP_DIRS, TEST_DIRS, is_test_file, iter_source_files, language_of, skipped_directory_text,
};
use crate::engine::{Baselines, MutateOptions, mutate_file, restore_backups, scan_file};
use crate::fs_util::resolve;
use crate::model::{FormResult, LineSet, RunResult};
use crate::report::{format_results, format_site_log};
use crate::runner::{Command as RunCommand, CommandRunner};

const HELP_TEMPLATE: &str = r#"Usage: mutator [options] [path-or-filter ...]

Discover mutation sites in Clojure, Java, Go, TypeScript, Rust, and Python,
run the project's tests against each one, and write `.metrics/mutate` for
uml-viewer. Namespaces and function names are the ones crapper writes into
`.metrics/crap.edn`.

A form id is `defn/<name>` or, for a private operation, `defn-/<name>`.
uml-viewer joins that name to the operation and joins `:namespace` to the
class.

Options:
  -h, --help                    Print this help and exit.
  --root <path>                 Project root. Metrics are written here.
                                Default: the current directory.
  -s, --source-root <path>      Walk this tree instead of the project root.
                                May be repeated.
  --changed                     Mutate added and modified source files from
                                git status.
  --scan                        List mutation sites. Do not run tests or
                                write metrics.
  --mutate-all                  Run every covered site, including ones the
                                last snapshot already killed.
  --since-last-run              Run survivors and sites in new or rewritten
                                functions. This is the default when a snapshot
                                exists.
  --lines <n,n,...>             Run only mutations on these source lines.
  --no-coverage                 Treat every site as covered.
  --use-existing-coverage       Read coverage already on disk. Same as
                                --reuse-coverage.
  --reuse-coverage              Read coverage already on disk.
  --coverage-command <cmd>      Run this command instead of the per-language
                                coverage tools, then read the reports it wrote.
  --test-command <cmd>          Run this command for the baseline and every
                                mutant. The working directory is the project
                                root.
  --timeout-factor <number>     Mutant timeout, as a multiple of the baseline
                                duration. Default: 10. The timeout is at least
                                2 seconds.
  --mutation-warning <number>   Warn when a file selects more covered sites
                                than this. Default: 50.
  --max-workers <number>        Run at most this many mutants of one file at
                                once. Default: half the cores, at least one.
                                The run uses the smaller of this limit, half
                                the cores, and the number of selected sites.
  --max-memory-mb <number>      Kill a mutant test run when its process group
                                uses more than this many MiB of resident
                                memory. Default: 4096. 0 turns the limit off.
                                The baseline run has no limit.
  --verbose                     Print each test command and each mutant.

Arguments:
  path              File or directory to mutate. Test paths are skipped.
  filter            When the argument is not a path, only source files whose
                    path contains this text are mutated.

With no paths, source files under the project root are mutated. Directories
named {skipped} are skipped.

The default, once a snapshot exists, reruns survivors and sites in functions
whose text changed. Killed mutants in unchanged functions are kept.

Selected mutants of one file run at the same time, at most one worker per two
cores, even when --max-workers asks for more. A worker is a symlink overlay under
target/mutation-workers with its own copy of the mutated file. The project
tree is left unchanged. A copy left under target/mutator-backup/ by an
interrupted older run is restored before a non-scan run.

Exit codes:
  0  every executed mutant was killed, or there was nothing to run
  1  usage error
  2  baseline tests failed
  3  at least one mutant survived

Coverage, when it is produced, uses the same commands as crapper:
  Clojure      clj -M:cov --lcov
  Java         Maven JaCoCo
  Go           go test ./... -coverprofile=...
  TypeScript   npm run coverage, Vitest, or c8
  Rust         cargo llvm-cov or cargo tarpaulin
  Python       coverage.py LCOV

A site on a line the report does not mark as hit is uncovered and is not run.
"#;

/// Whether the command asks for help or runs the mutation tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Help,
    Mutate,
}

/// Parsed command line. `message` and `exit_code` are used when `action` is `Help`.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub action: Action,
    pub message: String,
    pub exit_code: i32,
    pub project_root: PathBuf,
    pub source_roots: Vec<String>,
    pub positionals: Vec<String>,
    pub scan: bool,
    pub mutate_all: bool,
    pub since_last_run: bool,
    pub lines: Option<LineSet>,
    pub no_coverage: bool,
    pub use_existing_coverage: bool,
    pub coverage_command: Option<String>,
    pub test_command: Option<String>,
    pub timeout_factor: f64,
    pub mutation_warning: usize,
    pub max_workers: Option<usize>,
    pub max_memory_mb: u64,
    pub changed: bool,
    pub verbose: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            action: Action::Mutate,
            message: String::new(),
            exit_code: 0,
            project_root: PathBuf::from("."),
            source_roots: Vec::new(),
            positionals: Vec::new(),
            scan: false,
            mutate_all: false,
            since_last_run: false,
            lines: None,
            no_coverage: false,
            use_existing_coverage: false,
            coverage_command: None,
            test_command: None,
            timeout_factor: 10.0,
            mutation_warning: 50,
            max_workers: None,
            max_memory_mb: 4096,
            changed: false,
            verbose: false,
        }
    }
}

type ValueSetter = fn(&mut Options, &str) -> Result<(), String>;
type FlagSetter = fn(&mut Options);
type Predicate = fn(&Options) -> bool;
type Conflict = (Predicate, &'static str);

const VALUE_OPTIONS: &[(&[&str], ValueSetter)] = &[
    (&["-s", "--source-root"], add_source_root),
    (&["--root"], set_root),
    (&["--coverage-command"], |options, value| {
        set_text(options, value, |o| &mut o.coverage_command)
    }),
    (&["--test-command"], |options, value| {
        set_text(options, value, |o| &mut o.test_command)
    }),
    (&["--timeout-factor"], set_timeout_factor),
    (&["--mutation-warning"], set_mutation_warning),
    (&["--max-workers"], set_max_workers),
    (&["--max-memory-mb"], set_max_memory_mb),
    (&["--lines"], set_lines),
];

const FLAG_OPTIONS: &[(&[&str], FlagSetter)] = &[
    (&["--no-coverage"], |options| options.no_coverage = true),
    (
        &["--use-existing-coverage", "--reuse-coverage"],
        |options| options.use_existing_coverage = true,
    ),
    (&["--changed"], |options| options.changed = true),
    (&["--scan"], |options| options.scan = true),
    (&["--mutate-all"], |options| options.mutate_all = true),
    (&["--since-last-run"], |options| {
        options.since_last_run = true
    }),
    (&["--verbose"], |options| options.verbose = true),
];

fn add_source_root(options: &mut Options, value: &str) -> Result<(), String> {
    options.source_roots.push(value.to_string());
    Ok(())
}

fn set_root(options: &mut Options, value: &str) -> Result<(), String> {
    options.project_root = PathBuf::from(value);
    Ok(())
}

fn set_text(
    options: &mut Options,
    value: &str,
    field: fn(&mut Options) -> &mut Option<String>,
) -> Result<(), String> {
    *field(options) = Some(value.to_string());
    Ok(())
}

fn set_timeout_factor(options: &mut Options, value: &str) -> Result<(), String> {
    let factor = py_number::<f64>(value, float_error)?;
    if factor <= 0.0 {
        return Err("--timeout-factor requires a positive number".to_string());
    }
    options.timeout_factor = factor;
    Ok(())
}

fn set_mutation_warning(options: &mut Options, value: &str) -> Result<(), String> {
    options.mutation_warning = usize::try_from(py_number::<i64>(value, int_error)?)
        .map_err(|_| "--mutation-warning requires a non-negative integer".to_string())?;
    Ok(())
}

fn set_max_memory_mb(options: &mut Options, value: &str) -> Result<(), String> {
    options.max_memory_mb = u64::try_from(py_number::<i64>(value, int_error)?)
        .map_err(|_| "--max-memory-mb requires a non-negative integer".to_string())?;
    Ok(())
}

fn set_max_workers(options: &mut Options, value: &str) -> Result<(), String> {
    options.max_workers = Some(workers(value)?);
    Ok(())
}

fn set_lines(options: &mut Options, value: &str) -> Result<(), String> {
    options.lines = Some(parse_lines(value)?);
    Ok(())
}

fn py_number<T: FromStr>(value: &str, message: fn(&str) -> String) -> Result<T, String> {
    value.trim().parse::<T>().map_err(|_| message(value))
}

fn int_error(value: &str) -> String {
    format!("invalid literal for int() with base 10: '{value}'")
}

fn float_error(value: &str) -> String {
    format!("could not convert string to float: '{value}'")
}

fn workers(value: &str) -> Result<usize, String> {
    match py_number::<i64>(value, int_error)
        .ok()
        .and_then(|count| usize::try_from(count).ok())
    {
        Some(count) if count >= 1 => Ok(count),
        _ => Err("--max-workers requires a positive integer".to_string()),
    }
}

fn parse_lines(value: &str) -> Result<LineSet, String> {
    let mut found = LineSet::new();
    for piece in value
        .split(',')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
    {
        let number = py_number::<i64>(piece, int_error)?;
        if number < 1 {
            return Err("--lines requires positive line numbers".to_string());
        }
        let line = usize::try_from(number)
            .map_err(|_| "--lines requires positive line numbers".to_string())?;
        found.insert(line);
    }
    if found.is_empty() {
        return Err("--lines requires a line number".to_string());
    }
    Ok(found)
}

fn take<'a>(option: &str, next: Option<&'a String>) -> Result<&'a str, String> {
    match next {
        Some(value) if !value.is_empty() && !value.starts_with('-') => Ok(value.as_str()),
        _ => Err(format!("{option} requires a value")),
    }
}

/// Applies one argument; returns how many argv slots it used.
fn apply_arg(options: &mut Options, arg: &str, next: Option<&String>) -> Result<usize, String> {
    if let Some((_, set)) = VALUE_OPTIONS.iter().find(|(names, _)| names.contains(&arg)) {
        set(options, take(arg, next)?)?;
        return Ok(2);
    }
    if let Some((_, set)) = FLAG_OPTIONS.iter().find(|(names, _)| names.contains(&arg)) {
        set(options);
        return Ok(1);
    }
    if arg.starts_with('-') {
        return Err(format!("Unknown option: {arg}"));
    }
    options.positionals.push(arg.to_string());
    Ok(1)
}

const CONFLICTS: &[Conflict] = &[
    (
        |options| options.mutate_all && options.since_last_run,
        "--mutate-all cannot be combined with --since-last-run",
    ),
    (
        |options| options.no_coverage && options.coverage_command.is_some(),
        "--no-coverage cannot be combined with --coverage-command",
    ),
    (
        |options| options.scan && options.mutate_all,
        "--scan cannot be combined with --mutate-all",
    ),
    (
        |options| options.scan && options.max_workers.is_some(),
        "--scan cannot be combined with --max-workers",
    ),
];

fn check_conflicts(options: &Options) -> Result<(), String> {
    match CONFLICTS.iter().find(|(hit, _)| hit(options)) {
        Some((_, reason)) => Err(reason.to_string()),
        None => Ok(()),
    }
}

fn parse_all(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += apply_arg(&mut options, arg, args.get(index + 1))?;
    }
    check_conflicts(&options)?;
    Ok(options)
}

fn help_text() -> String {
    HELP_TEMPLATE.replace("{skipped}", &skipped_directory_text())
}

fn usage_error(message: &str) -> Options {
    Options {
        action: Action::Help,
        message: format!("{message}\n\n{}", help_text()),
        exit_code: 1,
        ..Options::default()
    }
}

/// Parses the arguments the way the original does. Help wins over every other error.
pub fn parse_args(args: &[String]) -> Options {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Options {
            action: Action::Help,
            message: help_text(),
            exit_code: 0,
            ..Options::default()
        };
    }
    parse_all(args).unwrap_or_else(|message| usage_error(&message))
}

/// git status failure: git's own exit code and its message.
#[derive(Debug)]
struct GitStatusError {
    code: i32,
    message: String,
}

fn git(root: &Path, git_args: &[&str]) -> Result<Vec<u8>, GitStatusError> {
    git_program("git", root, git_args)
}

fn git_program(program: &str, root: &Path, git_args: &[&str]) -> Result<Vec<u8>, GitStatusError> {
    let output = Command::new(program)
        .arg("-C")
        .arg(root)
        .args(git_args)
        .output()
        .map_err(|err| GitStatusError {
            code: 1,
            message: err.to_string(),
        })?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if stderr.is_empty() {
        "git status failed".to_string()
    } else {
        stderr
    };
    Err(GitStatusError {
        code: output.status.code().unwrap_or(1),
        message,
    })
}

fn toplevel_of(root: &Path) -> Result<PathBuf, GitStatusError> {
    let output = git(root, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(String::from_utf8_lossy(&output).trim()))
}

/// Added and modified files under `root`. A deletion is left out; a rename is the new file only.
fn changed_files(root: &Path) -> Result<Vec<PathBuf>, GitStatusError> {
    let toplevel = toplevel_of(root)?;
    let status = git(
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
    )?;
    let root_resolved = resolve(root);
    Ok(status
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.get(3..))
        .map(|path| resolve(&toplevel.join(&*String::from_utf8_lossy(path))))
        .filter(|path| path.is_file() && path.starts_with(&root_resolved))
        .collect())
}

fn in_skipped_dir(path: &Path, root: &Path, skip: &HashSet<&str>) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|name| skip.contains(name))
        })
    })
}

fn is_source(path: &Path) -> bool {
    language_of(path).is_some() && !is_test_file(path)
}

fn changed_sources(root: &Path) -> Result<Vec<PathBuf>, GitStatusError> {
    let skip: HashSet<&str> = SKIP_DIRS.iter().chain(TEST_DIRS.iter()).copied().collect();
    let root_resolved = resolve(root);
    Ok(changed_files(root)?
        .into_iter()
        .filter(|path| !in_skipped_dir(path, &root_resolved, &skip) && is_source(path))
        .collect())
}

/// Splits positional arguments into existing paths and text filters.
fn positionals<'a>(root: &Path, args: &'a [String]) -> (Vec<PathBuf>, Vec<&'a str>) {
    let mut existing = Vec::new();
    let mut filters = Vec::new();
    for arg in args {
        let candidate = root.join(arg);
        if candidate.exists() {
            existing.push(resolve(&candidate));
        } else {
            filters.push(arg.as_str());
        }
    }
    (existing, filters)
}

fn explicit_sources(existing: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in existing {
        if path.is_dir() {
            files.extend(iter_source_files(std::slice::from_ref(path)));
        } else if is_source(path) {
            files.push(path.clone());
        } else if is_test_file(path) {
            eprintln!("Skipping test file {}", path.display());
        }
    }
    files
}

fn choose_files(
    options: &Options,
    root: &Path,
    existing: &[PathBuf],
) -> Result<Vec<PathBuf>, GitStatusError> {
    if options.changed {
        return changed_sources(root);
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
        return Ok(explicit_sources(existing));
    }
    Ok(iter_source_files(&[root.to_path_buf()]))
}

fn matching(files: Vec<PathBuf>, filters: &[&str]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|path| {
            let text = path.to_string_lossy();
            filters.iter().any(|item| text.contains(*item))
        })
        .collect()
}

fn inside_root(files: Vec<PathBuf>, root: &Path) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter_map(|path| {
            let resolved = resolve(&path);
            if resolved.starts_with(root) {
                Some(resolved)
            } else {
                eprintln!(
                    "Skipping {}; it is outside {}",
                    path.display(),
                    root.display()
                );
                None
            }
        })
        .collect()
}

fn select_files(options: &Options, root: &Path) -> Result<Vec<PathBuf>, GitStatusError> {
    let (existing, filters) = positionals(root, &options.positionals);
    let files = choose_files(options, root, &existing)?;
    let mut selected = inside_root(matching(files, &filters), root);
    selected.sort_by_key(|path| path.to_string_lossy().into_owned());
    selected.dedup();
    Ok(selected)
}

fn run_coverage_command(options: &Options, root: &Path, command: &str) -> i32 {
    let runner = CommandRunner {
        verbose: options.verbose,
    };
    let result = runner.run(&RunCommand::Shell(command.to_string()), root, None);
    if let Some(report) = coverage_failure_report(result.code, &result.output) {
        eprint!("{report}");
    }
    result.code
}

/// What a failed coverage command prints: its exit code and the last 20 lines of its output.
fn coverage_failure_report(code: i32, output: &str) -> Option<String> {
    if code == 0 {
        return None;
    }
    let lines = splitlines(output);
    let tail = lines[lines.len().saturating_sub(20)..].join("\n");
    let mut report =
        format!("Coverage command exited {code}. Reports already on disk will not be read.\n");
    if !tail.is_empty() {
        report.push_str(&tail);
        report.push('\n');
    }
    Some(report)
}

fn prepare_coverage(options: &Options, root: &Path, files: &[PathBuf]) -> i32 {
    if options.no_coverage || options.use_existing_coverage || options.scan {
        return 0;
    }
    match &options.coverage_command {
        Some(command) => run_coverage_command(options, root, command),
        None => cover_run::run_coverage(root, files, None),
    }
}

fn coverage_for(options: &Options, root: &Path, path: &Path) -> Option<LineSet> {
    if options.no_coverage {
        return None;
    }
    covered_lines(root, path, language_of(path)?)
}

fn print_help(options: &Options) -> i32 {
    let ending = if options.message.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    if options.exit_code == 0 {
        print!("{}{ending}", options.message);
    } else {
        eprint!("{}{ending}", options.message);
    }
    options.exit_code
}

fn restore(root: &Path) {
    for path in restore_backups(root) {
        eprintln!("Restored {} from an interrupted mutation.", path.display());
    }
}

fn scan(options: &Options, root: &Path, files: &[PathBuf]) -> i32 {
    for path in files {
        let covered = coverage_for(options, root, path);
        print!(
            "{}",
            scan_file(
                path,
                root,
                covered.as_ref(),
                options.no_coverage,
                options.lines.as_ref(),
            )
        );
    }
    0
}

/// Results gathered across all files of one run.
#[derive(Default)]
struct Tally {
    forms: Vec<FormResult>,
    written: Vec<String>,
    baseline_failed: bool,
    survived: bool,
}

impl Tally {
    fn record(&mut self, result: RunResult) {
        if !result.skipped.is_empty() {
            eprintln!("Skipping {}: {}", result.path, result.skipped);
            return;
        }
        if result.baseline_failed {
            eprintln!("{}", result.baseline_message);
            self.baseline_failed = true;
            return;
        }
        print!("{}", format_site_log(&result.sites, &result.statuses));
        self.survived |= result.forms.iter().any(|form| form.survived > 0);
        self.forms.extend(result.forms);
        self.written.extend(result.written);
    }

    fn report(&self) {
        if !self.forms.is_empty() {
            print!("{}", format_results(&self.forms));
        }
        for path in &self.written {
            eprintln!("Wrote {path}");
        }
    }
}

fn finish(baseline_failed: bool, survived: bool) -> i32 {
    if baseline_failed {
        2
    } else if survived {
        3
    } else {
        0
    }
}

fn mutate_files(options: &Options, root: &Path, files: &[PathBuf]) -> i32 {
    let runner = CommandRunner {
        verbose: options.verbose,
    };
    let mut baselines = Baselines::default();
    let mut tally = Tally::default();
    for path in files {
        let covered = coverage_for(options, root, path);
        let mutate_options = MutateOptions {
            covered_lines: covered.as_ref(),
            ignore_coverage: options.no_coverage,
            mutate_all: options.mutate_all,
            lines: options.lines.as_ref(),
            test_command: options.test_command.as_deref(),
            timeout_factor: options.timeout_factor,
            mutation_warning: options.mutation_warning,
            max_workers: options.max_workers,
            max_memory_mb: options.max_memory_mb,
        };
        tally.record(mutate_file(
            path,
            root,
            &runner,
            &mutate_options,
            &mut baselines,
        ));
    }
    tally.report();
    finish(tally.baseline_failed, tally.survived)
}

fn execute(options: &Options, root: &Path, files: &[PathBuf]) -> i32 {
    if files.is_empty() {
        println!("No source files to mutate.");
        return 0;
    }
    if !options.scan {
        restore(root);
    }
    let coverage = prepare_coverage(options, root, files);
    if coverage != 0 {
        return coverage;
    }
    if options.scan {
        scan(options, root, files)
    } else {
        mutate_files(options, root, files)
    }
}

/// Runs the command line and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let options = parse_args(args);
    if options.action == Action::Help {
        return print_help(&options);
    }
    let root = resolve(&options.project_root);
    match select_files(&options, &root) {
        Ok(files) => execute(&options, &root, &files),
        Err(err) => {
            eprintln!("{}", err.message);
            err.code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("parent dir")).expect("create dir");
        fs::write(path, text).expect("write file");
    }

    fn run_git(root: &Path, git_args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(git_args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {git_args:?} failed");
    }

    /// MU-2: no arguments give the defaults of the original.
    #[test]
    fn mu_2_defaults_match_the_original() {
        let options = parse_args(&[]);
        assert_eq!(options, Options::default());
        assert_eq!(options.action, Action::Mutate);
        assert_eq!(options.project_root, PathBuf::from("."));
        assert_eq!(options.timeout_factor, 10.0);
        assert_eq!(options.mutation_warning, 50);
        assert!(options.lines.is_none());
        assert!(options.max_workers.is_none());
    }

    /// MU-25: the memory limit is 4096 MiB by default, and the flag sets it; 0 turns it off.
    #[test]
    fn mu_25_max_memory_mb_defaults_to_4096_and_takes_a_whole_number() {
        assert_eq!(parse_args(&[]).max_memory_mb, 4096);
        for (value, expected) in [("256", 256), ("0", 0), (" 7 ", 7)] {
            let options = parse_args(&args(&["--max-memory-mb", value]));
            assert_eq!(options.exit_code, 0, "{value:?}");
            assert_eq!(options.max_memory_mb, expected, "{value:?}");
        }
    }

    /// MU-25: a value that is not a whole number of 0 or more exits 1.
    #[test]
    fn mu_25_max_memory_mb_rejects_bad_values() {
        let cases = [
            ("abc", "invalid literal for int() with base 10: 'abc'"),
            ("1.5", "invalid literal for int() with base 10: '1.5'"),
            ("", "--max-memory-mb requires a value"),
            ("-1", "--max-memory-mb requires a value"),
            (" -1", "--max-memory-mb requires a non-negative integer"),
        ];
        for (value, reason) in cases {
            let options = parse_args(&args(&["--max-memory-mb", value]));
            assert_eq!(options.exit_code, 1, "{value:?}");
            assert!(options.message.starts_with(reason), "{value:?}");
        }
        assert_eq!(parse_args(&args(&["--max-memory-mb"])).exit_code, 1);
    }

    /// MU-2: the lines option parses positive numbers and drops duplicates.
    #[test]
    fn mu_2_lines_option_parses_positive_numbers() {
        let spaced = parse_args(&args(&["--lines", "2, 4", "src/demo.py"]));
        assert_eq!(spaced.lines, Some(LineSet::from([2, 4])));
        let duplicated = parse_args(&args(&["--lines", "3,5,3"]));
        assert_eq!(duplicated.lines, Some(LineSet::from([3, 5])));
    }

    /// MU-2: each option that takes a value stores that value.
    #[test]
    fn mu_2_value_options_store_their_values() {
        let options = parse_args(&args(&[
            "-s",
            "src",
            "--source-root",
            "lib",
            "--root",
            "/work",
            "--coverage-command",
            "make cov",
            "--test-command",
            "make test",
            "--timeout-factor",
            "2.5",
            "--mutation-warning",
            "7",
            "--max-workers",
            "3",
        ]));
        assert_eq!(options.exit_code, 0);
        assert_eq!(options.source_roots, vec!["src", "lib"]);
        assert_eq!(options.project_root, PathBuf::from("/work"));
        assert_eq!(options.coverage_command.as_deref(), Some("make cov"));
        assert_eq!(options.test_command.as_deref(), Some("make test"));
        assert_eq!(options.timeout_factor, 2.5);
        assert_eq!(options.mutation_warning, 7);
        assert_eq!(options.max_workers, Some(3));
    }

    /// MU-2: each switch sets its own field, and `--reuse-coverage` is an alias.
    #[test]
    fn mu_2_switches_set_their_fields() {
        let cases: &[(&str, Predicate)] = &[
            ("--changed", |o| o.changed),
            ("--scan", |o| o.scan),
            ("--mutate-all", |o| o.mutate_all),
            ("--since-last-run", |o| o.since_last_run),
            ("--no-coverage", |o| o.no_coverage),
            ("--use-existing-coverage", |o| o.use_existing_coverage),
            ("--reuse-coverage", |o| o.use_existing_coverage),
            ("--verbose", |o| o.verbose),
        ];
        for (flag, field) in cases {
            let options = parse_args(&args(&[*flag, "src/demo.py"]));
            assert!(field(&options), "{flag} should set its field");
            assert_eq!(options.exit_code, 0, "{flag}");
        }
    }

    /// MU-2: each flag pair the original rejects exits 1 with the reason and the help text.
    #[test]
    fn mu_2_rejected_combinations_exit_one_with_help() {
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (
                vec!["--mutate-all", "--since-last-run"],
                "--mutate-all cannot be combined with --since-last-run",
            ),
            (
                vec!["--no-coverage", "--coverage-command", "make cov"],
                "--no-coverage cannot be combined with --coverage-command",
            ),
            (
                vec!["--scan", "--mutate-all"],
                "--scan cannot be combined with --mutate-all",
            ),
            (
                vec!["--scan", "--max-workers", "2"],
                "--scan cannot be combined with --max-workers",
            ),
        ];
        let help = help_text();
        for (items, reason) in cases {
            let options = parse_args(&args(&items));
            assert_eq!(options.action, Action::Help, "{items:?}");
            assert_eq!(options.exit_code, 1, "{items:?}");
            assert!(options.message.starts_with(reason), "{items:?}");
            assert!(options.message.ends_with(help.as_str()), "{items:?}");
        }
    }

    /// MU-2: a value option without a usable value exits 1 and names the option.
    #[test]
    fn mu_2_missing_values_are_rejected() {
        let cases: Vec<Vec<&str>> = vec![
            vec!["--root"],
            vec!["--root", "--scan"],
            vec!["--root", ""],
            vec!["-s"],
            vec!["--coverage-command"],
            vec!["--test-command", "--verbose"],
            vec!["--lines"],
            vec!["--timeout-factor", "-1"],
            vec!["--max-workers", "-1"],
            vec!["--mutation-warning", "-3"],
        ];
        for items in cases {
            let options = parse_args(&args(&items));
            assert_eq!(options.exit_code, 1, "{items:?}");
            let expected = format!("{} requires a value", items[0]);
            assert!(options.message.starts_with(expected.as_str()), "{items:?}");
        }
    }

    /// MU-2: bad numbers, zero, and negative values are rejected with the wording of Python.
    #[test]
    fn mu_2_bad_numbers_are_rejected_with_python_wording() {
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (
                vec!["--timeout-factor", "abc"],
                "could not convert string to float: 'abc'",
            ),
            (
                vec!["--timeout-factor", "0"],
                "--timeout-factor requires a positive number",
            ),
            (
                vec!["--mutation-warning", "abc"],
                "invalid literal for int() with base 10: 'abc'",
            ),
            (
                vec!["--mutation-warning", " -3"],
                "--mutation-warning requires a non-negative integer",
            ),
            (
                vec!["--max-workers", "0"],
                "--max-workers requires a positive integer",
            ),
            (
                vec!["--max-workers", "abc"],
                "--max-workers requires a positive integer",
            ),
            (
                vec!["--lines", "0"],
                "--lines requires positive line numbers",
            ),
            (
                vec!["--lines", "3,-1"],
                "--lines requires positive line numbers",
            ),
            (vec!["--lines", ",,"], "--lines requires a line number"),
            (
                vec!["--lines", "x"],
                "invalid literal for int() with base 10: 'x'",
            ),
            (vec!["-x"], "Unknown option: -x"),
        ];
        for (items, reason) in cases {
            let options = parse_args(&args(&items));
            assert_eq!(options.exit_code, 1, "{items:?}");
            assert!(options.message.starts_with(reason), "{items:?}");
        }
    }

    /// MU-2: `-h` or `--help` anywhere wins over every other error.
    #[test]
    fn mu_2_help_flag_wins_over_other_errors() {
        for items in [
            vec!["--root", "--help"],
            vec!["--mutate-all", "--since-last-run", "-h"],
        ] {
            let options = parse_args(&args(&items));
            assert_eq!(options.action, Action::Help, "{items:?}");
            assert_eq!(options.exit_code, 0, "{items:?}");
        }
    }

    /// MU-1: the usage line names the tool `mutator`, and the skipped directory list is filled in.
    #[test]
    fn mu_1_help_names_the_tool_and_lists_skipped_directories() {
        let options = parse_args(&args(&["--help"]));
        assert_eq!(options.action, Action::Help);
        assert_eq!(options.exit_code, 0);
        assert!(options.message.starts_with("Usage: mutator "));
        assert!(options.message.contains("uml-viewer"));
        assert!(options.message.contains("__pycache__"));
        assert!(options.message.contains("testdata"));
        assert!(!options.message.contains("{skipped}"));
    }

    /// MU-3: the help text carries the exit-code table, and `--help` returns 0.
    #[test]
    fn mu_3_help_lists_the_exit_codes() {
        let message = parse_args(&args(&["--help"])).message;
        let lines = [
            "  0  every executed mutant was killed, or there was nothing to run",
            "  1  usage error",
            "  2  baseline tests failed",
            "  3  at least one mutant survived",
        ];
        for line in lines {
            assert!(message.contains(line), "{line}");
        }
        assert_eq!(run(&args(&["--help"])), 0);
    }

    /// MU-20: a failed baseline exits 2, a survivor exits 3, a clean run exits 0, and the baseline wins.
    #[test]
    fn mu_20_finish_maps_baseline_and_survivors_to_exit_codes() {
        assert_eq!(finish(false, false), 0);
        assert_eq!(finish(false, true), 3);
        assert_eq!(finish(true, false), 2);
        assert_eq!(finish(true, true), 2);
    }

    /// MU-7: `--changed` selects the modified, staged, and untracked source files in path order,
    /// and leaves out the deleted one.
    #[test]
    fn mu_7_changed_selects_modified_staged_and_untracked_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = resolve(dir.path());
        for name in ["a.py", "b.py", "c.py", "d.py"] {
            write(&root.join(name), "def f():\n    return 1\n");
        }
        run_git(&root, &["init", "-q"]);
        run_git(&root, &["add", "a.py", "d.py"]);
        run_git(
            &root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "init",
            ],
        );
        write(&root.join("a.py"), "def f():\n    return 2\n");
        run_git(&root, &["add", "b.py"]);
        fs::remove_file(root.join("d.py")).expect("remove tracked file");
        let root_text = root.to_str().expect("utf-8 path");
        let options = parse_args(&args(&["--changed", "--root", root_text]));
        let selected = select_files(&options, &root).expect("git status reads");
        assert_eq!(
            selected,
            vec![root.join("a.py"), root.join("b.py"), root.join("c.py")]
        );
    }

    /// MU-7: a directory, a named file, a filter, and `-s` pick the files they should; a test file
    /// named on the command line is skipped; a path outside the root is dropped.
    #[test]
    fn mu_7_select_files_takes_dirs_files_filters_and_source_roots() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = resolve(dir.path());
        write(
            &root.join("pkg/app.py"),
            "def place(x):\n    return x > 0\n",
        );
        write(
            &root.join("pkg/test_app.py"),
            "def test_place():\n    assert True\n",
        );
        write(&root.join("other/util.py"), "def util():\n    return 1\n");
        let outside = tempfile::tempdir().expect("temp dir");
        let outside_file = outside.path().join("outside.py");
        write(&outside_file, "def out():\n    return 1\n");
        let root_text = root.to_str().expect("utf-8 path");
        let outside_text = outside_file.to_str().expect("utf-8 path");
        let select = |rest: &[&str]| {
            let mut items = vec!["--root", root_text];
            items.extend_from_slice(rest);
            select_files(&parse_args(&args(&items)), &root).expect("selects")
        };
        assert_eq!(
            select(&["pkg/app.py", "pkg/test_app.py"]),
            vec![root.join("pkg/app.py")]
        );
        assert_eq!(select(&["other"]), vec![root.join("other/util.py")]);
        assert_eq!(select(&["util"]), vec![root.join("other/util.py")]);
        assert_eq!(select(&["pkg/app"]), vec![root.join("pkg/app.py")]);
        assert_eq!(select(&["-s", "other"]), vec![root.join("other/util.py")]);
        assert!(select(&[outside_text]).is_empty());
    }

    /// MU-3: a run with no source files prints the message and returns 0.
    #[test]
    fn mu_3_run_without_sources_returns_zero() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root_text = dir.path().to_str().expect("utf-8 path");
        assert_eq!(run(&args(&["--root", root_text])), 0);
    }

    /// MU-3: `--scan` returns 0 and writes no metrics.
    #[test]
    fn mu_3_scan_returns_zero_and_writes_no_metrics() {
        let dir = tempfile::tempdir().expect("temp dir");
        write(
            &dir.path().join("src/demo.py"),
            "def place(x):\n    return x > 0\n",
        );
        let root_text = dir.path().to_str().expect("utf-8 path");
        let code = run(&args(&[
            "--scan",
            "--no-coverage",
            "--root",
            root_text,
            "src/demo.py",
        ]));
        assert_eq!(code, 0);
        assert!(!dir.path().join(".metrics").exists());
    }

    /// MU-3: a failed coverage command returns its exit code, and the source is left unchanged.
    #[test]
    fn mu_3_failed_coverage_command_returns_its_code() {
        let dir = tempfile::tempdir().expect("temp dir");
        let app = dir.path().join("src/app.py");
        write(&app, "def place(x):\n    return x > 0\n");
        let root_text = dir.path().to_str().expect("utf-8 path");
        let code = run(&args(&[
            "--root",
            root_text,
            "--coverage-command",
            "exit 3",
            "--test-command",
            "false",
            "src/app.py",
        ]));
        assert_eq!(code, 3);
        let text = fs::read_to_string(&app).expect("read source");
        assert!(text.starts_with("def place"));
    }

    /// MU-3: a surviving mutant returns 3 and writes the snapshot under `.metrics/mutate`.
    #[test]
    fn mu_3_surviving_mutant_returns_three_and_writes_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        write(
            &dir.path().join("src/demo.py"),
            "def add(a, b):\n    if a > 0:\n        return a + b\n    return 0\n",
        );
        let command = "grep -q 'return a + b' src/demo.py";
        let root_text = dir.path().to_str().expect("utf-8 path");
        let code = run(&args(&[
            "--no-coverage",
            "--mutate-all",
            "--root",
            root_text,
            "--test-command",
            command,
            "src/demo.py",
        ]));
        assert_eq!(code, 3);
        let snapshot = dir.path().join(".metrics/mutate/demo.edn");
        let text = fs::read_to_string(snapshot).expect("snapshot written");
        assert!(text.contains(":id \"defn/add\""));
    }

    /// MU-2: the smallest accepted values are 1 for `--max-workers` and `--lines`.
    #[test]
    fn mu_2_one_is_the_smallest_accepted_worker_and_line() {
        assert_eq!(
            parse_args(&args(&["--max-workers", "1"])).max_workers,
            Some(1)
        );
        let lines = parse_args(&args(&["--lines", "1"]));
        assert_eq!(lines.exit_code, 0);
        assert_eq!(lines.lines, Some(LineSet::from([1])));
    }

    /// MU-2: verbose is off by default, both in `Options::default` and in a bare parse.
    #[test]
    fn mu_2_verbose_is_off_by_default() {
        assert!(!Options::default().verbose);
        assert!(!parse_args(&[]).verbose);
    }

    /// MU-7: a git that cannot start makes `--changed` fail with exit code 1.
    #[test]
    fn mu_7_git_that_cannot_start_exits_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = git_program("/nonexistent/mutator-git", dir.path(), &["status"])
            .expect_err("spawn fails");
        assert_eq!(err.code, 1);
        assert!(!err.message.is_empty());
    }

    /// MU-7: a git killed by a signal has no exit code, so the status is 1 with the default message.
    #[test]
    fn mu_7_git_killed_by_signal_exits_one() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let script = dir.path().join("fake-git");
        write(&script, "#!/bin/sh\nkill -9 $$\n");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        let err = git_program(
            script.to_str().expect("utf-8 path"),
            dir.path(),
            &["status"],
        )
        .expect_err("git is killed");
        assert_eq!(err.code, 1);
        assert_eq!(err.message, "git status failed");
    }

    /// MU-3: a coverage command that exits 0 gives 0 and reports nothing.
    #[test]
    fn mu_3_successful_coverage_command_returns_zero() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            run_coverage_command(&Options::default(), dir.path(), "true"),
            0
        );
        assert_eq!(coverage_failure_report(0, "noise"), None);
    }

    /// MU-3: a failed coverage report names the exit code and keeps only the last 20 lines.
    #[test]
    fn mu_3_coverage_failure_report_names_code_and_tail() {
        let output: String = (0..25).map(|n| format!("l{n}\n")).collect();
        let report = coverage_failure_report(3, &output).expect("failure reported");
        assert!(
            report.starts_with(
                "Coverage command exited 3. Reports already on disk will not be read.\n"
            )
        );
        assert!(report.contains("l5\n"));
        assert!(!report.contains("l4\n"));
        assert!(report.ends_with("l24\n"));
    }

    /// MU-3: a failed coverage command with no output reports only its exit code.
    #[test]
    fn mu_3_coverage_failure_without_output_has_no_tail() {
        assert_eq!(
            coverage_failure_report(2, ""),
            Some(
                "Coverage command exited 2. Reports already on disk will not be read.\n"
                    .to_string()
            )
        );
    }

    /// MU-3: `--no-coverage`, `--use-existing-coverage`, and `--scan` each skip the coverage command on their own.
    #[test]
    fn mu_3_prepare_coverage_skips_when_any_skip_flag_is_set() {
        let dir = tempfile::tempdir().expect("temp dir");
        let flags: [fn(&mut Options); 3] = [
            |o| o.no_coverage = true,
            |o| o.use_existing_coverage = true,
            |o| o.scan = true,
        ];
        for set in flags {
            let mut options = Options {
                coverage_command: Some("exit 3".to_string()),
                ..Options::default()
            };
            set(&mut options);
            assert_eq!(prepare_coverage(&options, dir.path(), &[]), 0);
        }
    }
}
