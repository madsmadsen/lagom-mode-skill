// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: src/crapper/coverage.py
// license: MIT

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Insertion-ordered map. Python dicts keep insertion order and key selection depends on it.
#[derive(Debug, Clone)]
pub struct Keyed<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for Keyed<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<V> Keyed<V> {
    /// Python `dict[key] = value`: replaces in place and keeps the first position.
    pub fn insert(&mut self, key: String, value: V) {
        match self.entries.iter().position(|(k, _)| *k == key) {
            Some(index) => self.entries[index].1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// Python `dict.setdefault(key, V::default())`.
    pub fn get_mut_or_default(&mut self, key: String) -> &mut V
    where
        V: Default,
    {
        let index = match self.entries.iter().position(|(k, _)| *k == key) {
            Some(index) => index,
            None => {
                self.entries.push((key, V::default()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[index].1
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

impl<V> IntoIterator for Keyed<V> {
    type Item = (String, V);
    type IntoIter = std::vec::IntoIter<(String, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// Python `_normalize_path`: URL-decode, then strip a leading `./` and `/`, then convert backslashes.
pub fn normalize_path(path: &str) -> String {
    let decoded = percent_decode(path);
    let text = decoded.replace('\\', "/");
    let text = text.strip_prefix("./").unwrap_or(&text);
    text.trim_start_matches('/').to_string()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match decode_escape(&bytes[index..]) {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_escape(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 3 || bytes[0] != b'%' {
        return None;
    }
    let high = (bytes[1] as char).to_digit(16)?;
    let low = (bytes[2] as char).to_digit(16)?;
    u8::try_from(high * 16 + low).ok()
}

/// Python `_segments`: normalize, then split on `/`, dropping empty parts.
fn segments(path: &str) -> Vec<String> {
    normalize_path(path)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// Python `_candidates`: relative, absolute, and `src/`-stripped spellings of a source path, deduplicated.
pub fn candidates(source_path: &str) -> Vec<String> {
    let relative = normalize_path(source_path);
    let absolute = if source_path.is_empty() {
        relative.clone()
    } else {
        normalize_path(&resolve(Path::new(source_path)).to_string_lossy())
    };
    let no_src = relative
        .strip_prefix("src/")
        .unwrap_or(&relative)
        .to_string();
    let absolute_no_src = absolute.replace("/src/", "/");
    let mut found: Vec<String> = Vec::new();
    for value in [relative, absolute, no_src, absolute_no_src] {
        if !value.is_empty() && !found.contains(&value) {
            found.push(value);
        }
    }
    found
}

/// Python `_source_parts`: the segment lists of every candidate spelling of every source path.
pub fn source_parts(paths: &[String]) -> Vec<Vec<String>> {
    let mut found: Vec<Vec<String>> = Vec::new();
    for source in paths {
        for candidate in candidates(source) {
            let parts = segments(&candidate);
            if !parts.is_empty() && !found.contains(&parts) {
                found.push(parts);
            }
        }
    }
    found
}

/// Python `Path.resolve()`: absolute, lexically normalized, with symlinks resolved on the longest existing prefix.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let normal = lexically_normal(&absolute);
    let mut prefix = normal.as_path();
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(real) = fs::canonicalize(prefix) {
            return tail.iter().rev().fold(real, |acc, name| acc.join(name));
        }
        match (prefix.parent(), prefix.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                prefix = parent;
            }
            _ => return normal,
        }
    }
}

fn lexically_normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Python `_lookup` with `_select_key`: find the single report key that matches a source path.
pub fn lookup<'a, V>(
    index: &'a Keyed<V>,
    source_path: &str,
    source_parts: &[Vec<String>],
) -> Option<&'a V> {
    let index_position = select_index(&index.entries, source_path, source_parts)?;
    Some(&index.entries[index_position].1)
}

fn select_index<V>(
    entries: &[(String, V)],
    source_path: &str,
    source_parts: &[Vec<String>],
) -> Option<usize> {
    let items: Vec<(usize, String)> = entries
        .iter()
        .enumerate()
        .map(|(position, (key, _))| (position, normalize_path(key)))
        .collect();
    let candidate_list = candidates(source_path);
    if let Some(position) = exact_index(&items, &candidate_list) {
        return Some(position);
    }
    let parts_of: Vec<Vec<String>> = candidate_list
        .iter()
        .map(|candidate| segments(candidate))
        .filter(|parts| !parts.is_empty())
        .collect();
    let forward = forward_matches(&items, &parts_of, source_parts);
    match forward.as_slice() {
        [only] => Some(*only),
        [] => reverse_index(&items, &parts_of, source_parts),
        _ => None,
    }
}

fn exact_index(items: &[(usize, String)], candidate_list: &[String]) -> Option<usize> {
    candidate_list.iter().find_map(|candidate| {
        items
            .iter()
            .find(|(_, norm)| norm == candidate)
            .map(|(position, _)| *position)
    })
}

fn forward_matches(
    items: &[(usize, String)],
    parts_of: &[Vec<String>],
    source_parts: &[Vec<String>],
) -> Vec<usize> {
    let mut forward: Vec<(usize, usize)> = Vec::new();
    for parts in parts_of {
        for (position, norm) in items {
            let key_parts = segments(norm);
            if !key_parts.ends_with(parts)
                || longer_source_owns(&key_parts, parts.len(), source_parts)
            {
                continue;
            }
            forward.push((key_parts.len() - parts.len(), *position));
        }
    }
    let Some(smallest) = forward.iter().map(|(extra, _)| *extra).min() else {
        return Vec::new();
    };
    let mut out: Vec<usize> = Vec::new();
    for (extra, position) in forward {
        if extra == smallest && !out.contains(&position) {
            out.push(position);
        }
    }
    out
}

fn longer_source_owns(
    key_parts: &[String],
    source_len: usize,
    source_parts: &[Vec<String>],
) -> bool {
    source_parts.iter().any(|other| {
        other.len() > source_len && other.len() <= key_parts.len() && key_parts.ends_with(other)
    })
}

fn reverse_index(
    items: &[(usize, String)],
    parts_of: &[Vec<String>],
    source_parts: &[Vec<String>],
) -> Option<usize> {
    let mut reverse: Vec<usize> = Vec::new();
    for parts in parts_of {
        for (position, norm) in items {
            let key_parts = segments(norm);
            if !is_proper_suffix(parts, &key_parts)
                || another_source_ends_with(&key_parts, source_parts, parts_of)
            {
                continue;
            }
            if !reverse.contains(position) {
                reverse.push(*position);
            }
        }
    }
    match reverse.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

fn is_proper_suffix(parts: &[String], key_parts: &[String]) -> bool {
    !key_parts.is_empty() && key_parts.len() < parts.len() && parts.ends_with(key_parts)
}

fn another_source_ends_with(
    key_parts: &[String],
    source_parts: &[Vec<String>],
    own: &[Vec<String>],
) -> bool {
    source_parts.iter().any(|other| {
        !own.contains(other) && other.len() >= key_parts.len() && other.ends_with(key_parts)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // CR-12: source path normalization strips URL escapes and leading dot/slash prefixes.
    #[test]
    fn cr_12_normalize_path_strips_url_and_dot_prefixes() {
        assert_eq!(normalize_path("%2E%2Fsrc%2Fcore.clj"), "src/core.clj");
        assert_eq!(normalize_path("./src/core.clj"), "src/core.clj");
        assert_eq!(normalize_path("/src/core.clj"), "src/core.clj");
    }

    // CR-12: when no key matches exactly, a key that is a longer path ending in the source wins.
    #[test]
    fn cr_12_a_short_report_key_matches_one_longer_source() {
        let index = Keyed {
            entries: vec![("proj/src/a.go".to_string(), 7)],
        };
        let parts = source_parts(&["src/a.go".to_string()]);
        assert_eq!(lookup(&index, "src/a.go", &parts), Some(&7));
    }

    // CR-12: two keys that tie on suffix length select nothing.
    #[test]
    fn cr_12_a_tie_on_suffix_length_selects_nothing() {
        let index = Keyed {
            entries: vec![("x/main.go".to_string(), 1), ("y/main.go".to_string(), 2)],
        };
        let parts = source_parts(&["z/main.go".to_string()]);
        assert_eq!(lookup(&index, "z/main.go", &parts), None);
    }

    // CR-12: an insert that repeats an existing key keeps its first position (Python dict semantics).
    #[test]
    fn cr_12_insert_replaces_in_place_and_keeps_order() {
        let mut index: Keyed<i32> = Keyed::default();
        index.insert("a".to_string(), 1);
        index.insert("b".to_string(), 2);
        index.insert("a".to_string(), 3);
        let order: Vec<(String, i32)> = index.into_iter().collect();
        assert_eq!(order, vec![("a".to_string(), 3), ("b".to_string(), 2)]);
    }
}
