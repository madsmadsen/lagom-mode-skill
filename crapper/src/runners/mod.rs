// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/runners.py
// license: MIT

//! Run each language's coverage tool, then leave the reports for the loader.
//!
//! Failures are reported and do not stop analysis. A language with no coverage
//! tool, or a failed run, scores its functions at 0%.

mod rustpy;
mod typescript_cov;

use std::collections::{BTreeSet, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::discover::language_of;

const MAVEN: &[&str] = &[
    "mvn",
    "-q",
    "org.jacoco:jacoco-maven-plugin:0.8.12:prepare-agent",
    "test",
    "org.jacoco:jacoco-maven-plugin:0.8.12:report",
];

const COVERAGE_DIRS: &[&str] = &["typescript", "rust", "go", "python"];

/// A coverage command: the user's `--coverage-command` text, or an argument vector.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// Runs through `sh -c`, exactly as the user typed it.
    Shell(String),
    /// Runs without a shell, so each element stays one argument.
    Argv(Vec<String>),
}

pub(crate) fn warn(message: &str) {
    eprintln!("{message}");
}

fn attempt(what: &str, result: io::Result<()>) {
    if let Err(err) = result {
        warn(&format!("{what}: {err}"));
    }
}

fn argv(parts: &[&str]) -> Cmd {
    Cmd::Argv(parts.iter().map(|part| part.to_string()).collect())
}

fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn show(cmd: &Cmd) -> String {
    match cmd {
        Cmd::Shell(text) => text.clone(),
        Cmd::Argv(parts) => parts
            .iter()
            .map(|part| quote(part))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Python's `shlex.quote`: safe words pass through, anything else is single-quoted.
fn quote(part: &str) -> String {
    if part.is_empty() {
        return "''".to_string();
    }
    if part.chars().all(is_shell_safe) {
        return part.to_string();
    }
    format!("'{}'", part.replace('\'', "'\"'\"'"))
}

fn is_shell_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)
}

pub(crate) fn run_shell(cmd: &Cmd, cwd: &Path) -> i32 {
    warn(&format!("+ ({}) {}", cwd.display(), show(cmd)));
    match spawn(cmd).current_dir(cwd).status() {
        Ok(status) => exit_code(status),
        Err(err) => {
            warn(&format!("Coverage command failed to start: {err}"));
            127
        }
    }
}

fn spawn(cmd: &Cmd) -> Command {
    match cmd {
        Cmd::Shell(text) => {
            let mut command = Command::new("sh");
            command.arg("-c").arg(text);
            command
        }
        Cmd::Argv(parts) => {
            let mut command = Command::new(parts.first().map_or("", String::as_str));
            command.args(parts.iter().skip(1));
            command
        }
    }
}

/// Python reports a signal death as the negated signal number.
fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| -status.signal().unwrap_or(0))
}

pub(crate) fn nearest(start: &Path, marker: &str, stop: &Path) -> Option<PathBuf> {
    let stop = resolved(stop);
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent().unwrap_or(Path::new("")).to_path_buf()
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

fn has_suffix(path: &Path, suffix: &str) -> bool {
    path.extension() == Some(OsStr::new(suffix.trim_start_matches('.')))
}

pub(crate) fn modules_with(
    files: &[PathBuf],
    suffix: &str,
    marker: &str,
    root: &Path,
) -> Vec<PathBuf> {
    let modules: BTreeSet<PathBuf> = files
        .iter()
        .filter(|file| has_suffix(file, suffix))
        .filter_map(|file| nearest(file, marker, root))
        .collect();
    modules.into_iter().collect()
}

pub(crate) fn coverage_report(root: &Path, module: &Path, language: &str) -> PathBuf {
    let root = resolved(root);
    let module = resolved(module);
    let base = root.join("target").join("coverage").join(language);
    if module == root {
        return base.join("lcov.info");
    }
    let slug = module
        .strip_prefix(&root)
        .expect("a module found under the project root lies under it")
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("__");
    base.join(slug).join("lcov.info")
}

fn unlink(path: &Path) {
    attempt(
        &format!("Could not remove {}", path.display()),
        fs::remove_file(path),
    );
}

fn clean_dir(path: &Path) {
    if path.exists() {
        attempt(
            &format!("Could not remove {}", path.display()),
            fs::remove_dir_all(path),
        );
    }
}

/// Every entry below `dir`, like Python's `rglob("*")`. Symlinked directories are not entered.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let descend = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let path = entry.path();
        if descend {
            found.extend(walk(&path));
        }
        found.push(path);
    }
    found
}

fn is_coverage_component(component: Component) -> bool {
    matches!(component, Component::Normal(name) if name.to_str().is_some_and(|name| COVERAGE_DIRS.contains(&name)))
}

fn outside_language_dirs(coverage: &Path, path: &Path) -> bool {
    path.strip_prefix(coverage)
        .is_ok_and(|relative| !relative.components().any(is_coverage_component))
}

fn remove_cloverage_html(coverage: &Path) {
    for path in walk(coverage) {
        let is_html = path.extension() == Some(OsStr::new("html"));
        if is_html && !path.is_dir() && outside_language_dirs(coverage, &path) {
            unlink(&path);
        }
    }
}

fn remove_empty_dirs(coverage: &Path) {
    let mut dirs: Vec<PathBuf> = walk(coverage)
        .into_iter()
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs
        .iter()
        .rev()
        .filter(|dir| outside_language_dirs(coverage, dir))
    {
        // A directory that still holds files stays, as upstream's failed rmdir does.
        let _ = fs::remove_dir(dir);
    }
}

fn clean_clojure(root: &Path) {
    let coverage = root.join("target").join("coverage");
    if !coverage.is_dir() {
        return;
    }
    let lcov = coverage.join("lcov.info");
    if lcov.is_file() {
        unlink(&lcov);
    }
    remove_cloverage_html(&coverage);
    remove_empty_dirs(&coverage);
}

fn clojure_commands() -> [Cmd; 2] {
    [argv(&["clj", "-M:cov", "--lcov"]), argv(&["clj", "-M:cov"])]
}

fn cover_clojure(root: &Path) {
    if !root.join("deps.edn").is_file() && !root.join("bb.edn").is_file() {
        warn("No deps.edn or bb.edn; skipping Clojure coverage.");
        return;
    }
    clean_clojure(root);
    let [with_lcov, plain] = clojure_commands();
    let mut code = run_shell(&with_lcov, root);
    if code != 0 {
        warn("clj -M:cov --lcov failed; retrying without --lcov.");
        code = run_shell(&plain, root);
    }
    if code != 0 {
        warn(&format!(
            "Clojure coverage exited {code}. Clojure coverage will score 0%."
        ));
    }
}

fn maven_command() -> Cmd {
    argv(MAVEN)
}

fn cover_java(root: &Path, files: &[PathBuf]) {
    let modules = modules_with(files, ".java", "pom.xml", root);
    if modules.is_empty() {
        warn("No pom.xml; skipping Java coverage.");
        return;
    }
    for module in &modules {
        clean_dir(&module.join("target").join("site").join("jacoco"));
        let exec_file = module.join("target").join("jacoco.exec");
        if exec_file.exists() {
            unlink(&exec_file);
        }
        let code = run_shell(&maven_command(), module);
        if code != 0 {
            warn(&format!(
                "Java coverage exited {code} in {}. Java coverage will score 0%.",
                module.display()
            ));
        }
    }
}

fn go_profile(module: &Path) -> PathBuf {
    module
        .join("target")
        .join("coverage")
        .join("go")
        .join("coverage.out")
}

fn go_command(profile: &Path) -> Cmd {
    Cmd::Argv(vec![
        "go".to_string(),
        "test".to_string(),
        "./...".to_string(),
        format!("-coverprofile={}", profile.display()),
    ])
}

fn cover_go(root: &Path, files: &[PathBuf]) {
    let modules = modules_with(files, ".go", "go.mod", root);
    if modules.is_empty() {
        warn("No go.mod; skipping Go coverage.");
        return;
    }
    for module in &modules {
        let profile = go_profile(module);
        if let Some(dir) = profile.parent() {
            attempt(
                &format!("Could not create {}", dir.display()),
                fs::create_dir_all(dir),
            );
        }
        if profile.exists() {
            unlink(&profile);
        }
        let code = run_shell(&go_command(&profile), module);
        if code != 0 {
            warn(&format!(
                "Go coverage exited {code} in {}. Go coverage will score 0%.",
                module.display()
            ));
        }
    }
}

fn run_custom(root: &Path, text: &str) -> i32 {
    let code = run_shell(&Cmd::Shell(text.to_string()), root);
    if code != 0 {
        warn(&format!(
            "Coverage command exited {code}. Reports already on disk will not be read."
        ));
    }
    code
}

pub fn run_coverage(root: &Path, files: &[PathBuf], command: Option<&str>) -> Option<i32> {
    let root = resolved(root);
    if let Some(text) = command.filter(|text| !text.is_empty()) {
        return Some(run_custom(&root, text));
    }
    let languages: HashSet<&str> = files.iter().filter_map(|file| language_of(file)).collect();
    if languages.contains("clojure") {
        cover_clojure(&root);
    }
    if languages.contains("java") {
        cover_java(&root, files);
    }
    if languages.contains("go") {
        cover_go(&root, files);
    }
    if languages.contains("typescript") {
        typescript_cov::cover_typescript(&root, files);
    }
    if languages.contains("python") {
        rustpy::cover_python(&root, files);
    }
    if languages.contains("rust") {
        rustpy::cover_rust(&root, files);
    }
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(root: &Path, relative: &str, text: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    // CR-13 a shell command runs through sh -c and returns the child's status
    #[test]
    fn cr_13_shell_command_returns_child_status() {
        let dir = tempdir().unwrap();
        assert_eq!(run_shell(&Cmd::Shell("exit 3".into()), dir.path()), 3);
        assert_eq!(run_shell(&Cmd::Shell("true".into()), dir.path()), 0);
        assert_eq!(run_shell(&Cmd::Shell("false".into()), dir.path()), 1);
    }

    // CR-13 a program that cannot start is reported with status 127
    #[test]
    fn cr_13_missing_program_returns_127() {
        let dir = tempdir().unwrap();
        let cmd = Cmd::Argv(vec!["/nonexistent/crapper-no-such-program".into()]);
        assert_eq!(run_shell(&cmd, dir.path()), 127);
    }

    // CR-13 an argument vector does not go through a shell
    #[test]
    fn cr_13_argument_vector_is_not_a_shell() {
        let dir = tempdir().unwrap();
        let marker = dir.path().join("pwned");
        let cmd = Cmd::Argv(vec!["true".into(), format!("a;touch {}", marker.display())]);
        assert_eq!(run_shell(&cmd, dir.path()), 0);
        assert!(!marker.exists());
    }

    // CR-13 show quotes argument vector parts the way shlex.quote does
    #[test]
    fn cr_13_show_quotes_like_shlex() {
        let cmd = Cmd::Argv(vec![
            "go".into(),
            "a b".into(),
            "it's".into(),
            "plain-1.0_x".into(),
            "".into(),
        ]);
        assert_eq!(show(&cmd), "go 'a b' 'it'\"'\"'s' plain-1.0_x ''");
        assert_eq!(show(&Cmd::Shell("exit 3".into())), "exit 3");
    }

    // CR-13 nearest finds the closest directory holding the marker and never climbs past stop
    #[test]
    fn cr_13_nearest_stops_at_the_project_root() {
        let dir = tempdir().unwrap();
        let root = resolved(dir.path());
        touch(&root, "pom.xml", "<project/>");
        let source = touch(&root, "a/b/Board.java", "class Board {}");
        assert_eq!(nearest(&source, "pom.xml", &root), Some(root.clone()));
        assert_eq!(nearest(&source, "go.mod", &root), None);
    }

    // CR-13 modules_with returns each module once, sorted, for files with the given suffix
    #[test]
    fn cr_13_modules_with_collects_nested_modules_once() {
        let dir = tempdir().unwrap();
        let root = resolved(dir.path());
        touch(&root, "pom.xml", "<project/>");
        touch(&root, "svc/pom.xml", "<project/>");
        let files = vec![
            touch(&root, "src/Board.java", "class Board {}"),
            touch(&root, "src/Other.java", "class Other {}"),
            touch(&root, "svc/App.java", "class App {}"),
            touch(&root, "src/notes.txt", "x"),
        ];
        assert_eq!(
            modules_with(&files, ".java", "pom.xml", &root),
            vec![root.clone(), root.join("svc")]
        );
    }

    // CR-13 clean_clojure removes only the top-level lcov, Cloverage html, and empty non-language dirs
    #[test]
    fn cr_13_clean_clojure_keeps_other_language_reports() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let kept = touch(root, "target/coverage/python/lcov.info", "keep");
        let kept_html = touch(root, "target/coverage/python/index.html", "keep");
        let stale = touch(root, "target/coverage/lcov.info", "old");
        let stale_html = touch(root, "target/coverage/index.html", "old");
        fs::create_dir_all(root.join("target/coverage/clojure/empty")).unwrap();
        clean_clojure(root);
        assert!(kept.is_file());
        assert!(kept_html.is_file());
        assert!(!stale.exists());
        assert!(!stale_html.exists());
        assert!(!root.join("target/coverage/clojure").exists());
    }

    // CR-13 coverage_report stores a module's lcov under its language, slugging nested paths
    #[test]
    fn cr_13_coverage_report_slugs_nested_modules() {
        let dir = tempdir().unwrap();
        let root = resolved(dir.path());
        fs::create_dir_all(root.join("src-tauri")).unwrap();
        fs::create_dir_all(root.join("a/b")).unwrap();
        assert_eq!(
            coverage_report(&root, &root, "rust"),
            root.join("target/coverage/rust/lcov.info")
        );
        assert_eq!(
            coverage_report(&root, &root.join("src-tauri"), "rust"),
            root.join("target/coverage/rust/src-tauri/lcov.info")
        );
        assert_eq!(
            coverage_report(&root, &root.join("a/b"), "rust"),
            root.join("target/coverage/rust/a__b/lcov.info")
        );
    }

    // CR-13 the Maven command runs the JaCoCo prepare-agent, test, and report goals
    #[test]
    fn cr_13_maven_command_runs_jacoco_goals() {
        let text = show(&maven_command());
        assert!(text.starts_with("mvn -q org.jacoco:jacoco-maven-plugin:0.8.12:prepare-agent test"));
        assert!(text.ends_with("org.jacoco:jacoco-maven-plugin:0.8.12:report"));
    }

    // CR-13 the Go profile lives under target/coverage/go and stays one argument when the path has spaces
    #[test]
    fn cr_13_go_coverprofile_stays_one_argument() {
        assert_eq!(
            go_profile(Path::new("/m")),
            PathBuf::from("/m/target/coverage/go/coverage.out")
        );
        let profile = Path::new("/tmp/my module/target/coverage/go/coverage.out");
        assert_eq!(
            go_command(profile),
            Cmd::Argv(vec![
                "go".into(),
                "test".into(),
                "./...".into(),
                "-coverprofile=/tmp/my module/target/coverage/go/coverage.out".into(),
            ])
        );
    }

    // CR-13 Clojure tries the --lcov alias first, then the plain alias
    #[test]
    fn cr_13_clojure_tries_lcov_then_plain() {
        let [first, second] = clojure_commands();
        assert_eq!(show(&first), "clj -M:cov --lcov");
        assert_eq!(show(&second), "clj -M:cov");
    }

    // CR-13 a custom --coverage-command status is returned unchanged
    #[test]
    fn cr_13_custom_command_status_is_returned() {
        let dir = tempdir().unwrap();
        assert_eq!(run_coverage(dir.path(), &[], Some("exit 3")), Some(3));
        assert_eq!(run_coverage(dir.path(), &[], Some("true")), Some(0));
    }

    // CR-13 an empty --coverage-command is no command, so the per-language path runs and returns 0
    #[test]
    fn cr_13_empty_command_is_no_command() {
        let dir = tempdir().unwrap();
        assert_eq!(run_coverage(dir.path(), &[], Some("")), Some(0));
    }
}
