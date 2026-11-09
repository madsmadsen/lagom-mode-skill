// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/runners.py
// license: MIT

//! TypeScript coverage: pick each package's test runner and collect LCOV.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{coverage_report, nearest, run_shell, warn, Cmd};
use crate::discover::{is_test_file, language_of};

const VITEST_CONFIGS: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
];

const MANIFESTS: &[&str] = &["package.json", "package-lock.json", "npm-shrinkwrap.json"];

/// Manifest name and its bytes before an install (`None` when absent).
type ManifestSnapshot = Vec<(PathBuf, Option<Vec<u8>>)>;

/// package.json is present and cannot be read as an object.
#[derive(Debug)]
struct PackageJsonError(String);

pub(crate) fn cover_typescript(root: &Path, files: &[PathBuf]) {
    let packages = typescript_packages(root, files);
    if packages.is_empty() {
        warn("No package.json; skipping TypeScript coverage.");
        return;
    }
    for package in &packages {
        cover_package(root, package, files);
    }
}

fn cover_package(root: &Path, package: &Path, files: &[PathBuf]) {
    let report = coverage_report(root, package, "typescript");
    let Some(report_dir) = report.parent() else {
        return;
    };
    let command = match typescript_command(package, files, report_dir) {
        Ok(Some(command)) => command,
        Ok(None) => {
            warn(&format!(
                "No package.json test script in {}; skipping TypeScript coverage.",
                package.display()
            ));
            return;
        }
        Err(error) => {
            warn(&error.0);
            return;
        }
    };
    if needs_vitest_provider(&command) && !ensure_vitest_coverage(package) {
        return;
    }
    if let Err(error) = prepare_report(report_dir) {
        warn(&format!(
            "Could not prepare {}: {error}",
            report_dir.display()
        ));
        return;
    }
    let code = run_shell(&Cmd::Argv(command), package);
    if code != 0 {
        warn(&format!(
            "TypeScript coverage exited {code} in {}. TypeScript coverage will score 0%.",
            package.display()
        ));
    }
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

fn ensure_vitest_coverage(package: &Path) -> bool {
    let provider = package.join("node_modules/@vitest/coverage-v8");
    if provider.is_dir() {
        return true;
    }
    let spec = vitest_provider_spec(vitest_version(package).as_deref());
    warn(&vitest_install_notice(&spec));
    let install = vitest_install_command(&spec);
    let code = restoring_manifests(package, || run_shell(&Cmd::Argv(install), package));
    let ready = code == Some(0) && provider.is_dir();
    if !ready {
        warn("Vitest's coverage provider is missing. TypeScript coverage will score 0%.");
    }
    ready
}

/// Runs `install` and then puts every manifest back the way it was before it ran.
/// Returns `None` (without running `install`) when the manifests cannot be read.
fn restoring_manifests<T>(package: &Path, install: impl FnOnce() -> T) -> Option<T> {
    let snapshot = match manifest_snapshot(package) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            warn(&format!(
                "Could not read package manifests in {}: {error}",
                package.display()
            ));
            return None;
        }
    };
    let result = install();
    for (path, content) in &snapshot {
        if let Err(error) = restore_one(path, content) {
            warn(&format!("Could not restore {}: {error}", path.display()));
        }
    }
    Some(result)
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
        None if path.is_file() => fs::remove_file(path),
        None => Ok(()),
        Some(bytes) if fs::read(path).ok().as_deref() != Some(bytes.as_slice()) => {
            fs::write(path, bytes)
        }
        Some(_) => Ok(()),
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
            .map(str::to_owned),
        Err(error) => {
            warn(&format!("{}: {error}", installed.display()));
            None
        }
    }
}

fn vitest_provider_spec(version: Option<&str>) -> String {
    match version.filter(|v| !v.is_empty()) {
        Some(version) => format!("@vitest/coverage-v8@{version}"),
        None => "@vitest/coverage-v8".to_string(),
    }
}

fn vitest_install_command(spec: &str) -> Vec<String> {
    let mut command = strings(&["npm", "install", "--no-save", "--no-package-lock"]);
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
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(data)) => Ok(Some(data)),
        Ok(_) => Err(PackageJsonError(format!(
            "{}: expected a JSON object",
            path.display()
        ))),
        Err(error) => Err(PackageJsonError(format!("{}: {error}", path.display()))),
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

    fn write(root: &Path, relative: &str, text: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
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
        assert!(ensure_vitest_coverage(root));
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

    // CR-14 the install never saves to package.json or the lockfile
    #[test]
    fn cr_14_install_command_never_saves_to_manifests() {
        assert_eq!(
            vitest_install_command("@vitest/coverage-v8@4.1.11"),
            strings(&[
                "npm",
                "install",
                "--no-save",
                "--no-package-lock",
                "@vitest/coverage-v8@4.1.11",
            ])
        );
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
}
