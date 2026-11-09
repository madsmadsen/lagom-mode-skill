// Derived from dryer by Robert C. Martin (https://github.com/unclebob/dryer), BSD Zero Clause License.
// Ports src/dryer/shape.py: printing, node counting, structural fingerprints, and Jaccard similarity.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::model::Norm;

/// Interns fingerprint strings into dense `u32` ids.
#[derive(Debug, Default)]
pub struct Interner {
    ids: HashMap<String, u32>,
    texts: Vec<String>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn id(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        let id = u32::try_from(self.texts.len()).expect("interner id fits in u32");
        self.texts.push(text.to_owned());
        self.ids.insert(text.to_owned(), id);
        id
    }

    pub fn text(&self, id: u32) -> &str {
        self.texts[id as usize].as_str()
    }
}

/// Print a normalized node the way Clojure `pr-str` prints it.
pub fn pr(node: &Norm) -> String {
    print_collecting(node, &mut Vec::new())
}

/// One plus the count of every nested node. Atoms count as one.
pub fn node_count(node: &Norm) -> usize {
    match node {
        Norm::List(items) => 1 + items.iter().map(node_count).sum::<usize>(),
        Norm::Kw(_) | Norm::Str(_) => 1,
    }
}

/// Sorted, deduplicated ids of the printed form of every list subtree
/// and every atom in the tree. A bare atom yields its own print.
pub fn fingerprints(node: &Norm, interner: &mut Interner) -> Vec<u32> {
    let mut prints = Vec::new();
    print_collecting(node, &mut prints);
    let mut ids: Vec<u32> = prints.iter().map(|text| interner.id(text)).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Jaccard similarity of two sorted, deduplicated fingerprint id lists.
pub fn jaccard(left: &[u32], right: &[u32]) -> f64 {
    let shared = shared_count(left, right);
    let union = left.len() + right.len() - shared;
    if union == 0 {
        return 0.0;
    }
    shared as f64 / union as f64
}

/// Prints `node`, pushing the print of every list and atom it visits into `prints`.
fn print_collecting(node: &Norm, prints: &mut Vec<String>) -> String {
    let text = match node {
        Norm::List(items) => print_list(items, prints),
        Norm::Kw(name) => format!(":{name}"),
        Norm::Str(text) => format!("\"{}\"", escape(text)),
    };
    prints.push(text.clone());
    text
}

fn print_list(items: &[Norm], prints: &mut Vec<String>) -> String {
    let pieces: Vec<String> = items
        .iter()
        .map(|child| print_collecting(child, prints))
        .collect();
    format!("[{}]", pieces.join(" "))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn shared_count(left: &[u32], right: &[u32]) -> usize {
    let (mut i, mut j, mut shared) = (0, 0, 0);
    while i < left.len() && j < right.len() {
        match left[i].cmp(&right[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    shared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Norm;

    fn sym(name: &'static str) -> Norm {
        Norm::Kw(name)
    }

    fn text(name: &str) -> Norm {
        Norm::Str(name.to_owned())
    }

    /// Fingerprint texts, sorted, for readable comparison.
    fn fingerprint_texts(node: &Norm, interner: &mut Interner) -> Vec<String> {
        let ids = fingerprints(node, interner);
        let mut texts: Vec<String> = ids.iter().map(|&id| interner.text(id).to_owned()).collect();
        texts.sort();
        texts
    }

    // DR-10: pr prints keywords as ":name" and strings quoted, lists in brackets.
    #[test]
    fn dr10_pr_prints_keyword_list_with_quoted_string() {
        let node = Norm::List(vec![sym("symbol"), text("a")]);
        assert_eq!(pr(&node), r#"[:symbol "a"]"#);
    }

    // DR-10: pr escapes backslash before double quote inside strings.
    #[test]
    fn dr10_pr_escapes_backslash_and_quote() {
        let node = text(r#"a\b"c"#);
        assert_eq!(pr(&node), r#""a\\b\"c""#);
    }

    // DR-10: node_count counts each list and each atom as one node.
    #[test]
    fn dr10_node_count_counts_lists_and_atoms_once_each() {
        let node = Norm::List(vec![sym("call"), Norm::List(vec![sym("name"), text("a")])]);
        assert_eq!(node_count(&node), 5);
        assert_eq!(node_count(&sym("x")), 1);
    }

    // DR-10: a bare atom has a single fingerprint, its own print.
    #[test]
    fn dr10_fingerprint_of_bare_atom_is_single_id() {
        let mut interner = Interner::new();
        let ids = fingerprints(&sym("x"), &mut interner);
        assert_eq!(ids.len(), 1);
        assert_eq!(interner.text(ids[0]), ":x");
    }

    // DR-10: fingerprints are the union of every list subtree print and every atom print.
    #[test]
    fn dr10_fingerprint_is_union_of_list_and_atom_prints() {
        let mut interner = Interner::new();
        let node = Norm::List(vec![sym("call"), Norm::List(vec![sym("name"), text("a")])]);
        let expected = vec![
            "\"a\"".to_owned(),
            ":call".to_owned(),
            ":name".to_owned(),
            "[:call [:name \"a\"]]".to_owned(),
            "[:name \"a\"]".to_owned(),
        ];
        assert_eq!(fingerprint_texts(&node, &mut interner), expected);
    }

    // DR-10: repeated prints collapse to one id; returned ids are sorted ascending.
    #[test]
    fn dr10_fingerprint_ids_are_sorted_and_deduplicated() {
        let mut interner = Interner::new();
        let node = Norm::List(vec![sym("x"), sym("x")]);
        let ids = fingerprints(&node, &mut interner);
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            fingerprint_texts(&node, &mut interner),
            vec![":x".to_owned(), "[:x :x]".to_owned()]
        );
    }

    // DR-10: identical fingerprint sets score 1.0.
    #[test]
    fn dr10_jaccard_identical_sets_score_one() {
        assert_eq!(jaccard(&[1, 2, 3], &[1, 2, 3]), 1.0);
    }

    // DR-10: disjoint fingerprint sets score 0.0.
    #[test]
    fn dr10_jaccard_disjoint_sets_score_zero() {
        assert_eq!(jaccard(&[1, 2], &[3, 4]), 0.0);
    }

    // DR-10: two empty sets score 0.0, not NaN.
    #[test]
    fn dr10_jaccard_empty_pair_scores_zero() {
        assert_eq!(jaccard(&[], &[]), 0.0);
    }

    // DR-10: two shared ids out of a union of three score exactly 2/3.
    #[test]
    fn dr10_jaccard_two_shared_of_three_union_is_two_thirds() {
        assert_eq!(jaccard(&[1, 2, 3], &[1, 2]), 2.0 / 3.0);
    }

    // DR-10: trees that differ in one string atom score below 1.0.
    #[test]
    fn dr10_one_str_atom_difference_scores_below_one() {
        let mut interner = Interner::new();
        let left = Norm::List(vec![sym("call"), text("a")]);
        let right = Norm::List(vec![sym("call"), text("b")]);
        let left_ids = fingerprints(&left, &mut interner);
        let right_ids = fingerprints(&right, &mut interner);
        let score = jaccard(&left_ids, &right_ids);
        assert!(score < 1.0, "score was {score}");
        assert!(score > 0.0, "score was {score}");
    }
}
