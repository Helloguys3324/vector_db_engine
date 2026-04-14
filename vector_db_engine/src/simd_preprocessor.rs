use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

const MAX_CANDIDATES: usize = 128;
const MAX_TOKEN_LEN: usize = 24;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub text: String,
    pub obfuscated: bool,
}

#[derive(Clone, Debug, Default)]
struct Chunk {
    text: String,
    obfuscated: bool,
}

/// Kept as `SimdBuffer` to avoid touching external call sites, but the payload prep
/// now mirrors the old JS detector logic (NFKC, leet folding, obfuscation candidates).
#[derive(Default)]
pub struct SimdBuffer {
    normalized_with_spaces: String,
    strict_candidates: Vec<Candidate>,
    collapsed_candidates: Vec<Candidate>,
    merged_candidates: Vec<Candidate>,
}

impl SimdBuffer {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn normalize_adversarial_text(&mut self, text: &str) {
        self.normalized_with_spaces = normalize_with_spaces(text, false);
        self.strict_candidates = extract_candidates(text, false);
        self.collapsed_candidates = extract_candidates(text, true);
        self.merged_candidates =
            merge_candidate_sets(&self.strict_candidates, &self.collapsed_candidates);
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.normalized_with_spaces
    }

    #[inline]
    pub fn strict_candidates(&self) -> &[Candidate] {
        &self.strict_candidates
    }

    #[inline]
    pub fn collapsed_candidates(&self) -> &[Candidate] {
        &self.collapsed_candidates
    }

    #[inline]
    pub fn merged_candidates(&self) -> &[Candidate] {
        &self.merged_candidates
    }
}

#[inline]
fn map_digit_to_leet(ch: char) -> char {
    match ch {
        '4' => 'a',
        '3' => 'e',
        '1' => 'i',
        '0' => 'o',
        '5' => 's',
        '7' => 't',
        '8' => 'b',
        '2' => 'z',
        '6' => 'g',
        '9' => 'g',
        _ => ch,
    }
}

#[inline]
fn map_symbol_to_leet(ch: char) -> Option<char> {
    match ch {
        '@' => Some('a'),
        '$' => Some('s'),
        '!' | '|' => Some('i'),
        '+' => Some('t'),
        _ => None,
    }
}

#[inline]
fn map_char(ch: char, map_digits_to_leet: bool) -> char {
    if ch.is_ascii_lowercase() {
        return ch;
    }

    if matches!(ch, ' ' | '\t' | '\n' | '\r') {
        return ' ';
    }

    if ch.is_ascii_digit() {
        return if map_digits_to_leet {
            map_digit_to_leet(ch)
        } else {
            ch
        };
    }

    if let Some(mapped) = map_symbol_to_leet(ch) {
        return mapped;
    }

    ' '
}

fn has_repeated_run(text: &str) -> bool {
    if text.len() < 2 {
        return false;
    }

    let lowered: Vec<char> = text.to_lowercase().chars().collect();
    let mut run = 1usize;
    let mut max_run = 1usize;

    for i in 1..lowered.len() {
        if lowered[i] == lowered[i - 1] {
            run += 1;
            max_run = max_run.max(run);
            if run >= 2 && matches!(lowered[i], 'z' | 'x' | 'q' | 'v' | 'j' | 'k') {
                return true;
            }
        } else {
            run = 1;
        }
    }

    max_run >= 3
}

#[inline]
fn vowel_count(text: &str) -> usize {
    text.chars()
        .filter(|c| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u'))
        .count()
}

fn is_consonant_heavy(text: &str) -> bool {
    if text.len() < 3 {
        return false;
    }

    let vowels = vowel_count(text);
    if text.len() <= 4 {
        return vowels == 0;
    }

    (vowels as f32 / text.len() as f32) <= 0.22
}

fn collapse_repeated_runs_with_limit(text: &str, max_run: usize) -> String {
    if text.is_empty() || max_run == 0 {
        return String::new();
    }

    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    let mut run = 0usize;

    for ch in text.chars() {
        if prev == Some(ch) {
            run += 1;
        } else {
            prev = Some(ch);
            run = 1;
        }

        if run > max_run {
            continue;
        }
        out.push(ch);
    }

    out
}

fn canonicalize_obfuscated_candidate(token: &str) -> Option<String> {
    let mut softened = collapse_repeated_runs_with_limit(token, 2);
    if softened.is_empty() {
        return None;
    }

    if softened.len() >= 5 && softened.ends_with("ie") {
        softened.truncate(softened.len() - 2);
        softened.push('y');
    }

    if softened != token {
        Some(softened)
    } else {
        None
    }
}

fn normalize_with_spaces(text: &str, collapse_repeats: bool) -> String {
    if text.is_empty() {
        return String::new();
    }

    let source: String = text.nfkc().collect::<String>().to_lowercase();
    let has_letters = source.chars().any(|c| c.is_ascii_lowercase());
    let has_strong_leet_markers = source
        .chars()
        .any(|c| matches!(c, '@' | '$' | '!' | '|' | '+'));
    let map_digits_to_leet = has_letters || has_strong_leet_markers;

    let mut out = String::with_capacity(source.len());
    let mut prev_non_space: Option<char> = None;
    let mut prev_was_space = true;

    for raw_char in source.chars() {
        let mapped = map_char(raw_char, map_digits_to_leet);
        if mapped == ' ' {
            if !prev_was_space {
                out.push(' ');
                prev_was_space = true;
            }
            continue;
        }

        if collapse_repeats && prev_non_space.is_some_and(|prev| prev == mapped) {
            continue;
        }

        out.push(mapped);
        prev_non_space = Some(mapped);
        prev_was_space = false;
    }

    out.trim().to_string()
}

fn compact_word(normalized: &str) -> String {
    normalized
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

pub fn normalize_token(text: &str, collapse_repeats: bool) -> String {
    compact_word(&normalize_with_spaces(text, collapse_repeats))
}

fn extract_candidates(text: &str, collapse_repeats: bool) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut chunks = Vec::<Chunk>::new();

    for raw_chunk in text.split_whitespace() {
        if raw_chunk.is_empty() {
            continue;
        }

        let normalized = normalize_with_spaces(raw_chunk, collapse_repeats);
        if normalized.is_empty() {
            continue;
        }

        let compacted = compact_word(&normalized);
        if compacted.is_empty() || compacted.len() > MAX_TOKEN_LEN {
            continue;
        }

        let has_non_alpha = raw_chunk.chars().any(|c| !c.is_ascii_alphabetic());
        let obfuscated =
            has_non_alpha || has_repeated_run(raw_chunk) || is_consonant_heavy(&compacted);

        if compacted.len() >= 2 && seen.insert(compacted.clone()) {
            candidates.push(Candidate {
                text: compacted.clone(),
                obfuscated,
            });
            if candidates.len() >= MAX_CANDIDATES {
                return candidates;
            }
        }

        if obfuscated {
            if let Some(canonicalized) = canonicalize_obfuscated_candidate(&compacted) {
                if canonicalized.len() >= 2
                    && canonicalized.len() <= MAX_TOKEN_LEN
                    && seen.insert(canonicalized.clone())
                {
                    candidates.push(Candidate {
                        text: canonicalized,
                        obfuscated: true,
                    });
                    if candidates.len() >= MAX_CANDIDATES {
                        return candidates;
                    }
                }
            }
        }

        chunks.push(Chunk {
            text: compacted,
            obfuscated,
        });
    }

    for start in 0..chunks.len() {
        if chunks[start].text.len() > 3 {
            continue;
        }

        let mut combined = chunks[start].text.clone();
        let mut has_obfuscated_chunk = chunks[start].obfuscated;
        let mut single_char_count = usize::from(chunks[start].text.len() == 1);

        for end in (start + 1)..usize::min(start + 5, chunks.len()) {
            if chunks[end].text.len() > 3 {
                break;
            }

            combined.push_str(&chunks[end].text);
            has_obfuscated_chunk |= chunks[end].obfuscated;
            if chunks[end].text.len() == 1 {
                single_char_count += 1;
            }

            if combined.len() > MAX_TOKEN_LEN {
                break;
            }

            if combined.len() >= 3
                && (has_obfuscated_chunk || single_char_count >= 2)
                && seen.insert(combined.clone())
            {
                candidates.push(Candidate {
                    text: combined.clone(),
                    obfuscated: true,
                });
                if candidates.len() >= MAX_CANDIDATES {
                    return candidates;
                }
            }
        }
    }

    candidates
}

fn merge_candidate_sets(strict: &[Candidate], collapsed: &[Candidate]) -> Vec<Candidate> {
    let mut merged = Vec::<Candidate>::new();
    let mut positions = HashMap::<String, usize>::new();

    for candidate in strict.iter().chain(collapsed.iter()) {
        if let Some(existing_idx) = positions.get(&candidate.text).copied() {
            if candidate.obfuscated {
                merged[existing_idx].obfuscated = true;
            }
            continue;
        }

        if merged.len() >= MAX_CANDIDATES {
            break;
        }

        positions.insert(candidate.text.clone(), merged.len());
        merged.push(candidate.clone());
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::SimdBuffer;

    #[test]
    fn extracts_ngga_candidate_from_masked_slur() {
        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text("n###gga");
        assert!(buffer
            .strict_candidates()
            .iter()
            .any(|c| c.text == "ngga" && c.obfuscated));
    }

    #[test]
    fn merges_spaced_letters_into_word_candidate() {
        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text("n i g g a");
        assert!(buffer
            .merged_candidates()
            .iter()
            .any(|c| c.text == "nigga" && c.obfuscated));
    }

    #[test]
    fn canonicalizes_pusssie_into_pussy_candidate() {
        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text("pusssie");
        assert!(buffer
            .merged_candidates()
            .iter()
            .any(|c| c.text == "pussy" && c.obfuscated));
    }
}
