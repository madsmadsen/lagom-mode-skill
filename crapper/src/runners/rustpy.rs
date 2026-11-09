// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/runners.py
// license: MIT

//! Rust and Python coverage: find the packages, pick the tool, install what is missing, run it.

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::{coverage_report, modules_with, nearest, run_shell, warn, Cmd};
use crate::discover::{is_test_file, resolve};

/// One line of an install flow: an announcement, or a command with an optional failure warning.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Announce(String),
    Run {
        cmd: Cmd,
        on_failure: Option<&'static str>,
    },
}

const PYTHON_MARKERS: [&str; 5] = [
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
    "pytest.ini",
    "tox.ini",
];
const PYTEST_CONFIGS: [&str; 3] = ["pyproject.toml", "setup.cfg", "tox.ini"];
const PROJECT_VENVS: [&str; 2] = [".venv/bin/python", "venv/bin/python"];

fn words(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn argv(parts: &[&str]) -> Cmd {
    Cmd::Argv(words(parts))
}

fn home() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Python's `shutil.which` for a name, searching the given `PATH` value.
fn on_path_in(path_var: &OsStr, name: &str) -> bool {
    env::split_paths(path_var).any(|dir| is_executable(&dir.join(name)))
}

fn on_path(name: &str) -> bool {
    on_path_in(&env::var_os("PATH").unwrap_or_default(), name)
}

fn is_python(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "py")
}

fn report_dir(report: &Path) -> &Path {
    report.parent().unwrap_or_else(|| Path::new("."))
}

fn make_report_dir(report: &Path) {
    fs::create_dir_all(report_dir(report))
        .unwrap_or_else(|err| panic!("cannot create {}: {err}", report_dir(report).display()));
}

fn remove_if_present(path: &Path) {
    if let Err(err) = fs::remove_file(path) {
        if err.kind() != ErrorKind::NotFound {
            panic!("cannot remove {}: {err}", path.display());
        }
    }
}

/// Run each step in order. Announcements go to stderr before the command they describe.
/// Returns the status of the last command, which decides whether the install worked.
fn execute(steps: &[Step], cwd: &Path, exec: &mut dyn FnMut(&Cmd, &Path) -> i32) -> i32 {
    let mut code = 0;
    for step in steps {
        match step {
            Step::Announce(message) => warn(message),
            Step::Run { cmd, on_failure } => {
                code = exec(cmd, cwd);
                warn_failure(code, *on_failure);
            }
        }
    }
    code
}

fn warn_failure(code: i32, on_failure: Option<&str>) {
    if let (true, Some(message)) = (code != 0, on_failure) {
        warn(message);
    }
}

fn rust_modules(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    modules_with(files, ".rs", "Cargo.toml", &root)
        .iter()
        .map(|module| resolve(module))
        .collect()
}

fn detected_rust_kind(llvm_on_path: bool, tarpaulin_on_path: bool) -> Option<&'static str> {
    match (llvm_on_path, tarpaulin_on_path) {
        (true, _) => Some("llvm-cov"),
        (false, true) => Some("tarpaulin"),
        (false, false) => None,
    }
}

/// The install flow used when neither Rust coverage tool is on PATH.
fn rust_install_plan(rustup_on_path: bool) -> Vec<Step> {
    let mut steps = vec![Step::Announce(
        "Neither cargo-llvm-cov nor cargo-tarpaulin is installed. Installing cargo-llvm-cov."
            .to_string(),
    )];
    if rustup_on_path {
        steps.push(Step::Run {
            cmd: argv(&["rustup", "component", "add", "llvm-tools-preview"]),
            on_failure: Some("rustup component add llvm-tools-preview failed."),
        });
    }
    steps.push(Step::Run {
        cmd: argv(&["cargo", "install", "cargo-llvm-cov", "--locked"]),
        on_failure: None,
    });
    steps
}

fn rust_kind_after_install(code: i32, llvm_on_path: bool) -> Option<&'static str> {
    (code == 0 && llvm_on_path).then_some("llvm-cov")
}

fn install_rust_kind(
    has_tool: &dyn Fn(&str) -> bool,
    home: &Path,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) -> Option<&'static str> {
    let code = execute(&rust_install_plan(has_tool("rustup")), home, exec);
    let kind = rust_kind_after_install(code, has_tool("cargo-llvm-cov"));
    if kind.is_none() {
        warn("Rust coverage will score 0%.");
    }
    kind
}

fn rust_kind(
    has_tool: &dyn Fn(&str) -> bool,
    home: &Path,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) -> Option<&'static str> {
    detected_rust_kind(has_tool("cargo-llvm-cov"), has_tool("cargo-tarpaulin"))
        .or_else(|| install_rust_kind(has_tool, home, exec))
}

fn rust_coverage_command(kind: &str, report: &Path) -> Cmd {
    if kind == "llvm-cov" {
        let mut parts = words(&["cargo", "llvm-cov", "--lcov", "--output-path"]);
        parts.push(report.display().to_string());
        return Cmd::Argv(parts);
    }
    let mut parts = words(&["cargo", "tarpaulin", "--out", "Lcov", "--output-dir"]);
    parts.push(report_dir(report).display().to_string());
    Cmd::Argv(parts)
}

fn python_package(file: &Path, root: &Path) -> PathBuf {
    PYTHON_MARKERS
        .iter()
        .find_map(|marker| nearest(file, marker, root))
        .map_or_else(|| root.to_path_buf(), |module| resolve(&module))
}

fn python_roots(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    let found: BTreeSet<PathBuf> = files
        .iter()
        .filter(|file| is_python(file))
        .map(|file| python_package(file, &root))
        .collect();
    found.into_iter().collect()
}

fn python_executable(package: &Path) -> String {
    PROJECT_VENVS
        .iter()
        .map(|relative| package.join(relative))
        .find(|candidate| candidate.is_file())
        .map_or_else(
            || "python3".to_string(),
            |candidate| candidate.display().to_string(),
        )
}

fn mentions_pytest(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains("pytest"))
}

fn uses_pytest(package: &Path) -> bool {
    if package.join("pytest.ini").is_file() || package.join("conftest.py").is_file() {
        return true;
    }
    PYTEST_CONFIGS
        .iter()
        .any(|name| mentions_pytest(&package.join(name)))
}

/// A top-level module is named without .py, which coverage.py --source requires.
fn source_name(relative: &Path) -> String {
    let name = if relative.components().count() == 1 {
        relative.file_stem()
    } else {
        relative.components().next().map(|part| part.as_os_str())
    };
    name.map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn python_sources(package: &Path, files: &[PathBuf]) -> String {
    let package = resolve(package);
    let tops: BTreeSet<String> = files
        .iter()
        .filter(|file| is_python(file) && !is_test_file(file))
        .filter_map(|file| {
            let relative = resolve(file);
            relative.strip_prefix(&package).ok().map(source_name)
        })
        .collect();
    tops.into_iter().collect::<Vec<_>>().join(",")
}

fn python_coverage_commands(
    py: &str,
    kind: &str,
    data_file: &Path,
    report: &Path,
    source: &str,
) -> (Cmd, Cmd) {
    let data = format!("--data-file={}", data_file.display());
    let mut run = words(&[py, "-m", "coverage", "run"]);
    run.push(data.clone());
    if !source.is_empty() {
        run.push(format!("--source={source}"));
    }
    if kind == "pytest" {
        run.extend(words(&["-m", "pytest"]));
    } else {
        run.extend(words(&["-m", "unittest", "discover", "-s", "."]));
    }
    let mut lcov = words(&[py, "-m", "coverage", "lcov"]);
    lcov.push(data);
    lcov.extend(words(&["-o"]));
    lcov.push(report.display().to_string());
    (Cmd::Argv(run), Cmd::Argv(lcov))
}

fn module_probe(py: &str, module: &str) -> Cmd {
    let mut parts = words(&[py, "-c"]);
    parts.push(format!("import {module}"));
    Cmd::Argv(parts)
}

/// The install flow used when `module` cannot be imported.
fn module_install_steps(py: &str, module: &str) -> Vec<Step> {
    let mut parts = words(&[py, "-m", "pip", "install", "--disable-pip-version-check"]);
    parts.push(module.to_string());
    vec![
        Step::Announce(format!(
            "Installing {module} for Python coverage. The project requirements are left unchanged."
        )),
        Step::Run {
            cmd: Cmd::Argv(parts),
            on_failure: None,
        },
    ]
}

fn ensure_python_module(
    py: &str,
    package: &Path,
    module: &str,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) -> bool {
    exec(&module_probe(py, module), package) == 0
        || execute(&module_install_steps(py, module), package, exec) == 0
}

fn python_kind(py: &str, package: &Path, exec: &mut dyn FnMut(&Cmd, &Path) -> i32) -> &'static str {
    if !uses_pytest(package) {
        return "unittest";
    }
    if ensure_python_module(py, package, "pytest", exec) {
        return "pytest";
    }
    warn(&format!(
        "pytest is missing in {}. Falling back to unittest.",
        package.display()
    ));
    "unittest"
}

/// The warning for a Python run. `lcov_code` is None when no data file was written.
fn python_run_warning(package: &Path, test_code: i32, lcov_code: Option<i32>) -> Option<String> {
    let place = package.display();
    match lcov_code {
        None if test_code != 0 => Some(format!(
            "Python coverage exited {test_code} in {place}. Python coverage will score 0%."
        )),
        None => None,
        Some(lcov) if lcov != 0 => Some(format!(
            "coverage lcov exited {lcov} in {place}. Python coverage will score 0%."
        )),
        Some(_) if test_code != 0 => Some(format!(
            "Python tests exited {test_code} in {place}. Coverage was still recorded."
        )),
        Some(_) => None,
    }
}

fn record_python_lcov(
    package: &Path,
    code: i32,
    data_file: &Path,
    lcov: &Cmd,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) {
    let lcov_code = data_file.exists().then(|| exec(lcov, package));
    if let Some(message) = python_run_warning(package, code, lcov_code) {
        warn(&message);
    }
}

fn cover_python_package(
    root: &Path,
    files: &[PathBuf],
    package: &Path,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) {
    let report = coverage_report(root, package, "python");
    let py = python_executable(package);
    if !ensure_python_module(&py, package, "coverage", exec) {
        warn(&format!(
            "coverage is missing in {}. Python coverage will score 0%.",
            package.display()
        ));
        return;
    }
    let data_file = report_dir(&report).join(".coverage");
    make_report_dir(&report);
    remove_if_present(&report);
    remove_if_present(&data_file);
    let kind = python_kind(&py, package, exec);
    let (run_cmd, lcov_cmd) = python_coverage_commands(
        &py,
        kind,
        &data_file,
        &report,
        &python_sources(package, files),
    );
    let code = exec(&run_cmd, package);
    record_python_lcov(package, code, &data_file, &lcov_cmd, exec);
}

fn cover_python_with(root: &Path, files: &[PathBuf], exec: &mut dyn FnMut(&Cmd, &Path) -> i32) {
    for package in python_roots(root, files) {
        cover_python_package(root, files, &package, exec);
    }
}

pub(crate) fn cover_python(root: &Path, files: &[PathBuf]) {
    cover_python_with(root, files, &mut run_shell);
}

fn run_rust_module(
    root: &Path,
    module: &Path,
    kind: &str,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) {
    let report = coverage_report(root, module, "rust");
    make_report_dir(&report);
    remove_if_present(&report);
    let code = exec(&rust_coverage_command(kind, &report), module);
    if code != 0 {
        warn(&format!(
            "Rust coverage exited {code} in {}. Rust coverage will score 0%.",
            module.display()
        ));
    }
}

fn cover_rust_with(
    root: &Path,
    files: &[PathBuf],
    has_tool: &dyn Fn(&str) -> bool,
    home: &Path,
    exec: &mut dyn FnMut(&Cmd, &Path) -> i32,
) {
    let modules = rust_modules(root, files);
    if modules.is_empty() {
        warn("No Cargo.toml; skipping Rust coverage.");
        return;
    }
    let Some(kind) = rust_kind(has_tool, home, exec) else {
        return;
    };
    for module in &modules {
        run_rust_module(root, module, kind, exec);
    }
}

pub(crate) fn cover_rust(root: &Path, files: &[PathBuf]) {
    cover_rust_with(root, files, &on_path, &home(), &mut run_shell);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn write(root: &Path, relative: &str, source: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        path
    }

    // CR-13
    #[test]
    fn cr_13_llvm_cov_command_writes_lcov_to_the_report_path() {
        let report = Path::new("/tmp/bookwriter/target/coverage/rust/src-tauri/lcov.info");
        assert_eq!(
            rust_coverage_command("llvm-cov", report),
            argv(&[
                "cargo",
                "llvm-cov",
                "--lcov",
                "--output-path",
                "/tmp/bookwriter/target/coverage/rust/src-tauri/lcov.info",
            ])
        );
    }

    // CR-13
    #[test]
    fn cr_13_tarpaulin_command_writes_lcov_into_the_report_directory() {
        let report = Path::new("/tmp/book/target/coverage/rust/lcov.info");
        assert_eq!(
            rust_coverage_command("tarpaulin", report),
            argv(&[
                "cargo",
                "tarpaulin",
                "--out",
                "Lcov",
                "--output-dir",
                "/tmp/book/target/coverage/rust",
            ])
        );
    }

    // CR-13
    #[test]
    fn cr_13_rust_module_is_the_nearest_cargo_package() {
        let tmp = tempdir().unwrap();
        write(
            tmp.path(),
            "src-tauri/Cargo.toml",
            "[package]\nname = \"bookwriter\"\n",
        );
        let lib = write(
            tmp.path(),
            "src-tauri/src/lib.rs",
            "pub fn read_text() {}\n",
        );
        assert_eq!(
            rust_modules(tmp.path(), &[lib]),
            vec![resolve(&tmp.path().join("src-tauri"))]
        );
    }

    // CR-13
    #[test]
    fn cr_13_python_project_uses_pytest_and_writes_lcov() {
        let tmp = tempdir().unwrap();
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
        assert_eq!(python_roots(root, &[source.clone()]), vec![resolve(root)]);
        assert!(uses_pytest(root));
        assert_eq!(python_sources(root, &[source, test.clone()]), "src");
        assert!(is_test_file(&test));
        let data = root.join("target/coverage/python/.coverage");
        let report = root.join("target/coverage/python/lcov.info");
        let (run, lcov) = python_coverage_commands("python3", "pytest", &data, &report, "src");
        let data_flag = format!("--data-file={}", data.display());
        let report_path = report.display().to_string();
        assert_eq!(
            run,
            argv(&[
                "python3",
                "-m",
                "coverage",
                "run",
                data_flag.as_str(),
                "--source=src",
                "-m",
                "pytest",
            ])
        );
        assert_eq!(
            lcov,
            argv(&[
                "python3",
                "-m",
                "coverage",
                "lcov",
                data_flag.as_str(),
                "-o",
                report_path.as_str(),
            ])
        );
    }

    // CR-13
    #[test]
    fn cr_13_python_package_without_pytest_uses_unittest() {
        let tmp = tempdir().unwrap();
        write(
            tmp.path(),
            "setup.py",
            "from setuptools import setup\nsetup(name='demo')\n",
        );
        let source = write(tmp.path(), "demo/app.py", "def run():\n    return 1\n");
        assert!(!uses_pytest(tmp.path()));
        let (run, _lcov) = python_coverage_commands(
            "python3",
            "unittest",
            Path::new("data"),
            Path::new("out"),
            "demo",
        );
        assert_eq!(
            run,
            argv(&[
                "python3",
                "-m",
                "coverage",
                "run",
                "--data-file=data",
                "--source=demo",
                "-m",
                "unittest",
                "discover",
                "-s",
                ".",
            ])
        );
        assert_eq!(
            python_roots(tmp.path(), &[source]),
            vec![resolve(tmp.path())]
        );
    }

    // CR-13
    #[test]
    fn cr_13_python_executable_prefers_a_project_venv() {
        let tmp = tempdir().unwrap();
        let binary = write(tmp.path(), ".venv/bin/python", "");
        assert_eq!(python_executable(tmp.path()), binary.display().to_string());
        let bare = tempdir().unwrap();
        assert_eq!(python_executable(bare.path()), "python3");
    }

    // CR-13
    #[test]
    fn cr_13_pytest_is_detected_from_conftest_or_a_config_mentioning_it() {
        let bare = tempdir().unwrap();
        assert!(!uses_pytest(bare.path()));
        write(bare.path(), "tox.ini", "[tox]\nenvlist = py3\n");
        assert!(!uses_pytest(bare.path()));
        write(bare.path(), "conftest.py", "");
        assert!(uses_pytest(bare.path()));
        let configured = tempdir().unwrap();
        write(configured.path(), "setup.cfg", "[tool:pytest]\n");
        assert!(uses_pytest(configured.path()));
    }

    // CR-13
    #[test]
    fn cr_13_python_kind_is_unittest_without_pytest_configuration() {
        let bare = tempdir().unwrap();
        let mut exec = |_: &Cmd, _: &Path| -> i32 { panic!("no command expected") };
        assert_eq!(python_kind("python3", bare.path(), &mut exec), "unittest");
    }

    // CR-13
    #[test]
    fn cr_13_python_sources_ignore_a_file_outside_the_package() {
        let tmp = tempdir().unwrap();
        let inside = write(tmp.path(), "src/app.py", "def run():\n    return 1\n");
        let outside = PathBuf::from("/tmp/crapper-outside.py");
        assert_eq!(python_sources(tmp.path(), &[inside, outside]), "src");
    }

    // CR-13
    #[test]
    fn cr_13_python_sources_name_a_top_level_module_without_its_suffix() {
        let tmp = tempdir().unwrap();
        let module = write(tmp.path(), "carddav.py", "");
        let test = write(tmp.path(), "test_carddav.py", "");
        assert_eq!(python_sources(tmp.path(), &[module, test]), "carddav");
    }

    // CR-13
    #[test]
    fn cr_13_python_run_warnings_name_each_failed_step() {
        let package = Path::new("/p");
        assert_eq!(
            python_run_warning(package, 0, Some(1)),
            Some("coverage lcov exited 1 in /p. Python coverage will score 0%.".to_string())
        );
        assert_eq!(
            python_run_warning(package, 7, Some(0)),
            Some("Python tests exited 7 in /p. Coverage was still recorded.".to_string())
        );
        assert_eq!(
            python_run_warning(package, 7, None),
            Some("Python coverage exited 7 in /p. Python coverage will score 0%.".to_string())
        );
        assert_eq!(python_run_warning(package, 0, Some(0)), None);
        assert_eq!(python_run_warning(package, 0, None), None);
    }

    // CR-14
    #[test]
    fn cr_14_rust_kind_prefers_llvm_cov_then_tarpaulin() {
        assert_eq!(detected_rust_kind(true, true), Some("llvm-cov"));
        assert_eq!(detected_rust_kind(false, true), Some("tarpaulin"));
        assert_eq!(detected_rust_kind(false, false), None);
    }

    // CR-14
    #[test]
    fn cr_14_rust_install_without_rustup_only_runs_cargo_install() {
        assert_eq!(
            rust_install_plan(false),
            vec![
                Step::Announce(
                    "Neither cargo-llvm-cov nor cargo-tarpaulin is installed. Installing cargo-llvm-cov."
                        .to_string()
                ),
                Step::Run {
                    cmd: argv(&["cargo", "install", "cargo-llvm-cov", "--locked"]),
                    on_failure: None,
                },
            ]
        );
    }

    // CR-14
    #[test]
    fn cr_14_rust_install_with_rustup_adds_the_llvm_tools_component_first() {
        let plan = rust_install_plan(true);
        assert_eq!(plan.len(), 3);
        assert_eq!(
            plan[1],
            Step::Run {
                cmd: argv(&["rustup", "component", "add", "llvm-tools-preview"]),
                on_failure: Some("rustup component add llvm-tools-preview failed."),
            }
        );
    }

    // CR-15
    #[test]
    fn cr_15_rust_install_is_announced_before_any_install_command() {
        let plan = rust_install_plan(true);
        assert!(matches!(plan.first(), Some(Step::Announce(_))));
        assert!(plan[1..]
            .iter()
            .all(|step| matches!(step, Step::Run { .. })));
    }

    // CR-14
    #[test]
    fn cr_14_rust_kind_after_install_needs_a_zero_status_and_the_tool_on_path() {
        assert_eq!(rust_kind_after_install(0, true), Some("llvm-cov"));
        assert_eq!(rust_kind_after_install(1, true), None);
        assert_eq!(rust_kind_after_install(0, false), None);
    }

    // CR-14
    #[test]
    fn cr_14_missing_python_module_is_probed_then_installed_with_pip() {
        assert_eq!(
            module_probe("python3", "coverage"),
            argv(&["python3", "-c", "import coverage"])
        );
        assert_eq!(
            module_install_steps("python3", "coverage")[1],
            Step::Run {
                cmd: argv(&[
                    "python3",
                    "-m",
                    "pip",
                    "install",
                    "--disable-pip-version-check",
                    "coverage",
                ]),
                on_failure: None,
            }
        );
    }

    // CR-15
    #[test]
    fn cr_15_python_install_is_announced_before_pip_runs() {
        let steps = module_install_steps("python3", "coverage");
        assert_eq!(
            steps[0],
            Step::Announce(
                "Installing coverage for Python coverage. The project requirements are left unchanged."
                    .to_string()
            )
        );
        assert!(matches!(steps[1], Step::Run { .. }));
    }

    // CR-14
    #[test]
    fn cr_14_path_lookup_needs_an_executable_file() {
        let tmp = tempdir().unwrap();
        let exe = write(tmp.path(), "cargo-llvm-cov", "");
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        let plain = write(tmp.path(), "cargo-tarpaulin", "");
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
        let path_var = tmp.path().as_os_str();
        assert!(on_path_in(path_var, "cargo-llvm-cov"));
        assert!(!on_path_in(path_var, "cargo-tarpaulin"));
        assert!(!on_path_in(path_var, "missing"));
    }

    fn is_coverage_run(cmd: &Cmd) -> bool {
        matches!(cmd, Cmd::Argv(parts) if parts.iter().any(|part| part.as_str() == "run"))
    }

    // CR-13
    #[test]
    fn cr_13_remove_if_present_deletes_an_existing_file() {
        let tmp = tempdir().unwrap();
        let file = write(tmp.path(), "lcov.info", "");
        remove_if_present(&file);
        assert!(!file.exists());
    }

    // CR-13
    #[test]
    fn cr_13_remove_if_present_ignores_a_missing_file() {
        let tmp = tempdir().unwrap();
        remove_if_present(&tmp.path().join("absent.info"));
    }

    // CR-13
    #[test]
    #[should_panic(expected = "cannot remove")]
    fn cr_13_remove_if_present_panics_on_any_other_failure() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("report");
        fs::create_dir(&dir).unwrap();
        remove_if_present(&dir);
    }

    // CR-13
    #[test]
    fn cr_13_execute_runs_each_command_in_order_and_returns_the_last_status() {
        let tmp = tempdir().unwrap();
        let steps = vec![
            Step::Announce("Installing demo.".to_string()),
            Step::Run {
                cmd: argv(&["a"]),
                on_failure: None,
            },
            Step::Run {
                cmd: argv(&["b"]),
                on_failure: Some("b failed."),
            },
        ];
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            if *cmd == argv(&["b"]) {
                3
            } else {
                0
            }
        };
        assert_eq!(execute(&steps, tmp.path(), &mut exec), 3);
        assert_eq!(
            calls,
            vec![
                (argv(&["a"]), tmp.path().to_path_buf()),
                (argv(&["b"]), tmp.path().to_path_buf()),
            ]
        );
    }

    // CR-14
    #[test]
    fn cr_14_rust_kind_uses_a_tool_already_on_path_without_installing() {
        let home = tempdir().unwrap();
        let has_tool = |name: &str| name == "cargo-tarpaulin";
        let mut exec = |_: &Cmd, _: &Path| -> i32 { panic!("no install expected") };
        assert_eq!(
            rust_kind(&has_tool, home.path(), &mut exec),
            Some("tarpaulin")
        );
    }

    // CR-14
    #[test]
    fn cr_14_rust_kind_installs_cargo_llvm_cov_when_no_tool_is_present() {
        let home = tempdir().unwrap();
        let installed = Cell::new(false);
        let has_tool =
            |name: &str| name == "rustup" || (name == "cargo-llvm-cov" && installed.get());
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            installed.set(true);
            0
        };
        assert_eq!(
            rust_kind(&has_tool, home.path(), &mut exec),
            Some("llvm-cov")
        );
        let at_home = home.path().to_path_buf();
        assert_eq!(
            calls,
            vec![
                (
                    argv(&["rustup", "component", "add", "llvm-tools-preview"]),
                    at_home.clone()
                ),
                (
                    argv(&["cargo", "install", "cargo-llvm-cov", "--locked"]),
                    at_home
                ),
            ]
        );
    }

    // CR-14
    #[test]
    fn cr_14_rust_kind_is_none_when_the_install_fails() {
        let home = tempdir().unwrap();
        let has_tool = |name: &str| name == "rustup";
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, _cwd: &Path| {
            calls.push(cmd.clone());
            1
        };
        assert_eq!(rust_kind(&has_tool, home.path(), &mut exec), None);
        assert_eq!(
            calls,
            vec![
                argv(&["rustup", "component", "add", "llvm-tools-preview"]),
                argv(&["cargo", "install", "cargo-llvm-cov", "--locked"]),
            ]
        );
    }

    // CR-13
    #[test]
    fn cr_13_rust_coverage_runs_the_tool_in_each_module_and_clears_the_old_report() {
        let tmp = tempdir().unwrap();
        write(
            tmp.path(),
            "src-tauri/Cargo.toml",
            "[package]\nname = \"bookwriter\"\n",
        );
        let lib = write(
            tmp.path(),
            "src-tauri/src/lib.rs",
            "pub fn read_text() {}\n",
        );
        let module = resolve(&tmp.path().join("src-tauri"));
        let report = coverage_report(tmp.path(), &module, "rust");
        write(
            tmp.path(),
            "target/coverage/rust/src-tauri/lcov.info",
            "stale",
        );
        let has_tool = |name: &str| name == "cargo-llvm-cov";
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            2
        };
        cover_rust_with(tmp.path(), &[lib], &has_tool, tmp.path(), &mut exec);
        assert_eq!(
            calls,
            vec![(rust_coverage_command("llvm-cov", &report), module)]
        );
        assert!(!report.exists());
        assert!(report.parent().unwrap().is_dir());
    }

    // CR-13
    #[test]
    fn cr_13_rust_coverage_without_a_cargo_manifest_runs_nothing() {
        let tmp = tempdir().unwrap();
        let lib = write(tmp.path(), "scripts/lib.rs", "");
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, _cwd: &Path| {
            calls.push(cmd.clone());
            0
        };
        cover_rust_with(tmp.path(), &[lib], &|_: &str| true, tmp.path(), &mut exec);
        assert!(calls.is_empty());
    }

    // CR-14
    #[test]
    fn cr_14_rust_coverage_stops_when_no_tool_can_be_installed() {
        let tmp = tempdir().unwrap();
        write(tmp.path(), "Cargo.toml", "[package]\nname = \"demo\"\n");
        let lib = write(tmp.path(), "src/lib.rs", "");
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, _cwd: &Path| {
            calls.push(cmd.clone());
            1
        };
        cover_rust_with(tmp.path(), &[lib], &|_: &str| false, tmp.path(), &mut exec);
        assert_eq!(
            calls,
            vec![argv(&["cargo", "install", "cargo-llvm-cov", "--locked"])]
        );
    }

    // CR-13
    #[test]
    fn cr_13_python_coverage_runs_pytest_and_records_lcov_once_data_is_written() {
        let tmp = tempdir().unwrap();
        let root = resolve(tmp.path());
        write(
            &root,
            "pyproject.toml",
            "[tool.pytest.ini_options]\ntestpaths = ['tests']\n",
        );
        let source = write(&root, "src/demo/box.py", "def choose(x):\n    return x\n");
        let test = write(
            &root,
            "tests/test_box.py",
            "def test_choose():\n    assert True\n",
        );
        let report = coverage_report(&root, &root, "python");
        let data = report_dir(&report).join(".coverage");
        let (run, lcov) = python_coverage_commands("python3", "pytest", &data, &report, "src");
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            if is_coverage_run(cmd) {
                write(cwd, "target/coverage/python/.coverage", "");
            }
            0
        };
        cover_python_with(&root, &[source, test], &mut exec);
        assert_eq!(
            calls,
            vec![
                (argv(&["python3", "-c", "import coverage"]), root.clone()),
                (argv(&["python3", "-c", "import pytest"]), root.clone()),
                (run, root.clone()),
                (lcov, root.clone()),
            ]
        );
    }

    // CR-13
    #[test]
    fn cr_13_python_coverage_without_a_data_file_skips_lcov_and_uses_unittest() {
        let tmp = tempdir().unwrap();
        let root = resolve(tmp.path());
        let source = write(&root, "demo/app.py", "def run():\n    return 1\n");
        let report = coverage_report(&root, &root, "python");
        let data = report_dir(&report).join(".coverage");
        let (run, _lcov) = python_coverage_commands("python3", "unittest", &data, &report, "demo");
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            0
        };
        cover_python_with(&root, &[source], &mut exec);
        assert_eq!(
            calls,
            vec![
                (argv(&["python3", "-c", "import coverage"]), root.clone()),
                (run, root.clone()),
            ]
        );
    }

    // CR-14
    #[test]
    fn cr_14_python_coverage_is_skipped_when_coverage_cannot_be_installed() {
        let tmp = tempdir().unwrap();
        let root = resolve(tmp.path());
        let source = write(&root, "demo/app.py", "");
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd, cwd: &Path| {
            calls.push((cmd.clone(), cwd.to_path_buf()));
            1
        };
        cover_python_with(&root, &[source], &mut exec);
        assert_eq!(
            calls,
            vec![
                (argv(&["python3", "-c", "import coverage"]), root.clone()),
                (
                    argv(&[
                        "python3",
                        "-m",
                        "pip",
                        "install",
                        "--disable-pip-version-check",
                        "coverage",
                    ]),
                    root.clone()
                ),
            ]
        );
    }

    // CR-14
    #[test]
    fn cr_14_python_kind_falls_back_to_unittest_when_pytest_cannot_be_installed() {
        let tmp = tempdir().unwrap();
        let root = resolve(tmp.path());
        write(&root, "pytest.ini", "[pytest]\n");
        let mut exec = |_: &Cmd, _: &Path| 1;
        assert_eq!(python_kind("python3", &root, &mut exec), "unittest");
    }
}
