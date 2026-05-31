use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

pub const MIN_FUZZY_SCORE_PER_CHAR: u32 = 14;

pub fn fuzzy_scored<K, S: AsRef<str>>(
    matcher: &mut Matcher,
    filter: &str,
    candidates: impl IntoIterator<Item = (K, S)>,
) -> Vec<(K, u32)> {
    let pattern = Pattern::parse(filter, CaseMatching::Ignore, Normalization::Smart);
    let threshold = filter.chars().count() as u32 * MIN_FUZZY_SCORE_PER_CHAR;
    let mut buf: Vec<char> = Vec::new();
    candidates
        .into_iter()
        .filter_map(|(key, hay)| {
            let haystack = Utf32Str::new(hay.as_ref(), &mut buf);
            pattern
                .score(haystack, matcher)
                .filter(|s| *s >= threshold)
                .map(|s| (key, s))
        })
        .collect()
}

pub fn fuzzy_sorted<K, S: AsRef<str>>(
    matcher: &mut Matcher,
    filter: &str,
    candidates: impl IntoIterator<Item = (K, S)>,
) -> Vec<K> {
    let mut scored = fuzzy_scored(matcher, filter, candidates);
    scored.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(k, _)| k).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleo_matcher::Config;

    fn matcher() -> Matcher {
        Matcher::new(Config::DEFAULT)
    }

    #[test]
    fn no_match_returns_empty() {
        let mut m = matcher();
        let out = fuzzy_sorted(&mut m, "xyz", vec![(0usize, "hello"), (1, "world")]);
        assert!(out.is_empty());
    }

    #[test]
    fn below_threshold_excluded() {
        let mut m = matcher();
        let out = fuzzy_sorted(&mut m, "abcde", vec![(0usize, "zzzzz"), (1, "abcde")]);
        assert_eq!(out, vec![1]);
    }

    #[test]
    fn contiguous_match_outranks_gapped() {
        let mut m = matcher();
        let out = fuzzy_sorted(&mut m, "quick", vec![(0usize, "q_u_i_c_k"), (1, "quick")]);
        assert_eq!(out.first(), Some(&1));
    }

    #[test]
    fn equal_scores_keep_input_order() {
        let mut m = matcher();
        let out = fuzzy_sorted(&mut m, "alpha", vec![(10usize, "alpha"), (20, "alpha"), (30, "alpha")]);
        assert_eq!(out, vec![10, 20, 30]);
    }

    #[test]
    fn scored_preserves_input_order() {
        let mut m = matcher();
        let out = fuzzy_scored(&mut m, "alpha", vec![(0usize, "alpha"), (1, "alpha"), (2, "alpha")]);
        let keys: Vec<usize> = out.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec![0, 1, 2]);
    }
}
