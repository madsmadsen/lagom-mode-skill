//! Port of mutator/src/mutator/metrics.py (BSD Zero Clause License, Robert C. Martin).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::bind::{mutation_file, mutation_namespace};
use crate::edn::{Value, parse, render};
use crate::model::{FormResult, History, PriorForm, Status};

type Outcomes = BTreeMap<String, Status>;

const DEDUPE_FIELDS: [&str; 4] = ["file", "id", "line", "hash"];

/// Forms and outcomes a namespace keeps from snapshots of other files.
#[derive(Default)]
struct Retained {
    forms: Vec<Value>,
    outcomes: Outcomes,
}

/// Source path relative to `root`, joined with `/`.
pub fn source_key(path: &Path, root: &Path) -> String {
    let path = resolve(path);
    let root = resolve(root);
    let relative = path.strip_prefix(&root).unwrap_or(path.as_path());
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Snapshot file for `namespace`; Err when a part is empty, `.` or `..`.
pub fn snapshot_path(root: &Path, namespace: &str) -> Result<PathBuf, String> {
    let unsafe_namespace = || format!("unsafe namespace {namespace:?}");
    let normalized = namespace.replace('\\', "/").replace("::", "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.iter().any(|part| matches!(*part, "" | "." | "..")) {
        return Err(unsafe_namespace());
    }
    let (last, dirs) = parts.split_last().ok_or_else(unsafe_namespace)?;
    let mut path = root.join(".metrics").join("mutate");
    path.extend(dirs);
    path.push(format!("{last}.edn"));
    Ok(path)
}

/// Prior form digests and outcomes recorded for `file_key`.
pub fn load_history(root: &Path, file_key: &str) -> History {
    let mut history = History::default();
    for (_, data) in read_snapshots(root) {
        if let Some(namespace) = text_field(&data, "namespace") {
            remember(&mut history, &data, namespace, file_key);
        }
    }
    history
}

/// Replace this file's forms, preserving other files that share a namespace.
///
/// Returns the snapshot paths for this write. A snapshot whose content is
/// unchanged is left untouched and still reported.
pub fn write_results(
    root: &Path,
    file_key: &str,
    forms: &[FormResult],
    outcomes: &BTreeMap<String, Status>,
) -> Vec<PathBuf> {
    let root = resolve(root);
    let incoming = group_forms(forms);
    let incoming_outcomes = file_outcomes(outcomes, file_key);
    let existing = read_snapshots(&root);
    let (mut retained, previous) = retain(&existing, file_key);
    let touched: BTreeSet<String> = incoming
        .keys()
        .chain(&previous)
        .chain(retained.keys())
        .cloned()
        .collect();
    let mut written: Vec<PathBuf> = Vec::new();
    for namespace in &touched {
        let kept = retained.remove(namespace).unwrap_or_default();
        let fresh = incoming
            .get(namespace)
            .map(Vec::as_slice)
            .unwrap_or_default();
        written.extend(write_namespace(
            &root,
            namespace,
            kept,
            fresh,
            &incoming_outcomes,
            file_key,
        ));
    }
    let canonical: BTreeSet<PathBuf> = touched
        .iter()
        .filter_map(|namespace| snapshot_path(&root, namespace).ok())
        .map(|path| resolve(&path))
        .collect();
    drop_stale(&existing, &touched, &canonical, file_key);
    remove_empty_dirs(&root.join(".metrics").join("mutate"));
    written
}

/// Canonical path when it exists, otherwise the path as given.
fn resolve(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn text_field<'a>(form: &'a Value, key: &str) -> Option<&'a str> {
    form.get(key).and_then(Value::as_str)
}

fn int(n: usize) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn text(s: &str) -> Value {
    Value::Str(s.to_string())
}

fn pair(key: &str, value: Value) -> (Value, Value) {
    (Value::Keyword(key.to_string()), value)
}

fn edn_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_edn(&root.join(".metrics").join("mutate"), &mut found);
    found.sort();
    found
}

fn collect_edn(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_edn(&path, found);
        } else if path.is_file() && is_edn(&path) {
            found.push(path);
        }
    }
}

fn is_edn(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".edn"))
}

fn read_snapshot(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    parse(&text)
        .ok()
        .filter(|data| matches!(data, Value::Map(_)))
}

fn read_snapshots(root: &Path) -> Vec<(PathBuf, Value)> {
    edn_files(root)
        .into_iter()
        .filter_map(|path| read_snapshot(&path).map(|data| (path, data)))
        .collect()
}

fn forms_of(data: &Value) -> impl Iterator<Item = &Value> {
    data.get("forms")
        .and_then(Value::as_vec)
        .into_iter()
        .flatten()
        .filter(|form| matches!(form, Value::Map(_)))
}

fn status_of(value: &Value) -> Option<Status> {
    let text = value.as_str()?;
    Status::parse(text.strip_prefix(':').unwrap_or(text))
}

fn outcomes_of(data: &Value) -> Outcomes {
    let Some(Value::Map(entries)) = data.get("outcomes") else {
        return Outcomes::new();
    };
    entries
        .iter()
        .filter_map(|(key, value)| Some((key.as_str()?.to_string(), status_of(value)?)))
        .collect()
}

/// The file a form belongs to: its own `file`, else the snapshot's `source`.
fn form_file<'a>(form: &'a Value, data: &'a Value) -> Option<&'a str> {
    text_field(form, "file")
        .filter(|name| !name.is_empty())
        .or_else(|| text_field(data, "source"))
}

fn mentions_file(data: &Value, file_key: &str) -> bool {
    forms_of(data).any(|form| form_file(form, data) == Some(file_key))
}

fn remember(history: &mut History, data: &Value, namespace: &str, file_key: &str) {
    for form in forms_of(data) {
        remember_form(history, form, data, namespace, file_key);
    }
    for (mutation, status) in outcomes_of(data) {
        if mutation_file(&mutation).as_deref() == Some(file_key) {
            history.outcomes.insert(mutation, status);
        }
    }
}

fn remember_form(
    history: &mut History,
    form: &Value,
    data: &Value,
    namespace: &str,
    file_key: &str,
) {
    if form_file(form, data) != Some(file_key) {
        return;
    }
    let (Some(id), Some(digest)) = (text_field(form, "id"), text_field(form, "hash")) else {
        return;
    };
    let prior = PriorForm {
        namespace: namespace.to_string(),
        id: id.to_string(),
        digest: digest.to_string(),
        file: file_key.to_string(),
    };
    history
        .forms
        .insert((namespace.to_string(), id.to_string()), prior);
}

fn render_form(form: &FormResult) -> Value {
    let kind = if form.private { "defn-" } else { "defn" };
    Value::Map(vec![
        pair("id", text(&form.id)),
        pair("kind", text(kind)),
        pair("file", text(&form.file)),
        pair("line", int(form.line)),
        pair("end-line", int(form.end_line)),
        pair("hash", text(&form.digest)),
        pair("killed", int(form.killed)),
        pair("survived", int(form.survived)),
        pair("uncovered", int(form.uncovered)),
        pair("sites", int(form.sites)),
    ])
}

fn now_stamp() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

fn render_snapshot(namespace: &str, source: &str, forms: Vec<Value>, outcomes: &Outcomes) -> Value {
    let outcome_map = outcomes
        .iter()
        .map(|(id, status)| (text(id), Value::Keyword(status.as_str().to_string())))
        .collect();
    Value::Map(vec![
        pair("version", Value::Int(1)),
        pair("tested-at", text(&now_stamp())),
        pair("source", text(source)),
        pair("namespace", text(namespace)),
        pair("outcomes", Value::Map(outcome_map)),
        pair("forms", Value::Vector(forms)),
    ])
}

fn kept_forms(data: &Value, file_key: &str) -> Vec<Value> {
    forms_of(data)
        .filter(|form| form_file(form, data) != Some(file_key))
        .cloned()
        .collect()
}

fn kept_outcomes(data: &Value, file_key: &str) -> Outcomes {
    outcomes_of(data)
        .into_iter()
        .filter(|(mutation, _)| mutation_file(mutation).as_deref() != Some(file_key))
        .collect()
}

fn keep(retained: &mut BTreeMap<String, Retained>, namespace: &str, data: &Value, file_key: &str) {
    let forms = kept_forms(data, file_key);
    let outcomes = kept_outcomes(data, file_key);
    if forms.is_empty() && outcomes.is_empty() {
        return;
    }
    let entry = retained.entry(namespace.to_string()).or_default();
    entry.forms.extend(forms);
    entry.outcomes.extend(outcomes);
}

fn retain(
    existing: &[(PathBuf, Value)],
    file_key: &str,
) -> (BTreeMap<String, Retained>, BTreeSet<String>) {
    let mut retained: BTreeMap<String, Retained> = BTreeMap::new();
    let mut previous = BTreeSet::new();
    for (_, data) in existing {
        let Some(namespace) = text_field(data, "namespace") else {
            continue;
        };
        if mentions_file(data, file_key) {
            previous.insert(namespace.to_string());
        }
        keep(&mut retained, namespace, data, file_key);
    }
    (retained, previous)
}

fn group_forms(forms: &[FormResult]) -> BTreeMap<String, Vec<&FormResult>> {
    let mut incoming: BTreeMap<String, Vec<&FormResult>> = BTreeMap::new();
    for form in forms {
        incoming
            .entry(form.namespace.clone())
            .or_default()
            .push(form);
    }
    incoming
}

fn file_outcomes(outcomes: &Outcomes, file_key: &str) -> Outcomes {
    outcomes
        .iter()
        .filter(|(mutation, _)| mutation_file(mutation).as_deref() == Some(file_key))
        .map(|(mutation, status)| (mutation.clone(), *status))
        .collect()
}

fn outcomes_for(outcomes: &Outcomes, namespace: &str) -> Outcomes {
    outcomes
        .iter()
        .filter(|(mutation, _)| mutation_namespace(mutation).as_deref() == Some(namespace))
        .map(|(mutation, status)| (mutation.clone(), *status))
        .collect()
}

fn form_order(form: &Value) -> (String, i64, String) {
    let file = text_field(form, "file").unwrap_or("None");
    let line = form.get("line").and_then(Value::as_i64).unwrap_or(0);
    let id = text_field(form, "id").unwrap_or("None");
    (file.to_string(), line, id.to_string())
}

fn dedupe_key(form: &Value) -> String {
    let fields = DEDUPE_FIELDS
        .iter()
        .map(|field| form.get(field).cloned().unwrap_or(Value::Nil))
        .collect();
    render(&Value::Vector(fields))
}

fn dedupe(forms: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    forms
        .into_iter()
        .filter(|form| seen.insert(dedupe_key(form)))
        .collect()
}

fn rendered_forms(kept: Vec<Value>, fresh: &[&FormResult]) -> Vec<Value> {
    let fresh_forms = fresh.iter().copied().map(render_form);
    let mut forms = dedupe(kept.into_iter().chain(fresh_forms).collect());
    forms.sort_by_key(form_order);
    forms
}

fn snapshot_source(fresh: &[&FormResult], kept: &[Value], file_key: &str) -> String {
    if let Some(form) = fresh.first() {
        return form.file.clone();
    }
    kept.first()
        .and_then(|form| text_field(form, "file"))
        .filter(|name| !name.is_empty())
        .unwrap_or(file_key)
        .to_string()
}

fn merged_outcomes(
    mut kept: Outcomes,
    fresh: &[&FormResult],
    outcomes: &Outcomes,
    namespace: &str,
) -> Outcomes {
    if !fresh.is_empty() {
        kept.extend(outcomes_for(outcomes, namespace));
    }
    kept
}

fn write_namespace(
    root: &Path,
    namespace: &str,
    kept: Retained,
    fresh: &[&FormResult],
    outcomes: &Outcomes,
    file_key: &str,
) -> Option<PathBuf> {
    let path = snapshot_path(root, namespace).ok()?;
    let source = snapshot_source(fresh, &kept.forms, file_key);
    let forms = rendered_forms(kept.forms, fresh);
    let merged = merged_outcomes(kept.outcomes, fresh, outcomes, namespace);
    if forms.is_empty() && merged.is_empty() {
        remove_quietly(&path);
        return None;
    }
    let snapshot = render_snapshot(namespace, &source, forms, &merged);
    if let Err(err) = persist_snapshot(&path, &snapshot) {
        eprintln!("Could not write {}: {err}", path.display());
        return None;
    }
    Some(path)
}

fn without_tested_at(entries: &[(Value, Value)]) -> Vec<&(Value, Value)> {
    entries
        .iter()
        .filter(|(key, _)| *key != Value::Keyword("tested-at".to_string()))
        .collect()
}

fn same_content(old: &Value, new: &Value) -> bool {
    match (old, new) {
        (Value::Map(a), Value::Map(b)) => without_tested_at(a) == without_tested_at(b),
        _ => false,
    }
}

/// Write `snapshot` unless the file already holds the same content apart from `tested-at`.
fn persist_snapshot(path: &Path, snapshot: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if read_snapshot(path).is_some_and(|old| same_content(&old, snapshot)) {
        return Ok(());
    }
    fs::write(path, format!("{}\n", render(snapshot)))
}

fn form_file_only(data: &Value, file_key: &str) -> bool {
    if forms_of(data).next().is_none() {
        return text_field(data, "source") == Some(file_key);
    }
    forms_of(data).all(|form| form_file(form, data) == Some(file_key))
}

fn stale(
    path: &Path,
    data: &Value,
    touched: &BTreeSet<String>,
    canonical: &BTreeSet<PathBuf>,
    file_key: &str,
) -> bool {
    if canonical.contains(&resolve(path)) {
        return false;
    }
    text_field(data, "namespace").is_some_and(|namespace| touched.contains(namespace))
        || form_file_only(data, file_key)
}

fn drop_stale(
    existing: &[(PathBuf, Value)],
    touched: &BTreeSet<String>,
    canonical: &BTreeSet<PathBuf>,
    file_key: &str,
) {
    for (path, data) in existing {
        if stale(path, data, touched, canonical, file_key) {
            remove_quietly(path);
        }
    }
}

/// Delete `path`; a file that is already gone is not an error.
fn remove_quietly(path: &Path) {
    match fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => {
            eprintln!("Could not write {}: {err}", path.display());
        }
        _ => {}
    }
}

fn subdirs(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn is_empty_dir(directory: &Path) -> bool {
    fs::read_dir(directory).is_ok_and(|mut entries| entries.next().is_none())
}

/// Remove `directory`'s empty descendants, deepest first; `directory` itself stays.
fn remove_empty_dirs(directory: &Path) {
    for child in subdirs(directory) {
        prune_dir(&child);
    }
}

fn prune_dir(directory: &Path) {
    for child in subdirs(directory) {
        prune_dir(&child);
    }
    if is_empty_dir(directory)
        && let Err(err) = fs::remove_dir(directory)
    {
        eprintln!("Could not write {}: {err}", directory.display());
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::*;

    const OUTCOME: &str = r#"["hide.clj","demo.core","defn-/hide",8,12,"false","true"]"#;
    const TESTED_AT_PREFIX: &str = "{:version 1 :tested-at \"";

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        (dir, root)
    }

    fn form(namespace: &str, name: &str, file: &str, digest: &str, killed: usize) -> FormResult {
        FormResult {
            id: format!("defn/{name}"),
            namespace: namespace.to_string(),
            name: name.to_string(),
            private: false,
            file: file.to_string(),
            line: 2,
            end_line: 4,
            digest: digest.to_string(),
            killed,
            survived: 0,
            uncovered: 1,
            sites: 2,
        }
    }

    fn outcomes(pairs: &[(&str, Status)]) -> Outcomes {
        pairs
            .iter()
            .map(|(id, status)| (id.to_string(), *status))
            .collect()
    }

    fn read_value(path: &Path) -> Value {
        parse(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn form_named<'a>(data: &'a Value, id: &str) -> &'a Value {
        forms_of(data)
            .find(|form| text_field(form, "id") == Some(id))
            .unwrap()
    }

    fn prior<'a>(history: &'a History, namespace: &str, id: &str) -> &'a PriorForm {
        history
            .forms
            .get(&(namespace.to_string(), id.to_string()))
            .unwrap()
    }

    fn stamp_in(text: &str) -> &str {
        let rest = &text[TESTED_AT_PREFIX.len()..];
        &rest[..rest.find('"').unwrap()]
    }

    fn write_a(root: &Path, forms: &[FormResult]) -> Vec<PathBuf> {
        write_results(root, "a.go", forms, &outcomes(&[]))
    }

    #[test]
    fn mu_4_write_then_load_history_round_trips_forms() {
        // MU-4: a written form digest is found again by load_history for its file
        let (_dir, root) = fixture();
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 1)]);
        let history = load_history(&root, "a.go");
        let found = prior(&history, "demo", "defn/Run");
        assert_eq!(found.digest, "hash-a");
        assert_eq!(found.file, "a.go");
        assert!(load_history(&root, "b.go").forms.is_empty());
    }

    #[test]
    fn mu_4_two_files_of_one_namespace_share_one_snapshot() {
        // MU-4: Go files of one namespace merge into a single snapshot
        let (_dir, root) = fixture();
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 1)]);
        write_results(
            &root,
            "b.go",
            &[form("demo", "Stop", "b.go", "hash-b", 4)],
            &outcomes(&[]),
        );
        let path = snapshot_path(&root, "demo").unwrap();
        let data = read_value(&path);
        assert_eq!(data.get("namespace").and_then(Value::as_str), Some("demo"));
        assert_eq!(
            form_named(&data, "defn/Run").get("killed"),
            Some(&Value::Int(1))
        );
        assert_eq!(
            form_named(&data, "defn/Stop").get("killed"),
            Some(&Value::Int(4))
        );
        assert_eq!(edn_files(&root), vec![path]);
    }

    #[test]
    fn mu_4_replacing_one_file_keeps_the_other_file() {
        // MU-4: rewriting a.go replaces its forms and keeps b.go's form
        let (_dir, root) = fixture();
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 1)]);
        write_results(
            &root,
            "b.go",
            &[form("demo", "Stop", "b.go", "hash-b", 4)],
            &outcomes(&[]),
        );
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 9)]);
        let data = read_value(&snapshot_path(&root, "demo").unwrap());
        assert_eq!(
            form_named(&data, "defn/Run").get("killed"),
            Some(&Value::Int(9))
        );
        assert_eq!(
            form_named(&data, "defn/Stop").get("killed"),
            Some(&Value::Int(4))
        );
    }

    #[test]
    fn mu_19_private_form_and_outcome_round_trip_by_mutation_id() {
        // MU-19: an outcome is keyed by its mutation id and read back as the same status
        let (_dir, root) = fixture();
        let private = FormResult {
            id: "defn-/hide".to_string(),
            private: true,
            ..form("demo.core", "hide", "hide.clj", "digest", 1)
        };
        let recorded = outcomes(&[(OUTCOME, Status::Killed)]);
        write_results(&root, "hide.clj", &[private], &recorded);
        let data = read_value(&snapshot_path(&root, "demo.core").unwrap());
        let kind = form_named(&data, "defn-/hide")
            .get("kind")
            .and_then(Value::as_str);
        assert_eq!(kind, Some("defn-"));
        let history = load_history(&root, "hide.clj");
        assert_eq!(prior(&history, "demo.core", "defn-/hide").digest, "digest");
        assert_eq!(history.outcomes.get(OUTCOME), Some(&Status::Killed));
    }

    #[test]
    fn mu_19_outcomes_render_sorted_by_mutation_key() {
        // MU-19: outcomes are written in sorted mutation-key order
        const LATE: &str = r#"["a.go","demo","defn/Run",1,2,"x","y"]"#;
        const EARLY: &str = r#"["a.go","demo","defn/Run",1,2,"a","b"]"#;
        let (_dir, root) = fixture();
        let recorded = outcomes(&[(LATE, Status::Killed), (EARLY, Status::Survived)]);
        write_results(
            &root,
            "a.go",
            &[form("demo", "Run", "a.go", "h", 1)],
            &recorded,
        );
        let data = read_value(&snapshot_path(&root, "demo").unwrap());
        let Some(Value::Map(entries)) = data.get("outcomes") else {
            panic!("no outcomes map");
        };
        let keys: Vec<&str> = entries.iter().filter_map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, [EARLY, LATE]);
    }

    #[test]
    fn mu_4_snapshot_paths_follow_the_namespace() {
        // MU-4: namespace separators map to directories and the last part to the file name
        let root = Path::new("/root");
        assert_eq!(
            snapshot_path(root, "example.com/demo.Widget"),
            Ok(root.join(".metrics/mutate/example.com/demo.Widget.edn"))
        );
        assert_eq!(
            snapshot_path(root, "crate::foo::Bar"),
            Ok(root.join(".metrics/mutate/crate/foo/Bar.edn"))
        );
    }

    #[test]
    fn mu_4_unsafe_namespace_is_an_error() {
        // MU-4: empty, dot, and dot-dot parts are rejected
        let root = Path::new(".");
        for bad in [
            "", ".", "..", "a/../b", "a/./b", "a//b", "a::", "::b", "x\\..\\y",
        ] {
            assert!(
                snapshot_path(root, bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn mu_4_a_non_canonical_snapshot_is_folded_into_the_namespace_file() {
        // MU-4: a legacy per-file snapshot for a touched namespace is folded in and removed
        const LEGACY: &str = r#"{:version 1 :namespace "demo" :source "b.go" :forms [{:id "defn/Stop" :file "b.go" :line 3 :hash "hash-b" :killed 4 :survived 0 :uncovered 1 :sites 2}] :outcomes {}}"#;
        let (_dir, root) = fixture();
        let legacy_dir = root.join(".metrics").join("mutate");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(legacy_dir.join("legacy.edn"), format!("{LEGACY}\n")).unwrap();
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 1)]);
        assert!(!legacy_dir.join("legacy.edn").exists());
        let data = read_value(&snapshot_path(&root, "demo").unwrap());
        assert_eq!(
            form_named(&data, "defn/Stop").get("killed"),
            Some(&Value::Int(4))
        );
        let run_file = form_named(&data, "defn/Run")
            .get("file")
            .and_then(Value::as_str);
        assert_eq!(run_file, Some("a.go"));
    }

    #[test]
    fn mu_4_dropping_a_file_removes_its_namespace_and_empty_directories() {
        // MU-4: removing a file's last forms deletes its snapshot and the empty directories
        let (_dir, root) = fixture();
        let namespace = "example.com/demo.Widget";
        write_results(
            &root,
            "a.go",
            &[form(namespace, "Run", "a.go", "h", 1)],
            &outcomes(&[]),
        );
        write_a(&root, &[]);
        assert!(!snapshot_path(&root, namespace).unwrap().exists());
        assert!(
            !root
                .join(".metrics")
                .join("mutate")
                .join("example.com")
                .exists()
        );
    }

    #[test]
    fn mu_4_non_ascii_names_round_trip_unescaped() {
        // MU-4: non-ASCII form names are written as UTF-8 and read back
        let (_dir, root) = fixture();
        write_a(&root, &[form("demo", "Zoë", "a.go", "hash-z", 1)]);
        let text = fs::read_to_string(snapshot_path(&root, "demo").unwrap()).unwrap();
        assert!(text.contains("\"defn/Zoë\""), "{text}");
        let history = load_history(&root, "a.go");
        assert_eq!(prior(&history, "demo", "defn/Zoë").digest, "hash-z");
    }

    #[test]
    fn mu_4_forms_sort_by_line_then_id() {
        // MU-4: forms are ordered by file, line, then id
        let (_dir, root) = fixture();
        let forms = [
            FormResult {
                line: 9,
                ..form("demo", "Zed", "a.go", "z", 1)
            },
            form("demo", "Beta", "a.go", "b", 1),
            form("demo", "Alpha", "a.go", "a", 1),
        ];
        write_a(&root, &forms);
        let data = read_value(&snapshot_path(&root, "demo").unwrap());
        let ids: Vec<&str> = forms_of(&data)
            .filter_map(|form| text_field(form, "id"))
            .collect();
        assert_eq!(ids, ["defn/Alpha", "defn/Beta", "defn/Zed"]);
    }

    #[test]
    fn mu_4_rendered_snapshot_keeps_the_python_key_order() {
        // MU-4: the rendered text has the Python key order, apart from the tested-at value
        let (_dir, root) = fixture();
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 1)]);
        let text = fs::read_to_string(snapshot_path(&root, "demo").unwrap()).unwrap();
        assert!(text.starts_with(TESTED_AT_PREFIX), "{text}");
        let stamp = stamp_in(&text);
        let normalized = text.replacen(stamp, "TS", 1);
        assert_eq!(
            normalized,
            "{:version 1 :tested-at \"TS\" :source \"a.go\" :namespace \"demo\" :outcomes {} \
             :forms [{:id \"defn/Run\" :kind \"defn\" :file \"a.go\" :line 2 :end-line 4 \
             :hash \"hash-a\" :killed 1 :survived 0 :uncovered 1 :sites 2}]}\n"
        );
    }

    #[test]
    fn mu_nfr_1_identical_writes_one_second_apart_keep_the_bytes() {
        // MU-NFR-1: rewriting identical inputs a second later leaves the file bytes unchanged
        let (_dir, root) = fixture();
        let forms = [form("demo", "Run", "a.go", "hash-a", 1)];
        let recorded = outcomes(&[(OUTCOME, Status::Killed)]);
        let first = write_results(&root, "a.go", &forms, &recorded);
        let path = snapshot_path(&root, "demo").unwrap();
        let before = fs::read(&path).unwrap();
        thread::sleep(Duration::from_millis(1100));
        let second = write_results(&root, "a.go", &forms, &recorded);
        let after = fs::read(&path).unwrap();
        assert_eq!(before, after);
        assert_eq!(first, vec![path.clone()]);
        assert_eq!(second, vec![path]);
    }

    #[test]
    fn mu_nfr_1_changed_inputs_change_the_bytes() {
        // MU-NFR-1: a changed form count is written to the file
        let (_dir, root) = fixture();
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 1)]);
        let path = snapshot_path(&root, "demo").unwrap();
        let before = fs::read(&path).unwrap();
        thread::sleep(Duration::from_millis(1100));
        write_a(&root, &[form("demo", "Run", "a.go", "hash-a", 2)]);
        assert_ne!(before, fs::read(&path).unwrap());
    }
}
