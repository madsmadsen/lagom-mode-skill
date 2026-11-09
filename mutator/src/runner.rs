//! Port of mutator/src/mutator/runner.py (BSD Zero Clause License, Robert C. Martin).

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, PipeReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{self, Child, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::Language;

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const TIMED_OUT_CODE: i32 = 124;
const SPAWN_FAILED_CODE: i32 = 127;
const UNKNOWN_EXIT_CODE: i32 = 1;

/// A test command: a shell string the user typed, or an argument vector run without a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Shell(String),
    Argv(Vec<String>),
}

/// The outcome of one command run.
#[derive(Clone, Debug)]
pub struct CommandResult {
    pub code: i32,
    pub timed_out: bool,
    pub seconds: f64,
    pub output: String,
}

/// Runs commands, printing each one to stderr first when verbose.
#[derive(Clone, Debug, Default)]
pub struct CommandRunner {
    pub verbose: bool,
}

impl CommandRunner {
    /// Run `command` in `cwd`, merging stdout and stderr into `output`.
    pub fn run(&self, command: &Command, cwd: &Path, timeout: Option<Duration>) -> CommandResult {
        if self.verbose {
            eprintln!("+ ({}) {}", cwd.display(), display_command(command));
        }
        let started = Instant::now();
        let (code, timed_out, output) = execute(command, cwd, timeout);
        CommandResult {
            code,
            timed_out,
            seconds: started.elapsed().as_secs_f64(),
            output,
        }
    }
}

/// A shell-looking rendering. An argument vector is quoted so it can be pasted.
pub fn display_command(command: &Command) -> String {
    match command {
        Command::Shell(text) => text.clone(),
        Command::Argv(parts) => parts
            .iter()
            .map(String::as_str)
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// A stable identity for a command, used as a baseline cache key.
pub fn command_key(command: &Command) -> String {
    match command {
        Command::Shell(text) => format!("sh\0{text}"),
        Command::Argv(parts) => std::iter::once("argv")
            .chain(parts.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\0"),
    }
}

/// The nearest directory at or above `start` that holds `marker`, never above `stop`.
pub fn nearest(start: &Path, marker: &str, stop: &Path) -> Option<PathBuf> {
    let start_dir = if start.is_dir() {
        start
    } else {
        start.parent()?
    };
    let stop = resolved(stop);
    let mut current = resolved(start_dir);
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

/// The test command and the directory it runs in.
///
/// A user override stays a shell string. Every generated command is an argument vector,
/// so a package path is one argument.
pub fn test_plan(
    root: &Path,
    source: &Path,
    language: Language,
    override_command: Option<&str>,
) -> (Command, PathBuf) {
    let root = resolved(root);
    match override_command.filter(|text| !text.is_empty()) {
        Some(text) => (Command::Shell(text.to_string()), root),
        None => plan(language, &root, source),
    }
}

fn execute(command: &Command, cwd: &Path, timeout: Option<Duration>) -> (i32, bool, String) {
    let (mut child, reader) = match spawn(command, cwd) {
        Ok(parts) => parts,
        Err(error) => return (SPAWN_FAILED_CODE, false, error.to_string()),
    };
    let drain = thread::spawn(move || read_all(reader));
    let (code, timed_out) = finish(&mut child, timeout);
    (code, timed_out, drain.join().unwrap_or_default())
}

fn spawn(command: &Command, cwd: &Path) -> io::Result<(Child, PipeReader)> {
    let (reader, writer) = io::pipe()?;
    let mut process = build_process(command)?;
    process
        .current_dir(cwd)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdout(Stdio::from(writer.try_clone()?))
        .stderr(Stdio::from(writer))
        .process_group(0);
    if let Some(path) = worker_python_path(cwd) {
        process.env("PYTHONPATH", path);
    }
    let child = process.spawn()?;
    Ok((child, reader))
}

fn build_process(command: &Command) -> io::Result<process::Command> {
    match command {
        Command::Shell(text) => {
            let mut process = process::Command::new("sh");
            process.arg("-c").arg(text);
            Ok(process)
        }
        Command::Argv(argv) => {
            let (program, args) = argv.split_first().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "empty argument vector")
            })?;
            let mut process = process::Command::new(program);
            process.args(args);
            Ok(process)
        }
    }
}

fn read_all(mut reader: PipeReader) -> String {
    let mut bytes = Vec::new();
    // A read error ends the capture; the bytes read before it are kept.
    let _ = reader.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn finish(child: &mut Child, timeout: Option<Duration>) -> (i32, bool) {
    let outcome = wait_for(child, timeout);
    if let Ok(Some(status)) = outcome {
        return (exit_code(status), false);
    }
    kill_group_and_reap(child);
    let timed_out = matches!(outcome, Ok(None));
    let code = if timed_out {
        TIMED_OUT_CODE
    } else {
        UNKNOWN_EXIT_CODE
    };
    (code, timed_out)
}

fn wait_for(child: &mut Child, timeout: Option<Duration>) -> io::Result<Option<ExitStatus>> {
    let Some(limit) = timeout else {
        return child.wait().map(Some);
    };
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        // Some once the elapsed time reaches the limit.
        if started.elapsed().checked_sub(limit).is_some() {
            return Ok(None);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn kill_group_and_reap(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `pid` is the child's pid and the child was spawned with `process_group(0)`,
        // so `-pid` names its own process group. The child is not reaped yet, so the pid and
        // the group id cannot be reused by another process before this call.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    let _ = child.wait();
}

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(UNKNOWN_EXIT_CODE)
}

fn shell_quote(part: &str) -> String {
    if !part.is_empty() && part.chars().all(is_shell_safe) {
        return part.to_string();
    }
    format!("'{}'", part.replace('\'', r#"'"'"'"#))
}

fn is_shell_safe(character: char) -> bool {
    character.is_ascii_alphanumeric() || "_@%+=:,./-".contains(character)
}

fn worker_python_path(cwd: &Path) -> Option<OsString> {
    let worker = worker_home(cwd)?;
    let mut entries = source_entries(cwd, &worker);
    if let Some(current) = std::env::var_os("PYTHONPATH").filter(|value| !value.is_empty()) {
        entries.push(current);
    }
    if entries.is_empty() {
        return None;
    }
    Some(join_path_list(&entries))
}

fn worker_home(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|candidate| is_worker_home(candidate))
        .map(Path::to_path_buf)
}

fn is_worker_home(candidate: &Path) -> bool {
    let Some(run) = candidate.parent() else {
        return false;
    };
    let Some(workers) = run.parent() else {
        return false;
    };
    name(candidate).starts_with("worker-")
        && name(run).starts_with("run-")
        && name(workers) == "mutation-workers"
}

fn name(path: &Path) -> &str {
    path.file_name().and_then(OsStr::to_str).unwrap_or("")
}

fn source_entries(cwd: &Path, worker: &Path) -> Vec<OsString> {
    let candidates = [
        cwd.join("src"),
        cwd.to_path_buf(),
        worker.join("src"),
        worker.to_path_buf(),
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    for directory in candidates {
        if !found.contains(&directory) && directory.is_dir() {
            found.push(directory);
        }
    }
    found.into_iter().map(PathBuf::into_os_string).collect()
}

fn join_path_list(entries: &[OsString]) -> OsString {
    let mut joined = OsString::new();
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            joined.push(":");
        }
        joined.push(entry);
    }
    joined
}

fn plan(language: Language, root: &Path, source: &Path) -> (Command, PathBuf) {
    match language {
        Language::Clojure => {
            let directory = nearest_marker(source, root, &["deps.edn", "bb.edn"]);
            (clojure_command(&directory), directory)
        }
        Language::Java => {
            let directory = nearest_marker(source, root, &["pom.xml"]);
            (
                argv(["mvn", "-q", "test", "-DexcludeTags=no-mutate"]),
                directory,
            )
        }
        Language::Go => {
            let directory = nearest_marker(source, root, &["go.mod"]);
            let package = go_package(&directory, source);
            (
                argv(["go", "test", "-count=1", package.as_str()]),
                directory,
            )
        }
        Language::TypeScript => {
            let directory = nearest_marker(source, root, &["package.json"]);
            (argv(["npm", "test"]), directory)
        }
        Language::Rust => {
            let directory = nearest_marker(source, root, &["Cargo.toml"]);
            (argv(["cargo", "test"]), directory)
        }
        Language::Python => {
            let directory =
                nearest_marker(source, root, &["pyproject.toml", "pytest.ini", "setup.cfg"]);
            (python_command(&directory), directory)
        }
    }
}

pub fn nearest_marker(source: &Path, root: &Path, markers: &[&str]) -> PathBuf {
    markers
        .iter()
        .find_map(|marker| nearest(source, marker, root))
        .unwrap_or_else(|| root.to_path_buf())
}

fn argv<'a>(parts: impl IntoIterator<Item = &'a str>) -> Command {
    Command::Argv(parts.into_iter().map(str::to_string).collect())
}

fn clojure_command(directory: &Path) -> Command {
    if let Some(command) = babashka_command(directory) {
        return command;
    }
    deps_command(&read(&directory.join("deps.edn")))
}

fn babashka_command(directory: &Path) -> Option<Command> {
    let bb = directory.join("bb.edn");
    if !bb.is_file() || directory.join("deps.edn").is_file() {
        return None;
    }
    if read(&bb).contains("spec") {
        Some(argv(["bb", "spec", "--tag", "~no-mutate"]))
    } else {
        Some(argv(["bb", "test"]))
    }
}

fn deps_command(text: &str) -> Command {
    match (text.contains(":spec"), text.contains("speclj")) {
        (true, true) => argv(["clj", "-M:spec", "--tag", "~no-mutate"]),
        (true, false) => argv(["clj", "-M:spec"]),
        (false, _) => argv(["clj", "-M:test"]),
    }
}

fn python_command(directory: &Path) -> Command {
    let interpreter = project_interpreter(directory);
    if uses_pytest(directory) {
        argv([interpreter.as_str(), "-m", "pytest"])
    } else {
        argv([interpreter.as_str(), "-m", "unittest", "discover"])
    }
}

fn uses_pytest(directory: &Path) -> bool {
    directory.join("pytest.ini").is_file()
        || directory.join("conftest.py").is_file()
        || read(&directory.join("pyproject.toml")).contains("pytest")
        || read(&directory.join("setup.cfg")).contains("pytest")
}

/// The project's virtualenv, or `python3` when the project has none.
///
/// The path stays absolute and the symlink is left in place: a worker does not link `.venv`,
/// and resolving `bin/python` would leave the virtualenv.
fn project_interpreter(directory: &Path) -> String {
    [".venv", "venv"]
        .iter()
        .map(|name| directory.join(name).join("bin").join("python"))
        .find(|candidate| is_executable_file(candidate))
        .map_or_else(
            || "python3".to_string(),
            |candidate| absolute(&candidate).display().to_string(),
        )
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn go_package(directory: &Path, source: &Path) -> String {
    let source_dir = resolved(source);
    let relative = source_dir
        .parent()
        .and_then(|dir| dir.strip_prefix(resolved(directory)).ok());
    match relative {
        Some(relative) if !relative.as_os_str().is_empty() => {
            format!("./{}", relative.display())
        }
        _ => ".".to_string(),
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn resolved(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| absolute(path))
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Language;
    use std::os::unix::fs::symlink;
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn shell(text: &str) -> Command {
        Command::Shell(text.to_string())
    }

    fn run_in(command: &Command, cwd: &Path) -> CommandResult {
        CommandRunner::default().run(command, cwd, None)
    }

    fn write_file(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directory");
        }
        fs::write(path, text).expect("write fixture file");
    }

    fn write_executable(path: &Path) {
        write_file(path, "#!/bin/sh\n");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod fixture");
    }

    fn display(path: &Path) -> String {
        path.display().to_string()
    }

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Runs `body` with PYTHONPATH set to `value` (or unset), then restores the previous value.
    fn with_python_path<T>(value: Option<&str>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = std::env::var_os("PYTHONPATH");
        set_python_path(value.map(OsStr::new));
        let result = body();
        set_python_path(previous.as_deref());
        result
    }

    fn set_python_path(value: Option<&OsStr>) {
        // SAFETY: every caller holds ENV_LOCK, so no other test in this module changes the environment at the same time.
        unsafe {
            match value {
                Some(value) => std::env::set_var("PYTHONPATH", value),
                None => std::env::remove_var("PYTHONPATH"),
            }
        }
    }

    fn python_path_in(cwd: &Path) -> String {
        run_in(&shell(r#"printf %s "$PYTHONPATH""#), cwd).output
    }

    // MU-14: the baseline runs the test command and keeps its exit status.
    #[test]
    fn mu_14_exit_code_is_the_child_exit_code() {
        let dir = tempdir().expect("tempdir");
        let result = run_in(&shell("exit 3"), dir.path());
        assert_eq!(result.code, 3);
        assert!(!result.timed_out);
    }

    // MU-14: a child killed by a signal reports 128 plus the signal number.
    #[test]
    fn mu_14_signal_exit_is_128_plus_signal() {
        let dir = tempdir().expect("tempdir");
        let result = run_in(&shell("kill -9 $$"), dir.path());
        assert_eq!(result.code, 137);
    }

    // MU-14: stdout and stderr end up in one output string.
    #[test]
    fn mu_14_output_merges_stdout_and_stderr() {
        let dir = tempdir().expect("tempdir");
        let result = run_in(&shell("echo out; echo err >&2"), dir.path());
        assert_eq!(result.output, "out\nerr\n");
    }

    // MU-14: a missing program is a spawn failure with exit code 127.
    #[test]
    fn mu_14_missing_binary_exits_127() {
        let dir = tempdir().expect("tempdir");
        let result = run_in(&argv(["mu-no-such-binary-xyz"]), dir.path());
        assert_eq!(result.code, 127);
        assert!(!result.output.is_empty());
    }

    // MU-14: an empty argument vector cannot spawn, so it reports 127 without panicking.
    #[test]
    fn mu_14_empty_argv_exits_127() {
        let dir = tempdir().expect("tempdir");
        let result = run_in(&Command::Argv(Vec::new()), dir.path());
        assert_eq!(result.code, 127);
    }

    // MU-14: an argument vector never goes through a shell.
    #[test]
    fn mu_14_argv_is_not_split_by_a_shell() {
        let dir = tempdir().expect("tempdir");
        let result = run_in(&argv(["/bin/echo", "a;echo no"]), dir.path());
        assert_eq!(result.code, 0);
        assert_eq!(result.output, "a;echo no\n");
    }

    // MU-14: the child never writes Python bytecode.
    #[test]
    fn mu_14_python_writes_no_bytecode() {
        let dir = tempdir().expect("tempdir");
        let result = run_in(
            &shell(r#"printf %s "$PYTHONDONTWRITEBYTECODE""#),
            dir.path(),
        );
        assert_eq!(result.output, "1");
    }

    // MU-17: inside a worker, the worker's sources come first on PYTHONPATH.
    #[test]
    fn mu_17_worker_overlay_puts_worker_sources_first_on_python_path() {
        let tmp = tempdir().expect("tempdir");
        let worker = tmp.path().join("target/mutation-workers/run-1/worker-0");
        let src = worker.join("src");
        fs::create_dir_all(&src).expect("create worker src");
        let result = run_in(&shell(r#"printf %s "$PYTHONPATH""#), &worker);
        let expected = format!("{}:{}", display(&src), display(&worker));
        assert!(
            result.output.starts_with(expected.as_str()),
            "{}",
            result.output
        );
    }

    // MU-17: only a worker-N directory under run-N under mutation-workers is a worker home.
    #[test]
    fn mu_17_worker_home_needs_the_worker_run_and_workers_names() {
        assert_eq!(
            worker_home(Path::new("/p/target/mutation-workers/run-1/worker-0/sub")),
            Some(PathBuf::from("/p/target/mutation-workers/run-1/worker-0"))
        );
        assert_eq!(worker_home(Path::new("/p/other/run-1/worker-0")), None);
        assert_eq!(
            worker_home(Path::new("/p/mutation-workers/job/worker-0")),
            None
        );
    }

    // MU-17: a worker with no src directory puts only the worker directory on PYTHONPATH.
    #[test]
    fn mu_17_worker_overlay_skips_missing_source_directories() {
        let tmp = tempdir().expect("tempdir");
        let worker = tmp.path().join("target/mutation-workers/run-1/worker-0");
        fs::create_dir_all(&worker).expect("create worker");
        let output = with_python_path(None, || python_path_in(&worker));
        assert_eq!(output, display(&worker));
    }

    // MU-17: a parent PYTHONPATH is kept after the worker entries.
    #[test]
    fn mu_17_worker_overlay_appends_the_parent_python_path() {
        let tmp = tempdir().expect("tempdir");
        let worker = tmp.path().join("target/mutation-workers/run-1/worker-0");
        let src = worker.join("src");
        fs::create_dir_all(&src).expect("create worker src");
        let output = with_python_path(Some("/mu/parent"), || python_path_in(&worker));
        assert_eq!(
            output,
            format!("{}:{}:/mu/parent", display(&src), display(&worker))
        );
    }

    // MU-17: an empty parent PYTHONPATH adds no empty entry.
    #[test]
    fn mu_17_worker_overlay_ignores_an_empty_parent_python_path() {
        let tmp = tempdir().expect("tempdir");
        let worker = tmp.path().join("target/mutation-workers/run-1/worker-0");
        let src = worker.join("src");
        fs::create_dir_all(&src).expect("create worker src");
        let output = with_python_path(Some(""), || python_path_in(&worker));
        assert_eq!(output, format!("{}:{}", display(&src), display(&worker)));
    }

    // MU-15: a timed-out command is killed, reports 124, and returns before the sleep ends.
    #[test]
    fn mu_15_timeout_kills_the_process_group() {
        let dir = tempdir().expect("tempdir");
        let started = Instant::now();
        let result = CommandRunner::default().run(
            &shell("sleep 5"),
            dir.path(),
            Some(Duration::from_millis(200)),
        );
        // The output reader only sees end-of-file once every writer is gone, so an early
        // return proves the shell's `sleep` child died with the group.
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(result.seconds < 2.0);
        assert!(result.timed_out);
        assert_eq!(result.code, 124);
    }

    // MU-15: output written before the timeout is kept.
    #[test]
    fn mu_15_timeout_keeps_output_written_before_the_kill() {
        let dir = tempdir().expect("tempdir");
        let result = CommandRunner::default().run(
            &shell("echo started; sleep 5"),
            dir.path(),
            Some(Duration::from_millis(200)),
        );
        assert!(result.timed_out);
        assert_eq!(result.output, "started\n");
    }

    // MU-14: a spawn failure is never reported as a timeout, even when a limit is set.
    #[test]
    fn mu_14_spawn_failure_is_not_a_timeout() {
        let dir = tempdir().expect("tempdir");
        let result = CommandRunner::default().run(
            &argv(["mu-no-such-binary-xyz"]),
            dir.path(),
            Some(Duration::from_secs(5)),
        );
        assert_eq!(result.code, 127);
        assert!(!result.timed_out);
    }

    // MU-15: a timeout kills the whole process group, so a background grandchild dies too.
    #[test]
    fn mu_15_timeout_kills_background_grandchildren() {
        let dir = tempdir().expect("tempdir");
        let marker = dir.path().join("survivor");
        let script = format!(
            "(sleep 1; touch '{}') >/dev/null 2>&1 & sleep 30 >/dev/null 2>&1",
            display(&marker)
        );
        let result = CommandRunner::default().run(
            &shell(&script),
            dir.path(),
            Some(Duration::from_millis(200)),
        );
        assert!(result.timed_out);
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            !marker.exists(),
            "a background grandchild survived the timeout"
        );
    }

    // MU-14: display renders shell text as typed and quotes each argument part.
    #[test]
    fn mu_14_display_command_quotes_argv_parts_like_shlex() {
        assert_eq!(display_command(&shell("go test ./...")), "go test ./...");
        assert_eq!(
            display_command(&argv(["go", "test", "./my dir"])),
            "go test './my dir'"
        );
        assert_eq!(
            display_command(&argv(["echo", "it's"])),
            r#"echo 'it'"'"'s'"#
        );
        assert_eq!(display_command(&argv(["echo", ""])), "echo ''");
    }

    // MU-14: a shell string and an argument vector never share a cache key.
    #[test]
    fn mu_14_command_key_separates_shell_and_argv() {
        assert_eq!(command_key(&argv(["a", "b"])), "argv\0a\0b");
        assert_eq!(command_key(&shell("a b")), "sh\0a b");
        assert_ne!(command_key(&shell("a b")), command_key(&argv(["a", "b"])));
    }

    // MU-14: nearest walks up from the source to the marker and stops at the root.
    #[test]
    fn mu_14_nearest_walks_up_to_the_marker() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        let nested = root.join("src/demo");
        fs::create_dir_all(&nested).expect("create nested");
        write_file(&root.join("pyproject.toml"), "");
        let source = nested.join("app.py");
        write_file(&source, "x = 1\n");
        let root_resolved = root.canonicalize().expect("canonical root");
        assert_eq!(
            nearest(&source, "pyproject.toml", &root),
            Some(root_resolved.clone())
        );
        assert_eq!(
            nearest(&nested, "pyproject.toml", &root),
            Some(root_resolved)
        );
        assert_eq!(nearest(&source, "missing.toml", &root), None);
    }

    // MU-14: clojure picks babashka or clojure aliases from the project files.
    #[test]
    fn mu_14_clojure_commands_follow_the_project_files() {
        let tmp = tempdir().expect("tempdir");
        let bb = tmp.path().join("bb-spec");
        write_file(&bb.join("bb.edn"), r#"{:tasks {spec "spec"}}"#);
        assert_eq!(
            clojure_command(&bb),
            argv(["bb", "spec", "--tag", "~no-mutate"])
        );

        let bb_test = tmp.path().join("bb-test");
        write_file(
            &bb_test.join("bb.edn"),
            r#"{:tasks {test (clojure "-M:test")}}"#,
        );
        assert_eq!(clojure_command(&bb_test), argv(["bb", "test"]));

        let mixed = tmp.path().join("mixed");
        write_file(&mixed.join("bb.edn"), r#"{:tasks {spec "spec"}}"#);
        write_file(&mixed.join("deps.edn"), "{:deps {}}");
        assert_eq!(clojure_command(&mixed), argv(["clj", "-M:test"]));

        let spec = tmp.path().join("spec");
        write_file(
            &spec.join("deps.edn"),
            "{:aliases {:spec {:extra-deps {speclj/speclj {}}}}}",
        );
        assert_eq!(
            clojure_command(&spec),
            argv(["clj", "-M:spec", "--tag", "~no-mutate"])
        );

        let spec_only = tmp.path().join("spec-only");
        write_file(&spec_only.join("deps.edn"), "{:aliases {:spec {}}}");
        assert_eq!(clojure_command(&spec_only), argv(["clj", "-M:spec"]));

        let plain = tmp.path().join("plain");
        write_file(&plain.join("deps.edn"), "{:deps {}}");
        assert_eq!(clojure_command(&plain), argv(["clj", "-M:test"]));
    }

    // MU-14: a project virtualenv is used by absolute path, and .venv wins over venv.
    #[test]
    fn mu_14_python_command_uses_an_absolute_project_interpreter() {
        let tmp = tempdir().expect("tempdir");
        let project = tmp.path().join("proj");
        let dot = project.join(".venv/bin/python");
        write_executable(&dot);
        write_file(&project.join("pytest.ini"), "[pytest]\n");
        assert_eq!(
            python_command(&project),
            argv([display(&dot).as_str(), "-m", "pytest"])
        );

        let other = tmp.path().join("other");
        let plain = other.join("venv/bin/python");
        write_executable(&plain);
        assert_eq!(
            python_command(&other),
            argv([display(&plain).as_str(), "-m", "unittest", "discover"])
        );

        let dot_other = other.join(".venv/bin/python");
        write_executable(&dot_other);
        assert_eq!(
            python_command(&other),
            argv([display(&dot_other).as_str(), "-m", "unittest", "discover"])
        );
    }

    // MU-14: a virtualenv symlink is kept, not resolved to its target.
    #[test]
    fn mu_14_python_command_keeps_the_virtualenv_symlink() {
        let tmp = tempdir().expect("tempdir");
        let project = tmp.path().join("proj");
        let target = project.join("real-python");
        write_executable(&target);
        let link = project.join(".venv/bin/python");
        fs::create_dir_all(link.parent().expect("link parent")).expect("create venv bin");
        symlink(&target, &link).expect("symlink venv python");
        write_file(&project.join("pytest.ini"), "[pytest]\n");
        assert_eq!(
            python_command(&project),
            argv([display(&link).as_str(), "-m", "pytest"])
        );
    }

    // MU-14: a virtualenv that cannot run, or a missing project, falls back to python3.
    #[test]
    fn mu_14_python_command_falls_back_when_the_virtualenv_cannot_run() {
        let tmp = tempdir().expect("tempdir");
        let project = tmp.path().join("proj");
        write_file(&project.join(".venv/bin/python"), "");
        assert_eq!(
            python_command(&project),
            argv(["python3", "-m", "unittest", "discover"])
        );

        let missing = tmp.path().join("missing");
        fs::create_dir_all(&missing).expect("create missing project");
        assert_eq!(
            python_command(&missing),
            argv(["python3", "-m", "unittest", "discover"])
        );
    }

    // MU-14: pytest is chosen by pytest.ini, conftest.py, setup.cfg, or pyproject mention.
    #[test]
    fn mu_14_python_commands_pick_pytest_or_unittest() {
        let tmp = tempdir().expect("tempdir");
        let pytest_ini = tmp.path().join("py");
        write_file(&pytest_ini.join("pytest.ini"), "[pytest]\n");
        assert_eq!(
            python_command(&pytest_ini),
            argv(["python3", "-m", "pytest"])
        );

        let unit = tmp.path().join("unit");
        fs::create_dir_all(&unit).expect("create unit project");
        assert_eq!(
            python_command(&unit),
            argv(["python3", "-m", "unittest", "discover"])
        );

        let cfg = tmp.path().join("cfg");
        write_file(&cfg.join("setup.cfg"), "[tool:pytest]\n");
        assert_eq!(python_command(&cfg), argv(["python3", "-m", "pytest"]));

        let project = tmp.path().join("proj");
        write_file(
            &project.join("pyproject.toml"),
            "[project]\ndependencies=['pytest']\n",
        );
        assert_eq!(python_command(&project), argv(["python3", "-m", "pytest"]));

        let plain_pyproject = tmp.path().join("plain-pyproject");
        write_file(&plain_pyproject.join("pyproject.toml"), "[project]\n");
        assert_eq!(
            python_command(&plain_pyproject),
            argv(["python3", "-m", "unittest", "discover"])
        );

        let conf = tmp.path().join("conf");
        write_file(&conf.join("conftest.py"), "");
        assert_eq!(python_command(&conf), argv(["python3", "-m", "pytest"]));
    }

    // MU-14: a go package path with metacharacters or spaces reaches go as one argument.
    #[test]
    fn mu_14_go_package_path_is_one_argument() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        let package = root.join("a;echo no");
        write_file(&root.join("go.mod"), "module example.com/demo\n");
        let source = package.join("widget.go");
        write_file(&source, "package main\nfunc Run() int { return 1 }\n");
        let root_resolved = root.canonicalize().expect("canonical root");

        let (command, directory) = test_plan(&root, &source, Language::Go, None);
        assert_eq!(command, argv(["go", "test", "-count=1", "./a;echo no"]));
        assert_eq!(directory, root_resolved);
        let Command::Argv(parts) = &command else {
            panic!("go plan must be an argument vector");
        };
        let echoed = run_in(&argv(["/bin/echo", parts[3].as_str()]), &directory);
        assert_eq!(echoed.code, 0);
        assert_eq!(echoed.output, "./a;echo no\n");

        let spaced = root.join("my dir/widget.go");
        write_file(&spaced, "package main\nfunc Run() int { return 1 }\n");
        let (spaced_command, _) = test_plan(&root, &spaced, Language::Go, None);
        assert_eq!(spaced_command, argv(["go", "test", "-count=1", "./my dir"]));
    }

    // MU-14: the package of a source at the module root is "."
    #[test]
    fn mu_14_go_package_at_module_root_is_dot() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        write_file(&root.join("go.mod"), "module example.com/demo\n");
        let source = root.join("widget.go");
        write_file(&source, "package main\n");
        let (command, _) = test_plan(&root, &source, Language::Go, None);
        assert_eq!(command, argv(["go", "test", "-count=1", "."]));
    }

    // MU-14: a non-empty override is a shell string; an empty one falls back to the plan.
    #[test]
    fn mu_14_test_plan_uses_the_override_as_a_shell_string() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        write_file(&root.join("go.mod"), "module example.com/demo\n");
        let source = root.join("widget.go");
        write_file(&source, "package main\n");
        let root_resolved = root.canonicalize().expect("canonical root");

        let (command, directory) = test_plan(&root, &source, Language::Go, Some("go test ./..."));
        assert_eq!(command, shell("go test ./..."));
        assert_eq!(directory, root_resolved);

        let (empty, _) = test_plan(&root, &source, Language::Go, Some(""));
        assert_eq!(empty, argv(["go", "test", "-count=1", "."]));
    }

    // MU-14: clojure runs in the directory of its deps.edn.
    #[test]
    fn mu_14_test_plan_clojure_runs_in_the_deps_directory() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        write_file(&root.join("deps.edn"), "{:deps {}}");
        let source = root.join("src/core.clj");
        write_file(&source, "(ns core)\n");
        let (command, directory) = test_plan(&root, &source, Language::Clojure, None);
        assert_eq!(command, argv(["clj", "-M:test"]));
        assert_eq!(directory, root.canonicalize().expect("canonical root"));
    }

    // MU-14: java runs mvn in the directory of the nearest pom.xml.
    #[test]
    fn mu_14_test_plan_java_runs_mvn_in_the_pom_directory() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        let module = root.join("module");
        write_file(&module.join("pom.xml"), "<project/>");
        let source = module.join("src/A.java");
        write_file(&source, "class A {}\n");
        let (command, directory) = test_plan(&root, &source, Language::Java, None);
        assert_eq!(
            command,
            argv(["mvn", "-q", "test", "-DexcludeTags=no-mutate"])
        );
        assert_eq!(directory, module.canonicalize().expect("canonical module"));
    }

    // MU-14: typescript runs npm test in the nearest package.json directory.
    #[test]
    fn mu_14_test_plan_typescript_runs_npm_test() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        write_file(&root.join("package.json"), "{}");
        let source = root.join("src/index.ts");
        write_file(&source, "export {};\n");
        let (command, directory) = test_plan(&root, &source, Language::TypeScript, None);
        assert_eq!(command, argv(["npm", "test"]));
        assert_eq!(directory, root.canonicalize().expect("canonical root"));
    }

    // MU-14: rust runs cargo test in the nearest Cargo.toml directory.
    #[test]
    fn mu_14_test_plan_rust_runs_cargo_test() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        write_file(&root.join("Cargo.toml"), "[package]\n");
        let source = root.join("src/lib.rs");
        write_file(&source, "\n");
        let (command, directory) = test_plan(&root, &source, Language::Rust, None);
        assert_eq!(command, argv(["cargo", "test"]));
        assert_eq!(directory, root.canonicalize().expect("canonical root"));
    }

    // MU-14: python uses the project's .venv interpreter when pyproject mentions pytest.
    #[test]
    fn mu_14_test_plan_python_uses_the_project_venv() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("proj");
        write_file(
            &root.join("pyproject.toml"),
            "[project]\ndependencies=['pytest']\n",
        );
        write_executable(&root.join(".venv/bin/python"));
        let source = root.join("src/app.py");
        write_file(&source, "x = 1\n");
        let root_resolved = root.canonicalize().expect("canonical root");
        let venv = display(&root_resolved.join(".venv/bin/python"));
        let (command, directory) = test_plan(&root, &source, Language::Python, None);
        assert_eq!(command, argv([venv.as_str(), "-m", "pytest"]));
        assert_eq!(directory, root_resolved);
    }
}
