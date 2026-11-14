use std::collections::HashMap;

use icu_normalizer::ComposingNormalizerBorrowed;

const MIN_SHARED_COMPONENTS: usize = 2;

pub struct PathIndex {
    keys: Vec<String>,
    folded: Vec<String>,
    by_path: HashMap<String, Vec<usize>>,
    by_name: HashMap<String, Vec<usize>>,
}

impl PathIndex {
    pub fn new(keys: Vec<String>) -> Self {
        let folded: Vec<String> = keys.iter().map(|key| fold(key)).collect();
        let mut by_path: HashMap<String, Vec<usize>> = HashMap::with_capacity(keys.len());
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::with_capacity(keys.len());
        for (ix, folded) in folded.iter().enumerate() {
            if let Some(name) = components(folded).next_back() {
                by_name.entry(name.to_string()).or_default().push(ix);
            }
            by_path.entry(folded.clone()).or_default().push(ix);
        }
        Self {
            keys,
            folded,
            by_path,
            by_name,
        }
    }

    pub fn find(&self, path: &str) -> Option<&str> {
        let folded = fold(path);
        if let Some(&[ix]) = self.by_path.get(&folded).map(Vec::as_slice) {
            return Some(&self.keys[ix]);
        }
        let wanted: Vec<&str> = components(&folded).collect();
        let candidates = self.by_name.get(*wanted.last()?)?;
        let mut best = 0;
        let mut holder = None;
        for &ix in candidates {
            let shared = components(&self.folded[ix])
                .rev()
                .zip(wanted.iter().rev())
                .take_while(|(have, want)| have == *want)
                .count();
            if shared > best {
                best = shared;
                holder = Some(ix);
            } else if shared == best {
                holder = None;
            }
        }
        if best < MIN_SHARED_COMPONENTS {
            return None;
        }
        holder.map(|ix| self.keys[ix].as_str())
    }
}

fn fold(path: &str) -> String {
    ComposingNormalizerBorrowed::new_nfc()
        .normalize(path)
        .to_lowercase()
}

fn components(path: &str) -> impl DoubleEndedIterator<Item = &str> {
    path.split(['/', '\\']).filter(|part| !part.is_empty())
}
