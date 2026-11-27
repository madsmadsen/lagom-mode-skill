// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! Port of src/mutator/workers.py.
//!
//! Each worker is a directory under target/mutation-workers. Project files are linked in. The file
//! under test is a private copy, so workers can mutate it at the same time without touching the
//! original tree. The overlay is removed when the file's mutants finish.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::os::unix::fs as unix_fs;
use std::panic::resume_unwind;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use regex::Regex;

use crate::model::{Site, Status};
use crate::runner::{Command, CommandResult, CommandRunner};
use crate::sites::apply_site;

// Build output and the worker tree itself must not be shared. A link to `target` would point a
// worker at the directory that contains the worker.
const SKIP_LINK: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".idea",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    ".metrics",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".clj-kondo",
    ".gradle",
];

// Playwright writes its results and reports into these directories by default. A worker must not
// link them, or the workers share the output of the project. A project-specific output name is not
// listed here.
const OUTPUT_DIRS: &[&str] = &["test-results", "playwright-report"];

const CONFIGS: &[&str] = &[
    "deps.edn",
    "bb.edn",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "go.mod",
    "go.sum",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "tsconfig.json",
    "Cargo.toml",
    "Cargo.lock",
    "pyproject.toml",
    "pytest.ini",
    "setup.cfg",
    "setup.py",
    "conftest.py",
    "requirements.txt",
    "Pipfile",
    "poetry.lock",
];

const EXTENSIONS: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];

/// A run that takes this share of its timeout is repeated (MU-21).
const NEAR_TIMEOUT: f64 = 0.9;

/// Node, and Vite in front of it, resolve a relative import from the real path of the module.
/// A symlinked test therefore loads the original source.
/// A lone '.' or '..' names the index file of a directory (MU-22, issue 29 of the original).
static SPECIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:from|import|require)\s*\(?\s*['"](\.[^'"]*)['"]"#)
        .expect("specifier pattern is valid")
});

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The mutants of one file, shared by the worker threads of one run.
struct Plan<'a> {
    root: &'a Path,
    relative: &'a str,
    text: &'a str,
    runner: &'a CommandRunner,
    command: &'a Command,
    cwd: &'a Path,
    timeout: Duration,
    max_memory: Option<u64>,
    file_key: &'a str,
}

struct Shared<'a> {
    plan: Plan<'a>,
    queue: Mutex<VecDeque<&'a Site>>,
    results: Mutex<BTreeMap<String, Status>>,
}

/// Deletes the run directory when dropped, also when a worker thread panics.
struct RunDir(PathBuf);

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = delete_tree(&self.0);
    }
}

fn processors() -> usize {
    thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or_default()
}

/// Half the cores, at least one. No run uses more workers than this.
fn worker_cap() -> usize {
    cap_for(processors())
}

fn cap_for(cores: usize) -> usize {
    (cores / 2).max(1)
}

/// The smaller of the sites, half the cores, and an explicit --max-workers.
pub fn worker_count(site_count: usize, requested: Option<usize>) -> usize {
    let cap = worker_cap();
    let limit = requested.unwrap_or(cap);
    site_count.min(cap).min(limit).max(1)
}

/// Creates a fresh `target/mutation-workers/run-<unique>` directory under `root`.
pub fn new_run_dir(root: &Path) -> io::Result<PathBuf> {
    let parent = root.join("target").join("mutation-workers");
    fs::create_dir_all(&parent)?;
    let directory = parent.join(format!("run-{}", unique_suffix()));
    fs::create_dir(&directory)?;
    Ok(directory)
}

fn unique_suffix() -> String {
    let counter = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{counter}", std::process::id())
}

/// Removes a tree. A link is removed itself and its target is never followed.
pub fn delete_tree(path: &Path) -> io::Result<()> {
    if path.is_symlink() {
        return fs::remove_file(path);
    }
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            delete_tree(&entry?.path())?;
        }
        return fs::remove_dir(path);
    }
    fs::remove_file(path)
}

fn link_to(link: &Path, target: &Path) -> io::Result<()> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)?;
    }
    unix_fs::symlink(target.canonicalize()?, link)
}

fn in_skip_list(name: &OsStr, extra: &[&str]) -> bool {
    SKIP_LINK
        .iter()
        .chain(extra)
        .any(|skipped| OsStr::new(skipped) == name)
}

fn copy_configs(worker: &Path, root: &Path, relative: &str) -> io::Result<()> {
    for name in CONFIGS {
        let source = root.join(name);
        if *name != relative && source.is_file() {
            fs::write(worker.join(name), fs::read(&source)?)?;
        }
    }
    Ok(())
}

fn link_children(worker_dir: &Path, real_dir: &Path, skip: &[&str]) -> io::Result<()> {
    if !real_dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(real_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let destination = worker_dir.join(&name);
        if in_skip_list(&name, skip) || destination.is_symlink() || destination.exists() {
            continue;
        }
        link_to(&destination, &entry.path())?;
    }
    Ok(())
}

fn expand_directory(link: &Path, real_dir: &Path) -> io::Result<()> {
    if !real_dir.is_dir() {
        return Ok(());
    }
    fs::remove_file(link)?;
    fs::create_dir(link)?;
    link_children(link, real_dir, &[])
}

fn prepare_dir(current: &Path, real: &Path) -> io::Result<()> {
    if current.is_symlink() {
        expand_directory(current, real)
    } else if !current.exists() {
        fs::create_dir_all(current)
    } else {
        Ok(())
    }
}

fn specifiers(text: &str) -> impl Iterator<Item = &str> {
    SPECIFIER
        .captures_iter(text)
        .filter_map(|captures| captures.get(1))
        .map(|group| group.as_str().split(['?', '#']).next().unwrap_or_default())
}

fn is_source(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str).is_some_and(|ext| {
        EXTENSIONS
            .iter()
            .any(|known| known.strip_prefix('.') == Some(ext))
    })
}

fn stem_of(base: &Path) -> PathBuf {
    if is_source(base) {
        base.with_extension("")
    } else {
        base.to_path_buf()
    }
}

fn with_suffix(stem: &Path, ext: &str) -> PathBuf {
    let mut name = stem.as_os_str().to_os_string();
    name.push(ext);
    PathBuf::from(name)
}

fn candidates(directory: &Path, specifier: &str) -> Vec<PathBuf> {
    let base = directory.join(specifier);
    let stem = stem_of(&base);
    let mut names = vec![base];
    names.extend(EXTENSIONS.iter().map(|ext| with_suffix(&stem, ext)));
    names.extend(
        EXTENSIONS
            .iter()
            .map(|ext| stem.join(format!("index{ext}"))),
    );
    names
}

/// Files that `importer` reaches through a relative import, as canonical paths under `root`.
fn imported_files(importer: &Path, root: &Path) -> BTreeSet<PathBuf> {
    let (Ok(text), Some(directory)) = (fs::read_to_string(importer), importer.parent()) else {
        return BTreeSet::new();
    };
    specifiers(&text)
        .flat_map(|specifier| candidates(directory, specifier))
        .filter_map(|candidate| candidate.canonicalize().ok())
        .filter(|path| path.starts_with(root) && path.is_file())
        .collect()
}

fn keep_dir(name: &OsStr) -> bool {
    !in_skip_list(name, &["node_modules"])
}

/// Files under `root` that `wanted` accepts. A directory is entered only when `keep` accepts it.
fn files_under(root: &Path, keep: fn(&OsStr) -> bool, wanted: fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if keep(&entry.file_name()) {
                    pending.push(path);
                }
            } else if wanted(&path) {
                found.push(path);
            }
        }
    }
    found
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    files_under(root, keep_dir, is_source)
}

/// The HTML walk skips the output directories of MU-28 too.
fn keep_html_dir(name: &OsStr) -> bool {
    keep_dir(name) && !OUTPUT_DIRS.iter().any(|dir| OsStr::new(dir) == name)
}

fn is_html(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("html")
}

/// Relative posix paths of the HTML files of the project (MU-29).
fn html_files(root: &Path) -> BTreeSet<String> {
    files_under(root, keep_html_dir, is_html)
        .iter()
        .filter_map(|path| relative_posix(path, root).ok())
        .collect()
}

/// Maps every imported file to the set of modules that import it.
fn reverse_graph(root: &Path) -> io::Result<BTreeMap<PathBuf, BTreeSet<PathBuf>>> {
    let mut reverse: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    for file in source_files(root) {
        let source = file.canonicalize()?;
        for imported in imported_files(&source, root) {
            reverse.entry(imported).or_default().insert(source.clone());
        }
    }
    Ok(reverse)
}

fn posix(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn relative_posix(path: &Path, root: &Path) -> io::Result<String> {
    path.strip_prefix(root).map(posix).map_err(io::Error::other)
}

/// Modules that reach `relative` through a relative import, as posix paths under `root`.
///
/// Those modules have to be real files in the worker. A symlink would send Node back to the
/// original tree.
fn importers_of(root: &Path, relative: &str) -> io::Result<BTreeSet<String>> {
    let root = root.canonicalize()?;
    let target = root.join(relative).canonicalize()?;
    let reverse = reverse_graph(&root)?;
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    let mut pending = vec![target.clone()];
    while let Some(current) = pending.pop() {
        for importer in reverse.get(&current).into_iter().flatten() {
            if *importer != target && found.insert(importer.clone()) {
                pending.push(importer.clone());
            }
        }
    }
    found
        .iter()
        .map(|path| relative_posix(path, &root))
        .collect()
}

fn copy_importer(worker: &Path, root: &Path, relative: &str) -> io::Result<()> {
    let mut real = root.to_path_buf();
    let mut current = worker.to_path_buf();
    let directories = Path::new(relative)
        .parent()
        .into_iter()
        .flat_map(Path::components);
    for segment in directories {
        real.push(segment);
        current.push(segment);
        prepare_dir(&current, &real)?;
    }
    let destination = worker.join(relative);
    if destination.is_symlink() {
        fs::remove_file(&destination)?;
    } else if destination.exists() {
        return Ok(());
    }
    fs::write(&destination, fs::read(root.join(relative))?)
}

/// Real directories along the source path. Every sibling is a link.
fn overlay(worker: &Path, root: &Path, relative: &str, original: &[u8]) -> io::Result<()> {
    let segments: Vec<&str> = relative.split('/').collect();
    for (depth, pair) in segments.windows(2).enumerate() {
        let rel_dir: PathBuf = segments[..=depth].iter().collect();
        link_children(&worker.join(&rel_dir), &root.join(&rel_dir), &[pair[1]])?;
    }
    let destination = worker.join(relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(destination, original)
}

fn build_worker(
    worker: &Path,
    root: &Path,
    relative: &str,
    original: &[u8],
    copies: &BTreeSet<String>,
) -> io::Result<()> {
    let first = relative.split('/').next().unwrap_or_default();
    fs::create_dir_all(worker)?;
    copy_configs(worker, root, relative)?;
    link_children(worker, root, &[first])?;
    overlay(worker, root, relative, original)?;
    copies
        .iter()
        .try_for_each(|copy| copy_importer(worker, root, copy))
}

/// The names of the JavaScript and TypeScript files that lie directly in `dir`. A link is not a
/// file here, so it is never copied (MU-30).
fn module_names(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let is_module = is_source(Path::new(&name)) && entry.file_type()?.is_file();
        if let Some(text) = name.to_str().filter(|_| is_module) {
            names.push(text.to_owned());
        }
    }
    Ok(names)
}

/// Files that `starts` reach through relative imports, transitively, as posix paths under `root`.
/// The starts are part of the result. A file is visited once, so an import cycle ends (MU-30).
fn import_closure(root: &Path, starts: &BTreeSet<String>) -> BTreeSet<String> {
    let Ok(root) = root.canonicalize() else {
        return starts.clone();
    };
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut pending: Vec<PathBuf> = starts
        .iter()
        .filter_map(|relative| root.join(relative).canonicalize().ok())
        .collect();
    while let Some(current) = pending.pop() {
        if seen.insert(current.clone()) {
            pending.extend(imported_files(&current, &root));
        }
    }
    seen.iter()
        .filter_map(|path| relative_posix(path, &root).ok())
        .collect()
}

/// The modules that import `relative`, the HTML files of the project, and the JavaScript and
/// TypeScript files that lie directly in its root, with every file that those reach through
/// relative imports, as posix paths. Each of them is a real file in a worker (MU-17, MU-29, MU-30).
/// The file under test is never in the set.
fn copied_files(root: &Path, relative: &str) -> io::Result<BTreeSet<String>> {
    let mut copies = importers_of(root, relative)?;
    copies.extend(html_files(root));
    let modules: BTreeSet<String> = module_names(root)?.into_iter().collect();
    copies.extend(import_closure(root, &modules));
    copies.remove(relative);
    Ok(copies)
}

/// The directory where the command runs, mapped into the worker. None unless that directory lies
/// inside the worker and is not the worker root. A directory outside the worker is the real
/// project.
fn inside_dir(worker: &Path, root: &Path, cwd: &Path) -> io::Result<Option<PathBuf>> {
    let inside = worker.canonicalize()?;
    let mapped = mapped_cwd(worker, root, cwd);
    Ok((mapped != inside && mapped.starts_with(&inside)).then_some(mapped))
}

/// Copies the JavaScript and TypeScript files that lie directly in the directory where the command
/// runs, and the files that those reach through relative imports, when that directory lies inside
/// the worker (MU-30). The file under test is not copied: the overlay has written it, and
/// `copy_importer` keeps a real file.
fn copy_command_modules(worker: &Path, root: &Path, cwd: &Path) -> io::Result<()> {
    if inside_dir(worker, root, cwd)?.is_none() {
        return Ok(());
    }
    let real = cwd.canonicalize()?;
    let dir = relative_posix(&real, &root.canonicalize()?)?;
    let modules: BTreeSet<String> = module_names(&real)?
        .iter()
        .map(|name| format!("{dir}/{name}"))
        .collect();
    import_closure(root, &modules)
        .iter()
        .try_for_each(|copy| copy_importer(worker, root, copy))
}

/// Builds `count` worker overlays under `base`, each holding a private copy of `relative`.
pub fn create_workers(
    base: &Path,
    root: &Path,
    relative: &str,
    original: &[u8],
    count: usize,
) -> io::Result<Vec<PathBuf>> {
    let copies = copied_files(root, relative)?;
    (0..count)
        .map(|index| -> io::Result<PathBuf> {
            let worker = base.join(format!("worker-{index}"));
            build_worker(&worker, root, relative, original, &copies)?;
            Ok(worker)
        })
        .collect()
}

/// Maps `cwd` from the project root into the worker. Outside the root, the worker is used.
pub fn mapped_cwd(worker: &Path, root: &Path, cwd: &Path) -> PathBuf {
    let Some(relative) = cwd
        .canonicalize()
        .ok()
        .zip(root.canonicalize().ok())
        .and_then(|(cwd, root)| cwd.strip_prefix(root).ok().map(Path::to_path_buf))
    else {
        return worker.to_path_buf();
    };
    let joined = worker.join(relative);
    joined.canonicalize().unwrap_or(joined)
}

/// Whether the mutant's run is near enough to its timeout that it must run again (MU-21).
/// A run that the memory limit killed is not run again (MU-25).
pub fn needs_second_run(first: &CommandResult, timeout: Duration) -> bool {
    !first.memory_killed
        && (first.timed_out || first.seconds >= NEAR_TIMEOUT * timeout.as_secs_f64())
}

/// A timed-out or failing run kills the mutant. A passing run lets it survive.
pub fn outcome_of(result: &CommandResult) -> Status {
    if result.timed_out || result.code != 0 {
        Status::Killed
    } else {
        Status::Survived
    }
}

/// Runs the command in `cwd`. A run near its timeout runs again with twice the timeout (MU-21).
/// Returns the final result and the timeout that the final run used.
fn run_checked(
    runner: &CommandRunner,
    command: &Command,
    cwd: &Path,
    limits: Limits,
) -> (CommandResult, Duration) {
    let first = runner.run_limited(command, cwd, Some(limits.timeout), limits.max_memory);
    if !needs_second_run(&first, limits.timeout) {
        return (first, limits.timeout);
    }
    let timeout = limits.timeout * 2;
    let second = runner.run_limited(command, cwd, Some(timeout), limits.max_memory);
    (second, timeout)
}

/// Writes one mutant into the worker, runs the command, and restores the original text.
fn run_mutant(directory: &Path, plan: &Plan<'_>, mutated: &str) -> io::Result<CommandResult> {
    let destination = directory.join(plan.relative);
    fs::write(&destination, mutated)?;
    let cwd = mapped_cwd(directory, plan.root, plan.cwd);
    let limits = Limits {
        timeout: plan.timeout,
        max_memory: plan.max_memory,
    };
    let (result, _) = run_checked(plan.runner, plan.command, &cwd, limits);
    fs::write(&destination, plan.text)?;
    Ok(result)
}

fn execute(directory: &Path, site: &Site, plan: &Plan<'_>) -> io::Result<Status> {
    if plan.text.get(site.start..site.end) != Some(site.original.as_str()) {
        eprintln!(
            "Skipped {}:{} {}; source bytes moved",
            plan.file_key,
            site.line,
            site.description()
        );
        return Ok(Status::Survived);
    }
    let mutated = apply_site(plan.text, site.start, site.end, &site.mutant);
    let result = run_mutant(directory, plan, &mutated)?;
    Ok(outcome_of(&result))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn next_site<'a>(queue: &Mutex<VecDeque<&'a Site>>) -> Option<&'a Site> {
    lock(queue).pop_front()
}

fn report(file_key: &str, site: &Site) {
    eprintln!("{file_key}:{} {}", site.line, site.description());
}

fn drain(directory: &Path, shared: &Shared<'_>) {
    while let Some(site) = next_site(&shared.queue) {
        if shared.plan.runner.verbose {
            report(shared.plan.file_key, site);
        }
        let status = execute(directory, site, &shared.plan)
            .unwrap_or_else(|error| panic!("mutant {} failed: {error}", site.mutation_id));
        lock(&shared.results).insert(site.mutation_id.clone(), status);
    }
}

fn or_fail<T>(result: io::Result<T>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("mutation workers: {what} failed: {error}"))
}

fn relative_key(root: &Path, source: &Path) -> io::Result<String> {
    relative_posix(&source.canonicalize()?, &root.canonicalize()?)
}

/// What ends a mutant's test run: its time and, when set, the memory of its process group in bytes.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub timeout: Duration,
    pub max_memory: Option<u64>,
}

/// The unmutated command failed in a worker (MU-27).
#[derive(Debug)]
pub struct WorkerFailure {
    /// The lowest failing worker index.
    pub worker: usize,
    /// `exit code <c>`, `timed out after <s> seconds`, or `memory limit of <m> MiB`.
    pub reason: String,
    /// The full output of the failing run.
    pub output: String,
    /// True only when more than one worker ran and worker 0 passes when run alone after the failure.
    pub passes_alone: bool,
}

/// Whether a run fails the baseline: it timed out, was killed for memory, or exited non-zero.
fn failed(result: &CommandResult) -> bool {
    result.timed_out || result.memory_killed || result.code != 0
}

/// The reason for a failed run. A timeout wins over a memory kill, which wins over the exit code.
fn reason_of(result: &CommandResult, timeout: Duration, limits: Limits) -> String {
    if result.timed_out {
        format!("timed out after {} seconds", timeout.as_secs())
    } else if result.memory_killed {
        format!("memory limit of {} MiB", memory_mib(limits.max_memory))
    } else {
        format!("exit code {}", result.code)
    }
}

fn memory_mib(max_memory: Option<u64>) -> u64 {
    max_memory.unwrap_or_default() / (1024 * 1024)
}

/// Runs the unmutated command in every worker at the same time, one thread per worker.
fn run_all(
    directories: &[PathBuf],
    root: &Path,
    cwd: &Path,
    runner: &CommandRunner,
    command: &Command,
    limits: Limits,
) -> Vec<(CommandResult, Duration)> {
    thread::scope(|scope| {
        let handles: Vec<_> = directories
            .iter()
            .map(|directory| {
                let worker_cwd = mapped_cwd(directory, root, cwd);
                scope.spawn(move || run_checked(runner, command, &worker_cwd, limits))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_else(|panic| resume_unwind(panic)))
            .collect()
    })
}

/// Whether worker 0 passes when it runs alone after a failure. False when only one worker ran.
fn passes_alone(
    directories: &[PathBuf],
    root: &Path,
    cwd: &Path,
    runner: &CommandRunner,
    command: &Command,
    limits: Limits,
) -> bool {
    match directories {
        [first, _, ..] => {
            let worker_cwd = mapped_cwd(first, root, cwd);
            let (result, _) = run_checked(runner, command, &worker_cwd, limits);
            !failed(&result)
        }
        _ => false,
    }
}

/// Runs the unmutated command in each worker at the same time (MU-27, AC-21). Returns the failure
/// of the lowest failing worker, or None when every worker passes.
pub fn check_workers(
    directories: &[PathBuf],
    root: &Path,
    cwd: &Path,
    runner: &CommandRunner,
    command: &Command,
    limits: Limits,
) -> Option<WorkerFailure> {
    let runs = run_all(directories, root, cwd, runner, command, limits);
    let (worker, (result, timeout)) = runs
        .iter()
        .enumerate()
        .find(|(_, (result, _))| failed(result))?;
    Some(WorkerFailure {
        worker,
        reason: reason_of(result, *timeout, limits),
        output: result.output.clone(),
        passes_alone: passes_alone(directories, root, cwd, runner, command, limits),
    })
}

/// Removes the output links of MU-28 from the worker root and from the directory where the command
/// runs. Only links are removed, and the target of a link is never touched. The command directory
/// counts only when it lies inside the worker. A directory outside the worker is the real project.
fn drop_output_links(worker: &Path, root: &Path, cwd: &Path) -> io::Result<()> {
    let mut dirs = vec![worker.to_path_buf()];
    dirs.extend(inside_dir(worker, root, cwd)?);
    for dir in &dirs {
        for name in OUTPUT_DIRS {
            let link = dir.join(name);
            if link.is_symlink() {
                fs::remove_file(link)?;
            }
        }
    }
    Ok(())
}

/// Mutates `sites` in parallel overlays of `root`. The source stays put. First the unmutated
/// command runs once in every worker (MU-27). If one of those runs fails, no mutant runs and the
/// failure is returned.
/// Before that, each worker drops its output links (MU-28) and copies the JavaScript and
/// TypeScript files of the command directory (MU-30).
#[allow(clippy::too_many_arguments)]
pub fn run_mutants(
    root: &Path,
    source: &Path,
    original: &[u8],
    sites: &[Site],
    max_workers: Option<usize>,
    runner: &CommandRunner,
    command: &Command,
    cwd: &Path,
    limits: Limits,
    file_key: &str,
    outcomes: &mut BTreeMap<String, Status>,
) -> Option<WorkerFailure> {
    if sites.is_empty() {
        return None;
    }
    let text =
        str::from_utf8(original).unwrap_or_else(|error| panic!("{file_key} is not UTF-8: {error}"));
    let run = RunDir(or_fail(new_run_dir(root), "run directory"));
    let relative = or_fail(relative_key(root, source), "source path");
    let count = worker_count(sites.len(), max_workers);
    let directories = or_fail(
        create_workers(&run.0, root, &relative, original, count),
        "worker overlays",
    );
    for directory in &directories {
        or_fail(drop_output_links(directory, root, cwd), "output links");
        or_fail(
            copy_command_modules(directory, root, cwd),
            "command modules",
        );
    }
    if let Some(failure) = check_workers(&directories, root, cwd, runner, command, limits) {
        return Some(failure);
    }
    let shared = Shared {
        plan: Plan {
            root,
            relative: &relative,
            text,
            runner,
            command,
            cwd,
            timeout: limits.timeout,
            max_memory: limits.max_memory,
            file_key,
        },
        queue: Mutex::new(sites.iter().collect()),
        results: Mutex::new(BTreeMap::new()),
    };
    thread::scope(|scope| {
        for directory in &directories {
            let shared = &shared;
            scope.spawn(move || drain(directory, shared));
        }
    });
    outcomes.extend(
        shared
            .results
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner),
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// A real copy of `relative` in the worker. Its canonical path lies inside the worker. A link
    /// of a directory on the way would resolve it to the project, so the check does not stop at the
    /// last component.
    fn assert_real_copy(worker: &Path, root: &Path, relative: &str) {
        let copied = worker.join(relative);
        let inside = worker.canonicalize().unwrap().join(relative);
        assert_eq!(copied.canonicalize().unwrap(), inside, "{relative}");
        assert_eq!(
            fs::read(&copied).unwrap(),
            fs::read(root.join(relative)).unwrap()
        );
    }

    fn site(start: usize, end: usize, original: &str, mutant: &str, id: &str) -> Site {
        Site {
            file: "src/x.txt".into(),
            namespace: "demo".into(),
            form_id: "defn/x".into(),
            name: "x".into(),
            line: 1,
            start,
            end,
            original: original.into(),
            mutant: mutant.into(),
            category: "test".into(),
            mutation_id: id.into(),
            anchor_line: 1,
        }
    }

    fn result(code: i32, timed_out: bool) -> CommandResult {
        CommandResult {
            code,
            timed_out,
            memory_killed: false,
            seconds: 0.0,
            output: String::new(),
        }
    }

    // MU-17: worker count is the minimum of sites, half the cores, and the requested cap.
    #[test]
    fn mu_17_worker_count_follows_sites_half_the_cores_and_the_requested_cap() {
        let cap = (processors() / 2).max(1);
        assert_eq!(worker_cap(), cap);
        assert_eq!(worker_count(1000, None), cap);
        assert_eq!(worker_count(1000, Some(10_000)), cap);
        assert_eq!(worker_count(10, Some(1)), 1);
        assert_eq!(worker_count(2, Some(8)), cap.min(2));
        assert_eq!(worker_count(0, None), 1);
    }

    // MU-17: half the cores, and at least one worker on a host with 0, 1, or 2 cores.
    #[test]
    fn mu_17_cap_for_is_half_the_cores_and_at_least_one() {
        let caps: Vec<usize> = [0, 1, 2, 3, 4, 9]
            .iter()
            .map(|cores| cap_for(*cores))
            .collect();
        assert_eq!(caps, vec![1, 1, 1, 1, 2, 4]);
    }

    // MU-17: workers keep a private copy of the file and link the rest of the project.
    #[test]
    fn mu_17_workers_keep_a_private_copy_and_link_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/demo.py", "def place(x):\n    return x > 0\n");
        put(root, "src/other.py", "kept = True\n");
        put(root, "tests/keep.txt", "keep");
        put(root, "pyproject.toml", "[project]\nname = 'demo'\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let workers = create_workers(&base, root, "src/demo.py", &original, 2).unwrap();
        assert_eq!(workers.len(), 2);
        for worker in &workers {
            let private = worker.join("src/demo.py");
            assert!(private.is_file() && !private.is_symlink());
            assert_eq!(fs::read(&private).unwrap(), original);
            assert!(worker.join("src/other.py").is_symlink());
            assert!(worker.join("tests").is_symlink());
            assert_eq!(
                fs::read_to_string(worker.join("tests/keep.txt")).unwrap(),
                "keep"
            );
            let config = worker.join("pyproject.toml");
            assert!(config.is_file() && !config.is_symlink());
        }
        fs::write(workers[0].join("src/demo.py"), "changed\n").unwrap();
        assert_eq!(fs::read(workers[1].join("src/demo.py")).unwrap(), original);

        delete_tree(&base).unwrap();
        assert_eq!(fs::read(root.join("src/demo.py")).unwrap(), original);
        assert_eq!(
            fs::read_to_string(root.join("src/other.py")).unwrap(),
            "kept = True\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("tests/keep.txt")).unwrap(),
            "keep"
        );
        assert!(!base.exists());
    }

    // MU-17: config files are copied, except the file under test, which is overlaid.
    #[test]
    fn mu_17_config_files_are_copied_and_the_source_config_is_overlaid() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "Cargo.toml", "[package]\n");
        put(root, "setup.py", "print('real')\n");
        let original = fs::read(root.join("setup.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "setup.py", &original, 1).unwrap()[0].clone();
        let copied = worker.join("Cargo.toml");
        assert!(copied.is_file() && !copied.is_symlink());
        assert_eq!(fs::read_to_string(copied).unwrap(), "[package]\n");
        let overlaid = worker.join("setup.py");
        assert!(overlaid.is_file() && !overlaid.is_symlink());
        assert_eq!(fs::read(overlaid).unwrap(), original);
        delete_tree(&base).unwrap();
    }

    // MU-17: a module reached by a relative import is copied, not linked, so Node loads the copy.
    #[test]
    fn mu_17_relative_imports_are_copied_not_linked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/find.mjs", "export const value = \"real\"\n");
        put(
            root,
            "src/find.test.mjs",
            "import { value } from \"./find.mjs\";\n",
        );
        put(root, "src/notes.mjs", "export const notes = 1\n");
        put(
            root,
            "src/main.mjs",
            "import { value } from \"./find.mjs\";\n",
        );
        put(
            root,
            "src/app.test.mjs",
            "import { main } from \"./main.mjs\";\n",
        );
        put(
            root,
            "tests/use.mjs",
            "import { value } from \"../src/find.mjs\";\n",
        );
        put(root, "tests/other.test.mjs", "export const n = 1\n");
        let source = root.join("src/find.mjs");
        let original = fs::read(&source).unwrap();

        let expected: BTreeSet<String> = [
            "src/app.test.mjs",
            "src/find.test.mjs",
            "src/main.mjs",
            "tests/use.mjs",
        ]
        .iter()
        .map(|path| path.to_string())
        .collect();
        assert_eq!(importers_of(root, "src/find.mjs").unwrap(), expected);

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/find.mjs", &original, 1).unwrap()[0].clone();
        for relative in expected {
            let copied = worker.join(&relative);
            assert!(copied.is_file() && !copied.is_symlink(), "{relative}");
        }
        assert!((worker.join("src/notes.mjs")).is_symlink());
        assert!(!worker.join("tests").is_symlink());
        assert!(worker.join("tests/other.test.mjs").is_symlink());
        delete_tree(&base).unwrap();
        assert_eq!(fs::read(source).unwrap(), original);
    }

    // MU-22: the specifiers `'.'` and `'..'` are relative imports of the `index` file.
    #[test]
    fn mu_22_dot_and_dot_dot_imports_reach_the_index_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        put(
            &root,
            "src/calc/index.ts",
            "export const big = (x) => x > 10;\n",
        );
        put(
            &root,
            "src/calc/index.test.ts",
            "import { big } from '.';\n",
        );
        put(
            &root,
            "src/calc/sub/deep.test.ts",
            "const { big } = require(\"..\");\n",
        );
        put(&root, "src/calc/other.test.ts", "import x from './none';\n");
        let expected: BTreeSet<String> = ["src/calc/index.test.ts", "src/calc/sub/deep.test.ts"]
            .iter()
            .map(|path| path.to_string())
            .collect();
        assert_eq!(importers_of(&root, "src/calc/index.ts").unwrap(), expected);
    }

    // MU-17: each run directory is new and lives under target/mutation-workers.
    #[test]
    fn mu_17_run_dirs_are_unique_and_live_under_target() {
        let dir = tempfile::tempdir().unwrap();
        let first = new_run_dir(dir.path()).unwrap();
        let second = new_run_dir(dir.path()).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            first.parent().unwrap(),
            dir.path().join("target/mutation-workers")
        );
        assert!(first.is_dir() && second.is_dir());
        delete_tree(&first).unwrap();
        delete_tree(&second).unwrap();
    }

    // MU-17: deleting a worker tree removes its links and leaves the link targets intact.
    #[test]
    fn mu_17_delete_tree_never_follows_a_link() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "real/keep.txt", "keep");
        let base = dir.path().join("base");
        fs::create_dir_all(&base).unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), base.join("link")).unwrap();

        delete_tree(&base).unwrap();
        assert!(!base.exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("real/keep.txt")).unwrap(),
            "keep"
        );
        delete_tree(&dir.path().join("missing")).unwrap();
    }

    // MU-17: each mutant is seen by the command in a worker copy, the source is untouched, and
    // the worker tree is empty afterwards.
    #[test]
    fn mu_17_mutants_run_in_worker_copies_and_the_source_stays_put() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/x.txt", "a > b\nextra\n");
        let source = root.join("src/x.txt");
        let original = fs::read(&source).unwrap();
        let sites = [
            site(2, 3, ">", "<", "killed-id"),
            site(6, 11, "extra", "x-tra", "survived-id"),
        ];
        let command = Command::Shell(r#"grep -q "a > b" src/x.txt"#.to_string());
        let mut outcomes = BTreeMap::new();

        run_mutants(
            root,
            &source,
            &original,
            &sites,
            None,
            &CommandRunner::default(),
            &command,
            root,
            Limits {
                timeout: Duration::from_secs(20),
                max_memory: None,
            },
            "src/x.txt",
            &mut outcomes,
        );

        assert_eq!(outcomes.get("killed-id"), Some(&Status::Killed));
        assert_eq!(outcomes.get("survived-id"), Some(&Status::Survived));
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_eq!(
            fs::read_dir(root.join("target/mutation-workers"))
                .unwrap()
                .count(),
            0
        );
    }

    // MU-17: a site whose source bytes moved since discovery is reported and survives.
    #[test]
    fn mu_17_a_site_whose_source_bytes_moved_survives() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/x.txt", "a > b\n");
        let source = root.join("src/x.txt");
        let original = fs::read(&source).unwrap();
        let sites = [site(2, 3, "zzz", "<", "moved-id")];
        let command = Command::Shell(r#"grep -q "a > b" src/x.txt"#.to_string());
        let mut outcomes = BTreeMap::new();

        run_mutants(
            root,
            &source,
            &original,
            &sites,
            Some(1),
            &CommandRunner::default(),
            &command,
            root,
            Limits {
                timeout: Duration::from_secs(20),
                max_memory: None,
            },
            "src/x.txt",
            &mut outcomes,
        );

        assert_eq!(outcomes.get("moved-id"), Some(&Status::Survived));
        assert_eq!(fs::read(&source).unwrap(), original);
    }

    // MU-21: a timed-out mutant gets a second run, whatever its seconds say.
    #[test]
    fn mu_21_a_timed_out_run_needs_a_second_run() {
        assert!(needs_second_run(
            &result(124, true),
            Duration::from_secs(10)
        ));
    }

    // MU-21: a run at 0.89 of its timeout is not repeated, a run at 0.9 of it is.
    #[test]
    fn mu_21_a_run_within_ten_percent_of_the_timeout_needs_a_second_run() {
        let timeout = Duration::from_secs(10);
        let took = |seconds| CommandResult {
            seconds,
            ..result(0, false)
        };
        assert!(!needs_second_run(&took(8.9), timeout));
        assert!(needs_second_run(&took(9.0), timeout));
    }

    // MU-21: a fast run that did not time out is not repeated.
    #[test]
    fn mu_21_a_fast_run_is_not_repeated() {
        let fast = CommandResult {
            seconds: 0.01,
            ..result(0, false)
        };
        assert!(!needs_second_run(&fast, Duration::from_secs(10)));
    }

    // MU-25: a run that the memory limit killed is never run again, even near its timeout.
    #[test]
    fn mu_25_a_memory_kill_needs_no_second_run() {
        let killed = CommandResult {
            memory_killed: true,
            timed_out: true,
            seconds: 99.0,
            ..result(137, false)
        };
        assert!(!needs_second_run(&killed, Duration::from_secs(10)));
        assert_eq!(outcome_of(&killed), Status::Killed);
    }

    // MU-25: a mutant over the memory limit is killed, and its test command runs once.
    #[test]
    fn mu_25_a_mutant_over_the_memory_limit_is_killed_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        put(&root, "src/x.txt", "a > b\n");
        let source = root.join("src/x.txt");
        let original = fs::read(&source).unwrap();
        let log = dir.path().join("runs.log");
        let sites = [site(2, 3, ">", "<", "big-id")];
        let command = Command::Shell(format!(
            "grep -q 'a > b' src/x.txt || {{ echo run >> {}; sleep 30; }}",
            log.display()
        ));
        let mut outcomes = BTreeMap::new();

        run_mutants(
            &root,
            &source,
            &original,
            &sites,
            Some(1),
            &CommandRunner::default(),
            &command,
            &root,
            Limits {
                timeout: Duration::from_secs(60),
                max_memory: Some(1),
            },
            "src/x.txt",
            &mut outcomes,
        );

        assert_eq!(outcomes.get("big-id"), Some(&Status::Killed));
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
    }

    // MU-21: a timeout or a nonzero exit kills the mutant, a clean exit lets it survive.
    #[test]
    fn mu_21_outcome_is_killed_on_timeout_or_nonzero_exit() {
        assert_eq!(outcome_of(&result(124, true)), Status::Killed);
        assert_eq!(outcome_of(&result(1, false)), Status::Killed);
        assert_eq!(outcome_of(&result(0, false)), Status::Survived);
    }

    // MU-17: a missing intermediate directory in the worker is created, an existing one is kept.
    #[test]
    fn mu_17_copy_importer_creates_missing_directories_and_keeps_existing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        put(&root, "deep/nested/imp.mjs", "export const i = 1\n");
        let original = fs::read(root.join("deep/nested/imp.mjs")).unwrap();

        let missing = dir.path().join("missing");
        fs::create_dir_all(&missing).unwrap();
        copy_importer(&missing, &root, "deep/nested/imp.mjs").unwrap();
        let copied = missing.join("deep/nested/imp.mjs");
        assert!(copied.is_file() && !copied.is_symlink());
        assert_eq!(fs::read(&copied).unwrap(), original);
        assert!(missing.join("deep").is_dir() && !missing.join("deep").is_symlink());

        let existing = dir.path().join("existing");
        put(&existing, "deep/nested/keep.txt", "keep");
        copy_importer(&existing, &root, "deep/nested/imp.mjs").unwrap();
        assert_eq!(
            fs::read(existing.join("deep/nested/imp.mjs")).unwrap(),
            original
        );
        assert_eq!(
            fs::read_to_string(existing.join("deep/nested/keep.txt")).unwrap(),
            "keep"
        );
    }

    // MU-17: only files with a known source extension are scanned for imports.
    #[test]
    fn mu_17_only_known_extensions_are_scanned_for_imports() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        put(&root, "src/target.mjs", "export const t = 1\n");
        put(
            &root,
            "src/main.mjs",
            "import { t } from \"./target.mjs\";\n",
        );
        put(
            &root,
            "src/notes.txt",
            "import { t } from \"./target.mjs\";\n",
        );

        let expected: BTreeSet<String> = ["src/main.mjs".to_string()].into_iter().collect();
        assert_eq!(importers_of(&root, "src/target.mjs").unwrap(), expected);
    }

    // MU-17: an import counts only when it resolves to a real file under the root.
    #[test]
    fn mu_17_an_import_counts_only_when_it_is_a_file_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let raw_root = dir.path().join("root");
        put(&raw_root, "src/real.mjs", "export const r = 1\n");
        put(&raw_root, "src/lib/.keep", "");
        put(dir.path(), "outside/x.mjs", "export const x = 1\n");
        put(
            &raw_root,
            "src/main.mjs",
            "import a from \"./real.mjs\";\nimport b from \"../../outside/x.mjs\";\nimport c from \"./lib\";\n",
        );
        let root = raw_root.canonicalize().unwrap();
        let importer = root.join("src/main.mjs");
        let expected = BTreeSet::from([root.join("src/real.mjs")]);
        assert_eq!(imported_files(&importer, &root), expected);
    }

    // MU-17: each call in one root takes a new name, and every run directory exists afterwards.
    #[test]
    fn mu_17_each_run_dir_in_one_root_gets_a_new_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let runs: Vec<PathBuf> = (0..3).map(|_| new_run_dir(dir.path()).unwrap()).collect();
        assert!(runs.iter().all(|run| run.is_dir()));
        let distinct: BTreeSet<&PathBuf> = runs.iter().collect();
        assert_eq!(distinct.len(), 3);
        for run in &runs {
            delete_tree(run).unwrap();
        }
    }

    // MU-21: a run that times out is repeated once with twice the timeout, and a run that
    // finishes within that doubled budget survives.
    #[test]
    fn mu_21_a_run_that_times_out_is_repeated_with_twice_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        put(&root, "src/x.txt", "a > b\n");
        let source = root.join("src/x.txt");
        let original = fs::read(&source).unwrap();
        let log = dir.path().join("runs.log");
        let sites = [site(2, 3, ">", "<", "slow-id")];
        let command = Command::Shell(format!(
            "grep -q 'a > b' src/x.txt || {{ echo run >> {} && sleep 1.5; }}",
            log.display()
        ));
        let mut outcomes = BTreeMap::new();

        run_mutants(
            &root,
            &source,
            &original,
            &sites,
            Some(1),
            &CommandRunner::default(),
            &command,
            &root,
            Limits {
                timeout: Duration::from_secs(1),
                max_memory: None,
            },
            "src/x.txt",
            &mut outcomes,
        );

        assert_eq!(outcomes.get("slow-id"), Some(&Status::Survived));
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
    }

    // MU-21: a run that finishes well inside its timeout is not repeated.
    #[test]
    fn mu_21_a_fast_run_is_run_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        put(&root, "src/x.txt", "a > b\n");
        let source = root.join("src/x.txt");
        let original = fs::read(&source).unwrap();
        let log = dir.path().join("runs.log");
        let sites = [site(2, 3, ">", "<", "quick-id")];
        let command = Command::Shell(format!(
            "grep -q 'a > b' src/x.txt || echo run >> {}",
            log.display()
        ));
        let mut outcomes = BTreeMap::new();

        run_mutants(
            &root,
            &source,
            &original,
            &sites,
            Some(1),
            &CommandRunner::default(),
            &command,
            &root,
            Limits {
                timeout: Duration::from_secs(20),
                max_memory: None,
            },
            "src/x.txt",
            &mut outcomes,
        );

        assert_eq!(outcomes.get("quick-id"), Some(&Status::Survived));
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
    }

    fn limits(seconds: u64) -> Limits {
        Limits {
            timeout: Duration::from_secs(seconds),
            max_memory: None,
        }
    }

    // MU-27 / AC-21: a command that fails in one worker is a baseline failure. The lowest
    // failing worker is named. The command fails alone too, so passes_alone is false.
    #[test]
    fn mu_27_ac_21_the_lowest_failing_worker_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();
        let base = new_run_dir(root).unwrap();
        let workers = create_workers(&base, root, "src/demo.py", &original, 2).unwrap();
        let script = r#"case "$(basename "$PWD")" in worker-*) exit 1;; esac"#;
        let command = Command::Argv(vec!["sh".into(), "-c".into(), script.into()]);

        let failure = check_workers(
            &workers,
            root,
            root,
            &CommandRunner::default(),
            &command,
            limits(20),
        )
        .expect("worker 0 fails");
        assert_eq!(failure.worker, 0);
        assert_eq!(failure.reason, "exit code 1");
        assert!(!failure.passes_alone);
        delete_tree(&base).unwrap();
    }

    // MU-27 / AC-21: a command that passes alone but fails when two workers run at once is a
    // baseline failure, and the failure says that the command passes alone.
    #[test]
    fn mu_27_ac_21_a_clash_between_workers_passes_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let lock = scratch.path().join("lock");
        let base = new_run_dir(root).unwrap();
        let workers = create_workers(&base, root, "src/demo.py", &original, 2).unwrap();
        let script = format!(
            "mkdir '{0}' 2>/dev/null || exit 1; sleep 1; rmdir '{0}'",
            lock.display()
        );
        let command = Command::Argv(vec!["sh".into(), "-c".into(), script]);

        let failure = check_workers(
            &workers,
            root,
            root,
            &CommandRunner::default(),
            &command,
            limits(20),
        )
        .expect("one worker loses the lock");
        assert!(failure.worker < 2);
        assert!(failure.passes_alone);
        delete_tree(&base).unwrap();
    }

    // MU-27 / AC-21: a command that passes in the only worker is no baseline failure.
    #[test]
    fn mu_27_ac_21_a_passing_worker_gives_no_failure() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();
        let base = new_run_dir(root).unwrap();
        let workers = create_workers(&base, root, "src/demo.py", &original, 1).unwrap();
        let command = Command::Argv(vec!["sh".into(), "-c".into(), "exit 0".into()]);

        assert!(
            check_workers(
                &workers,
                root,
                root,
                &CommandRunner::default(),
                &command,
                limits(20),
            )
            .is_none()
        );
        delete_tree(&base).unwrap();
    }

    // MU-27 / AC-21: a run is a failure when it times out, when the memory limit kills it, or
    // when it exits non-zero. A run that does none of these is not a failure.
    #[test]
    fn mu_27_ac_21_a_timeout_a_memory_kill_or_an_exit_code_is_a_failure() {
        assert!(failed(&result(0, true)));
        assert!(failed(&CommandResult {
            code: 0,
            timed_out: false,
            memory_killed: true,
            seconds: 0.0,
            output: String::new(),
        }));
        assert!(!failed(&result(0, false)));
    }

    // MU-27 / AC-21: the reason names the timeout, the memory limit in MiB, or the exit code.
    // A timeout wins over a memory kill.
    #[test]
    fn mu_27_ac_21_the_reason_names_the_timeout_memory_limit_or_exit_code() {
        let timeout = Duration::from_secs(7);
        let no_memory_cap = limits(7);
        let capped = Limits {
            timeout,
            max_memory: Some(300 * 1024 * 1024),
        };
        let memory_killed = CommandResult {
            code: 0,
            timed_out: false,
            memory_killed: true,
            seconds: 0.0,
            output: String::new(),
        };
        let both = CommandResult {
            code: 0,
            timed_out: true,
            memory_killed: true,
            seconds: 0.0,
            output: String::new(),
        };
        assert_eq!(
            reason_of(&result(0, true), timeout, no_memory_cap),
            "timed out after 7 seconds"
        );
        assert_eq!(
            reason_of(&memory_killed, timeout, capped),
            "memory limit of 300 MiB"
        );
        assert_eq!(
            reason_of(&both, timeout, capped),
            "timed out after 7 seconds"
        );
        assert_eq!(
            reason_of(&result(3, false), timeout, no_memory_cap),
            "exit code 3"
        );
    }

    // MU-27 / AC-21: with one worker there is no second worker to compare with. A command that
    // fails in that worker is a baseline failure, and the failure does not pass alone.
    #[test]
    fn mu_27_ac_21_a_failing_single_worker_does_not_pass_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();
        let base = new_run_dir(root).unwrap();
        let workers = create_workers(&base, root, "src/demo.py", &original, 1).unwrap();
        let command = Command::Argv(vec!["sh".into(), "-c".into(), "exit 1".into()]);

        let failure = check_workers(
            &workers,
            root,
            root,
            &CommandRunner::default(),
            &command,
            limits(20),
        )
        .expect("the only worker fails");
        assert_eq!(failure.worker, 0);
        assert!(!failure.passes_alone);
        delete_tree(&base).unwrap();
    }

    // MU-28 / AC-22: a worker does not link `test-results` or `playwright-report` at the project
    // root. The drop removes both links from the worker and keeps the files of the project.
    #[test]
    fn mu_28_ac_22_the_root_output_links_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        put(root, "test-results/marker.txt", "keep");
        put(root, "playwright-report/marker.txt", "keep");
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        for name in OUTPUT_DIRS {
            assert!(
                worker.join(name).is_symlink(),
                "{name} is linked before the drop"
            );
        }
        drop_output_links(&worker, root, root).unwrap();
        for name in OUTPUT_DIRS {
            assert!(
                fs::symlink_metadata(worker.join(name)).is_err(),
                "{name} is still in the worker"
            );
        }
        assert_eq!(
            fs::read_to_string(root.join("test-results/marker.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(root.join("playwright-report/marker.txt")).unwrap(),
            "keep"
        );
        delete_tree(&base).unwrap();
    }

    // MU-28 / AC-22: the directory where the command runs is mapped into the worker. Its output
    // link goes too, and the link of a source directory stays.
    #[test]
    fn mu_28_ac_22_the_cwd_output_link_is_dropped_and_source_links_stay() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "packages/app/src/x.ts", "export const x = 1 > 0;\n");
        put(root, "packages/app/test-results/marker.txt", "keep");
        put(root, "packages/app/src/test-results/marker.txt", "keep");
        let original = fs::read(root.join("packages/app/src/x.ts")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker =
            create_workers(&base, root, "packages/app/src/x.ts", &original, 1).unwrap()[0].clone();
        assert!(worker.join("packages/app/test-results").is_symlink());
        drop_output_links(&worker, root, &root.join("packages/app")).unwrap();
        assert!(fs::symlink_metadata(worker.join("packages/app/test-results")).is_err());
        assert!(worker.join("packages/app/src/test-results").is_symlink());
        assert_eq!(
            fs::read_to_string(root.join("packages/app/test-results/marker.txt")).unwrap(),
            "keep"
        );
        delete_tree(&base).unwrap();
    }

    // MU-28 / AC-22: a command directory outside the overlay resolves to the real project. A link
    // of the same name there is a link of the project, so it must stay.
    #[test]
    fn mu_28_ac_22_a_link_of_the_real_project_is_never_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        put(root, "other/keep.txt", "keep");
        put(root, "elsewhere/keep.txt", "keep");
        std::os::unix::fs::symlink(root.join("elsewhere"), root.join("other/test-results"))
            .unwrap();
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        drop_output_links(&worker, root, &root.join("other")).unwrap();
        assert!(
            fs::symlink_metadata(root.join("other/test-results"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(root.join("elsewhere/keep.txt")).unwrap(),
            "keep"
        );
        delete_tree(&base).unwrap();
    }

    // MU-29: each HTML file of the project is a real copy in the worker. Its parent directories are
    // real directories, and the other entries of those directories stay links.
    #[test]
    fn mu_29_html_files_are_real_copies_in_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "pages/page.html", "<p>page</p>\n");
        put(root, "pages/notes.txt", "notes\n");
        put(root, "index.html", "<html></html>\n");
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        for relative in ["index.html", "pages/page.html"] {
            let copied = worker.join(relative);
            assert!(copied.is_file() && !copied.is_symlink(), "{relative}");
            assert_eq!(
                fs::read(&copied).unwrap(),
                fs::read(root.join(relative)).unwrap()
            );
        }
        let pages = worker.join("pages");
        assert!(pages.is_dir() && !pages.is_symlink());
        assert!(worker.join("pages/notes.txt").is_symlink());

        delete_tree(&base).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("index.html")).unwrap(),
            "<html></html>\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("pages/page.html")).unwrap(),
            "<p>page</p>\n"
        );
    }

    // MU-29: the walk for HTML files skips the directories that the links skip, and the output
    // directories. An HTML file under them is not copied.
    #[test]
    fn mu_29_html_files_skip_dependency_and_output_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "docs/a.html", "a");
        put(root, "node_modules/b.html", "b");
        put(root, "dist/c.html", "c");
        put(root, "playwright-report/d.html", "d");
        put(root, "test-results/e.html", "e");
        let expected: BTreeSet<String> = ["docs/a.html"]
            .iter()
            .map(|path| path.to_string())
            .collect();
        assert_eq!(html_files(root), expected);
    }

    // MU-30: each JavaScript or TypeScript file that lies directly in the project root is a real
    // copy in the worker. A link of the project stays a link. The other directories do not change.
    #[test]
    fn mu_30_root_modules_are_real_copies_in_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "vite.config.ts", "export default {}\n");
        put(root, "playwright.config.mjs", "export default {}\n");
        put(root, "notes.txt", "notes\n");
        put(root, "scripts/helper.ts", "export const h = 1;\n");
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        std::os::unix::fs::symlink(
            root.join("scripts/helper.ts"),
            root.join("linked.config.js"),
        )
        .unwrap();
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        for relative in ["vite.config.ts", "playwright.config.mjs"] {
            let copied = worker.join(relative);
            assert!(copied.is_file() && !copied.is_symlink(), "{relative}");
            assert_eq!(
                fs::read(&copied).unwrap(),
                fs::read(root.join(relative)).unwrap()
            );
        }
        for relative in ["notes.txt", "scripts", "linked.config.js"] {
            assert!(worker.join(relative).is_symlink(), "{relative}");
        }
        let config = root.join("vite.config.ts");
        assert!(config.is_file() && !config.is_symlink());
        assert!(root.join("linked.config.js").is_symlink());
        delete_tree(&base).unwrap();
    }

    // MU-30: the directory where the command runs holds real copies of its JavaScript and
    // TypeScript files when that directory lies inside the worker. Its other files stay links.
    #[test]
    fn mu_30_command_directory_modules_are_real_copies_in_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "packages/app/vite.config.ts", "export default {}\n");
        put(root, "packages/app/notes.txt", "notes\n");
        put(root, "packages/app/src/x.ts", "export const x = 1 > 0;\n");
        let original = fs::read(root.join("packages/app/src/x.ts")).unwrap();
        let cwd = root.join("packages/app");

        let base = new_run_dir(root).unwrap();
        let worker =
            create_workers(&base, root, "packages/app/src/x.ts", &original, 1).unwrap()[0].clone();
        copy_command_modules(&worker, root, &cwd).unwrap();
        let copied = worker.join("packages/app/vite.config.ts");
        assert!(copied.is_file() && !copied.is_symlink());
        assert_eq!(
            fs::read(&copied).unwrap(),
            fs::read(root.join("packages/app/vite.config.ts")).unwrap()
        );
        assert!(worker.join("packages/app/notes.txt").is_symlink());
        delete_tree(&base).unwrap();
    }

    // MU-30: a command directory outside the worker is the real project. Its files are not copied,
    // and its link in the worker stays a link.
    #[test]
    fn mu_30_command_directory_outside_the_worker_is_not_changed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(root, "other/vite.config.ts", "export default {}\n");
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();
        let cwd = root.join("other");

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        copy_command_modules(&worker, root, &cwd).unwrap();
        assert!(worker.join("other").is_symlink());
        let config = root.join("other/vite.config.ts");
        assert!(config.is_file() && !config.is_symlink());
        assert_eq!(fs::read_to_string(config).unwrap(), "export default {}\n");
        delete_tree(&base).unwrap();
    }

    // MU-30: a file that a root module imports through a relative import is a real copy too, at
    // any depth. The other files of its directories stay links.
    #[test]
    fn mu_30_files_a_root_module_imports_are_real_copies_in_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(
            root,
            "vite.config.ts",
            "import { entries } from './scripts/entries.ts'\nexport default {}\n",
        );
        put(
            root,
            "scripts/entries.ts",
            "import { base } from '../shared/paths.ts'\nexport const entries = base\n",
        );
        put(root, "shared/paths.ts", "export const base = 1\n");
        put(root, "scripts/unused.ts", "export const unused = 2\n");
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        for relative in ["vite.config.ts", "scripts/entries.ts", "shared/paths.ts"] {
            assert_real_copy(&worker, root, relative);
        }
        assert!(worker.join("scripts/unused.ts").is_symlink());
        let scripts = worker.join("scripts");
        assert!(scripts.is_dir() && !scripts.is_symlink());
        delete_tree(&base).unwrap();
    }

    // MU-30: an import cycle ends, and every file of the cycle is a real copy.
    #[test]
    fn mu_30_an_import_cycle_ends_and_copies_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(
            root,
            "vite.config.ts",
            "import './lib/a.ts'\nexport default {}\n",
        );
        put(root, "lib/a.ts", "import './b.ts'\nexport const a = 1\n");
        put(root, "lib/b.ts", "import './a.ts'\nexport const b = 2\n");
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        for relative in ["vite.config.ts", "lib/a.ts", "lib/b.ts"] {
            assert_real_copy(&worker, root, relative);
        }
        delete_tree(&base).unwrap();
    }

    // MU-30: an import by package name and an import of a missing file copy nothing extra, and
    // they do not stop the run.
    #[test]
    fn mu_30_package_and_missing_imports_copy_nothing_extra() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(
            root,
            "vite.config.ts",
            "import x from 'left-pad'\nimport y from './missing.ts'\nexport default {}\n",
        );
        put(
            root,
            "node_modules/left-pad/index.js",
            "module.exports = 1\n",
        );
        put(root, "src/demo.py", "def f(a):\n    return a > 1\n");
        let original = fs::read(root.join("src/demo.py")).unwrap();

        let base = new_run_dir(root).unwrap();
        let worker = create_workers(&base, root, "src/demo.py", &original, 1).unwrap()[0].clone();
        let config = worker.join("vite.config.ts");
        assert!(config.is_file() && !config.is_symlink());
        assert!(worker.join("node_modules").is_symlink());
        assert!(!worker.join("missing.ts").exists());
        delete_tree(&base).unwrap();
    }

    // MU-30: a file that the module of the command directory imports is a real copy when that
    // directory lies inside the worker.
    #[test]
    fn mu_30_command_directory_imports_are_real_copies_in_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        put(
            root,
            "packages/app/vite.config.ts",
            "import { h } from './helpers/h.ts'\nexport default {}\n",
        );
        put(root, "packages/app/helpers/h.ts", "export const h = 1\n");
        put(root, "packages/app/src/x.ts", "export const x = 1 > 0;\n");
        let original = fs::read(root.join("packages/app/src/x.ts")).unwrap();
        let cwd = root.join("packages/app");

        let base = new_run_dir(root).unwrap();
        let worker =
            create_workers(&base, root, "packages/app/src/x.ts", &original, 1).unwrap()[0].clone();
        copy_command_modules(&worker, root, &cwd).unwrap();
        assert_real_copy(&worker, root, "packages/app/helpers/h.ts");
        delete_tree(&base).unwrap();
    }
}
