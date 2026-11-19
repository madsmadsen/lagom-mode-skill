// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/runners.py

//! TypeScript coverage: pick each package's test runner and collect LCOV.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::{coverage_report, exit_warning, nearest, run_shell, warn, Cmd};
use crate::discover::{is_test_file, language_of};

const VITEST_CONFIGS: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
];

/// Files restored after an install: the manifest and lockfile of every supported package manager.
const MANIFESTS: &[&str] = &[
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "bun.lock",
];

static SEMVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
        .expect("valid SemVer regex")
});

/// Manifest name and its bytes before an install (`None` when absent).
type ManifestSnapshot = Vec<(PathBuf, Option<Vec<u8>>)>;

/// package.json is present and cannot be read as an object.
#[derive(Debug)]
struct PackageJsonError(String);

pub(crate) fn cover_typescript(root: &Path, files: &[PathBuf]) -> Result<(), String> {
    cover_typescript_with(root, files, &mut run_shell)
}

fn cover_typescript_with(
    root: &Path,
    files: &[PathBuf],
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) -> Result<(), String> {
    let packages = typescript_packages(root, files);
    if packages.is_empty() {
        warn("No package.json; skipping TypeScript coverage.");
        return Ok(());
    }
    for package in &packages {
        cover_package_with(root, package, files, exec)?;
    }
    Ok(())
}

fn cover_package_with(
    root: &Path,
    package: &Path,
    files: &[PathBuf],
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) -> Result<(), String> {
    let Some((report_dir, command)) = plan_package(root, package, files)? else {
        return Ok(());
    };
    if let Some(command) = ready_for_run(package, &report_dir, command, exec) {
        run_report(package, command, exec);
    }
    Ok(())
}

/// The report directory and command for one package; `None` when the package is skipped.
fn plan_package(
    root: &Path,
    package: &Path,
    files: &[PathBuf],
) -> Result<Option<(PathBuf, Vec<String>)>, String> {
    let report = coverage_report(root, package, "typescript")?;
    let Some(report_dir) = report.parent().map(Path::to_path_buf) else {
        return Ok(None);
    };
    Ok(planned_command(package, files, &report_dir).map(|command| (report_dir, command)))
}

fn planned_command(package: &Path, files: &[PathBuf], report_dir: &Path) -> Option<Vec<String>> {
    match typescript_command(package, files, report_dir) {
        Ok(Some(command)) => Some(command),
        Ok(None) => {
            warn(&format!(
                "No package.json test script in {}; skipping TypeScript coverage.",
                package.display()
            ));
            None
        }
        Err(error) => {
            warn(&error.0);
            None
        }
    }
}

/// Makes the package ready for `command`: provider installed and report directory empty.
/// `None` skips the package.
fn ready_for_run(
    package: &Path,
    report_dir: &Path,
    command: Vec<String>,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) -> Option<Vec<String>> {
    if !provider_ready(package, &command, exec) {
        return None;
    }
    if let Err(error) = prepare_report(report_dir) {
        warn(&format!(
            "Could not prepare {}: {error}",
            report_dir.display()
        ));
        return None;
    }
    Some(command)
}

/// Runs `command` and reports its exit status. A command that downloads its tool is announced
/// on stderr before it starts (CR-15).
fn run_report(package: &Path, command: Vec<String>, exec: &mut dyn FnMut(&Cmd, &Path) -> i32) {
    if let Some(notice) = install_notice(&command) {
        warn(&notice);
    }
    let code = exec(&Cmd::Argv(command), package);
    if let Some(message) = exit_warning("TypeScript", code, Some(package)) {
        warn(&message);
    }
}

fn provider_ready(
    package: &Path,
    command: &[String],
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) -> bool {
    !needs_vitest_provider(command) || ensure_vitest_coverage_with(package, exec)
}

fn prepare_report(report_dir: &Path) -> io::Result<()> {
    if report_dir.exists() {
        fs::remove_dir_all(report_dir)?;
    }
    fs::create_dir_all(report_dir)
}

fn needs_vitest_provider(command: &[String]) -> bool {
    if command.starts_with(&strings(&["npm", "run", "coverage"])) {
        return false;
    }
    command
        .iter()
        .any(|part| Path::new(part).file_name() == Some(OsStr::new("vitest")))
}

fn ensure_vitest_coverage_with(package: &Path, exec: &mut dyn FnMut(&Cmd, &Path) -> i32) -> bool {
    if provider_installed(package) {
        return true;
    }
    let spec = vitest_provider_spec(vitest_version(package).as_deref());
    warn(&vitest_install_notice(&spec));
    install_provider(package, &spec, exec)
}

fn install_provider(package: &Path, spec: &str, exec: &mut dyn FnMut(&Cmd, &Path) -> i32) -> bool {
    let install = vitest_install_command(package, spec);
    let code = restoring_manifests(package, || exec(&Cmd::Argv(install), package));
    let ready = code == Some(0) && provider_installed(package);
    if let Some(message) = provider_missing_warning(ready) {
        warn(message);
    }
    ready
}

/// The warning for a package whose coverage provider is not ready; `None` when it is.
fn provider_missing_warning(ready: bool) -> Option<&'static str> {
    (!ready).then_some("Vitest's coverage provider is missing. TypeScript coverage will score 0%.")
}

fn provider_installed(package: &Path) -> bool {
    package.join("node_modules/@vitest/coverage-v8").is_dir()
}

/// Runs `install` and then puts every manifest back the way it was before it ran.
/// Returns `None` (without running `install`) when the manifests cannot be read.
fn restoring_manifests<T>(package: &Path, install: impl FnOnce() -> T) -> Option<T> {
    let snapshot = manifest_snapshot(package)
        .map_err(|error| {
            warn(&format!(
                "Could not read package manifests in {}: {error}",
                package.display()
            ));
        })
        .ok()?;
    let result = install();
    restore_all(&snapshot);
    Some(result)
}

fn restore_all(snapshot: &ManifestSnapshot) {
    for (path, content) in snapshot {
        if let Err(error) = restore_one(path, content) {
            warn(&format!("Could not restore {}: {error}", path.display()));
        }
    }
}

fn manifest_snapshot(package: &Path) -> io::Result<ManifestSnapshot> {
    MANIFESTS
        .iter()
        .map(|name| -> io::Result<(PathBuf, Option<Vec<u8>>)> {
            let path = package.join(name);
            let content = if path.is_file() {
                Some(fs::read(&path)?)
            } else {
                None
            };
            Ok((path, content))
        })
        .collect()
}

fn restore_one(path: &Path, content: &Option<Vec<u8>>) -> io::Result<()> {
    match content {
        None => remove_if_present(path),
        Some(bytes) => write_if_changed(path, bytes),
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    if path.is_file() {
        fs::remove_file(path)
    } else {
        Ok(())
    }
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).ok().as_deref() == Some(bytes) {
        Ok(())
    } else {
        fs::write(path, bytes)
    }
}

fn vitest_version(package: &Path) -> Option<String> {
    let installed = package.join("node_modules/vitest/package.json");
    if !installed.is_file() {
        return None;
    }
    let text = fs::read_to_string(&installed).ok()?;
    match serde_json::from_str::<Value>(&text) {
        Ok(data) => data
            .get("version")
            .and_then(Value::as_str)
            .filter(|version| SEMVER.is_match(version))
            .map(str::to_owned),
        Err(error) => {
            warn(&format!("{}: {error}", installed.display()));
            None
        }
    }
}

fn vitest_provider_spec(version: Option<&str>) -> String {
    match version {
        Some(version) => format!("@vitest/coverage-v8@{version}"),
        None => "@vitest/coverage-v8".to_string(),
    }
}

/// The package manager that installs the coverage provider. A pnpm store wins, then the first
/// lockfile, then the `packageManager` field of package.json. npm is the default.
fn package_manager(package: &Path) -> &'static str {
    if package.join("node_modules/.pnpm").is_dir() {
        return "pnpm";
    }
    lockfile_manager(package)
        .or_else(|| declared_manager(package))
        .unwrap_or("npm")
}

/// Lockfiles that name a manager, in the order they are checked.
const LOCKFILE_MANAGERS: &[(&str, &[&str])] = &[
    ("pnpm", &["pnpm-lock.yaml"]),
    ("yarn", &["yarn.lock"]),
    ("bun", &["bun.lockb", "bun.lock"]),
];

fn lockfile_manager(package: &Path) -> Option<&'static str> {
    LOCKFILE_MANAGERS
        .iter()
        .find(|(_, files)| files.iter().any(|file| package.join(file).is_file()))
        .map(|(manager, _)| *manager)
}

/// The manager named by the `packageManager` field of package.json, such as `pnpm@9.15.9`.
fn declared_manager(package: &Path) -> Option<&'static str> {
    let field = package_json(package)
        .ok()
        .flatten()
        .and_then(|data| {
            data.get("packageManager")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    ["pnpm", "yarn", "bun"]
        .into_iter()
        .find(|manager| field.starts_with(*manager))
}

/// The command that installs the coverage provider with `package`'s own package manager.
/// Lifecycle scripts never run.
fn vitest_install_command(package: &Path, spec: &str) -> Vec<String> {
    let mut command = match package_manager(package) {
        "pnpm" => strings(&["pnpm", "add", "-D", "--no-lockfile", "--ignore-scripts"]),
        "yarn" => strings(&["yarn", "add", "-D", "--no-lockfile", "--ignore-scripts"]),
        "bun" => strings(&["bun", "add", "-d", "--no-save", "--ignore-scripts"]),
        _ => strings(&["npm", "install", "--no-save", "--ignore-scripts"]),
    };
    command.push(spec.to_string());
    command
}

fn vitest_install_notice(spec: &str) -> String {
    format!(
        "Installing {spec} so Vitest can write LCOV. \
         package.json and the lockfile are restored afterward."
    )
}

fn typescript_packages(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    let mut packages = BTreeSet::new();
    for file in files {
        if language_of(file) != Some("typescript") {
            continue;
        }
        if let Some(package) = nearest(file, "package.json", &root) {
            packages.insert(resolve(&package));
        }
    }
    packages.into_iter().collect()
}

fn typescript_command(
    package: &Path,
    files: &[PathBuf],
    report_dir: &Path,
) -> Result<Option<Vec<String>>, PackageJsonError> {
    let Some(data) = package_json(package)? else {
        return Ok(None);
    };
    let scripts = scripts_of(&data);
    if scripts.contains_key("coverage") {
        return Ok(Some(strings(&["npm", "run", "coverage"])));
    }
    if uses_vitest(package, &scripts) {
        return Ok(Some(vitest_coverage_command(package, files, report_dir)));
    }
    if !scripts.contains_key("test") {
        return Ok(None);
    }
    Ok(Some(c8_command(report_dir)))
}

fn package_json(package: &Path) -> Result<Option<Map<String, Value>>, PackageJsonError> {
    let path = package.join("package.json");
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .map_err(|error| PackageJsonError(format!("{}: {error}", path.display())))?;
    parse_object(&path, &text).map(Some)
}

fn parse_object(path: &Path, text: &str) -> Result<Map<String, Value>, PackageJsonError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| PackageJsonError(format!("{}: {error}", path.display())))?;
    match value {
        Value::Object(data) => Ok(data),
        _ => Err(PackageJsonError(format!(
            "{}: expected a JSON object",
            path.display()
        ))),
    }
}

fn scripts_of(data: &Map<String, Value>) -> Map<String, Value> {
    data.get("scripts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn uses_vitest(package: &Path, scripts: &Map<String, Value>) -> bool {
    if scripts
        .get("test")
        .is_some_and(|test| test.to_string().contains("vitest"))
    {
        return true;
    }
    VITEST_CONFIGS
        .iter()
        .any(|name| package.join(name).is_file())
}

/// The notice for a command that downloads a tool from the network before it runs.
/// `npx --yes <package>` fetches the package on demand, so the port announces it (CR-15).
fn install_notice(command: &[String]) -> Option<String> {
    let mut parts = command.iter().map(String::as_str);
    if parts.next() != Some("npx") || parts.next() != Some("--yes") {
        return None;
    }
    let package = parts.next()?;
    Some(format!(
        "Installing {package} with npx --yes. npx downloads it from the network before the run."
    ))
}

fn c8_command(report_dir: &Path) -> Vec<String> {
    let mut command = strings(&["npx", "--yes", "c8", "--reporter=lcov", "--reports-dir"]);
    command.push(report_dir.display().to_string());
    command.extend(strings(&["npm", "test"]));
    command
}

fn vitest_includes(package: &Path, files: &[PathBuf]) -> Vec<String> {
    let package = resolve(package);
    let mut includes = Vec::new();
    for file in files {
        if language_of(file) != Some("typescript") || is_test_file(file) {
            continue;
        }
        if let Ok(relative) = resolve(file).strip_prefix(&package) {
            includes.push(relative.to_string_lossy().into_owned());
        }
    }
    includes
}

fn vitest_coverage_command(package: &Path, files: &[PathBuf], report_dir: &Path) -> Vec<String> {
    let binary = package.join("node_modules/.bin/vitest");
    let mut command = if binary.is_file() {
        vec![binary.display().to_string()]
    } else {
        strings(&["npx", "vitest"])
    };
    command.extend(strings(&["run", "--coverage", "--coverage.reporter=lcov"]));
    command.push(format!(
        "--coverage.reportsDirectory={}",
        report_dir.display()
    ));
    command.push("--coverage.reportOnFailure=true".to_string());
    command.extend(
        vitest_includes(package, files)
            .into_iter()
            .map(|include| format!("--coverage.include={include}")),
    );
    command
}

fn resolve(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command the fake executor saw, with the directory it was asked to run in.
    type Calls = Vec<(Cmd, PathBuf)>;

    fn write(root: &Path, relative: &str, text: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn is_install(cmd: &Cmd) -> bool {
        matches!(cmd, Cmd::Argv(parts) if parts.iter().any(|part| part == "install"))
    }

    // CR-13 vitest owns its coverage; c8 is not used and test files are not included
    #[test]
    fn cr_13_vitest_uses_its_own_coverage_instead_of_c8() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "package.json", r#"{"scripts":{"test":"vitest run"}}"#);
        write(root, "vitest.config.ts", "export default {}\n");
        let source = write(root, "src/book.ts", "export function plan() { return 1 }\n");
        let test_file = write(root, "src/book.test.ts", "import { plan } from './book'\n");
        let report_dir = root.join("target/coverage/typescript");
        let command = typescript_command(root, &[source, test_file], &report_dir)
            .unwrap()
            .unwrap();
        let text = command.join(" ");
        assert!(!text.contains("c8"));
        assert!(text.contains("vitest run --coverage"));
        assert!(text.contains("--coverage.reporter=lcov"));
        assert!(text.contains(&format!(
            "--coverage.reportsDirectory={}",
            report_dir.display()
        )));
        assert!(text.contains("--coverage.include=src/book.ts"));
        assert!(!text.contains("book.test.ts"));
    }

    // CR-13 an existing coverage script is run as-is
    #[test]
    fn cr_13_existing_coverage_script_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{"scripts":{"coverage":"vitest run --coverage","test":"vitest run"}}"#,
        );
        let command = typescript_command(root, &[], &root.join("coverage")).unwrap();
        assert_eq!(command, Some(strings(&["npm", "run", "coverage"])));
    }

    // CR-13 a plain test script is measured through c8
    #[test]
    fn cr_13_node_test_script_still_uses_c8() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        let report_dir = root.join("target/coverage/typescript");
        let command = typescript_command(root, &[], &report_dir).unwrap();
        let mut expected = strings(&["npx", "--yes", "c8", "--reporter=lcov", "--reports-dir"]);
        expected.push(report_dir.display().to_string());
        expected.extend(strings(&["npm", "test"]));
        assert_eq!(command, Some(expected));
    }

    // CR-13 the nearest package.json of a source file is its TypeScript package
    #[test]
    fn cr_13_typescript_package_is_found_from_a_source_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "web/package.json",
            r#"{"scripts":{"test":"vitest run"}}"#,
        );
        let source = write(root, "web/src/app.ts", "export const n = 1\n");
        assert_eq!(
            typescript_packages(root, &[source]),
            vec![fs::canonicalize(root.join("web")).unwrap()]
        );
    }

    // CR-13 no package.json means no command; an unreadable one is an error, not a missing script
    #[test]
    fn cr_13_package_json_without_a_test_script_has_no_coverage_command() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let report_dir = root.join("cov");
        assert_eq!(typescript_command(root, &[], &report_dir).unwrap(), None);
        write(root, "package.json", "{");
        assert!(typescript_command(root, &[], &report_dir)
            .unwrap_err()
            .0
            .contains("package.json"));
        write(root, "package.json", "[]");
        assert!(typescript_command(root, &[], &report_dir)
            .unwrap_err()
            .0
            .contains("expected a JSON object"));
        write(root, "package.json", r#"{"scripts":["nope"]}"#);
        assert_eq!(typescript_command(root, &[], &report_dir).unwrap(), None);
    }

    // CR-13 only source files outside the package are dropped from the vitest include list
    #[test]
    fn cr_13_vitest_include_skips_a_file_outside_the_package() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let package = root.join("web");
        let inside = write(&package, "src/app.ts", "export const n = 1\n");
        let outside = root.join("outside.ts");
        assert_eq!(
            vitest_includes(&package, &[inside, outside]),
            vec!["src/app.ts".to_string()]
        );
    }

    // CR-13 the report directory is emptied before the coverage run
    #[test]
    fn cr_13_prepare_report_replaces_an_existing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let report_dir = tmp.path().join("target/coverage/typescript");
        fs::create_dir_all(&report_dir).unwrap();
        fs::write(report_dir.join("old.info"), "old").unwrap();
        prepare_report(&report_dir).unwrap();
        assert!(report_dir.is_dir());
        assert!(!report_dir.join("old.info").exists());
    }

    // CR-13 a typescript package with a test script runs its command in the package directory
    #[test]
    fn cr_13_typescript_package_runs_its_test_command_in_the_package() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "web/package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        let source = write(root, "web/src/app.ts", "export const n = 1\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            0
        })
        .expect("covers");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, fs::canonicalize(root.join("web")).unwrap());
        let Cmd::Argv(parts) = &calls[0].0 else {
            panic!("expected an argv command");
        };
        assert_eq!(parts.first().map(String::as_str), Some("npx"));
        assert!(parts.ends_with(&strings(&["npm", "test"])));
    }

    // CR-13 no typescript source file means no package and no command
    #[test]
    fn cr_13_only_non_typescript_files_run_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        let source = write(root, "src/app.py", "print(1)\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            0
        })
        .expect("covers");
        assert!(calls.is_empty());
    }

    // CR-13 a package without a test or coverage script runs nothing
    #[test]
    fn cr_13_package_without_a_test_script_runs_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "package.json", r#"{"scripts":{"build":"tsc"}}"#);
        let source = write(root, "src/app.ts", "export const n = 1\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            0
        })
        .expect("covers");
        assert!(calls.is_empty());
    }

    // CR-13 an unreadable package.json runs nothing
    #[test]
    fn cr_13_unreadable_package_json_runs_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "package.json", "{");
        let source = write(root, "src/app.ts", "export const n = 1\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            0
        })
        .expect("covers");
        assert!(calls.is_empty());
    }

    // CR-13 a failing package does not stop the packages after it
    #[test]
    fn cr_13_a_failed_package_does_not_stop_the_next_package() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "api/package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        write(
            root,
            "web/package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        let api = write(root, "api/src/a.ts", "export const a = 1\n");
        let web = write(root, "web/src/w.ts", "export const w = 1\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[api, web], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            1
        })
        .expect("covers");
        let cwds: Vec<PathBuf> = calls.iter().map(|(_, cwd)| cwd.clone()).collect();
        assert_eq!(
            cwds,
            vec![
                fs::canonicalize(root.join("api")).unwrap(),
                fs::canonicalize(root.join("web")).unwrap(),
            ]
        );
    }

    // CR-13 the stale report directory is cleared before the coverage command runs
    #[test]
    fn cr_13_stale_report_is_cleared_before_the_coverage_run() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        let source = write(root, "src/app.ts", "export const n = 1\n");
        let report_dir = coverage_report(root, root, "typescript")
            .expect("report path")
            .parent()
            .unwrap()
            .to_path_buf();
        fs::create_dir_all(&report_dir).unwrap();
        let stale = report_dir.join("stale.info");
        fs::write(&stale, "old").unwrap();
        let mut ran = false;
        let mut stale_present_at_run = true;
        cover_typescript_with(root, &[source], &mut |_: &Cmd, _: &Path| {
            ran = true;
            stale_present_at_run = stale.exists();
            0
        })
        .expect("covers");
        assert!(ran);
        assert!(!stale_present_at_run);
    }

    // CR-3, AC-26
    #[test]
    fn cr_3_ac_26_a_typescript_package_outside_the_root_is_an_error_and_runs_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir(&root).unwrap();
        let outside = tmp.path().join("outside");
        write(
            &outside,
            "package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        );
        let source = write(&outside, "src/a.ts", "export const a = 1\n");
        let mut calls: Calls = Vec::new();
        let err = cover_typescript_with(&root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            0
        })
        .expect_err("outside the root");
        assert!(err.contains("is not in the subpath of"), "{err}");
        assert!(calls.is_empty());
    }

    // CR-14 a vitest package without its coverage provider installs it, then runs vitest
    #[test]
    fn cr_14_vitest_provider_is_installed_before_the_vitest_run() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "package.json", r#"{"scripts":{"test":"vitest run"}}"#);
        write(root, "vitest.config.ts", "export default {}\n");
        let source = write(root, "src/book.ts", "export const n = 1\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            if is_install(cmd) {
                fs::create_dir_all(cwd.join("node_modules/@vitest/coverage-v8")).unwrap();
            }
            0
        })
        .expect("covers");
        assert_eq!(calls.len(), 2);
        assert!(is_install(&calls[0].0));
        assert_eq!(
            calls[0].0,
            Cmd::Argv(strings(&[
                "npm",
                "install",
                "--no-save",
                "--ignore-scripts",
                "@vitest/coverage-v8",
            ]))
        );
        let Cmd::Argv(run) = &calls[1].0 else {
            panic!("expected an argv command");
        };
        assert!(run.contains(&"run".to_string()));
    }

    // CR-14 a failed provider install skips the vitest run
    #[test]
    fn cr_14_failed_provider_install_skips_the_vitest_run() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "package.json", r#"{"scripts":{"test":"vitest run"}}"#);
        write(root, "vitest.config.ts", "export default {}\n");
        let source = write(root, "src/book.ts", "export const n = 1\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            1
        })
        .expect("covers");
        assert_eq!(calls.len(), 1);
        assert!(is_install(&calls[0].0));
    }

    // CR-14 an installed coverage provider is not installed again
    #[test]
    fn cr_14_installed_provider_is_not_reinstalled() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "package.json", r#"{"scripts":{"test":"vitest run"}}"#);
        write(root, "vitest.config.ts", "export default {}\n");
        fs::create_dir_all(root.join("node_modules/@vitest/coverage-v8")).unwrap();
        let source = write(root, "src/book.ts", "export const n = 1\n");
        let mut calls: Calls = Vec::new();
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            0
        })
        .expect("covers");
        assert_eq!(calls.len(), 1);
        assert!(!is_install(&calls[0].0));
    }

    // CR-14 package.json is put back after the provider install even when the install rewrites it
    #[test]
    fn cr_14_manifests_are_restored_after_the_provider_install() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let original = "{\"scripts\":{\"test\":\"vitest run\"}}";
        let package_json = write(root, "package.json", original);
        write(root, "vitest.config.ts", "export default {}\n");
        let source = write(root, "src/book.ts", "export const n = 1\n");
        cover_typescript_with(root, &[source], &mut |cmd: &Cmd, cwd: &Path| {
            if is_install(cmd) {
                fs::write(&package_json, "{\"name\":\"changed\"}\n").unwrap();
                fs::create_dir_all(cwd.join("node_modules/@vitest/coverage-v8")).unwrap();
            }
            0
        })
        .expect("covers");
        assert_eq!(fs::read_to_string(&package_json).unwrap(), original);
    }

    // CR-14 package.json, the lockfile, and the shrinkwrap are put back after an install
    #[test]
    fn cr_14_manifests_are_restored_after_a_coverage_install() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let package_json = write(root, "package.json", "{\"name\":\"demo\"}\n");
        let lock = write(root, "package-lock.json", "{\"lockfileVersion\":3}\n");
        let shrink = root.join("npm-shrinkwrap.json");
        let result = restoring_manifests(root, || {
            fs::write(&package_json, "{\"name\":\"changed\"}\n").unwrap();
            fs::remove_file(&lock).unwrap();
            fs::write(&shrink, "{}\n").unwrap();
            1
        });
        assert_eq!(result, Some(1));
        assert_eq!(fs::read(&package_json).unwrap(), b"{\"name\":\"demo\"}\n");
        assert_eq!(fs::read(&lock).unwrap(), b"{\"lockfileVersion\":3}\n");
        assert!(!shrink.exists());
    }

    // CR-14 a lockfile that an install creates is removed again
    #[test]
    fn cr_14_lockfile_created_by_an_install_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lock = root.join("package-lock.json");
        let result = restoring_manifests(root, || {
            fs::write(&lock, "{}").unwrap();
            0
        });
        assert_eq!(result, Some(0));
        assert!(!lock.exists());
    }

    // CR-14 the installed vitest version is read from node_modules
    #[test]
    fn cr_14_vitest_version_is_read_from_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert_eq!(vitest_version(root), None);
        write(root, "node_modules/vitest/package.json", "{not json");
        assert_eq!(vitest_version(root), None);
        write(
            root,
            "node_modules/vitest/package.json",
            "{\"version\": \"4.1.11\"}\n",
        );
        assert_eq!(vitest_version(root), Some("4.1.11".to_string()));
    }

    // CR-14 an installed coverage provider short-circuits the install
    #[test]
    fn cr_14_installed_vitest_provider_needs_no_install() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("node_modules/@vitest/coverage-v8")).unwrap();
        let ready = ensure_vitest_coverage_with(root, &mut |_: &Cmd, _: &Path| -> i32 {
            panic!("no install expected when the provider is installed")
        });
        assert!(ready);
    }

    // CR-14 the provider is pinned to the installed vitest version
    #[test]
    fn cr_14_vitest_provider_spec_pins_the_installed_version() {
        assert_eq!(
            vitest_provider_spec(Some("4.1.11")),
            "@vitest/coverage-v8@4.1.11"
        );
        assert_eq!(vitest_provider_spec(None), "@vitest/coverage-v8");
    }

    /// The first command that `ensure_vitest_coverage_with` runs in `package`, or `None` when it runs none.
    fn install_command_in(package: &Path) -> Option<Vec<String>> {
        let mut seen = None;
        ensure_vitest_coverage_with(package, &mut |cmd: &Cmd, _: &Path| -> i32 {
            if let Cmd::Argv(parts) = cmd {
                seen.get_or_insert_with(|| parts.clone());
            }
            0
        });
        seen
    }

    // CR-19 AC-28 AC-32: the npm install keeps the lockfile in force and runs no install script
    #[test]
    fn cr_19_ac_28_ac_32_npm_install_has_no_package_lock_flag_and_ignores_scripts() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            install_command_in(tmp.path()),
            Some(strings(&[
                "npm",
                "install",
                "--no-save",
                "--ignore-scripts",
                "@vitest/coverage-v8"
            ]))
        );
    }

    // CR-18 AC-27 AC-32: pnpm-lock.yaml or node_modules/.pnpm selects pnpm
    #[test]
    fn cr_18_ac_27_pnpm_lockfile_or_store_selects_pnpm() {
        let expected = Some(strings(&[
            "pnpm",
            "add",
            "-D",
            "--no-lockfile",
            "--ignore-scripts",
            "@vitest/coverage-v8",
        ]));
        let by_lock = tempfile::tempdir().unwrap();
        write(by_lock.path(), "pnpm-lock.yaml", "lockfileVersion: '9.0'\n");
        assert_eq!(install_command_in(by_lock.path()), expected);
        let by_store = tempfile::tempdir().unwrap();
        fs::create_dir_all(by_store.path().join("node_modules/.pnpm")).unwrap();
        assert_eq!(install_command_in(by_store.path()), expected);
    }

    // CR-18 AC-27 AC-32: yarn.lock selects yarn
    #[test]
    fn cr_18_ac_27_yarn_lockfile_selects_yarn() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "yarn.lock", "");
        assert_eq!(
            install_command_in(tmp.path()),
            Some(strings(&[
                "yarn",
                "add",
                "-D",
                "--no-lockfile",
                "--ignore-scripts",
                "@vitest/coverage-v8"
            ]))
        );
    }

    // CR-18 AC-27 AC-32: bun.lock or bun.lockb selects bun
    #[test]
    fn cr_18_ac_27_bun_lockfile_selects_bun() {
        let expected = Some(strings(&[
            "bun",
            "add",
            "-d",
            "--no-save",
            "--ignore-scripts",
            "@vitest/coverage-v8",
        ]));
        for lock in ["bun.lock", "bun.lockb"] {
            let tmp = tempfile::tempdir().unwrap();
            write(tmp.path(), lock, "");
            assert_eq!(install_command_in(tmp.path()), expected, "{lock}");
        }
    }

    // CR-18 AC-27: the packageManager field decides only when no lockfile or store exists
    #[test]
    fn cr_18_ac_27_package_manager_field_selects_the_manager_after_the_lockfiles() {
        let manager = |field: &str| {
            let tmp = tempfile::tempdir().unwrap();
            write(
                tmp.path(),
                "package.json",
                &format!("{{\"packageManager\": \"{field}\"}}"),
            );
            install_command_in(tmp.path()).unwrap()[0].clone()
        };
        assert_eq!(manager("pnpm@9.15.9"), "pnpm");
        assert_eq!(manager("yarn@4.1.0"), "yarn");
        assert_eq!(manager("bun@1.1.0"), "bun");
        assert_eq!(manager("npm@10.2.0"), "npm");
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "package.json",
            "{\"packageManager\": \"pnpm@9.15.9\"}",
        );
        write(tmp.path(), "yarn.lock", "");
        assert_eq!(install_command_in(tmp.path()).unwrap()[0], "yarn");
        let both = tempfile::tempdir().unwrap();
        write(both.path(), "yarn.lock", "");
        write(both.path(), "pnpm-lock.yaml", "");
        assert_eq!(install_command_in(both.path()).unwrap()[0], "pnpm");
        let late = tempfile::tempdir().unwrap();
        write(late.path(), "bun.lock", "");
        write(late.path(), "yarn.lock", "");
        assert_eq!(install_command_in(late.path()).unwrap()[0], "yarn");
    }

    // CR-18 AC-27: after the install, all seven manifests have their old bytes and no new one stays
    #[test]
    fn cr_18_ac_27_every_lockfile_is_restored_and_a_new_one_is_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "package.json", "{\"name\":\"demo\"}\n");
        write(root, "pnpm-lock.yaml", "lock: one\n");
        write(root, "yarn.lock", "yarn one\n");
        write(root, "bun.lockb", "bun binary");
        let ready = ensure_vitest_coverage_with(root, &mut |_: &Cmd, cwd: &Path| -> i32 {
            fs::write(cwd.join("package.json"), "{}").unwrap();
            fs::write(cwd.join("pnpm-lock.yaml"), "lock: two\n").unwrap();
            fs::remove_file(cwd.join("yarn.lock")).unwrap();
            fs::write(cwd.join("bun.lock"), "new").unwrap();
            fs::write(cwd.join("package-lock.json"), "{}").unwrap();
            fs::write(cwd.join("npm-shrinkwrap.json"), "{}").unwrap();
            fs::create_dir_all(cwd.join("node_modules/@vitest/coverage-v8")).unwrap();
            0
        });
        assert!(ready);
        assert_eq!(
            fs::read_to_string(root.join("package.json")).unwrap(),
            "{\"name\":\"demo\"}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("pnpm-lock.yaml")).unwrap(),
            "lock: one\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("yarn.lock")).unwrap(),
            "yarn one\n"
        );
        assert_eq!(fs::read(root.join("bun.lockb")).unwrap(), b"bun binary");
        for gone in ["bun.lock", "package-lock.json", "npm-shrinkwrap.json"] {
            assert!(!root.join(gone).exists(), "{gone} stays");
        }
    }

    // CR-23 AC-32: a SemVer version with a pre-release and build part is kept in the spec
    #[test]
    fn cr_23_ac_32_a_semver_version_with_prerelease_and_build_is_pinned() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "node_modules/vitest/package.json",
            "{\"version\": \"4.1.11-beta.1+build.2\"}",
        );
        assert_eq!(
            install_command_in(tmp.path()).unwrap().last().unwrap(),
            "@vitest/coverage-v8@4.1.11-beta.1+build.2"
        );
    }

    // CR-23 AC-32: a version that is not SemVer gives a spec with no version
    #[test]
    fn cr_23_ac_32_a_version_that_is_not_semver_is_not_pinned() {
        for bad in [
            "file:../untrusted",
            "4.1",
            "v4.1.11",
            "4.1.11\n",
            "4.1.11 ",
            "",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            write(
                tmp.path(),
                "node_modules/vitest/package.json",
                &serde_json::json!({ "version": bad }).to_string(),
            );
            assert_eq!(
                install_command_in(tmp.path()).unwrap().last().unwrap(),
                "@vitest/coverage-v8",
                "{bad:?}"
            );
        }
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "node_modules/vitest/package.json",
            "{\"version\": 4}",
        );
        assert_eq!(vitest_version(tmp.path()), None);
    }

    // CR-14 only vitest commands need the coverage provider; the npm coverage script and c8 do not
    #[test]
    fn cr_14_only_vitest_commands_need_the_coverage_provider() {
        assert!(needs_vitest_provider(&strings(&["npx", "vitest", "run"])));
        assert!(needs_vitest_provider(&strings(&[
            "/p/node_modules/.bin/vitest",
            "run"
        ])));
        assert!(!needs_vitest_provider(&strings(&[
            "npm", "run", "coverage"
        ])));
        assert!(!needs_vitest_provider(&strings(&["npx", "--yes", "c8"])));
    }

    // CR-15 the install notice names the provider spec and says manifests are restored
    #[test]
    fn cr_15_install_notice_names_the_spec_and_the_restore() {
        assert_eq!(
            vitest_install_notice("@vitest/coverage-v8@4.1.11"),
            "Installing @vitest/coverage-v8@4.1.11 so Vitest can write LCOV. \
             package.json and the lockfile are restored afterward."
        );
    }

    // CR-14 a provider install that exits nonzero is not ready, even when the provider directory exists
    #[test]
    fn cr_14_failed_provider_install_is_not_ready_even_if_the_provider_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let ready = ensure_vitest_coverage_with(tmp.path(), &mut |_: &Cmd, path: &Path| -> i32 {
            fs::create_dir_all(path.join("node_modules/@vitest/coverage-v8")).unwrap();
            1
        });
        assert!(!ready);
    }

    // CR-14 a provider install that exits 0 but leaves no provider directory is not ready
    #[test]
    fn cr_14_provider_install_without_the_directory_is_not_ready() {
        let tmp = tempfile::tempdir().unwrap();
        let ready = ensure_vitest_coverage_with(tmp.path(), &mut |_: &Cmd, _: &Path| -> i32 { 0 });
        assert!(!ready);
    }

    // CR-14 a provider install that exits 0 and leaves the provider directory is ready
    #[test]
    fn cr_14_provider_install_with_the_directory_is_ready() {
        let tmp = tempfile::tempdir().unwrap();
        let ready = ensure_vitest_coverage_with(tmp.path(), &mut |_: &Cmd, path: &Path| -> i32 {
            fs::create_dir_all(path.join("node_modules/@vitest/coverage-v8")).unwrap();
            0
        });
        assert!(ready);
    }

    // CR-14 only an unready provider gets the missing-provider warning
    #[test]
    fn cr_14_provider_missing_warning_is_only_for_an_unready_provider() {
        assert_eq!(provider_missing_warning(true), None);
        assert_eq!(
            provider_missing_warning(false),
            Some("Vitest's coverage provider is missing. TypeScript coverage will score 0%.")
        );
    }
}
