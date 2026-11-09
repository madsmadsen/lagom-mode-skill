//! Port of mutator/src/mutator/workers.py (BSD Zero Clause License, Robert C. Martin).
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
static SPECIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:from|import|require)\s*\(?\s*['"](\.[^'"]+)['"]"#)
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

/// The smaller of the sites, the cores, and an explicit --max-workers.
pub fn worker_count(site_count: usize, requested: Option<usize>) -> usize {
    let cores = processors();
    let limit = requested.unwrap_or(cores);
    site_count.min(cores).min(limit).max(1)
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

fn source_files(root: &Path) -> Vec<PathBuf> {
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
                if keep_dir(&entry.file_name()) {
                    pending.push(path);
                }
            } else if is_source(&path) {
                found.push(path);
            }
        }
    }
    found
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
    importers: &BTreeSet<String>,
) -> io::Result<()> {
    let first = relative.split('/').next().unwrap_or_default();
    fs::create_dir_all(worker)?;
    copy_configs(worker, root, relative)?;
    link_children(worker, root, &[first])?;
    overlay(worker, root, relative, original)?;
    importers
        .iter()
        .try_for_each(|importer| copy_importer(worker, root, importer))
}

/// Builds `count` worker overlays under `base`, each holding a private copy of `relative`.
pub fn create_workers(
    base: &Path,
    root: &Path,
    relative: &str,
    original: &[u8],
    count: usize,
) -> io::Result<Vec<PathBuf>> {
    let importers = importers_of(root, relative)?;
    (0..count)
        .map(|index| -> io::Result<PathBuf> {
            let worker = base.join(format!("worker-{index}"));
            build_worker(&worker, root, relative, original, &importers)?;
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
pub fn needs_second_run(timed_out: bool, seconds: f64, timeout: Duration) -> bool {
    timed_out || seconds >= NEAR_TIMEOUT * timeout.as_secs_f64()
}

/// A timed-out or failing run kills the mutant. A passing run lets it survive.
pub fn outcome_of(result: &CommandResult) -> Status {
    if result.timed_out || result.code != 0 {
        Status::Killed
    } else {
        Status::Survived
    }
}

/// Writes one mutant into the worker, runs the command, and restores the original text.
fn run_mutant(
    directory: &Path,
    plan: &Plan<'_>,
    mutated: &str,
    timeout: Duration,
) -> io::Result<CommandResult> {
    let destination = directory.join(plan.relative);
    fs::write(&destination, mutated)?;
    let cwd = mapped_cwd(directory, plan.root, plan.cwd);
    let result = plan.runner.run(plan.command, &cwd, Some(timeout));
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
    let first = run_mutant(directory, plan, &mutated, plan.timeout)?;
    let result = if needs_second_run(first.timed_out, first.seconds, plan.timeout) {
        run_mutant(directory, plan, &mutated, plan.timeout * 2)?
    } else {
        first
    };
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

/// Mutates `sites` in parallel overlays of `root`. The source stays put.
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
    timeout: Duration,
    file_key: &str,
    outcomes: &mut BTreeMap<String, Status>,
) {
    if sites.is_empty() {
        return;
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
    let shared = Shared {
        plan: Plan {
            root,
            relative: &relative,
            text,
            runner,
            command,
            cwd,
            timeout,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
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
            seconds: 0.0,
            output: String::new(),
        }
    }

    // MU-17: worker count is the minimum of sites, cores, and the requested cap.
    #[test]
    fn mu_17_worker_count_follows_sites_cores_and_the_requested_cap() {
        let cores = processors();
        assert_eq!(worker_count(10, None), cores.min(10));
        assert_eq!(worker_count(10, Some(3)), 3.min(cores));
        assert_eq!(worker_count(2, Some(8)), cores.min(2));
        assert_eq!(worker_count(0, None), 1);
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
            Duration::from_secs(20),
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
        let command = Command::Shell("exit 1".to_string());
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
            Duration::from_secs(20),
            "src/x.txt",
            &mut outcomes,
        );

        assert_eq!(outcomes.get("moved-id"), Some(&Status::Survived));
        assert_eq!(fs::read(&source).unwrap(), original);
    }

    // MU-21: a timed-out mutant gets a second run, whatever its seconds say.
    #[test]
    fn mu_21_a_timed_out_run_needs_a_second_run() {
        assert!(needs_second_run(true, 0.0, Duration::from_secs(10)));
    }

    // MU-21: a run at 0.89 of its timeout is not repeated, a run at 0.9 of it is.
    #[test]
    fn mu_21_a_run_within_ten_percent_of_the_timeout_needs_a_second_run() {
        let timeout = Duration::from_secs(10);
        assert!(!needs_second_run(false, 8.9, timeout));
        assert!(needs_second_run(false, 9.0, timeout));
    }

    // MU-21: a fast run that did not time out is not repeated.
    #[test]
    fn mu_21_a_fast_run_is_not_repeated() {
        assert!(!needs_second_run(false, 0.01, Duration::from_secs(10)));
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
        let command = Command::Shell(format!("echo run >> {} && sleep 1.5", log.display()));
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
            Duration::from_secs(1),
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
        let command = Command::Shell(format!("echo run >> {}", log.display()));
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
            Duration::from_secs(20),
            "src/x.txt",
            &mut outcomes,
        );

        assert_eq!(outcomes.get("quick-id"), Some(&Status::Survived));
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
    }
}
