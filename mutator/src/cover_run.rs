//! Port of crapper/src/crapper/runners.py (BSD Zero Clause License, Robert C. Martin).

use crate::discover::{is_test_file, language_of};
use crate::model::Language;
use crate::runner::Command;
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

/// One planned action: a notice line, or a command with an optional failure line.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    Notice(&'static str),
    Run {
        command: Command,
        on_failure: Option<&'static str>,
    },
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

/// Python's `Path.resolve`: canonical for existing paths, and for a missing file the
/// canonical parent joined with the file name.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => resolve(parent).join(name),
        _ => std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
    }
}

/// The nearest ancestor of `start` (or its parent when a file) that holds `marker`, up to `stop`.
fn nearest(start: &Path, marker: &str, stop: &Path) -> Option<PathBuf> {
    let stop = resolve(stop);
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    loop {
        if current.join(marker).is_file() {
            return Some(current);
        }
        if current == stop {
            return None;
        }
        current = current.parent()?.to_path_buf();
    }
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

fn report_exit(language: &str, code: i32, module: &Path) {
    if code != 0 {
        warn(&format!(
            "{language} coverage exited {code} in {}. {language} coverage will score 0%.",
            module.display()
        ));
    }
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
        Language::Java => cover_java(root, files),
        Language::Go => cover_go(root, files),
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

fn cover_clojure(root: &Path) {
    if !root.join("deps.edn").is_file() && !root.join("bb.edn").is_file() {
        warn("No deps.edn or bb.edn; skipping Clojure coverage.");
        return;
    }
    clean_clojure(root);
    if run_shell(&argv(CLOJURE_LCOV), root) == 0 {
        return;
    }
    warn("clj -M:cov --lcov failed; retrying without --lcov.");
    let code = run_shell(&argv(CLOJURE_PLAIN), root);
    if code != 0 {
        warn(&format!(
            "Clojure coverage exited {code}. Clojure coverage will score 0%."
        ));
    }
}

// Java

fn cover_java(root: &Path, files: &[PathBuf]) {
    let modules = modules_with(files, "java", "pom.xml", root);
    if modules.is_empty() {
        warn("No pom.xml; skipping Java coverage.");
        return;
    }
    for module in modules {
        clean_dir(&module.join("target/site/jacoco"));
        remove_file_if_present(&module.join("target/jacoco.exec"));
        report_exit("Java", run_shell(&argv(MAVEN), &module), &module);
    }
}

// Go

fn go_test_args(profile: &Path) -> Vec<String> {
    let mut args = words(&["go", "test", "./..."]);
    args.push(format!("-coverprofile={}", profile.display()));
    args
}

fn cover_go(root: &Path, files: &[PathBuf]) {
    let modules = modules_with(files, "go", "go.mod", root);
    if modules.is_empty() {
        warn("No go.mod; skipping Go coverage.");
        return;
    }
    for module in modules {
        let profile = module.join("target/coverage/go/coverage.out");
        reset_report(&profile);
        let code = run_shell(&Command::Argv(go_test_args(&profile)), &module);
        report_exit("Go", code, &module);
    }
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

/// Installs Vitest's coverage provider without keeping manifest changes; true when present.
fn ensure_vitest_coverage(package: &Path) -> bool {
    let provider = package.join(VITEST_PROVIDER);
    if provider.is_dir() {
        return true;
    }
    let spec = vitest_provider_spec(vitest_version(package).as_deref());
    warn(&format!(
        "Installing {spec} so Vitest can write LCOV. package.json and the lockfile are restored afterward."
    ));
    let snapshot = manifest_snapshot(package);
    let code = run_shell(
        &argv(&[
            "npm",
            "install",
            "--no-save",
            "--no-package-lock",
            spec.as_str(),
        ]),
        package,
    );
    restore_manifests(&snapshot);
    let ready = code == 0 && provider.is_dir();
    if !ready {
        warn("Vitest's coverage provider is missing. TypeScript coverage will score 0%.");
    }
    ready
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

fn typescript_plan(package: &Path, files: &[PathBuf], report: &Path) -> Option<Vec<String>> {
    match typescript_command(package, files, report_dir(report)) {
        Ok(Some(command)) => Some(command),
        Ok(None) => {
            warn(&format!(
                "No package.json test script in {}; skipping TypeScript coverage.",
                package.display()
            ));
            None
        }
        Err(message) => {
            warn(&message);
            None
        }
    }
}

fn cover_typescript_package(root: &Path, files: &[PathBuf], package: &Path) {
    let report = coverage_report(root, package, Language::TypeScript);
    let Some(command) = typescript_plan(package, files, &report) else {
        return;
    };
    if needs_vitest_provider(&command) && !ensure_vitest_coverage(package) {
        return;
    }
    prepare_report(&report);
    report_exit(
        "TypeScript",
        run_shell(&Command::Argv(command), package),
        package,
    );
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

fn python_module_for(file: &Path, root: &Path) -> PathBuf {
    PYTHON_MARKERS
        .iter()
        .find_map(|marker| nearest(file, marker, root))
        .unwrap_or_else(|| root.to_path_buf())
}

fn python_roots(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    let roots: BTreeSet<PathBuf> = files
        .iter()
        .filter(|file| file.extension() == Some(OsStr::new("py")))
        .map(|file| resolve(&python_module_for(file, &root)))
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

fn ensure_python_module(py: &str, package: &Path, module: &str) -> bool {
    let probe = format!("import {module}");
    if run_shell(&argv(&[py, "-c", probe.as_str()]), package) == 0 {
        return true;
    }
    warn(&format!(
        "Installing {module} for Python coverage. The project requirements are left unchanged."
    ));
    run_shell(
        &argv(&[
            py,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            module,
        ]),
        package,
    ) == 0
}

fn python_kind(py: &str, package: &Path) -> PythonKind {
    if !uses_pytest(package) {
        return PythonKind::Unittest;
    }
    if ensure_python_module(py, package, "pytest") {
        return PythonKind::Pytest;
    }
    warn(&format!(
        "pytest is missing in {}. Falling back to unittest.",
        package.display()
    ));
    PythonKind::Unittest
}

fn record_python_lcov(package: &Path, code: i32, data_file: &Path, lcov: &Command) {
    if !data_file.exists() {
        report_exit("Python", code, package);
        return;
    }
    let lcov_code = run_shell(lcov, package);
    if lcov_code != 0 {
        warn(&format!(
            "coverage lcov exited {lcov_code} in {}. Python coverage will score 0%.",
            package.display()
        ));
    } else if code != 0 {
        warn(&format!(
            "Python tests exited {code} in {}. Coverage was still recorded.",
            package.display()
        ));
    }
}

fn cover_python_package(root: &Path, files: &[PathBuf], package: &Path) {
    let report = coverage_report(root, package, Language::Python);
    let py = python_executable(package);
    if !ensure_python_module(&py, package, "coverage") {
        warn(&format!(
            "coverage is missing in {}. Python coverage will score 0%.",
            package.display()
        ));
        return;
    }
    reset_report(&report);
    let data_file = report_dir(&report).join(".coverage");
    remove_file_if_present(&data_file);
    let (run, lcov) = python_coverage_commands(
        &py,
        python_kind(&py, package),
        &data_file,
        &report,
        &python_sources(package, files),
    );
    let code = run_shell(&Command::Argv(run), package);
    record_python_lcov(package, code, &data_file, &Command::Argv(lcov));
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
    let mut plan = vec![Step::Notice(rust_install_notice())];
    if has_rustup {
        plan.push(Step::Run {
            command: argv(&["rustup", "component", "add", "llvm-tools-preview"]),
            on_failure: Some("rustup component add llvm-tools-preview failed."),
        });
    }
    plan.push(Step::Run {
        command: argv(&["cargo", "install", "cargo-llvm-cov", "--locked"]),
        on_failure: None,
    });
    plan
}

fn run_step(step: &Step, cwd: &Path) -> i32 {
    match step {
        Step::Notice(text) => {
            warn(text);
            0
        }
        Step::Run {
            command,
            on_failure,
        } => {
            let code = run_shell(command, cwd);
            if let Some(message) = on_failure.filter(|_| code != 0) {
                warn(message);
            }
            code
        }
    }
}

/// Runs the plan in order and returns the last step's status (the install itself).
fn run_plan(plan: &[Step], cwd: &Path) -> i32 {
    plan.iter().fold(0, |_, step| run_step(step, cwd))
}

fn install_llvm_cov() -> Option<RustTool> {
    let plan = rust_install_plan(which("rustup").is_some());
    let cwd = env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let code = run_plan(&plan, &cwd);
    if code == 0 && which("cargo-llvm-cov").is_some() {
        return Some(RustTool::LlvmCov);
    }
    warn("Rust coverage will score 0%.");
    None
}

fn rust_kind() -> Option<RustTool> {
    let path_var = env::var_os("PATH").unwrap_or_default();
    installed_rust_tool(&path_var).or_else(install_llvm_cov)
}

fn cover_rust_module(root: &Path, module: &Path, tool: RustTool) {
    let report = coverage_report(root, module, Language::Rust);
    reset_report(&report);
    let code = run_shell(&Command::Argv(rust_coverage_command(tool, &report)), module);
    report_exit("Rust", code, module);
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
            on_failure: None,
        };
        let rustup = Step::Run {
            command: argv(&["rustup", "component", "add", "llvm-tools-preview"]),
            on_failure: Some("rustup component add llvm-tools-preview failed."),
        };
        let notice = Step::Notice(rust_install_notice());
        assert_eq!(
            rust_install_plan(true),
            vec![notice.clone(), rustup, install.clone()]
        );
        assert_eq!(rust_install_plan(false), vec![notice, install]);
    }
}
