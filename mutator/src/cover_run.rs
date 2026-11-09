//! Port of crapper/src/crapper/runners.py (BSD Zero Clause License, Robert C. Martin).

use crate::discover::{is_test_file, language_of};
use crate::fs_util::resolve;
use crate::model::Language;
use crate::runner::{Command, nearest, nearest_marker};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitStatus};

const MAVEN: &[&str] = &[
    "mvn",
    "-q",
    "org.jacoco:jacoco-maven-plugin:0.8.12:prepare-agent",
    "test",
    "org.jacoco:jacoco-maven-plugin:0.8.12:report",
];
const CLOJURE_LCOV: &[&str] = &["clj", "-M:cov", "--lcov"];
const CLOJURE_PLAIN: &[&str] = &["clj", "-M:cov"];
const COVERAGE_DIRS: &[&str] = &["typescript", "rust", "go", "python"];
const VITEST_CONFIGS: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
];
const VITEST_PROVIDER: &str = "node_modules/@vitest/coverage-v8";
const MANIFESTS: &[&str] = &["package.json", "package-lock.json", "npm-shrinkwrap.json"];
const PYTHON_MARKERS: &[&str] = &[
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
    "pytest.ini",
    "tox.ini",
];
const PYTEST_CONFIGS: &[&str] = &["pyproject.toml", "setup.cfg", "tox.ini"];
const COVER_ORDER: [Language; 6] = [
    Language::Clojure,
    Language::Java,
    Language::Go,
    Language::TypeScript,
    Language::Python,
    Language::Rust,
];

type ManifestSnapshot = Vec<(PathBuf, Option<Vec<u8>>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RustTool {
    LlvmCov,
    Tarpaulin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PythonKind {
    Pytest,
    Unittest,
}

/// One planned action. Plans are pure data; `execute` interprets them.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    Notice(String),
    Run {
        command: Command,
        failure: Option<Failure>,
    },
    Disk(Disk),
    Require {
        check: Check,
        message: String,
    },
    Branch {
        check: Check,
        then: Vec<Step>,
        otherwise: Vec<Step>,
    },
}

/// The text printed when a planned command exits nonzero.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Failure {
    Text(String),
    Exit { before: String, after: String },
}

impl Failure {
    fn render(&self, code: i32) -> String {
        match self {
            Failure::Text(text) => text.clone(),
            Failure::Exit { before, after } => format!("{before}{code}{after}"),
        }
    }
}

/// A filesystem change applied by the executor.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Disk {
    Prepare(PathBuf),
    Reset(PathBuf),
    Remove(PathBuf),
    Restore(ManifestSnapshot),
    Clean(PathBuf),
    Clojure(PathBuf),
}

/// A probe that decides which branch a plan takes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Check {
    /// The command exits zero.
    Command(Command),
    /// The last Run exited zero and the directory exists.
    Installed(PathBuf),
    /// The last Run exited zero.
    LastRunOk,
}

fn run(command: Command) -> Step {
    Step::Run {
        command,
        failure: None,
    }
}

/// Runs the coverage tools for the languages in `files`, or the custom `command` alone.
pub fn run_coverage(root: &Path, files: &[PathBuf], command: Option<&str>) -> i32 {
    let root = resolve(root);
    if let Some(text) = command.filter(|text| !text.is_empty()) {
        return run_custom(text, &root);
    }
    let languages: BTreeSet<Language> = files.iter().filter_map(|file| language_of(file)).collect();
    for language in COVER_ORDER
        .into_iter()
        .filter(|language| languages.contains(language))
    {
        cover_language(language, &root, files);
    }
    0
}

/// Finds an executable named `name` on the PATH, as `shutil.which` does.
pub fn which(name: &str) -> Option<PathBuf> {
    which_in(name, &env::var_os("PATH")?)
}

fn which_in(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    env::split_paths(path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn warn(message: &str) {
    eprintln!("{message}");
}

fn words(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn argv(parts: &[&str]) -> Command {
    Command::Argv(words(parts))
}

fn quote(part: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if !part.is_empty() && part.chars().all(safe) {
        part.to_owned()
    } else {
        format!("'{}'", part.replace('\'', "'\"'\"'"))
    }
}

fn show(command: &Command) -> String {
    match command {
        Command::Shell(text) => text.clone(),
        Command::Argv(parts) => parts
            .iter()
            .map(|part| quote(part))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn process_for(command: &Command) -> Process {
    match command {
        Command::Shell(text) => {
            let mut process = Process::new("sh");
            process.arg("-c").arg(text);
            process
        }
        Command::Argv(parts) => {
            let mut process = Process::new(parts.first().map_or("", String::as_str));
            process.args(parts.iter().skip(1));
            process
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| -signal))
        .unwrap_or(1)
}

/// Echoes `+ (cwd) command` to stderr, runs it with inherited stdio, returns its status.
fn run_shell(command: &Command, cwd: &Path) -> i32 {
    warn(&format!("+ ({}) {}", cwd.display(), show(command)));
    match process_for(command).current_dir(cwd).status() {
        Ok(status) => exit_code(status),
        Err(error) => {
            warn(&format!("Coverage command failed to start: {error}"));
            127
        }
    }
}

fn run_custom(text: &str, root: &Path) -> i32 {
    let code = run_shell(&Command::Shell(text.to_owned()), root);
    if code != 0 {
        warn(&format!(
            "Coverage command exited {code}. Reports already on disk will not be read."
        ));
    }
    code
}

/// Every entry below `dir`, without following symlinked directories.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    found
}

fn report_io(path: &Path, result: io::Result<()>) {
    if let Err(error) = result {
        warn(&format!("{}: {error}", path.display()));
    }
}

fn ensure_dir(path: &Path) {
    report_io(path, fs::create_dir_all(path));
}

fn clean_dir(path: &Path) {
    if path.exists() {
        report_io(path, fs::remove_dir_all(path));
    }
}

fn remove_file_if_present(path: &Path) {
    if path.exists() {
        report_io(path, fs::remove_file(path));
    }
}

fn report_dir(report: &Path) -> &Path {
    report.parent().unwrap_or(report)
}

/// Replaces the report's directory with an empty one.
fn prepare_report(report: &Path) {
    clean_dir(report_dir(report));
    ensure_dir(report_dir(report));
}

/// Keeps the report's directory but removes the report file a previous run left behind.
fn reset_report(report: &Path) {
    ensure_dir(report_dir(report));
    remove_file_if_present(report);
}

/// The warning for a planned command that exits nonzero, naming the language and module.
fn scored_exit(language: &str, module: &Path) -> Failure {
    Failure::Exit {
        before: format!("{language} coverage exited "),
        after: format!(
            " in {}. {language} coverage will score 0%.",
            module.display()
        ),
    }
}

/// Runs each module's plan from inside that module; with no modules, prints `none` instead.
fn cover_modules(root: &Path, modules: Vec<PathBuf>, none: &str, plan: fn(&Path) -> Vec<Step>) {
    if modules.is_empty() {
        execute(&[Step::Notice(none.to_owned())], root);
    }
    for module in &modules {
        execute(&plan(module), module);
    }
}

/// One language whose modules are the directories that hold a `marker` file.
struct Marked {
    extension: &'static str,
    marker: &'static str,
    none: &'static str,
    plan: fn(&Path) -> Vec<Step>,
}

const JAVA: Marked = Marked {
    extension: "java",
    marker: "pom.xml",
    none: "No pom.xml; skipping Java coverage.",
    plan: java_module_plan,
};

const GO: Marked = Marked {
    extension: "go",
    marker: "go.mod",
    none: "No go.mod; skipping Go coverage.",
    plan: go_module_plan,
};

/// Covers the modules that contain `files` with the language's extension and marker file.
fn cover_marked(root: &Path, files: &[PathBuf], marked: &Marked) {
    let modules = modules_with(files, marked.extension, marked.marker, root);
    cover_modules(root, modules, marked.none, marked.plan);
}

/// `target/coverage/<language>/lcov.info`, or a slug of the module path below the root.
fn coverage_report(root: &Path, module: &Path, language: Language) -> PathBuf {
    let root = resolve(root);
    let module = resolve(module);
    let base = root.join("target").join("coverage").join(language.name());
    match module.strip_prefix(&root) {
        Ok(relative) if !relative.as_os_str().is_empty() => base
            .join(posix(relative).replace('/', "__"))
            .join("lcov.info"),
        _ => base.join("lcov.info"),
    }
}

/// A relative path joined with `/`, the way Python's `as_posix` renders it.
fn posix(relative: &Path) -> String {
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn modules_with(files: &[PathBuf], extension: &str, marker: &str, root: &Path) -> Vec<PathBuf> {
    let modules: BTreeSet<PathBuf> = files
        .iter()
        .filter(|file| file.extension() == Some(OsStr::new(extension)))
        .filter_map(|file| nearest(file, marker, root))
        .collect();
    modules.into_iter().collect()
}

fn cover_language(language: Language, root: &Path, files: &[PathBuf]) {
    match language {
        Language::Clojure => cover_clojure(root),
        Language::Java => cover_marked(root, files, &JAVA),
        Language::Go => cover_marked(root, files, &GO),
        Language::TypeScript => cover_typescript(root, files),
        Language::Python => cover_python(root, files),
        Language::Rust => cover_rust(root, files),
    }
}

// Clojure

fn is_shared_report(coverage: &Path, path: &Path) -> bool {
    path.strip_prefix(coverage).is_ok_and(|relative| {
        relative
            .iter()
            .all(|part| !COVERAGE_DIRS.iter().any(|dir| part == *dir))
    })
}

fn remove_cloverage_html(coverage: &Path) {
    for path in walk(coverage) {
        let is_html = path.extension() == Some(OsStr::new("html"));
        if is_html && path.is_file() && is_shared_report(coverage, &path) {
            report_io(&path, fs::remove_file(&path));
        }
    }
}

fn remove_empty_dirs(coverage: &Path) {
    let mut directories: Vec<PathBuf> = walk(coverage)
        .into_iter()
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    for path in directories
        .iter()
        .rev()
        .filter(|path| is_shared_report(coverage, path))
    {
        // A directory that still holds files stays, so a failed removal is expected.
        let _ = fs::remove_dir(path);
    }
}

/// Removes Cloverage's HTML and top-level lcov, keeping the other language reports.
fn clean_clojure(root: &Path) {
    let coverage = root.join("target").join("coverage");
    if !coverage.is_dir() {
        return;
    }
    remove_file_if_present(&coverage.join("lcov.info"));
    remove_cloverage_html(&coverage);
    remove_empty_dirs(&coverage);
}

fn clojure_plan(root: &Path) -> Vec<Step> {
    if !root.join("deps.edn").is_file() && !root.join("bb.edn").is_file() {
        return vec![Step::Notice(
            "No deps.edn or bb.edn; skipping Clojure coverage.".to_owned(),
        )];
    }
    vec![
        Step::Disk(Disk::Clojure(root.to_path_buf())),
        Step::Branch {
            check: Check::Command(argv(CLOJURE_LCOV)),
            then: Vec::new(),
            otherwise: vec![
                Step::Notice("clj -M:cov --lcov failed; retrying without --lcov.".to_owned()),
                Step::Run {
                    command: argv(CLOJURE_PLAIN),
                    failure: Some(Failure::Exit {
                        before: "Clojure coverage exited ".to_owned(),
                        after: ". Clojure coverage will score 0%.".to_owned(),
                    }),
                },
            ],
        },
    ]
}

fn cover_clojure(root: &Path) {
    execute(&clojure_plan(root), root);
}

// Java

fn java_module_plan(module: &Path) -> Vec<Step> {
    vec![
        Step::Disk(Disk::Clean(module.join("target/site/jacoco"))),
        Step::Disk(Disk::Remove(module.join("target/jacoco.exec"))),
        Step::Run {
            command: argv(MAVEN),
            failure: Some(scored_exit("Java", module)),
        },
    ]
}

// Go

fn go_test_args(profile: &Path) -> Vec<String> {
    let mut args = words(&["go", "test", "./..."]);
    args.push(format!("-coverprofile={}", profile.display()));
    args
}

fn go_module_plan(module: &Path) -> Vec<Step> {
    let profile = module.join("target/coverage/go/coverage.out");
    vec![
        Step::Disk(Disk::Reset(profile.clone())),
        Step::Run {
            command: Command::Argv(go_test_args(&profile)),
            failure: Some(scored_exit("Go", module)),
        },
    ]
}

// TypeScript

fn package_json(package: &Path) -> Result<Option<Map<String, Value>>, String> {
    let path = package.join("package.json");
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    match value {
        Value::Object(data) => Ok(Some(data)),
        _ => Err(format!("{}: expected a JSON object", path.display())),
    }
}

fn scripts_of(mut data: Map<String, Value>) -> Map<String, Value> {
    match data.remove("scripts") {
        Some(Value::Object(scripts)) => scripts,
        _ => Map::new(),
    }
}

fn uses_vitest(package: &Path, scripts: &Map<String, Value>) -> bool {
    let test_mentions = scripts
        .get("test")
        .is_some_and(|test| test.to_string().contains("vitest"));
    test_mentions
        || VITEST_CONFIGS
            .iter()
            .any(|name| package.join(name).is_file())
}

fn vitest_includes(package: &Path, files: &[PathBuf]) -> Vec<String> {
    let package = resolve(package);
    files
        .iter()
        .filter(|file| language_of(file) == Some(Language::TypeScript) && !is_test_file(file))
        .filter_map(|file| resolve(file).strip_prefix(&package).ok().map(posix))
        .collect()
}

fn vitest_coverage_command(package: &Path, files: &[PathBuf], report_dir: &Path) -> Vec<String> {
    let binary = package.join("node_modules/.bin/vitest");
    let mut command = if binary.is_file() {
        vec![binary.display().to_string()]
    } else {
        words(&["npx", "vitest"])
    };
    command.extend(words(&["run", "--coverage", "--coverage.reporter=lcov"]));
    command.push(format!(
        "--coverage.reportsDirectory={}",
        report_dir.display()
    ));
    command.push("--coverage.reportOnFailure=true".to_owned());
    command.extend(
        vitest_includes(package, files)
            .into_iter()
            .map(|include| format!("--coverage.include={include}")),
    );
    command
}

fn c8_command(report_dir: &Path) -> Vec<String> {
    let mut command = words(&["npx", "--yes", "c8", "--reporter=lcov", "--reports-dir"]);
    command.push(report_dir.display().to_string());
    command.extend(words(&["npm", "test"]));
    command
}

/// Argument vector that writes LCOV into `report_dir`; `None` when there is no test script.
fn typescript_command(
    package: &Path,
    files: &[PathBuf],
    report_dir: &Path,
) -> Result<Option<Vec<String>>, String> {
    let Some(data) = package_json(package)? else {
        return Ok(None);
    };
    let scripts = scripts_of(data);
    if scripts.contains_key("coverage") {
        return Ok(Some(words(&["npm", "run", "coverage"])));
    }
    if uses_vitest(package, &scripts) {
        return Ok(Some(vitest_coverage_command(package, files, report_dir)));
    }
    if !scripts.contains_key("test") {
        return Ok(None);
    }
    Ok(Some(c8_command(report_dir)))
}

fn vitest_version(package: &Path) -> Option<String> {
    let installed = package.join("node_modules/vitest/package.json");
    let text = fs::read_to_string(&installed).ok()?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(data)) => data.get("version")?.as_str().map(str::to_owned),
        Ok(_) => None,
        Err(error) => {
            warn(&format!("{}: {error}", installed.display()));
            None
        }
    }
}

fn vitest_provider_spec(version: Option<&str>) -> String {
    match version {
        Some(version) => format!("@vitest/coverage-v8@{version}"),
        None => "@vitest/coverage-v8".to_owned(),
    }
}

fn manifest_snapshot(package: &Path) -> ManifestSnapshot {
    MANIFESTS
        .iter()
        .map(|name| {
            let path = package.join(name);
            let content = fs::read(&path).ok();
            (path, content)
        })
        .collect()
}

fn restore_manifest(path: &Path, content: Option<&[u8]>) {
    match content {
        None => remove_file_if_present(path),
        Some(bytes) => {
            if fs::read(path).ok().as_deref() != Some(bytes) {
                report_io(path, fs::write(path, bytes));
            }
        }
    }
}

fn restore_manifests(snapshot: &ManifestSnapshot) {
    for (path, content) in snapshot {
        restore_manifest(path, content.as_deref());
    }
}

fn needs_vitest_provider(command: &[String]) -> bool {
    let custom_script = command
        .iter()
        .map(String::as_str)
        .take(3)
        .eq(["npm", "run", "coverage"]);
    !custom_script
        && command
            .iter()
            .any(|part| Path::new(part).file_name() == Some(OsStr::new("vitest")))
}

fn typescript_packages(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    let packages: BTreeSet<PathBuf> = files
        .iter()
        .filter(|file| language_of(file) == Some(Language::TypeScript))
        .filter_map(|file| nearest(file, "package.json", &root))
        .map(|package| resolve(&package))
        .collect();
    packages.into_iter().collect()
}

/// What a missing Vitest coverage provider needs: the pinned install spec and the manifests to restore.
struct ProviderSetup {
    spec: String,
    manifests: ManifestSnapshot,
}

fn provider_setup(package: &Path) -> ProviderSetup {
    ProviderSetup {
        spec: vitest_provider_spec(vitest_version(package).as_deref()),
        manifests: manifest_snapshot(package),
    }
}

/// Steps that install Vitest's coverage provider, restore the manifests, and require the provider.
fn vitest_provider_steps(package: &Path, setup: ProviderSetup) -> Vec<Step> {
    vec![
        Step::Notice(format!(
            "Installing {} so Vitest can write LCOV. package.json and the lockfile are restored afterward.",
            setup.spec
        )),
        run(argv(&[
            "npm",
            "install",
            "--no-save",
            "--no-package-lock",
            setup.spec.as_str(),
        ])),
        Step::Disk(Disk::Restore(setup.manifests)),
        Step::Require {
            check: Check::Installed(package.join(VITEST_PROVIDER)),
            message: "Vitest's coverage provider is missing. TypeScript coverage will score 0%."
                .to_owned(),
        },
    ]
}

/// The steps that cover one TypeScript package, or the message that explains why there are none.
fn typescript_plan(
    package: &Path,
    report: &Path,
    command: Result<Option<Vec<String>>, String>,
) -> Vec<Step> {
    let command = match command {
        Err(message) => return vec![Step::Notice(message)],
        Ok(None) => {
            return vec![Step::Notice(format!(
                "No package.json test script in {}; skipping TypeScript coverage.",
                package.display()
            ))];
        }
        Ok(Some(command)) => command,
    };
    let mut steps = if needs_vitest_provider(&command) && !package.join(VITEST_PROVIDER).is_dir() {
        vitest_provider_steps(package, provider_setup(package))
    } else {
        Vec::new()
    };
    steps.push(Step::Disk(Disk::Prepare(report.to_path_buf())));
    steps.push(Step::Run {
        command: Command::Argv(command),
        failure: Some(Failure::Exit {
            before: "TypeScript coverage exited ".to_owned(),
            after: format!(
                " in {}. TypeScript coverage will score 0%.",
                package.display()
            ),
        }),
    });
    steps
}

fn cover_typescript_package(root: &Path, files: &[PathBuf], package: &Path) {
    let report = coverage_report(root, package, Language::TypeScript);
    let command = typescript_command(package, files, report_dir(&report));
    execute(&typescript_plan(package, &report, command), package);
}

fn cover_typescript(root: &Path, files: &[PathBuf]) {
    let packages = typescript_packages(root, files);
    if packages.is_empty() {
        warn("No package.json; skipping TypeScript coverage.");
        return;
    }
    for package in packages {
        cover_typescript_package(root, files, &package);
    }
}

// Python

fn python_roots(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    let roots: BTreeSet<PathBuf> = files
        .iter()
        .filter(|file| file.extension() == Some(OsStr::new("py")))
        .map(|file| resolve(&nearest_marker(file, &root, PYTHON_MARKERS)))
        .collect();
    roots.into_iter().collect()
}

fn python_executable(package: &Path) -> String {
    [".venv/bin/python", "venv/bin/python"]
        .iter()
        .map(|relative| package.join(relative))
        .find(|candidate| candidate.is_file())
        .map_or_else(
            || "python3".to_owned(),
            |candidate| candidate.display().to_string(),
        )
}

fn config_mentions_pytest(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains("pytest"))
}

fn uses_pytest(package: &Path) -> bool {
    ["pytest.ini", "conftest.py"]
        .iter()
        .any(|name| package.join(name).is_file())
        || PYTEST_CONFIGS
            .iter()
            .any(|name| config_mentions_pytest(&package.join(name)))
}

/// A top-level module is named without `.py`, which coverage.py `--source` requires.
fn source_name(relative: &Path) -> String {
    let single = relative.components().count() == 1;
    let name = if single {
        relative.file_stem()
    } else {
        relative.iter().next()
    };
    name.map(|part| part.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn python_sources(package: &Path, files: &[PathBuf]) -> String {
    let package = resolve(package);
    let tops: BTreeSet<String> = files
        .iter()
        .filter(|file| file.extension() == Some(OsStr::new("py")) && !is_test_file(file))
        .filter_map(|file| resolve(file).strip_prefix(&package).ok().map(source_name))
        .collect();
    tops.into_iter().collect::<Vec<_>>().join(",")
}

fn python_coverage_commands(
    py: &str,
    kind: PythonKind,
    data_file: &Path,
    report: &Path,
    source: &str,
) -> (Vec<String>, Vec<String>) {
    let data = format!("--data-file={}", data_file.display());
    let mut run = words(&[py, "-m", "coverage", "run"]);
    run.push(data.clone());
    if !source.is_empty() {
        run.push(format!("--source={source}"));
    }
    run.extend(match kind {
        PythonKind::Pytest => words(&["-m", "pytest"]),
        PythonKind::Unittest => words(&["-m", "unittest", "discover", "-s", "."]),
    });
    let mut lcov = words(&[py, "-m", "coverage", "lcov"]);
    lcov.extend([data, "-o".to_owned(), report.display().to_string()]);
    (run, lcov)
}

/// Installs `module` with pip, after a notice that the project requirements are untouched.
fn install_module(py: &str, module: &str) -> Vec<Step> {
    vec![
        Step::Notice(format!(
            "Installing {module} for Python coverage. The project requirements are left unchanged."
        )),
        run(argv(&[
            py,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            module,
        ])),
    ]
}

fn import_probe(py: &str, module: &str) -> Check {
    let probe = format!("import {module}");
    Check::Command(argv(&[py, "-c", probe.as_str()]))
}

/// Pytest when it imports, otherwise a notice and unittest discovery.
fn test_steps(
    py: &str,
    package: &Path,
    uses_pytest: bool,
    pytest_run: Command,
    unittest_run: Command,
) -> Vec<Step> {
    if !uses_pytest {
        return vec![run(unittest_run)];
    }
    let mut otherwise = install_module(py, "pytest");
    otherwise.push(Step::Branch {
        check: Check::LastRunOk,
        then: vec![run(pytest_run.clone())],
        otherwise: vec![
            Step::Notice(format!(
                "pytest is missing in {}. Falling back to unittest.",
                package.display()
            )),
            run(unittest_run),
        ],
    });
    vec![Step::Branch {
        check: import_probe(py, "pytest"),
        then: vec![run(pytest_run)],
        otherwise,
    }]
}

/// Everything before the test run: coverage present, report and data reset, then the tests.
/// The last Run in the plan is the test run, so `execute` returns its exit code.
fn python_run_plan(
    py: &str,
    package: &Path,
    uses_pytest: bool,
    data: &Path,
    report: &Path,
    source: &str,
) -> Vec<Step> {
    let command = |kind: PythonKind| {
        Command::Argv(python_coverage_commands(py, kind, data, report, source).0)
    };
    let mut install = install_module(py, "coverage");
    install.push(Step::Require {
        check: Check::LastRunOk,
        message: format!(
            "coverage is missing in {}. Python coverage will score 0%.",
            package.display()
        ),
    });
    let mut steps = vec![
        Step::Branch {
            check: import_probe(py, "coverage"),
            then: Vec::new(),
            otherwise: install,
        },
        Step::Disk(Disk::Reset(report.to_path_buf())),
        Step::Disk(Disk::Remove(data.to_path_buf())),
    ];
    steps.extend(test_steps(
        py,
        package,
        uses_pytest,
        command(PythonKind::Pytest),
        command(PythonKind::Unittest),
    ));
    steps
}

/// Records LCOV after the tests exited with `code`, and warns as the outcome requires.
fn python_record_plan(package: &Path, code: i32, data_exists: bool, lcov: Command) -> Vec<Step> {
    if !data_exists {
        return if code == 0 {
            Vec::new()
        } else {
            vec![Step::Notice(format!(
                "Python coverage exited {code} in {}. Python coverage will score 0%.",
                package.display()
            ))]
        };
    }
    let tests_warning = if code == 0 {
        Vec::new()
    } else {
        vec![Step::Notice(format!(
            "Python tests exited {code} in {}. Coverage was still recorded.",
            package.display()
        ))]
    };
    vec![
        Step::Run {
            command: lcov,
            failure: Some(Failure::Exit {
                before: "coverage lcov exited ".to_owned(),
                after: format!(" in {}. Python coverage will score 0%.", package.display()),
            }),
        },
        Step::Branch {
            check: Check::LastRunOk,
            then: tests_warning,
            otherwise: Vec::new(),
        },
    ]
}

fn cover_python_package(root: &Path, files: &[PathBuf], package: &Path) {
    let report = coverage_report(root, package, Language::Python);
    let py = python_executable(package);
    let data = report_dir(&report).join(".coverage");
    let source = python_sources(package, files);
    let (_, lcov) = python_coverage_commands(&py, PythonKind::Unittest, &data, &report, &source);
    let plan = python_run_plan(&py, package, uses_pytest(package), &data, &report, &source);
    if let Some(code) = execute(&plan, package) {
        execute(
            &python_record_plan(package, code, data.exists(), Command::Argv(lcov)),
            package,
        );
    }
}

fn cover_python(root: &Path, files: &[PathBuf]) {
    for package in python_roots(root, files) {
        cover_python_package(root, files, &package);
    }
}

// Rust

fn rust_modules(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    modules_with(files, "rs", "Cargo.toml", &root)
        .into_iter()
        .map(|module| resolve(&module))
        .collect()
}

fn rust_coverage_command(tool: RustTool, report: &Path) -> Vec<String> {
    match tool {
        RustTool::LlvmCov => {
            let mut command = words(&["cargo", "llvm-cov", "--lcov", "--output-path"]);
            command.push(report.display().to_string());
            command
        }
        RustTool::Tarpaulin => {
            let mut command = words(&["cargo", "tarpaulin", "--out", "Lcov", "--output-dir"]);
            command.push(report_dir(report).display().to_string());
            command
        }
    }
}

fn installed_rust_tool(path_var: &OsStr) -> Option<RustTool> {
    if which_in("cargo-llvm-cov", path_var).is_some() {
        Some(RustTool::LlvmCov)
    } else if which_in("cargo-tarpaulin", path_var).is_some() {
        Some(RustTool::Tarpaulin)
    } else {
        None
    }
}

fn rust_install_notice() -> &'static str {
    "Neither cargo-llvm-cov nor cargo-tarpaulin is installed. Installing cargo-llvm-cov."
}

/// The ordered install steps; the notice always precedes the cargo install command.
fn rust_install_plan(has_rustup: bool) -> Vec<Step> {
    let mut plan = vec![Step::Notice(rust_install_notice().to_owned())];
    if has_rustup {
        plan.push(Step::Run {
            command: argv(&["rustup", "component", "add", "llvm-tools-preview"]),
            failure: Some(Failure::Text(
                "rustup component add llvm-tools-preview failed.".to_owned(),
            )),
        });
    }
    plan.push(run(argv(&[
        "cargo",
        "install",
        "cargo-llvm-cov",
        "--locked",
    ])));
    plan
}

/// Interprets a plan in order. Returns the last Run's status, or None when a Require stopped it.
fn execute(steps: &[Step], cwd: &Path) -> Option<i32> {
    let mut last = 0;
    run_steps(steps, cwd, &mut last).then_some(last)
}

fn run_steps(steps: &[Step], cwd: &Path, last: &mut i32) -> bool {
    steps.iter().all(|step| run_step(step, cwd, last))
}

/// Runs one step; false stops the plan.
fn run_step(step: &Step, cwd: &Path, last: &mut i32) -> bool {
    match step {
        Step::Notice(text) => {
            warn(text);
            true
        }
        Step::Run { command, failure } => {
            *last = run_reported(command, cwd, failure.as_ref());
            true
        }
        Step::Disk(disk) => {
            run_disk(disk);
            true
        }
        Step::Require { check, message } => require(check, message, cwd, *last),
        Step::Branch {
            check,
            then,
            otherwise,
        } => {
            let branch = if holds(check, cwd, *last) {
                then
            } else {
                otherwise
            };
            run_steps(branch, cwd, last)
        }
    }
}

fn require(check: &Check, message: &str, cwd: &Path, last: i32) -> bool {
    let ok = holds(check, cwd, last);
    if !ok {
        warn(message);
    }
    ok
}

fn holds(check: &Check, cwd: &Path, last: i32) -> bool {
    match check {
        Check::Command(command) => run_shell(command, cwd) == 0,
        Check::Installed(dir) => last == 0 && dir.is_dir(),
        Check::LastRunOk => last == 0,
    }
}

/// Runs a planned command, printing its failure text when it exits nonzero.
fn run_reported(command: &Command, cwd: &Path, failure: Option<&Failure>) -> i32 {
    let code = run_shell(command, cwd);
    if let Some(failure) = failure.filter(|_| code != 0) {
        warn(&failure.render(code));
    }
    code
}

fn run_disk(disk: &Disk) {
    match disk {
        Disk::Prepare(report) => prepare_report(report),
        Disk::Reset(report) => reset_report(report),
        Disk::Remove(path) => remove_file_if_present(path),
        Disk::Restore(snapshot) => restore_manifests(snapshot),
        Disk::Clean(dir) => clean_dir(dir),
        Disk::Clojure(root) => clean_clojure(root),
    }
}

fn install_llvm_cov() -> Option<RustTool> {
    let plan = rust_install_plan(which("rustup").is_some());
    let cwd = env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    if execute(&plan, &cwd) == Some(0) && which("cargo-llvm-cov").is_some() {
        return Some(RustTool::LlvmCov);
    }
    warn("Rust coverage will score 0%.");
    None
}

fn rust_kind() -> Option<RustTool> {
    let path_var = env::var_os("PATH").unwrap_or_default();
    installed_rust_tool(&path_var).or_else(install_llvm_cov)
}

fn rust_module_plan(root: &Path, module: &Path, tool: RustTool) -> Vec<Step> {
    let report = coverage_report(root, module, Language::Rust);
    vec![
        Step::Disk(Disk::Reset(report.clone())),
        Step::Run {
            command: Command::Argv(rust_coverage_command(tool, &report)),
            failure: Some(scored_exit("Rust", module)),
        },
    ]
}

fn cover_rust_module(root: &Path, module: &Path, tool: RustTool) {
    execute(&rust_module_plan(root, module, tool), module);
}

fn cover_rust(root: &Path, files: &[PathBuf]) {
    let modules = rust_modules(root, files);
    if modules.is_empty() {
        warn("No Cargo.toml; skipping Rust coverage.");
        return;
    }
    let Some(tool) = rust_kind() else {
        return;
    };
    for module in modules {
        cover_rust_module(root, &module, tool);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(root: &Path, relative: &str, text: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(report_dir(&path)).expect("create test directory");
        fs::write(&path, text).expect("write test file");
        path
    }

    fn fake_tool(dir: &Path, name: &str, mode: u32) {
        let path = write(dir, name, "#!/bin/sh\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("set tool mode");
    }

    #[test]
    fn mu_10_vitest_package_uses_its_own_coverage_not_c8() {
        // MU-10: a Vitest package runs Vitest's own LCOV coverage, never c8.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        write(root, "package.json", r#"{"scripts":{"test":"vitest run"}}"#);
        write(root, "vitest.config.ts", "export default {}\n");
        let source = write(root, "src/book.ts", "export function plan() { return 1 }\n");
        let test = write(root, "src/book.test.ts", "import { plan } from './book'\n");
        let report = root.join("target/coverage/typescript");
        let command = typescript_command(root, &[source, test], &report)
            .expect("valid package.json")
            .expect("package has a test script");
        let text = command.join(" ");
        assert!(!text.contains("c8"));
        assert!(text.contains("vitest run --coverage"));
        assert!(text.contains("--coverage.reporter=lcov"));
        assert!(text.contains(&format!("--coverage.reportsDirectory={}", report.display())));
        assert!(text.contains("--coverage.include=src/book.ts"));
        assert!(!text.contains("book.test.ts"));
    }

    #[test]
    fn mu_10_existing_coverage_script_is_left_alone() {
        // MU-10: a project's own `coverage` script is run as is.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{"scripts":{"coverage":"vitest run --coverage","test":"vitest run"}}"#,
        );
        assert_eq!(
            typescript_command(root, &[], &root.join("coverage")),
            Ok(Some(words(&["npm", "run", "coverage"])))
        );
    }

    #[test]
    fn mu_10_node_test_script_still_uses_c8() {
        // MU-10: a plain node test script is wrapped in c8 with LCOV output.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        let report = root.join("target/coverage/typescript");
        let report_text = report.display().to_string();
        assert_eq!(
            typescript_command(root, &[], &report),
            Ok(Some(words(&[
                "npx",
                "--yes",
                "c8",
                "--reporter=lcov",
                "--reports-dir",
                &report_text,
                "npm",
                "test",
            ])))
        );
    }

    #[test]
    fn mu_10_missing_test_script_or_package_json_has_no_coverage_command() {
        // MU-10: without a package.json or a test script there is no coverage command.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let report = root.join("cov");
        assert_eq!(typescript_command(root, &[], &report), Ok(None));
        write(root, "package.json", r#"{"scripts": {}}"#);
        assert_eq!(typescript_command(root, &[], &report), Ok(None));
        write(root, "package.json", r#"{"scripts": ["nope"]}"#);
        assert_eq!(typescript_command(root, &[], &report), Ok(None));
    }

    #[test]
    fn mu_10_invalid_package_json_is_an_error_not_a_missing_script() {
        // MU-10: unreadable or non-object package.json is reported as an Err.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let report = root.join("cov");
        write(root, "package.json", "{");
        let error = typescript_command(root, &[], &report).expect_err("invalid JSON");
        assert!(error.contains("package.json"));
        write(root, "package.json", "[]");
        let error = typescript_command(root, &[], &report).expect_err("array is not an object");
        assert!(error.contains("expected a JSON object"));
    }

    #[test]
    fn mu_10_vitest_include_skips_a_file_outside_the_package() {
        // MU-10: only source files inside the package become coverage includes.
        let tmp = tempdir().expect("tempdir");
        let outside_dir = tempdir().expect("tempdir");
        let package = tmp.path().join("web");
        let inside = write(&package, "src/app.ts", "export const n = 1\n");
        let outside = write(outside_dir.path(), "elsewhere.ts", "export const m = 2\n");
        assert_eq!(
            vitest_includes(&package, &[inside, outside]),
            vec!["src/app.ts".to_owned()]
        );
    }

    #[test]
    fn mu_10_rust_module_is_the_nearest_cargo_package() {
        // MU-10: a Rust source belongs to the nearest Cargo.toml above it.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        write(root, "src-tauri/Cargo.toml", "[package]\nname = \"demo\"\n");
        let lib = write(root, "src-tauri/src/lib.rs", "pub fn read_text() {}\n");
        assert_eq!(
            rust_modules(root, &[lib]),
            vec![resolve(&root.join("src-tauri"))]
        );
    }

    #[test]
    fn mu_10_python_package_without_pytest_uses_unittest() {
        // MU-10: a setup.py project without pytest runs unittest discovery.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "setup.py",
            "from setuptools import setup\nsetup(name='demo')\n",
        );
        let source = write(root, "demo/app.py", "def run():\n    return 1\n");
        assert!(!uses_pytest(root));
        let (run, _lcov) = python_coverage_commands(
            "python3",
            PythonKind::Unittest,
            Path::new("data"),
            Path::new("out"),
            "demo",
        );
        assert!(run.ends_with(&words(&["-m", "unittest", "discover", "-s", "."])));
        assert_eq!(python_roots(root, &[source]), vec![resolve(root)]);
    }

    #[test]
    fn mu_10_python_project_uses_pytest_and_coverage_lcov() {
        // MU-10: a pytest project records coverage with --source and writes lcov.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "pyproject.toml",
            "[tool.pytest.ini_options]\ntestpaths = ['tests']\n",
        );
        let source = write(root, "src/demo/box.py", "def choose(x):\n    return x\n");
        let test = write(
            root,
            "tests/test_box.py",
            "def test_choose():\n    assert True\n",
        );
        assert_eq!(
            python_roots(root, std::slice::from_ref(&source)),
            vec![resolve(root)]
        );
        assert!(uses_pytest(root));
        assert_eq!(python_sources(root, &[source, test.clone()]), "src");
        assert!(is_test_file(&test));
        let data = root.join("target/coverage/python/.coverage");
        let report = root.join("target/coverage/python/lcov.info");
        let data_arg = format!("--data-file={}", data.display());
        let report_arg = report.display().to_string();
        let (run, lcov) =
            python_coverage_commands("python3", PythonKind::Pytest, &data, &report, "src");
        assert_eq!(
            run,
            words(&[
                "python3",
                "-m",
                "coverage",
                "run",
                data_arg.as_str(),
                "--source=src",
                "-m",
                "pytest",
            ])
        );
        assert_eq!(
            lcov,
            words(&[
                "python3",
                "-m",
                "coverage",
                "lcov",
                data_arg.as_str(),
                "-o",
                report_arg.as_str(),
            ])
        );
    }

    #[test]
    fn mu_10_python_executable_prefers_a_project_venv() {
        // MU-10: a project's .venv interpreter is preferred over python3.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let binary = write(root, ".venv/bin/python", "");
        assert_eq!(python_executable(root), binary.display().to_string());
        let bare = tempdir().expect("tempdir");
        assert_eq!(python_executable(bare.path()), "python3");
    }

    #[test]
    fn mu_10_python_sources_ignore_a_file_outside_the_package() {
        // MU-10: a Python file outside the package never adds a --source entry.
        let tmp = tempdir().expect("tempdir");
        let outside_dir = tempdir().expect("tempdir");
        let inside = write(tmp.path(), "src/app.py", "def run():\n    return 1\n");
        let outside = write(outside_dir.path(), "crapper-outside.py", "x = 1\n");
        assert_eq!(python_sources(tmp.path(), &[inside, outside]), "src");
    }

    #[test]
    fn mu_10_python_sources_name_a_top_level_module_without_its_suffix() {
        // MU-10: a top-level module is named without .py for --source.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let module = root.join("carddav.py");
        let test = root.join("test_carddav.py");
        assert_eq!(python_sources(root, &[module, test]), "carddav");
    }

    #[test]
    fn mu_10_rust_coverage_command_for_each_tool() {
        // MU-10: each Rust tool gets its absolute LCOV output path or directory.
        let report = PathBuf::from("/tmp/bookwriter/target/coverage/rust/src-tauri/lcov.info");
        let report_text = report.display().to_string();
        assert_eq!(
            rust_coverage_command(RustTool::LlvmCov, &report),
            words(&["cargo", "llvm-cov", "--lcov", "--output-path", &report_text])
        );
        let tarpaulin = PathBuf::from("/tmp/book/target/coverage/rust/lcov.info");
        assert_eq!(
            rust_coverage_command(RustTool::Tarpaulin, &tarpaulin),
            words(&[
                "cargo",
                "tarpaulin",
                "--out",
                "Lcov",
                "--output-dir",
                "/tmp/book/target/coverage/rust",
            ])
        );
    }

    #[test]
    fn mu_10_nested_module_coverage_report_is_slugged() {
        // MU-10: a nested module's report is slugged; the root module keeps the plain path.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let module = root.join("src-tauri");
        fs::create_dir_all(&module).expect("create module");
        let base = resolve(root).join("target/coverage/rust");
        assert_eq!(
            coverage_report(root, &module, Language::Rust),
            base.join("src-tauri/lcov.info")
        );
        assert_eq!(
            coverage_report(root, root, Language::Rust),
            base.join("lcov.info")
        );
    }

    #[test]
    fn mu_10_go_coverprofile_stays_one_argument() {
        // MU-10: a profile path with a space stays one -coverprofile argument.
        let profile = Path::new("/tmp/my module/target/coverage/go/coverage.out");
        assert_eq!(
            go_test_args(profile),
            words(&[
                "go",
                "test",
                "./...",
                "-coverprofile=/tmp/my module/target/coverage/go/coverage.out",
            ])
        );
    }

    #[test]
    fn mu_10_clean_clojure_keeps_other_language_reports() {
        // MU-10: Clojure cleanup removes Cloverage output and keeps other languages' reports.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let coverage = root.join("target/coverage");
        write(root, "target/coverage/old/index.html", "x");
        write(root, "target/coverage/notes.txt", "keep");
        write(root, "target/coverage/notes/keep.txt", "keep");
        write(root, "target/coverage/python/index.html", "py");
        write(root, "target/coverage/lcov.info", "old");
        clean_clojure(root);
        assert!(!coverage.join("old").exists());
        assert!(!coverage.join("lcov.info").exists());
        assert!(coverage.join("notes.txt").is_file());
        assert!(coverage.join("notes/keep.txt").is_file());
        assert!(coverage.join("python/index.html").is_file());
    }

    #[test]
    fn mu_10_clean_dir_removes_a_tree_and_ignores_a_missing_one() {
        // MU-10: a stale report directory is removed; a missing one is not an error.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        write(root, "target/coverage/x.txt", "x");
        clean_dir(&root.join("target/coverage"));
        assert!(!root.join("target/coverage").exists());
        clean_dir(&root.join("missing"));
    }

    #[test]
    fn mu_10_manifests_are_restored_after_an_install() {
        // MU-10: manifests return to their snapshot after a provider install.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let package_json = write(root, "package.json", "{\"name\":\"demo\"}\n");
        let lock = write(root, "package-lock.json", "{\"lockfileVersion\":3}\n");
        let snapshot = manifest_snapshot(root);
        fs::write(&package_json, "{\"name\":\"changed\"}\n").expect("change manifest");
        fs::remove_file(&lock).expect("remove lock");
        write(root, "npm-shrinkwrap.json", "{}\n");
        restore_manifests(&snapshot);
        assert_eq!(
            fs::read_to_string(&package_json).expect("read manifest"),
            "{\"name\":\"demo\"}\n"
        );
        assert_eq!(
            fs::read_to_string(&lock).expect("read lock"),
            "{\"lockfileVersion\":3}\n"
        );
        assert!(!root.join("npm-shrinkwrap.json").exists());
    }

    #[test]
    fn mu_10_vitest_version_is_read_from_the_installed_package() {
        // MU-10: the installed Vitest version is read; unreadable versions are None.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        assert_eq!(vitest_version(root), None);
        write(root, "node_modules/vitest/package.json", "{not json");
        assert_eq!(vitest_version(root), None);
        write(
            root,
            "node_modules/vitest/package.json",
            "{\"version\": \"4.1.11\"}\n",
        );
        assert_eq!(vitest_version(root), Some("4.1.11".to_owned()));
    }

    #[test]
    fn mu_10_vitest_provider_is_pinned_to_the_installed_version() {
        // MU-10: the provider install is pinned to the installed Vitest version when known.
        assert_eq!(
            vitest_provider_spec(Some("4.1.11")),
            "@vitest/coverage-v8@4.1.11"
        );
        assert_eq!(vitest_provider_spec(None), "@vitest/coverage-v8");
    }

    #[test]
    fn mu_10_echo_quotes_argv_like_shlex() {
        // MU-10: the echoed command quotes argv parts the way shlex.quote does.
        assert_eq!(quote("plain-arg.rs"), "plain-arg.rs");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\"'\"'s'");
        assert_eq!(quote(""), "''");
        assert_eq!(show(&argv(&["true", "a;touch x"])), "true 'a;touch x'");
        assert_eq!(show(&Command::Shell("exit 3".to_owned())), "exit 3");
    }

    #[test]
    fn mu_10_run_shell_returns_status_and_never_splits_argv() {
        // MU-10: a shell string returns its status, a missing program is 127, and an
        // argument vector never runs a second command.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        assert_eq!(run_shell(&Command::Shell("exit 3".to_owned()), root), 3);
        assert_eq!(run_shell(&argv(&["mu-no-such-program-xyz"]), root), 127);
        let marker = root.join("pwned");
        let marker_arg = format!("a;touch {}", marker.display());
        assert_eq!(run_shell(&argv(&["true", marker_arg.as_str()]), root), 0);
        assert!(!marker.exists());
    }

    #[test]
    fn mu_11_empty_path_finds_neither_rust_tool() {
        // MU-11: with an empty PATH directory neither cargo-llvm-cov nor cargo-tarpaulin is found.
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().as_os_str();
        assert_eq!(which_in("cargo-llvm-cov", path), None);
        assert_eq!(which_in("cargo-tarpaulin", path), None);
        assert_eq!(installed_rust_tool(path), None);
    }

    #[test]
    fn mu_11_which_in_needs_an_executable_file() {
        // MU-11: a file without the executable bit is not a tool on the PATH.
        let tmp = tempdir().expect("tempdir");
        fake_tool(tmp.path(), "cargo-llvm-cov", 0o644);
        assert_eq!(which_in("cargo-llvm-cov", tmp.path().as_os_str()), None);
        fake_tool(tmp.path(), "cargo-llvm-cov", 0o755);
        assert_eq!(
            which_in("cargo-llvm-cov", tmp.path().as_os_str()),
            Some(tmp.path().join("cargo-llvm-cov"))
        );
    }

    #[test]
    fn mu_11_installed_tool_prefers_llvm_cov() {
        // MU-11: an installed cargo-llvm-cov wins over cargo-tarpaulin.
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().as_os_str();
        fake_tool(tmp.path(), "cargo-tarpaulin", 0o755);
        assert_eq!(installed_rust_tool(path), Some(RustTool::Tarpaulin));
        fake_tool(tmp.path(), "cargo-llvm-cov", 0o755);
        assert_eq!(installed_rust_tool(path), Some(RustTool::LlvmCov));
    }

    #[test]
    fn mu_11_install_notice_names_the_tool_it_installs() {
        // MU-11: the notice text says neither tool is installed and names cargo-llvm-cov.
        assert_eq!(
            rust_install_notice(),
            "Neither cargo-llvm-cov nor cargo-tarpaulin is installed. Installing cargo-llvm-cov."
        );
    }

    #[test]
    fn mu_11_notice_is_emitted_before_the_install_command() {
        // MU-11: the install plan prints the notice before any install command runs.
        let install = Step::Run {
            command: argv(&["cargo", "install", "cargo-llvm-cov", "--locked"]),
            failure: None,
        };
        let rustup = Step::Run {
            command: argv(&["rustup", "component", "add", "llvm-tools-preview"]),
            failure: Some(Failure::Text(
                "rustup component add llvm-tools-preview failed.".to_owned(),
            )),
        };
        let notice = Step::Notice(rust_install_notice().to_owned());
        assert_eq!(
            rust_install_plan(true),
            vec![notice.clone(), rustup, install.clone()]
        );
        assert_eq!(rust_install_plan(false), vec![notice, install]);
    }

    // MU-10: a lookup error or a missing test script produces only a notice, never a command.
    #[test]
    fn mu_10_typescript_plan_without_command_only_notices() {
        let package = Path::new("pkg");
        let report = Path::new("pkg/target/coverage/typescript/lcov.info");
        assert_eq!(
            typescript_plan(package, report, Err("bad json".to_owned())),
            vec![Step::Notice("bad json".to_owned())]
        );
        assert_eq!(
            typescript_plan(package, report, Ok(None)),
            vec![Step::Notice(
                "No package.json test script in pkg; skipping TypeScript coverage.".to_owned()
            )]
        );
    }

    // MU-10: without a missing provider, the plan prepares the report and runs the command.
    #[test]
    fn mu_10_typescript_plan_without_provider_prepares_then_runs() {
        let package = Path::new("pkg");
        let report = Path::new("pkg/target/coverage/typescript/lcov.info");
        let steps = typescript_plan(package, report, Ok(Some(words(&["npm", "test"]))));
        assert_eq!(
            steps,
            vec![
                Step::Disk(Disk::Prepare(report.to_path_buf())),
                Step::Run {
                    command: Command::Argv(words(&["npm", "test"])),
                    failure: Some(Failure::Exit {
                        before: "TypeScript coverage exited ".to_owned(),
                        after: " in pkg. TypeScript coverage will score 0%.".to_owned(),
                    }),
                },
            ]
        );
    }

    // MU-10: with no provider installed, the plan installs it, restores manifests, requires it, then runs.
    #[test]
    fn mu_10_typescript_plan_with_provider_installs_before_running() {
        let package = Path::new("pkg");
        let report = Path::new("pkg/target/coverage/typescript/lcov.info");
        let manifests: ManifestSnapshot = MANIFESTS
            .iter()
            .map(|name| (package.join(name), None))
            .collect();
        let steps = typescript_plan(package, report, Ok(Some(words(&["npx", "vitest"]))));
        assert_eq!(
            steps,
            vec![
                Step::Notice(
                    "Installing @vitest/coverage-v8 so Vitest can write LCOV. package.json and the lockfile are restored afterward.".to_owned()
                ),
                Step::Run {
                    command: argv(&[
                        "npm",
                        "install",
                        "--no-save",
                        "--no-package-lock",
                        "@vitest/coverage-v8",
                    ]),
                    failure: None,
                },
                Step::Disk(Disk::Restore(manifests)),
                Step::Require {
                    check: Check::Installed(package.join(VITEST_PROVIDER)),
                    message: "Vitest's coverage provider is missing. TypeScript coverage will score 0%.".to_owned(),
                },
                Step::Disk(Disk::Prepare(report.to_path_buf())),
                Step::Run {
                    command: Command::Argv(words(&["npx", "vitest"])),
                    failure: Some(Failure::Exit {
                        before: "TypeScript coverage exited ".to_owned(),
                        after: " in pkg. TypeScript coverage will score 0%.".to_owned(),
                    }),
                },
            ]
        );
    }

    // MU-10: without pytest, the Python run plan is coverage check, reset, remove, then unittest.
    #[test]
    fn mu_10_python_run_plan_without_pytest_runs_unittest_last() {
        let package = Path::new("pkg");
        let data = Path::new("pkg/target/coverage/python/.coverage");
        let report = Path::new("pkg/target/coverage/python/lcov.info");
        let steps = python_run_plan("python3", package, false, data, report, "src");
        let (unittest, _) =
            python_coverage_commands("python3", PythonKind::Unittest, data, report, "src");
        assert_eq!(steps.len(), 4);
        assert!(matches!(steps[0], Step::Branch { .. }));
        assert_eq!(steps[1], Step::Disk(Disk::Reset(report.to_path_buf())));
        assert_eq!(steps[2], Step::Disk(Disk::Remove(data.to_path_buf())));
        assert_eq!(
            steps[3],
            Step::Run {
                command: Command::Argv(unittest),
                failure: None,
            }
        );
    }

    // MU-10: with pytest in use, the run plan ends in a branch that picks pytest or falls back.
    #[test]
    fn mu_10_python_run_plan_with_pytest_branches_at_the_end() {
        let package = Path::new("pkg");
        let data = Path::new("pkg/target/coverage/python/.coverage");
        let report = Path::new("pkg/target/coverage/python/lcov.info");
        let steps = python_run_plan("python3", package, true, data, report, "src");
        assert_eq!(steps.len(), 4);
        assert!(matches!(steps[3], Step::Branch { .. }));
    }

    // MU-10: no data file and a zero exit records nothing and warns nothing.
    #[test]
    fn mu_10_python_record_plan_without_data_and_success_is_empty() {
        let package = Path::new("pkg");
        let lcov = argv(&["coverage", "lcov"]);
        assert_eq!(python_record_plan(package, 0, false, lcov), Vec::new());
    }

    // MU-10: no data file and a nonzero exit reports the exit code.
    #[test]
    fn mu_10_python_record_plan_without_data_reports_nonzero_exit() {
        let package = Path::new("pkg");
        let lcov = argv(&["coverage", "lcov"]);
        assert_eq!(
            python_record_plan(package, 3, false, lcov),
            vec![Step::Notice(
                "Python coverage exited 3 in pkg. Python coverage will score 0%.".to_owned()
            )]
        );
    }

    // MU-10: with data, LCOV runs and a zero test exit adds no warning.
    #[test]
    fn mu_10_python_record_plan_with_data_and_success_records_lcov() {
        let package = Path::new("pkg");
        let lcov = argv(&["coverage", "lcov"]);
        assert_eq!(
            python_record_plan(package, 0, true, lcov.clone()),
            vec![
                Step::Run {
                    command: lcov,
                    failure: Some(Failure::Exit {
                        before: "coverage lcov exited ".to_owned(),
                        after: " in pkg. Python coverage will score 0%.".to_owned(),
                    }),
                },
                Step::Branch {
                    check: Check::LastRunOk,
                    then: Vec::new(),
                    otherwise: Vec::new(),
                },
            ]
        );
    }

    // MU-10: with data and a nonzero test exit, LCOV is still recorded and the test exit is warned.
    #[test]
    fn mu_10_python_record_plan_with_data_warns_about_failed_tests() {
        let package = Path::new("pkg");
        let lcov = argv(&["coverage", "lcov"]);
        let steps = python_record_plan(package, 2, true, lcov);
        assert_eq!(
            steps[1],
            Step::Branch {
                check: Check::LastRunOk,
                then: vec![Step::Notice(
                    "Python tests exited 2 in pkg. Coverage was still recorded.".to_owned()
                )],
                otherwise: Vec::new(),
            }
        );
    }

    // MU-10: failure text renders the exit code only for the Exit form.
    #[test]
    fn mu_10_failure_render_uses_code_only_for_exit() {
        assert_eq!(Failure::Text("plain".to_owned()).render(9), "plain");
        let exit = Failure::Exit {
            before: "a ".to_owned(),
            after: " b".to_owned(),
        };
        assert_eq!(exit.render(9), "a 9 b");
    }

    // MU-10: execute returns the last Run's status.
    #[test]
    fn mu_10_execute_returns_last_run_status() {
        let steps = vec![
            run(Command::Shell("exit 3".to_owned())),
            run(Command::Shell("exit 5".to_owned())),
        ];
        assert_eq!(execute(&steps, Path::new(".")), Some(5));
    }

    // MU-10: a failed Require stops the plan and execute returns None.
    #[test]
    fn mu_10_execute_require_failure_aborts_plan() {
        let steps = vec![
            Step::Require {
                check: Check::Command(Command::Shell("exit 1".to_owned())),
                message: "missing".to_owned(),
            },
            Step::Notice("never printed".to_owned()),
        ];
        assert_eq!(execute(&steps, Path::new(".")), None);
    }

    // MU-10: a Branch runs the then-steps when its check holds and the otherwise-steps when not.
    #[test]
    fn mu_10_execute_branch_picks_then_or_otherwise() {
        let branch = |exit: &str| {
            vec![Step::Branch {
                check: Check::Command(Command::Shell(exit.to_owned())),
                then: vec![run(Command::Shell("exit 5".to_owned()))],
                otherwise: vec![run(Command::Shell("exit 6".to_owned()))],
            }]
        };
        assert_eq!(execute(&branch("exit 0"), Path::new(".")), Some(5));
        assert_eq!(execute(&branch("exit 1"), Path::new(".")), Some(6));
    }

    // MU-10: Installed holds only when the last run succeeded and the directory exists.
    #[test]
    fn mu_10_installed_check_needs_success_and_directory() {
        let dir = tempdir().unwrap();
        let present = Check::Installed(dir.path().to_path_buf());
        let absent = Check::Installed(dir.path().join("missing"));
        assert!(holds(&present, dir.path(), 0));
        assert!(!holds(&present, dir.path(), 2));
        assert!(!holds(&absent, dir.path(), 0));
    }

    // MU-10: a Require over Installed aborts when the last run failed, even with the directory present.
    #[test]
    fn mu_10_execute_installed_require_aborts_after_failed_install() {
        let dir = tempdir().unwrap();
        let steps = vec![
            run(Command::Shell("exit 2".to_owned())),
            Step::Require {
                check: Check::Installed(dir.path().to_path_buf()),
                message: "missing".to_owned(),
            },
        ];
        assert_eq!(execute(&steps, dir.path()), None);
    }

    // MU-10: Disk::Remove deletes the file; Disk::Reset keeps the directory but drops the report.
    #[test]
    fn mu_10_disk_steps_remove_and_reset_files() {
        let dir = tempdir().unwrap();
        let data = dir.path().join(".coverage");
        let report = dir.path().join("lcov.info");
        fs::write(&data, b"x").unwrap();
        fs::write(&report, b"x").unwrap();
        let steps = vec![
            Step::Disk(Disk::Remove(data.clone())),
            Step::Disk(Disk::Reset(report.clone())),
        ];
        assert_eq!(execute(&steps, dir.path()), Some(0));
        assert!(!data.exists());
        assert!(!report.exists());
        assert!(dir.path().is_dir());
    }

    // MU-10: Disk::Prepare creates the report's directory.
    #[test]
    fn mu_10_disk_prepare_creates_report_directory() {
        let dir = tempdir().unwrap();
        let report = dir.path().join("target/lcov.info");
        execute(&[Step::Disk(Disk::Prepare(report))], dir.path());
        assert!(dir.path().join("target").is_dir());
    }

    // MU-10: Disk::Restore rewrites recorded manifests and removes those that were absent.
    #[test]
    fn mu_10_disk_restore_rewrites_or_removes_manifests() {
        let dir = tempdir().unwrap();
        let kept = dir.path().join("package.json");
        let added = dir.path().join("package-lock.json");
        fs::write(&kept, b"changed").unwrap();
        fs::write(&added, b"new").unwrap();
        let snapshot: ManifestSnapshot = vec![
            (kept.clone(), Some(b"original".to_vec())),
            (added.clone(), None),
        ];
        execute(&[Step::Disk(Disk::Restore(snapshot))], dir.path());
        assert_eq!(fs::read(&kept).unwrap(), b"original");
        assert!(!added.exists());
    }

    // MU-10: without deps.edn or bb.edn, Clojure only prints the skip notice.
    #[test]
    fn mu_10_clojure_plan_without_markers_only_notices() {
        let dir = tempdir().unwrap();
        assert_eq!(
            clojure_plan(dir.path()),
            vec![Step::Notice(
                "No deps.edn or bb.edn; skipping Clojure coverage.".to_owned()
            )]
        );
    }

    // MU-10: with deps.edn, Clojure cleans its report, tries the lcov alias, then falls back.
    #[test]
    fn mu_10_clojure_plan_with_markers_cleans_then_falls_back() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("deps.edn"), b"{}").unwrap();
        let plan = clojure_plan(dir.path());
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0], Step::Disk(Disk::Clojure(dir.path().to_path_buf())));
        assert_eq!(
            plan[1],
            Step::Branch {
                check: Check::Command(argv(CLOJURE_LCOV)),
                then: Vec::new(),
                otherwise: vec![
                    Step::Notice("clj -M:cov --lcov failed; retrying without --lcov.".to_owned()),
                    Step::Run {
                        command: argv(CLOJURE_PLAIN),
                        failure: Some(Failure::Exit {
                            before: "Clojure coverage exited ".to_owned(),
                            after: ". Clojure coverage will score 0%.".to_owned(),
                        }),
                    },
                ],
            }
        );
    }

    // MU-10: a Java module cleans its JaCoCo report, removes the exec file, then runs Maven.
    #[test]
    fn mu_10_java_module_plan_cleans_then_runs_maven() {
        let module = Path::new("svc");
        assert_eq!(
            java_module_plan(module),
            vec![
                Step::Disk(Disk::Clean(module.join("target/site/jacoco"))),
                Step::Disk(Disk::Remove(module.join("target/jacoco.exec"))),
                Step::Run {
                    command: argv(MAVEN),
                    failure: Some(Failure::Exit {
                        before: "Java coverage exited ".to_owned(),
                        after: " in svc. Java coverage will score 0%.".to_owned(),
                    }),
                },
            ]
        );
    }

    // MU-10: a Go module resets its profile, then runs go test writing that profile.
    #[test]
    fn mu_10_go_module_plan_resets_profile_then_tests() {
        let module = Path::new("svc");
        let profile = module.join("target/coverage/go/coverage.out");
        assert_eq!(
            go_module_plan(module),
            vec![
                Step::Disk(Disk::Reset(profile.clone())),
                Step::Run {
                    command: Command::Argv(go_test_args(&profile)),
                    failure: Some(Failure::Exit {
                        before: "Go coverage exited ".to_owned(),
                        after: " in svc. Go coverage will score 0%.".to_owned(),
                    }),
                },
            ]
        );
    }

    // MU-10: a Rust module resets its report, then runs the chosen coverage tool.
    #[test]
    fn mu_10_rust_module_plan_resets_report_then_runs_tool() {
        let root = Path::new("/root");
        let module = Path::new("/root/crate");
        let report = coverage_report(root, module, Language::Rust);
        assert_eq!(
            rust_module_plan(root, module, RustTool::LlvmCov),
            vec![
                Step::Disk(Disk::Reset(report.clone())),
                Step::Run {
                    command: Command::Argv(rust_coverage_command(RustTool::LlvmCov, &report)),
                    failure: Some(Failure::Exit {
                        before: "Rust coverage exited ".to_owned(),
                        after: " in /root/crate. Rust coverage will score 0%.".to_owned(),
                    }),
                },
            ]
        );
    }

    // MU-10: scored_exit names the language and module in its exit line.
    #[test]
    fn mu_10_scored_exit_names_language_and_module() {
        assert_eq!(
            scored_exit("Go", Path::new("svc")).render(4),
            "Go coverage exited 4 in svc. Go coverage will score 0%."
        );
    }

    // MU-10: cover_modules runs one plan inside each module.
    #[test]
    fn mu_10_cover_modules_runs_plan_in_each_module() {
        fn prepare_out(module: &Path) -> Vec<Step> {
            vec![Step::Disk(Disk::Prepare(module.join("out/lcov.info")))]
        }
        let dir = tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        cover_modules(dir.path(), vec![a.clone(), b.clone()], "none", prepare_out);
        assert!(a.join("out").is_dir());
        assert!(b.join("out").is_dir());
    }

    // MU-10: with no modules, cover_modules prints the notice and plans nothing.
    #[test]
    fn mu_10_cover_modules_without_modules_plans_nothing() {
        fn never(_: &Path) -> Vec<Step> {
            panic!("no module should be planned")
        }
        let dir = tempdir().unwrap();
        cover_modules(dir.path(), Vec::new(), "none", never);
    }

    // MU-10: the Clojure, Java, Go, and Rust entry points skip when their markers are absent.
    #[test]
    fn mu_10_entry_points_skip_without_markers() {
        let dir = tempdir().unwrap();
        cover_clojure(dir.path());
        cover_language(Language::Java, dir.path(), &[]);
        cover_language(Language::Go, dir.path(), &[]);
        cover_rust(dir.path(), &[]);
        assert!(!dir.path().join("target").exists());
    }

    // MU-10: Disk::Clean empties a directory and Disk::Clojure drops the top-level lcov.
    #[test]
    fn mu_10_disk_clean_and_clojure_remove_reports() {
        let dir = tempdir().unwrap();
        let cleaned = dir.path().join("jacoco");
        fs::create_dir_all(&cleaned).unwrap();
        fs::write(cleaned.join("index.html"), b"x").unwrap();
        let lcov = dir.path().join("target/coverage/lcov.info");
        fs::create_dir_all(lcov.parent().unwrap()).unwrap();
        fs::write(&lcov, b"x").unwrap();
        let steps = vec![
            Step::Disk(Disk::Clean(cleaned.clone())),
            Step::Disk(Disk::Clojure(dir.path().to_path_buf())),
        ];
        execute(&steps, dir.path());
        assert!(!cleaned.join("index.html").exists());
        assert!(!lcov.exists());
    }

    // MU-11: each Language arm reaches its own coverage function; with no files, every arm only skips.
    #[test]
    fn mu_11_cover_language_each_arm_skips_without_files() {
        let dir = tempdir().unwrap();
        for language in [
            Language::Clojure,
            Language::Java,
            Language::Go,
            Language::TypeScript,
            Language::Python,
            Language::Rust,
        ] {
            cover_language(language, dir.path(), &[]);
        }
        assert!(!dir.path().join("target").exists());
    }

    // MU-11: a TypeScript package with no test script plans only a notice, so no tool runs.
    #[test]
    fn mu_11_cover_typescript_skips_package_without_test_script() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            br#"{"scripts":{"build":"tsc"}}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let file = dir.path().join("src/index.ts");
        fs::write(&file, b"export {};").unwrap();
        cover_typescript(dir.path(), &[file]);
        assert!(!dir.path().join("target").exists());
    }

    // MU-11: a package.json that is not an object plans only the lookup error, so no tool runs.
    #[test]
    fn mu_11_cover_typescript_package_reports_bad_package_json() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), b"[1]").unwrap();
        cover_typescript_package(dir.path(), &[], dir.path());
        assert!(!dir.path().join("target").exists());
    }

    // MU-11: with no TypeScript files, cover_typescript warns and plans no package.
    #[test]
    fn mu_11_cover_typescript_without_files_warns_and_returns() {
        let dir = tempdir().unwrap();
        cover_typescript(dir.path(), &[]);
        assert!(!dir.path().join("target").exists());
    }
}
