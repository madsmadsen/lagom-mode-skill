//! Port of mutator/src/mutator/engine.py (BSD Zero Clause License, Robert C. Martin).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::bind::{self, Span};
use crate::discover;
use crate::metrics;
use crate::model::{FormResult, History, Language, LineSet, RunResult, Site, Status};
use crate::report;
use crate::runner::{self, Command, CommandRunner};
use crate::workers;

/// Baseline cache: (command key, cwd) -> (passed, seconds, failure tail).
pub type Baselines = HashMap<(String, String), (bool, f64, String)>;

type FormKey = (String, String);
type Digests = BTreeMap<FormKey, String>;
type Outcomes = BTreeMap<String, Status>;
type Coverage = HashMap<String, bool>;

const MIN_TIMEOUT_SECS: f64 = 2.0;
const MAX_TIMEOUT_SECS: f64 = u32::MAX as f64;
const TAIL_LINES: usize = 20;

pub struct MutateOptions<'a> {
    pub covered_lines: Option<&'a LineSet>,
    pub ignore_coverage: bool,
    pub mutate_all: bool,
    pub lines: Option<&'a LineSet>,
    pub test_command: Option<&'a str>,
    pub timeout_factor: f64,
    pub mutation_warning: usize,
    pub max_workers: Option<usize>,
}

struct Target<'a> {
    path: &'a Path,
    root: &'a Path,
    source: &'a str,
    file_key: &'a str,
    language: Language,
}

#[derive(Default)]
struct Tally {
    killed: usize,
    survived: usize,
    uncovered: usize,
}

/// Restore sources left mutated by an interrupted run; returns the restored paths.
pub fn restore_backups(root: &Path) -> Vec<PathBuf> {
    let base = root.join("target").join("mutator-backup");
    if !base.is_dir() {
        return Vec::new();
    }
    let mut backups = files_under(&base);
    backups.sort();
    backups
        .iter()
        .filter_map(|backup| restore_one(&base, root, backup))
        .collect()
}

/// Mutate one source file and record its results; returns what the caller should report.
pub fn mutate_file(
    path: &Path,
    root: &Path,
    runner: &CommandRunner,
    opts: &MutateOptions,
    baselines: &mut Baselines,
) -> RunResult {
    let root = resolve(root);
    let path = resolve(path);
    let file_key = metrics::source_key(&path, &root);
    let Some(language) = discover::language_of(&path) else {
        return skipped(file_key, "unsupported file");
    };
    let source = match read_source(&path) {
        Ok(text) => text,
        Err(reason) => return skipped(file_key, &reason),
    };
    let target = Target {
        path: &path,
        root: &root,
        source: &source,
        file_key: &file_key,
        language,
    };
    let found = bind::sites_in_file(&source, &path, &root, &file_key);
    let covered = coverage_map(&found, opts.covered_lines, opts.ignore_coverage);
    warn_uncovered(&file_key, &found, opts);
    let history = metrics::load_history(&root, &file_key);
    let digests = bind::form_digests(&source, &path, &root);
    let selected = select(&found, &covered, &history, &digests, opts);
    let mut outcomes = carry_forward(&found, &covered, &history, &digests);
    warn_many(&file_key, selected.len(), opts.mutation_warning);
    if let Some(failure) =
        apply_selected(&target, &selected, runner, opts, baselines, &mut outcomes)
    {
        return failure;
    }
    let forms = form_results(&target, &digests, &found, &covered, &outcomes, opts.lines);
    let written = metrics::write_results(&root, &file_key, &forms, &outcomes);
    restore_if_dirty(&path, source.as_bytes());
    RunResult {
        statuses: statuses(&found, &covered, &outcomes),
        written: written
            .iter()
            .map(|item| item.to_string_lossy().into_owned())
            .collect(),
        sites: found,
        forms,
        path: file_key,
        ..RunResult::default()
    }
}

/// Scan one file and return the site inventory text.
pub fn scan_file(
    path: &Path,
    root: &Path,
    covered_lines: Option<&LineSet>,
    ignore_coverage: bool,
    lines: Option<&LineSet>,
) -> String {
    let root = resolve(root);
    let path = resolve(path);
    let file_key = metrics::source_key(&path, &root);
    let source = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => return format!("Scan skipped {file_key}: {err}\n"),
    };
    let mut found = bind::sites_in_file(&source, &path, &root, &file_key);
    if let Some(set) = lines {
        found.retain(|site| set.contains(&site.line));
    }
    let covered = (!ignore_coverage && covered_lines.is_some())
        .then(|| coverage_map(&found, covered_lines, false));
    let history = metrics::load_history(&root, &file_key);
    let digests = bind::form_digests(&source, &path, &root);
    let changed = changed_forms(&history, &digests);
    report::format_scan(&file_key, &found, &changed, covered.as_ref())
}

fn skipped(file_key: String, reason: &str) -> RunResult {
    RunResult {
        path: file_key,
        skipped: reason.to_owned(),
        ..RunResult::default()
    }
}

fn resolve(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn read_source(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| format!("cannot read: {err}"))?;
    String::from_utf8(bytes).map_err(|_| "file is not UTF-8".to_owned())
}

fn form_key(site: &Site) -> FormKey {
    (site.namespace.clone(), site.form_id.clone())
}

fn site_covered(site: &Site, lines: Option<&LineSet>, ignore_coverage: bool) -> bool {
    ignore_coverage
        || lines.is_some_and(|set| set.contains(&site.line) || set.contains(&site.anchor_line))
}

fn covered_at(covered: &Coverage, site: &Site) -> bool {
    covered.get(&site.mutation_id).copied().unwrap_or(false)
}

fn coverage_map(found: &[Site], lines: Option<&LineSet>, ignore_coverage: bool) -> Coverage {
    found
        .iter()
        .map(|site| {
            (
                site.mutation_id.clone(),
                site_covered(site, lines, ignore_coverage),
            )
        })
        .collect()
}

fn warn_uncovered(file_key: &str, found: &[Site], opts: &MutateOptions) {
    if opts.covered_lines.is_some() || opts.ignore_coverage || found.is_empty() {
        return;
    }
    eprintln!(
        "No coverage data for {file_key}; its sites are uncovered. Pass --no-coverage to run them anyway."
    );
}

fn warn_many(file_key: &str, selected: usize, mutation_warning: usize) {
    if selected > mutation_warning {
        eprintln!("WARNING: {selected} covered mutations selected in {file_key}.");
    }
}

/// True when the form's digest matches the snapshot, so its sites are unchanged.
fn unchanged(site: &Site, history: &History, digests: &Digests) -> bool {
    let key = form_key(site);
    history
        .forms
        .get(&key)
        .is_some_and(|prior| digests.get(&key) == Some(&prior.digest))
}

fn needs_run(site: &Site, history: &History, digests: &Digests, mutate_all: bool) -> bool {
    if mutate_all || !unchanged(site, history, digests) {
        return true;
    }
    history.outcomes.get(&site.mutation_id) != Some(&Status::Killed)
}

fn select(
    found: &[Site],
    covered: &Coverage,
    history: &History,
    digests: &Digests,
    opts: &MutateOptions,
) -> Vec<Site> {
    found
        .iter()
        .filter(|site| covered_at(covered, site))
        .filter(|site| opts.lines.is_none_or(|lines| lines.contains(&site.line)))
        .filter(|site| needs_run(site, history, digests, opts.mutate_all))
        .cloned()
        .collect()
}

fn carry_forward(
    found: &[Site],
    covered: &Coverage,
    history: &History,
    digests: &Digests,
) -> Outcomes {
    let by_id: HashMap<&str, &Site> = found
        .iter()
        .map(|site| (site.mutation_id.as_str(), site))
        .collect();
    history
        .outcomes
        .iter()
        .filter(|(mutation, _)| {
            by_id
                .get(mutation.as_str())
                .is_some_and(|site| covered_at(covered, site) && unchanged(site, history, digests))
        })
        .map(|(mutation, status)| (mutation.clone(), *status))
        .collect()
}

fn changed_forms(history: &History, digests: &Digests) -> HashSet<FormKey> {
    if history.forms.is_empty() {
        return HashSet::new();
    }
    digests
        .iter()
        .filter(|(key, digest)| {
            history
                .forms
                .get(*key)
                .is_none_or(|prior| prior.digest != **digest)
        })
        .map(|(key, _)| key.clone())
        .collect()
}

fn timeout_for(seconds: f64, factor: f64) -> Duration {
    let raw = seconds * factor;
    let secs = if raw.is_nan() {
        MIN_TIMEOUT_SECS
    } else {
        raw.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
    };
    Duration::from_secs_f64(secs)
}

fn tail_of(output: &str) -> String {
    let lines = bind::splitlines(output);
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

fn measure_baseline(runner: &CommandRunner, command: &Command, cwd: &Path) -> (bool, f64, String) {
    let result = runner.run(command, cwd, None);
    if result.code == 0 {
        (true, result.seconds, String::new())
    } else {
        (false, result.seconds, tail_of(&result.output))
    }
}

fn baseline_failure(file_key: &str, command: &Command, tail: &str) -> RunResult {
    let mut message = format!(
        "Baseline failed for {file_key}: {}",
        runner::display_command(command)
    );
    if !tail.is_empty() {
        message.push('\n');
        message.push_str(tail);
    }
    RunResult {
        path: file_key.to_owned(),
        baseline_failed: true,
        baseline_message: message,
        ..RunResult::default()
    }
}

/// Run the selected mutants against the cached baseline; a failed baseline aborts the file.
fn apply_selected(
    target: &Target,
    selected: &[Site],
    runner: &CommandRunner,
    opts: &MutateOptions,
    baselines: &mut Baselines,
    outcomes: &mut Outcomes,
) -> Option<RunResult> {
    if selected.is_empty() {
        return None;
    }
    let (command, cwd) =
        runner::test_plan(target.root, target.path, target.language, opts.test_command);
    let key = (
        runner::command_key(&command),
        cwd.to_string_lossy().into_owned(),
    );
    let (passed, seconds, tail) = baselines
        .entry(key)
        .or_insert_with(|| measure_baseline(runner, &command, &cwd));
    if !*passed {
        return Some(baseline_failure(target.file_key, &command, tail));
    }
    let timeout = timeout_for(*seconds, opts.timeout_factor);
    workers::run_mutants(
        target.root,
        target.path,
        target.source.as_bytes(),
        selected,
        opts.max_workers,
        runner,
        &command,
        &cwd,
        timeout,
        target.file_key,
        outcomes,
    );
    None
}

fn group_by_form(sites: &[Site]) -> HashMap<FormKey, Vec<&Site>> {
    let mut grouped: HashMap<FormKey, Vec<&Site>> = HashMap::new();
    for site in sites {
        grouped.entry(form_key(site)).or_default().push(site);
    }
    grouped
}

fn tally(owned: &[&Site], covered: &Coverage, outcomes: &Outcomes, ran_all: bool) -> Tally {
    let mut tally = Tally::default();
    for site in owned {
        if !covered_at(covered, site) {
            tally.uncovered += 1;
            continue;
        }
        match outcomes.get(&site.mutation_id) {
            Some(Status::Killed) => tally.killed += 1,
            Some(Status::Survived) => tally.survived += 1,
            None if ran_all => tally.survived += 1,
            None => {}
        }
    }
    tally
}

fn form_result(
    target: &Target,
    digests: &Digests,
    key: FormKey,
    span: Span,
    sites: usize,
    tally: Tally,
) -> FormResult {
    let digest = digests.get(&key).cloned().unwrap_or_default();
    FormResult {
        namespace: key.0,
        id: key.1,
        name: span.name,
        private: span.private,
        file: target.file_key.to_owned(),
        line: span.start,
        end_line: span.end,
        digest,
        killed: tally.killed,
        survived: tally.survived,
        uncovered: tally.uncovered,
        sites,
    }
}

fn form_results(
    target: &Target,
    digests: &Digests,
    sites: &[Site],
    covered: &Coverage,
    outcomes: &Outcomes,
    lines: Option<&LineSet>,
) -> Vec<FormResult> {
    let mut spans = bind::form_spans(target.source, target.path, target.root);
    spans.sort_by_key(|(_, span)| span.start);
    let grouped = group_by_form(sites);
    spans
        .into_iter()
        .map(|(key, span)| {
            let owned: &[&Site] = match grouped.get(&key) {
                Some(list) => list,
                None => &[],
            };
            let counts = tally(owned, covered, outcomes, lines.is_none());
            form_result(target, digests, key, span, owned.len(), counts)
        })
        .collect()
}

fn status_label(site: &Site, covered: &Coverage, outcomes: &Outcomes) -> Option<&'static str> {
    if !covered_at(covered, site) {
        return Some("uncovered");
    }
    outcomes
        .get(&site.mutation_id)
        .map(|status| status.as_str())
}

fn statuses(found: &[Site], covered: &Coverage, outcomes: &Outcomes) -> BTreeMap<String, String> {
    found
        .iter()
        .filter_map(|site| {
            status_label(site, covered, outcomes)
                .map(|label| (site.mutation_id.clone(), label.to_owned()))
        })
        .collect()
}

fn restore_if_dirty(path: &Path, original: &[u8]) {
    if fs::read(path).is_ok_and(|current| current == original) {
        return;
    }
    if let Err(err) = fs::write(path, original) {
        eprintln!("mutator: could not restore {}: {err}", path.display());
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
            } else if path.is_file() {
                found.push(path);
            }
        }
    }
    found
}

fn restore_one(base: &Path, root: &Path, backup: &Path) -> Option<PathBuf> {
    let relative = backup.strip_prefix(base).ok()?;
    let target = root.join(relative);
    match write_restored(backup, &target) {
        Ok(()) => Some(target),
        Err(err) => {
            eprintln!("mutator: could not restore {}: {err}", target.display());
            None
        }
    }
}

fn write_restored(backup: &Path, target: &Path) -> io::Result<()> {
    let bytes = fs::read(backup)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, bytes)?;
    fs::remove_file(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    const SOURCE: &str = "def add(a, b):\n    if a > 0:\n        return a + b\n    return a + b\n";
    const CHECK: &str = "grep -q 'a > 0' src/demo.py";

    fn options(command: &str, mutate_all: bool) -> MutateOptions<'_> {
        MutateOptions {
            covered_lines: None,
            ignore_coverage: true,
            mutate_all,
            lines: None,
            test_command: Some(command),
            timeout_factor: 10.0,
            mutation_warning: 50,
            max_workers: None,
        }
    }

    fn project(source: &str) -> (TempDir, PathBuf) {
        let root = tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("src")).expect("mkdir");
        let path = root.path().join("src").join("demo.py");
        fs::write(&path, source).expect("write source");
        (root, path)
    }

    fn call_log() -> (TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let log = dir.path().join("calls.log");
        (dir, log)
    }

    fn counted_command(log: &Path, check: &str) -> String {
        format!("echo call >> '{}'; {check}", log.display())
    }

    fn calls(log: &Path) -> usize {
        fs::read_to_string(log).map_or(0, |text| text.lines().count())
    }

    fn run(path: &Path, root: &Path, opts: &MutateOptions) -> RunResult {
        mutate_file(
            path,
            root,
            &CommandRunner::default(),
            opts,
            &mut Baselines::new(),
        )
    }

    // MU-19: mutate_all runs the baseline plus every site, and writes a snapshot.
    #[test]
    fn mu_19_mutate_all_runs_every_site_and_writes_snapshot() {
        let (root, path) = project(SOURCE);
        let (_dir, log) = call_log();
        let command = counted_command(&log, CHECK);
        let result = run(&path, root.path(), &options(&command, true));
        let form = &result.forms[0];
        assert_eq!(form.id, "defn/add");
        assert_eq!(form.namespace, "demo");
        assert_eq!((form.killed, form.survived, form.sites), (2, 2, 4));
        assert_eq!(calls(&log), 1 + form.sites);
        assert_eq!(fs::read_to_string(&path).expect("read"), SOURCE);
        let snapshot = metrics::snapshot_path(root.path(), "demo").expect("snapshot path");
        assert!(snapshot.is_file());
    }

    // MU-19: an unchanged rerun reruns only the survivors; killed sites carry forward.
    #[test]
    fn mu_19_unchanged_rerun_runs_only_the_survivors() {
        let (root, path) = project(SOURCE);
        let (_dir, log) = call_log();
        let command = counted_command(&log, CHECK);
        run(&path, root.path(), &options(&command, true));
        let before = calls(&log);
        let second = run(&path, root.path(), &options(&command, false));
        // one baseline plus the two surviving sites
        assert_eq!(calls(&log) - before, 1 + 2);
        let form = &second.forms[0];
        assert_eq!((form.killed, form.survived), (2, 2));
    }

    // MU-19: a changed function reruns all of its sites.
    #[test]
    fn mu_19_changed_function_reruns_all_its_sites() {
        let (root, path) = project(SOURCE);
        let (_dir, log) = call_log();
        let command = counted_command(&log, CHECK);
        run(&path, root.path(), &options(&command, true));
        let changed = SOURCE.replace("if a > 0:", "if a > 0:  # positive");
        fs::write(&path, &changed).expect("write changed");
        let before = calls(&log);
        run(&path, root.path(), &options(&command, false));
        // one baseline plus all four sites
        assert_eq!(calls(&log) - before, 1 + 4);
    }

    // MU-14: a failing baseline writes no snapshot and leaves the source untouched.
    #[test]
    fn mu_14_failed_baseline_writes_no_snapshot() {
        let (root, path) = project(SOURCE);
        let result = run(&path, root.path(), &options("false", true));
        assert!(result.baseline_failed);
        let snapshot = metrics::snapshot_path(root.path(), "demo").expect("snapshot path");
        assert!(!snapshot.exists());
        assert_eq!(fs::read_to_string(&path).expect("read"), SOURCE);
    }

    // MU-14: a failing baseline keeps the bytes of an existing snapshot.
    #[test]
    fn mu_14_failed_baseline_keeps_existing_snapshot_bytes() {
        let (root, path) = project(SOURCE);
        run(&path, root.path(), &options("true", true));
        let snapshot = metrics::snapshot_path(root.path(), "demo").expect("snapshot path");
        let before = fs::read(&snapshot).expect("read snapshot");
        let result = run(&path, root.path(), &options("false", true));
        assert!(result.baseline_failed);
        assert_eq!(fs::read(&snapshot).expect("read snapshot"), before);
    }

    // MU-20: a failing baseline is flagged so the CLI can exit 2, with no forms reported.
    #[test]
    fn mu_20_failed_baseline_is_flagged_for_exit_two() {
        let (root, path) = project(SOURCE);
        let result = run(&path, root.path(), &options("false", true));
        assert!(result.baseline_failed);
        assert!(result.baseline_message.starts_with("Baseline failed for "));
        assert!(result.baseline_message.contains("src/demo.py"));
        assert!(result.forms.is_empty());
    }

    // MU-16: uncovered sites are counted and never executed.
    #[test]
    fn mu_16_uncovered_sites_are_counted_and_never_run() {
        let (root, path) = project(SOURCE);
        let (_dir, log) = call_log();
        let command = counted_command(&log, CHECK);
        let empty = LineSet::new();
        let opts = MutateOptions {
            covered_lines: Some(&empty),
            ignore_coverage: false,
            ..options(&command, true)
        };
        let result = run(&path, root.path(), &opts);
        assert_eq!(calls(&log), 0);
        let form = &result.forms[0];
        assert_eq!(
            (form.uncovered, form.killed, form.survived, form.sites),
            (4, 0, 0, 4)
        );
        assert_eq!(result.statuses.len(), 4);
        assert!(result.statuses.values().all(|label| label == "uncovered"));
    }

    // MU-16: a `+` on a continuation line is covered through its statement's anchor line.
    #[test]
    fn mu_16_site_inside_covered_multi_line_statement_is_run() {
        let source = "def add(a, b):\n    return (a\n            + b)\n";
        let (root, path) = project(source);
        let (_dir, log) = call_log();
        let command = counted_command(&log, "grep -q '+ b' src/demo.py");
        let lines = LineSet::from([2]);
        let opts = MutateOptions {
            covered_lines: Some(&lines),
            ignore_coverage: false,
            ..options(&command, true)
        };
        let result = run(&path, root.path(), &opts);
        let form = &result.forms[0];
        assert_eq!(form.uncovered, 0);
        assert_eq!(form.killed, 1);
    }

    // MU-15: the timeout is at least two seconds, scales with the baseline, and saturates.
    #[test]
    fn mu_15_timeout_has_a_two_second_floor_and_saturates() {
        assert_eq!(timeout_for(0.01, 10.0), Duration::from_secs(2));
        assert_eq!(timeout_for(0.5, 10.0), Duration::from_secs(5));
        assert_eq!(timeout_for(0.5, f64::NAN), Duration::from_secs(2));
        assert_eq!(
            timeout_for(f64::INFINITY, 10.0),
            Duration::from_secs(u64::from(u32::MAX))
        );
    }

    // MU-18: an interrupted mutant is restored from its backup and the backup removed.
    #[test]
    fn mu_18_restore_backups_puts_an_interrupted_mutant_back() {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("src").join("demo.py");
        let backup = root.path().join("target/mutator-backup/src/demo.py");
        fs::create_dir_all(source.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(backup.parent().expect("parent")).expect("mkdir");
        fs::write(&source, "mutated\n").expect("write source");
        fs::write(&backup, "original\n").expect("write backup");
        let restored = restore_backups(root.path());
        assert_eq!(restored, vec![source.clone()]);
        assert_eq!(fs::read_to_string(&source).expect("read"), "original\n");
        assert!(!backup.exists());
    }

    // MU-18: missing directories are recreated and restores run in sorted path order.
    #[test]
    fn mu_18_restore_backups_recreates_directories_in_sorted_order() {
        let root = tempdir().expect("tempdir");
        let base = root.path().join("target/mutator-backup");
        fs::create_dir_all(base.join("src/b")).expect("mkdir");
        fs::write(base.join("src/b/two.py"), "two\n").expect("write");
        fs::write(base.join("src/a.py"), "one\n").expect("write");
        let restored = restore_backups(root.path());
        let src = root.path().join("src");
        assert_eq!(restored, vec![src.join("a.py"), src.join("b/two.py")]);
        assert_eq!(
            fs::read_to_string(src.join("b/two.py")).expect("read"),
            "two\n"
        );
        assert!(!base.join("src/a.py").exists());
    }

    // MU-18: without a backup directory nothing is restored.
    #[test]
    fn mu_18_restore_backups_without_backup_dir_restores_nothing() {
        let root = tempdir().expect("tempdir");
        assert!(restore_backups(root.path()).is_empty());
    }

    // MU-19: without a snapshot, scan marks nothing as changed and prints no footer.
    #[test]
    fn mu_19_scan_without_snapshot_marks_nothing() {
        let (root, path) = project(SOURCE);
        let out = scan_file(&path, root.path(), None, true, None);
        assert!(out.starts_with("Scan: 4 mutation sites in "), "{out}");
        assert!(!out.lines().any(|line| line.starts_with('*')), "{out}");
        assert!(!out.contains("marks a function"), "{out}");
    }

    // MU-19: with a snapshot, only the function whose text changed is starred.
    #[test]
    fn mu_19_scan_stars_only_the_changed_function() {
        let source = format!("{SOURCE}\ndef sub(a, b):\n    return a - b\n");
        let (root, path) = project(&source);
        run(&path, root.path(), &options("true", true));
        fs::write(&path, source.replace("return a - b", "return a * b")).expect("write");
        let out = scan_file(&path, root.path(), None, true, None);
        let starred: Vec<&str> = out
            .lines()
            .filter(|line| line.starts_with('*') && line.contains("[defn"))
            .collect();
        assert!(!starred.is_empty(), "{out}");
        assert!(
            starred.iter().all(|line| line.contains("[defn/sub]")),
            "{out}"
        );
        assert!(
            out.lines()
                .any(|line| line.starts_with(' ') && line.contains("[defn/add]")),
            "{out}"
        );
        assert!(out.ends_with("* marks a function whose text changed since the last snapshot.\n"));
    }
}
