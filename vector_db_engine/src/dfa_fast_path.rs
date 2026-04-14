use std::collections::{HashMap, HashSet};

use aho_corasick::{AhoCorasick, MatchKind};

use crate::simd_preprocessor::{normalize_token, Candidate};

const MIN_MATCH_LENGTH: usize = 4;
const MAX_TOKEN_LEN: usize = 24;

type EdgeKey = (u8, u8);
type EdgeBuckets = HashMap<EdgeKey, Vec<String>>;

/// L1 lexical detector:
/// 1. Raw Aho-Corasick for exact phrase matches.
/// 2. Normalized set/index for obfuscation + bounded fuzzy checks.
pub struct DfaEngine {
    raw_ac: AhoCorasick,
    normalized_set: HashSet<String>,
    bad_words_by_len_edge: HashMap<usize, EdgeBuckets>,
    min_match_length: usize,
}

impl DfaEngine {
    pub fn new(patterns: &[&str]) -> Self {
        let cleaned_patterns: Vec<&str> = patterns
            .iter()
            .copied()
            .filter(|p| !p.trim().is_empty())
            .collect();

        let raw_ac = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .ascii_case_insensitive(true)
            .build(&cleaned_patterns)
            .expect("Failed to build raw Aho-Corasick automaton");

        let mut normalized_set = HashSet::<String>::new();
        for pattern in &cleaned_patterns {
            let normalized = normalize_token(pattern, false);
            if normalized.len() >= MIN_MATCH_LENGTH && normalized.len() <= MAX_TOKEN_LEN {
                normalized_set.insert(normalized);
            }
        }

        let mut normalized_words: Vec<String> = normalized_set.iter().cloned().collect();
        normalized_words.sort_unstable();

        let mut bad_words_by_len_edge = HashMap::<usize, EdgeBuckets>::new();
        for word in &normalized_words {
            let bytes = word.as_bytes();
            if bytes.len() < MIN_MATCH_LENGTH {
                continue;
            }

            let edge = (bytes[0], bytes[bytes.len() - 1]);
            bad_words_by_len_edge
                .entry(bytes.len())
                .or_default()
                .entry(edge)
                .or_default()
                .push(word.clone());
        }

        Self {
            raw_ac,
            normalized_set,
            bad_words_by_len_edge,
            min_match_length: MIN_MATCH_LENGTH,
        }
    }

    #[inline(always)]
    pub fn scan(&self, text: &str) -> bool {
        self.raw_ac.is_match(text)
    }

    pub fn scan_candidates(&self, candidates: &[Candidate], require_obfuscated: bool) -> bool {
        for candidate in candidates {
            let token = candidate.text.as_str();
            if token.len() < self.min_match_length || token.len() > MAX_TOKEN_LEN {
                continue;
            }

            if require_obfuscated && !candidate.obfuscated {
                continue;
            }

            if self.normalized_set.contains(token) {
                return true;
            }

            if candidate.obfuscated && self.has_fuzzy_match(token) {
                return true;
            }
        }

        false
    }

    fn has_fuzzy_match(&self, token: &str) -> bool {
        let token_bytes = token.as_bytes();
        if token_bytes.len() < self.min_match_length {
            return false;
        }

        let max_dist = if token_bytes.len() <= self.min_match_length + 1 {
            1
        } else {
            2
        };

        let min_len = self
            .min_match_length
            .max(token_bytes.len().saturating_sub(max_dist));
        let max_len = (token_bytes.len() + max_dist).min(MAX_TOKEN_LEN);
        let edge = (token_bytes[0], token_bytes[token_bytes.len() - 1]);

        for len in min_len..=max_len {
            let Some(edge_map) = self.bad_words_by_len_edge.get(&len) else {
                continue;
            };
            let Some(bucket) = edge_map.get(&edge) else {
                continue;
            };

            for bad_word in bucket {
                if damerau_levenshtein_limited(token, bad_word, max_dist).is_some() {
                    return true;
                }
            }
        }

        false
    }
}

fn damerau_levenshtein_limited(a: &str, b: &str, max_dist: usize) -> Option<usize> {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_len = a_bytes.len();
    let b_len = b_bytes.len();

    if a_len.abs_diff(b_len) > max_dist {
        return None;
    }

    let mut prev_prev = vec![0usize; b_len + 1];
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        let mut row_min = curr[0];

        for j in 1..=b_len {
            let substitution_cost = usize::from(a_bytes[i - 1] != b_bytes[j - 1]);
            let mut value = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + substitution_cost);

            if i > 1
                && j > 1
                && a_bytes[i - 1] == b_bytes[j - 2]
                && a_bytes[i - 2] == b_bytes[j - 1]
            {
                value = value.min(prev_prev[j - 2] + 1);
            }

            curr[j] = value;
            row_min = row_min.min(value);
        }

        if row_min > max_dist {
            return None;
        }

        std::mem::swap(&mut prev_prev, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }

    let distance = prev[b_len];
    (distance <= max_dist).then_some(distance)
}

#[cfg(test)]
mod tests {
    use crate::simd_preprocessor::SimdBuffer;

    use super::DfaEngine;

    #[test]
    fn catches_masked_slur_with_missing_letter() {
        let patterns = ["nigga", "nigger"];
        let engine = DfaEngine::new(&patterns);

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text("n###gga");

        assert!(engine.scan_candidates(buffer.strict_candidates(), false));
    }

    #[test]
    fn catches_spaced_single_letter_obfuscation() {
        let patterns = ["nigga"];
        let engine = DfaEngine::new(&patterns);

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text("n i g g a");

        assert!(engine.scan_candidates(buffer.merged_candidates(), false));
    }
}
