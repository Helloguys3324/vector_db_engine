use crate::simd_preprocessor::{normalize_token, Candidate};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use unicode_normalization::UnicodeNormalization;

const DEFAULT_CLEAN_LEXICON_LIMIT: usize = 100_000;
const MAX_CLEAN_LEXICON_LIMIT: usize = 500_000;
const DEFAULT_MIN_MATCH_LENGTH: usize = 4;
const DEFAULT_ANALYSIS_CACHE_SIZE: usize = 10_000;
const MAX_ANALYSIS_CACHE_SIZE: usize = 100_000;
const MAX_ANALYSIS_CACHE_TEXT_LENGTH: usize = 256;
const DEFAULT_VECTOR_FALLBACK_MIN_CHARS: usize = 16;
const DEFAULT_VECTOR_FALLBACK_MIN_TOKENS: usize = 3;

const DEFAULT_BLOOM_BITS: usize = 40_000_000;
const DEFAULT_BLOOM_HASHES: usize = 7;

const SHORT_ACRONYM_TAGS: [&str; 5] = [
    "acronym",
    "acronyms",
    "short-acronym",
    "abbreviation",
    "abbreviations",
];

const AGGRESSIVE_SHORT_TAGS: [&str; 4] = ["aggressive", "severe", "racism", "hate"];

const MONEY_TOKENS: [&str; 12] = [
    "usd", "eur", "btc", "eth", "ton", "trx", "usdt", "wallet", "card", "cvv", "otp", "pin",
];

const URGENCY_TOKENS: [&str; 11] = [
    "urgent",
    "immediately",
    "now",
    "verify",
    "confirm",
    "claim",
    "winner",
    "bonus",
    "profit",
    "investment",
    "security",
];

const SEMANTIC_TRIGGER_TOKENS: [&str; 21] = [
    "die", "kill", "suicide", "yourself", "end", "rope", "jump", "wallet", "crypto", "airdrop",
    "seed", "scam", "fraud", "phishing", "btc", "eth", "usdt", "card", "cvv", "otp", "pin",
];

const HIGH_RISK_PHRASES: [&str; 10] = [
    "kill yourself",
    "end yourself",
    "go die",
    "just die",
    "die now",
    "commit suicide",
    "hang yourself",
    "jump off",
    "gas yourself",
    "off yourself",
];

const EMBEDDED_MODERATION_DB_JSON: &str = include_str!("embedded_js/moderation-db.json");
const EMBEDDED_LEGACY_EXTERNAL_JSON: &str = include_str!("embedded_js/merged-external.json");
const EMBEDDED_DECISION_MODEL_JSON: &str = include_str!("embedded_js/decision-model.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Review,
    Block,
}

#[derive(Clone, Copy, Debug)]
struct DecisionFeatures {
    bias: f32,
    native_exact: f32,
    exact: f32,
    fuzzy_strong: f32,
    fuzzy_weak: f32,
    skeleton: f32,
    obfuscated: f32,
    hard_separator: f32,
    leet: f32,
    digit: f32,
    hyphen_only: f32,
    apostrophe: f32,
    alpha_only: f32,
    likely_clean: f32,
    short_token: f32,
    long_token: f32,
}

impl Default for DecisionFeatures {
    fn default() -> Self {
        Self {
            bias: -2.35,
            native_exact: 3.2,
            exact: 2.4,
            fuzzy_strong: 1.45,
            fuzzy_weak: 0.75,
            skeleton: 0.6,
            obfuscated: 0.95,
            hard_separator: 1.15,
            leet: 1.05,
            digit: 0.45,
            hyphen_only: -2.7,
            apostrophe: -1.15,
            alpha_only: -0.35,
            likely_clean: -2.1,
            short_token: -0.9,
            long_token: 0.35,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DecisionModel {
    review_threshold: f32,
    block_threshold: f32,
    features: DecisionFeatures,
}

impl Default for DecisionModel {
    fn default() -> Self {
        Self {
            review_threshold: 0.46,
            block_threshold: 0.66,
            features: DecisionFeatures::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SurfaceSignals {
    pub digit: bool,
    pub leet: bool,
    pub hard_separator: bool,
    pub hyphen_only: bool,
    pub apostrophe: bool,
    pub alpha_only: bool,
}

#[derive(Clone, Debug)]
struct HeuristicEvidence {
    exact: bool,
    fuzzy_strong: bool,
    fuzzy_weak: bool,
    skeleton: bool,
    obfuscated: bool,
    likely_clean: bool,
    short_token: bool,
    long_token: bool,
    min_token_len: usize,
    matched: bool,
}

impl HeuristicEvidence {
    fn empty() -> Self {
        Self {
            exact: false,
            fuzzy_strong: false,
            fuzzy_weak: false,
            skeleton: false,
            obfuscated: false,
            likely_clean: false,
            short_token: false,
            long_token: false,
            min_token_len: 99,
            matched: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ContextGuards {
    date_like: bool,
    numeric_noise: bool,
    code_like: bool,
    math_like: bool,
}

#[derive(Clone, Debug)]
pub struct DetectionAnalysis {
    pub matched: bool,
    pub is_profane: bool,
    pub decision: Decision,
    pub score: f32,
    pub linear: f32,
    pub surface: SurfaceSignals,
}

#[derive(Clone, Copy, Debug)]
struct DecisionInfo {
    score: f32,
    linear: f32,
    decision: Decision,
}

#[derive(Clone, Copy, Debug)]
struct DecisionSignals {
    native_exact: f32,
    exact: f32,
    fuzzy_strong: f32,
    fuzzy_weak: f32,
    skeleton: f32,
    obfuscated: f32,
    hard_separator: f32,
    leet: f32,
    digit: f32,
    hyphen_only: f32,
    apostrophe: f32,
    alpha_only: f32,
    likely_clean: f32,
    short_token: f32,
    long_token: f32,
}

#[derive(Default)]
struct AnalysisCache {
    map: HashMap<String, DetectionAnalysis>,
    order: VecDeque<String>,
}

impl AnalysisCache {
    fn get(&mut self, key: &str) -> Option<DetectionAnalysis> {
        let value = self.map.get(key).cloned()?;
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
        Some(value)
    }

    fn insert(&mut self, key: String, value: DetectionAnalysis, capacity: usize) {
        self.order.retain(|k| k != &key);
        self.map.insert(key.clone(), value);
        self.order.push_back(key);

        while self.map.len() > capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

#[derive(Clone, Debug)]
struct BloomFilter {
    bits: usize,
    hashes: usize,
    bytes: Vec<u8>,
}

impl BloomFilter {
    fn new(bits: usize, hashes: usize) -> Self {
        let bounded_bits = bits.max(1024);
        let bounded_hashes = hashes.max(2);
        Self {
            bits: bounded_bits,
            hashes: bounded_hashes,
            bytes: vec![0u8; (bounded_bits + 7) / 8],
        }
    }

    fn fnv1a32(s: &str) -> u32 {
        let mut hash: u32 = 0x811c9dc5;
        for byte in s.as_bytes() {
            hash ^= u32::from(*byte);
            hash = hash.wrapping_mul(0x01000193);
        }
        hash
    }

    fn djb2(s: &str) -> u32 {
        let mut hash: u32 = 5381;
        for byte in s.as_bytes() {
            hash = ((hash << 5).wrapping_add(hash)).wrapping_add(u32::from(*byte));
        }
        hash
    }

    fn bit_index(&self, h1: u32, h2: u32, i: usize) -> usize {
        let ii = i as u32;
        ((h1.wrapping_add(ii.wrapping_mul(h2.wrapping_add(ii)))) as usize) % self.bits
    }

    fn add(&mut self, s: &str) {
        let h1 = Self::fnv1a32(s);
        let h2 = Self::djb2(s) | 1;
        for i in 0..self.hashes {
            let idx = self.bit_index(h1, h2, i);
            self.bytes[idx >> 3] |= 1 << (idx & 7);
        }
    }

    fn has(&self, s: &str) -> bool {
        let h1 = Self::fnv1a32(s);
        let h2 = Self::djb2(s) | 1;
        for i in 0..self.hashes {
            let idx = self.bit_index(h1, h2, i);
            if (self.bytes[idx >> 3] & (1 << (idx & 7))) == 0 {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Deserialize)]
struct RawDbEntry {
    #[serde(default, rename = "match")]
    match_field: Option<String>,
    #[serde(default)]
    word: Option<serde_json::Value>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    severity: Option<i32>,
}

#[derive(Debug, Deserialize, Default)]
struct RawModerationDb {
    #[serde(default)]
    entries: Vec<RawDbEntry>,
    #[serde(default)]
    whitelist: Vec<String>,
    #[serde(default, rename = "slangMap")]
    slang_map: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawLegacyEntry {
    #[serde(default)]
    word: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    severity: Option<i32>,
}

#[derive(Debug, Deserialize, Default)]
struct DecisionModelFile {
    #[serde(default, rename = "reviewThreshold")]
    review_threshold: Option<f32>,
    #[serde(default, rename = "blockThreshold")]
    block_threshold: Option<f32>,
    #[serde(default)]
    features: Option<DecisionFeaturesFile>,
}

#[derive(Debug, Deserialize, Default)]
struct DecisionFeaturesFile {
    #[serde(default)]
    bias: Option<f32>,
    #[serde(default, rename = "nativeExact")]
    native_exact: Option<f32>,
    #[serde(default)]
    exact: Option<f32>,
    #[serde(default, rename = "fuzzyStrong")]
    fuzzy_strong: Option<f32>,
    #[serde(default, rename = "fuzzyWeak")]
    fuzzy_weak: Option<f32>,
    #[serde(default)]
    skeleton: Option<f32>,
    #[serde(default)]
    obfuscated: Option<f32>,
    #[serde(default, rename = "hardSeparator")]
    hard_separator: Option<f32>,
    #[serde(default)]
    leet: Option<f32>,
    #[serde(default)]
    digit: Option<f32>,
    #[serde(default, rename = "hyphenOnly")]
    hyphen_only: Option<f32>,
    #[serde(default)]
    apostrophe: Option<f32>,
    #[serde(default, rename = "alphaOnly")]
    alpha_only: Option<f32>,
    #[serde(default, rename = "likelyClean")]
    likely_clean: Option<f32>,
    #[serde(default, rename = "shortToken")]
    short_token: Option<f32>,
    #[serde(default, rename = "longToken")]
    long_token: Option<f32>,
}

pub struct JsParityEngine {
    min_match_length: usize,
    bad_word_set: HashSet<String>,
    bad_words_by_len_edge: HashMap<usize, HashMap<(u8, u8), Vec<String>>>,
    bad_skeletons_by_len_head: HashMap<usize, HashMap<u8, Vec<String>>>,
    whitelist_set: HashSet<String>,
    clean_word_set: HashSet<String>,
    clean_bloom: Option<BloomFilter>,
    short_acronym_set: HashSet<String>,
    aggressive_short_acronym_set: HashSet<String>,
    enable_heuristic: bool,
    use_decision_layer: bool,
    block_on_review: bool,
    enable_short_acronyms: bool,
    native_fast_path: bool,
    decision_model: DecisionModel,
    enable_vector_fallback: bool,
    vector_fallback_min_chars: usize,
    vector_fallback_min_tokens: usize,
    analysis_cache_size: usize,
    analysis_cache: Mutex<AnalysisCache>,
}

impl JsParityEngine {
    pub fn new(patterns: &[&str]) -> Self {
        let min_match_length = DEFAULT_MIN_MATCH_LENGTH;
        let profanity_root = resolve_profanity_root();
        let moderation_db_path = profanity_root
            .as_ref()
            .map(|root| root.join("src").join("database").join("moderation-db.json"));
        let legacy_external_path = profanity_root.as_ref().map(|root| {
            root.join("src")
                .join("database")
                .join("external")
                .join("merged-external.json")
        });
        let clean_lexicon_path = profanity_root
            .as_ref()
            .map(|root| root.join("Largest.list.of.english.words.txt"));
        let decision_model_path = profanity_root
            .as_ref()
            .map(|root| root.join("src").join("config").join("decision-model.json"));

        let mut bad_word_set = HashSet::<String>::new();
        let mut whitelist_set = HashSet::<String>::new();
        let mut short_acronym_set = HashSet::<String>::new();
        let mut aggressive_short_acronym_set = HashSet::<String>::new();

        Self::ingest_moderation_database_from_str(
            EMBEDDED_MODERATION_DB_JSON,
            min_match_length,
            &mut bad_word_set,
            &mut whitelist_set,
            &mut short_acronym_set,
            &mut aggressive_short_acronym_set,
        );
        if let Some(path) = moderation_db_path.as_deref() {
            Self::ingest_moderation_database(
                path,
                min_match_length,
                &mut bad_word_set,
                &mut whitelist_set,
                &mut short_acronym_set,
                &mut aggressive_short_acronym_set,
            );
        }

        let exclude_legacy_external =
            std::env::var("OMEGA_EXCLUDE_LEGACY_EXTERNAL").unwrap_or_default() == "1";
        if !exclude_legacy_external {
            Self::ingest_legacy_external_from_str(
                EMBEDDED_LEGACY_EXTERNAL_JSON,
                min_match_length,
                &mut bad_word_set,
            );
            if let Some(path) = legacy_external_path.as_deref() {
                Self::ingest_legacy_external(path, min_match_length, &mut bad_word_set);
            }
        }

        // If JS sources are unavailable, fall back to precompiled Rust patterns.
        if bad_word_set.is_empty() {
            for pattern in patterns {
                let normalized = normalize_token(pattern, false);
                if normalized.len() >= min_match_length {
                    bad_word_set.insert(normalized);
                }
            }
        }

        let (top_words, clean_bloom) = clean_lexicon_path
            .as_deref()
            .map(|path| Self::load_clean_lexicon_assets(path, DEFAULT_CLEAN_LEXICON_LIMIT))
            .unwrap_or((Vec::new(), None));
        let clean_word_set = top_words
            .into_iter()
            .map(|word| normalize_token(&word, false))
            .filter(|word| word.len() >= min_match_length && word.len() <= 24)
            .filter(|word| !bad_word_set.contains(word) && !whitelist_set.contains(word))
            .collect::<HashSet<_>>();

        let (bad_words_by_len_edge, bad_skeletons_by_len_head) =
            Self::build_bad_word_indexes(&bad_word_set, min_match_length);

        let mut decision_model = Self::load_decision_model_from_str(
            EMBEDDED_DECISION_MODEL_JSON,
            DecisionModel::default(),
        );
        if let Some(path) = decision_model_path.as_deref() {
            decision_model = Self::load_decision_model(path, decision_model);
        }

        Self {
            min_match_length,
            bad_word_set,
            bad_words_by_len_edge,
            bad_skeletons_by_len_head,
            whitelist_set,
            clean_word_set,
            clean_bloom,
            short_acronym_set,
            aggressive_short_acronym_set,
            enable_heuristic: true,
            use_decision_layer: true,
            block_on_review: false,
            enable_short_acronyms: true,
            native_fast_path: false,
            decision_model,
            enable_vector_fallback: true,
            vector_fallback_min_chars: DEFAULT_VECTOR_FALLBACK_MIN_CHARS,
            vector_fallback_min_tokens: DEFAULT_VECTOR_FALLBACK_MIN_TOKENS,
            analysis_cache_size: DEFAULT_ANALYSIS_CACHE_SIZE.clamp(0, MAX_ANALYSIS_CACHE_SIZE),
            analysis_cache: Mutex::new(AnalysisCache::default()),
        }
    }

    pub fn analyze(
        &self,
        text: &str,
        native_raw_hit: bool,
        strict_candidates: &[Candidate],
        collapsed_candidates: &[Candidate],
        merged_candidates: &[Candidate],
    ) -> DetectionAnalysis {
        let cache_eligible = text.len() >= 12 || text.chars().any(|ch| ch.is_whitespace());
        let can_use_cache = self.analysis_cache_size > 0
            && cache_eligible
            && text.len() <= MAX_ANALYSIS_CACHE_TEXT_LENGTH;
        let cache_key = if can_use_cache {
            Some(format!(
                "{}:{}",
                if self.use_decision_layer { 1 } else { 0 },
                text
            ))
        } else {
            None
        };

        if let Some(key) = &cache_key {
            if let Ok(mut cache) = self.analysis_cache.lock() {
                if let Some(hit) = cache.get(key) {
                    return hit;
                }
            }
        }

        let analysis = self.evaluate_detection(
            text,
            native_raw_hit,
            strict_candidates,
            collapsed_candidates,
            merged_candidates,
        );

        if let Some(key) = cache_key {
            if let Ok(mut cache) = self.analysis_cache.lock() {
                cache.insert(key, analysis.clone(), self.analysis_cache_size);
            }
        }

        analysis
    }

    pub fn should_skip_lexical_stage(&self, strict_candidates: &[Candidate]) -> bool {
        self.lexical_skip_reason(strict_candidates).is_some()
    }

    pub fn lexical_skip_reason(&self, strict_candidates: &[Candidate]) -> Option<String> {
        if strict_candidates.is_empty() {
            return None;
        }

        if strict_candidates
            .iter()
            .any(|candidate| Self::is_semantic_trigger_token(candidate.text.as_str()))
        {
            return None;
        }

        if strict_candidates.len() != 1 {
            return None;
        }

        let candidate = &strict_candidates[0];
        let token_len = candidate.text.len();
        if token_len < self.min_match_length || token_len > 24 {
            return None;
        }

        let token = candidate.text.as_str();
        if self.whitelist_set.contains(token) {
            return Some(format!("single-token whitelist hit '{}'", token));
        }

        if self.bad_word_set.contains(token) || candidate.obfuscated {
            return None;
        }

        if self.is_known_clean_word(token) {
            Some(format!("single-token clean-lexicon hit '{}'", token))
        } else {
            None
        }
    }

    pub fn should_run_vector_fallback(&self, text: &str, surface: SurfaceSignals) -> bool {
        if !self.enable_vector_fallback {
            return false;
        }

        let normalized = text.nfkc().collect::<String>().trim().to_string();
        if Self::contains_semantic_trigger_signal(&normalized) {
            return true;
        }

        if normalized.len() < self.vector_fallback_min_chars {
            return false;
        }

        let token_count = normalized
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .count();
        if token_count < self.vector_fallback_min_tokens {
            return false;
        }

        if surface.leet || surface.hard_separator {
            return false;
        }

        let has_link = Self::contains_link_signal(&normalized);
        let has_money = Self::contains_money_signal(&normalized);
        let has_urgency = Self::contains_urgency_signal(&normalized);

        has_link || has_money || has_urgency || token_count >= self.vector_fallback_min_tokens + 2
    }

    pub fn profanity_seed_terms(&self, max_terms: usize) -> Vec<String> {
        if max_terms == 0 {
            return Vec::new();
        }

        let mut buckets: Vec<Vec<String>> = vec![Vec::new(); 26];
        let mut misc = Vec::<String>::new();
        for word in &self.bad_word_set {
            if word.len() < self.min_match_length || word.len() > 24 {
                continue;
            }
            if self.whitelist_set.contains(word) {
                continue;
            }
            if let Some(idx) = letter_bucket_index(word) {
                buckets[idx].push(word.clone());
            } else {
                misc.push(word.clone());
            }
        }

        for bucket in &mut buckets {
            bucket.sort_unstable();
        }
        misc.sort_unstable();

        let mut out = Vec::<String>::with_capacity(max_terms);
        let mut cursor = 0usize;
        let mut progressed = true;

        while out.len() < max_terms && progressed {
            progressed = false;
            for bucket in &buckets {
                if out.len() >= max_terms {
                    break;
                }
                if cursor < bucket.len() {
                    out.push(bucket[cursor].clone());
                    progressed = true;
                }
            }
            if out.len() >= max_terms {
                break;
            }
            if cursor < misc.len() {
                out.push(misc[cursor].clone());
                progressed = true;
            }
            cursor += 1;
        }

        out
    }

    pub fn profanity_vector_candidates(
        &self,
        text: &str,
        merged_candidates: &[Candidate],
        surface: SurfaceSignals,
    ) -> Vec<String> {
        if !self.enable_vector_fallback || merged_candidates.is_empty() {
            return Vec::new();
        }

        let token_count = text
            .split_whitespace()
            .filter(|segment| !segment.is_empty())
            .count();
        if token_count > 8 && !surface.leet && !surface.hard_separator {
            return Vec::new();
        }

        let mut seen = HashSet::<String>::new();
        let mut out = Vec::<String>::new();

        for candidate in merged_candidates.iter().take(32) {
            let token = candidate.text.as_str();
            if token.len() < self.min_match_length || token.len() > 24 {
                continue;
            }
            if self.bad_word_set.contains(token) || self.whitelist_set.contains(token) {
                continue;
            }
            if !candidate.obfuscated && !surface.leet && !surface.hard_separator && !surface.digit {
                continue;
            }
            if self.is_known_clean_word(token) {
                continue;
            }

            if seen.insert(token.to_string()) {
                out.push(token.to_string());
                if out.len() >= 8 {
                    break;
                }
            }
        }

        out
    }

    fn evaluate_detection(
        &self,
        text: &str,
        native_raw_hit: bool,
        strict_candidates: &[Candidate],
        collapsed_candidates: &[Candidate],
        merged_candidates: &[Candidate],
    ) -> DetectionAnalysis {
        let surface = Self::collect_surface_signals(text);

        if Self::contains_high_risk_phrase(text) {
            return DetectionAnalysis {
                matched: true,
                is_profane: true,
                decision: Decision::Block,
                score: 1.0,
                linear: 8.0,
                surface,
            };
        }

        if self.native_fast_path
            && !self.enable_heuristic
            && !self.enable_short_acronyms
            && !self.use_decision_layer
        {
            return DetectionAnalysis {
                matched: native_raw_hit,
                is_profane: native_raw_hit,
                decision: if native_raw_hit {
                    Decision::Block
                } else {
                    Decision::Allow
                },
                score: if native_raw_hit { 1.0 } else { 0.0 },
                linear: if native_raw_hit { 1.0 } else { 0.0 },
                surface,
            };
        }

        let short_acronym_hit = self.has_short_acronym_hit(strict_candidates, surface);
        let native_hit =
            native_raw_hit && self.is_native_hit_valid(strict_candidates, collapsed_candidates);

        let skip_heuristic_on_clean_alpha = !native_hit
            && !short_acronym_hit
            && surface.alpha_only
            && !surface.digit
            && !surface.leet
            && !surface.hard_separator
            && !surface.hyphen_only
            && !surface.apostrophe
            && !strict_candidates.is_empty()
            && strict_candidates.iter().all(|candidate| {
                !candidate.obfuscated
                    && candidate.text.len() >= self.min_match_length
                    && self.is_known_clean_word(&candidate.text)
            });

        let mut heuristic = if self.enable_heuristic && !skip_heuristic_on_clean_alpha {
            self.collect_heuristic_evidence(merged_candidates)
        } else {
            HeuristicEvidence::empty()
        };

        let needs_context_guards = !native_hit && !short_acronym_hit && heuristic.matched;
        if needs_context_guards {
            let guards = Self::collect_context_guards(text);
            let should_suppress = guards.date_like
                || guards.numeric_noise
                || guards.code_like
                || (guards.math_like && !heuristic.exact && !heuristic.fuzzy_strong);
            if should_suppress {
                heuristic.exact = false;
                heuristic.fuzzy_strong = false;
                heuristic.fuzzy_weak = false;
                heuristic.skeleton = false;
                heuristic.matched = false;
            }
        }

        let matched = short_acronym_hit || native_hit || heuristic.matched;
        let signals = self.build_decision_signals(native_hit, &heuristic, surface);
        let decision_info = Self::evaluate_decision_signals(signals, self.decision_model);

        if !matched {
            return DetectionAnalysis {
                matched: false,
                is_profane: false,
                decision: Decision::Allow,
                score: decision_info.score,
                linear: decision_info.linear,
                surface,
            };
        }

        let should_block = short_acronym_hit
            || decision_info.decision == Decision::Block
            || (self.block_on_review && decision_info.decision == Decision::Review);
        let is_profane = if self.use_decision_layer {
            should_block
        } else {
            true
        };

        DetectionAnalysis {
            matched,
            is_profane,
            decision: if short_acronym_hit {
                Decision::Block
            } else {
                decision_info.decision
            },
            score: decision_info.score,
            linear: decision_info.linear,
            surface,
        }
    }

    fn is_native_hit_valid(
        &self,
        strict_candidates: &[Candidate],
        collapsed_candidates: &[Candidate],
    ) -> bool {
        for candidate in strict_candidates {
            if candidate.text.len() < self.min_match_length {
                continue;
            }
            if self.bad_word_set.contains(&candidate.text)
                && !self.whitelist_set.contains(&candidate.text)
            {
                return true;
            }
        }

        for candidate in collapsed_candidates {
            if candidate.text.len() < self.min_match_length || !candidate.obfuscated {
                continue;
            }
            if self.bad_word_set.contains(&candidate.text)
                && !self.whitelist_set.contains(&candidate.text)
            {
                return true;
            }
        }

        false
    }

    fn has_short_acronym_hit(
        &self,
        strict_candidates: &[Candidate],
        surface: SurfaceSignals,
    ) -> bool {
        if !self.enable_short_acronyms || self.short_acronym_set.is_empty() {
            return false;
        }

        for candidate in strict_candidates {
            let token = candidate.text.as_str();
            if token.len() >= self.min_match_length || token.len() < 2 {
                continue;
            }
            if self.whitelist_set.contains(token) {
                continue;
            }
            if !self.short_acronym_set.contains(token) {
                continue;
            }
            if self.aggressive_short_acronym_set.contains(token) {
                return true;
            }
            if (surface.hard_separator || surface.leet) && token.len() >= 3 {
                return true;
            }
        }

        false
    }

    fn collect_heuristic_evidence(&self, candidates: &[Candidate]) -> HeuristicEvidence {
        let mut evidence = HeuristicEvidence::empty();
        if candidates.is_empty() {
            return evidence;
        }

        let limited: Vec<&Candidate> = candidates.iter().take(128).collect();
        for candidate in &limited {
            if candidate.text.len() < self.min_match_length {
                continue;
            }
            evidence.min_token_len = evidence.min_token_len.min(candidate.text.len());
            if candidate.obfuscated {
                evidence.obfuscated = true;
            }
            if self.bad_word_set.contains(&candidate.text)
                && !self.whitelist_set.contains(&candidate.text)
            {
                evidence.exact = true;
            }
        }

        if evidence.exact {
            evidence.matched = true;
            evidence.short_token = evidence.min_token_len <= self.min_match_length + 1;
            evidence.long_token = evidence.min_token_len >= 8 && evidence.min_token_len < 99;
            return evidence;
        }

        for candidate in &limited {
            let token = candidate.text.as_str();
            if token.len() < self.min_match_length || token.len() > 24 {
                continue;
            }
            if self.whitelist_set.contains(token) {
                continue;
            }

            evidence.min_token_len = evidence.min_token_len.min(token.len());
            if candidate.obfuscated {
                evidence.obfuscated = true;
            }

            let likely_clean = self.is_known_clean_word(token);
            if likely_clean {
                evidence.likely_clean = true;
            }
            if likely_clean && !candidate.obfuscated {
                continue;
            }
            if !candidate.obfuscated && token.len() > 6 {
                continue;
            }

            let max_dist = if candidate.obfuscated {
                if token.len() <= self.min_match_length + 1 {
                    1
                } else {
                    2
                }
            } else {
                1
            };
            let min_len = self
                .min_match_length
                .max(token.len().saturating_sub(max_dist));
            let max_len = token.len() + max_dist;
            let bytes = token.as_bytes();
            let edge = (bytes[0], bytes[bytes.len() - 1]);

            for len in min_len..=max_len {
                let Some(edge_map) = self.bad_words_by_len_edge.get(&len) else {
                    continue;
                };
                let Some(bucket) = edge_map.get(&edge) else {
                    continue;
                };

                for bad_word in bucket {
                    let Some(distance) = damerau_levenshtein_limited(token, bad_word, max_dist)
                    else {
                        continue;
                    };

                    if distance <= 1 {
                        evidence.fuzzy_strong = true;
                    }
                    if distance == 2
                        && candidate.obfuscated
                        && token.len() >= self.min_match_length
                        && !likely_clean
                    {
                        evidence.fuzzy_weak = true;
                    }
                }
            }
        }

        for candidate in &limited {
            let token = candidate.text.as_str();
            if token.len() < self.min_match_length || token.len() > 24 {
                continue;
            }
            if !candidate.obfuscated || self.whitelist_set.contains(token) {
                continue;
            }

            let likely_clean = self.is_known_clean_word(token);
            if likely_clean {
                evidence.likely_clean = true;
                continue;
            }

            let skeleton = consonant_skeleton(token);
            if skeleton.len() < self.min_match_length.saturating_sub(1) {
                continue;
            }

            let max_dist = 1usize;
            let min_len = self
                .min_match_length
                .saturating_sub(1)
                .max(skeleton.len().saturating_sub(max_dist));
            let max_len = skeleton.len() + max_dist;
            let head = skeleton.as_bytes()[0];

            for len in min_len..=max_len {
                let Some(head_map) = self.bad_skeletons_by_len_head.get(&len) else {
                    continue;
                };
                let Some(bucket) = head_map.get(&head) else {
                    continue;
                };

                for bad_skeleton in bucket {
                    if damerau_levenshtein_limited(&skeleton, bad_skeleton, max_dist).is_some() {
                        evidence.skeleton = true;
                    }
                }
            }
        }

        evidence.matched =
            evidence.exact || evidence.fuzzy_strong || evidence.fuzzy_weak || evidence.skeleton;
        evidence.short_token = evidence.min_token_len <= self.min_match_length + 1;
        evidence.long_token = evidence.min_token_len >= 8 && evidence.min_token_len < 99;
        evidence
    }

    fn build_decision_signals(
        &self,
        native_hit: bool,
        evidence: &HeuristicEvidence,
        surface: SurfaceSignals,
    ) -> DecisionSignals {
        DecisionSignals {
            native_exact: bool_to_f32(native_hit),
            exact: bool_to_f32(evidence.exact),
            fuzzy_strong: bool_to_f32(evidence.fuzzy_strong),
            fuzzy_weak: bool_to_f32(evidence.fuzzy_weak),
            skeleton: bool_to_f32(evidence.skeleton),
            obfuscated: bool_to_f32(evidence.obfuscated),
            hard_separator: bool_to_f32(surface.hard_separator),
            leet: bool_to_f32(surface.leet),
            digit: bool_to_f32(surface.digit),
            hyphen_only: bool_to_f32(surface.hyphen_only),
            apostrophe: bool_to_f32(surface.apostrophe),
            alpha_only: bool_to_f32(surface.alpha_only),
            likely_clean: if native_hit {
                0.0
            } else {
                bool_to_f32(evidence.likely_clean)
            },
            short_token: bool_to_f32(evidence.short_token),
            long_token: bool_to_f32(evidence.long_token),
        }
    }

    fn evaluate_decision_signals(signals: DecisionSignals, model: DecisionModel) -> DecisionInfo {
        let w = model.features;
        let linear = w.bias
            + signals.native_exact * w.native_exact
            + signals.exact * w.exact
            + signals.fuzzy_strong * w.fuzzy_strong
            + signals.fuzzy_weak * w.fuzzy_weak
            + signals.skeleton * w.skeleton
            + signals.obfuscated * w.obfuscated
            + signals.hard_separator * w.hard_separator
            + signals.leet * w.leet
            + signals.digit * w.digit
            + signals.hyphen_only * w.hyphen_only
            + signals.apostrophe * w.apostrophe
            + signals.alpha_only * w.alpha_only
            + signals.likely_clean * w.likely_clean
            + signals.short_token * w.short_token
            + signals.long_token * w.long_token;

        let score = sigmoid(linear);
        let decision = if score >= model.block_threshold {
            Decision::Block
        } else if score >= model.review_threshold {
            Decision::Review
        } else {
            Decision::Allow
        };

        DecisionInfo {
            score,
            linear,
            decision,
        }
    }

    fn is_known_clean_word(&self, token: &str) -> bool {
        if self.bad_word_set.contains(token) || self.whitelist_set.contains(token) {
            return false;
        }
        if self.clean_word_set.contains(token) {
            return true;
        }
        if token.len() < self.min_match_length {
            return false;
        }
        self.clean_bloom
            .as_ref()
            .is_some_and(|bloom| bloom.has(token))
    }

    fn ingest_moderation_database(
        path: &Path,
        min_match_length: usize,
        bad_word_set: &mut HashSet<String>,
        whitelist_set: &mut HashSet<String>,
        short_acronym_set: &mut HashSet<String>,
        aggressive_short_acronym_set: &mut HashSet<String>,
    ) {
        let Ok(raw) = fs::read_to_string(path) else {
            return;
        };
        Self::ingest_moderation_database_from_str(
            &raw,
            min_match_length,
            bad_word_set,
            whitelist_set,
            short_acronym_set,
            aggressive_short_acronym_set,
        );
    }

    fn ingest_moderation_database_from_str(
        raw: &str,
        min_match_length: usize,
        bad_word_set: &mut HashSet<String>,
        whitelist_set: &mut HashSet<String>,
        short_acronym_set: &mut HashSet<String>,
        aggressive_short_acronym_set: &mut HashSet<String>,
    ) {
        let raw_without_bom = raw.trim_start_matches('\u{feff}');
        let Ok(db) = serde_json::from_str::<RawModerationDb>(raw_without_bom) else {
            return;
        };

        for entry in db.entries {
            let severity = entry.severity.unwrap_or(3);
            if severity < 2 {
                continue;
            }

            let tags = entry
                .tags
                .iter()
                .map(|t| t.to_ascii_lowercase())
                .collect::<HashSet<_>>();
            let category = entry
                .category
                .as_deref()
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let has_short_tag = tags
                .iter()
                .any(|tag| SHORT_ACRONYM_TAGS.contains(&tag.as_str()));
            let is_short = is_short_acronym_entry(&category, &tags);
            let is_aggressive = has_short_tag
                && tags
                    .iter()
                    .any(|tag| AGGRESSIVE_SHORT_TAGS.contains(&tag.as_str()));

            let variants = entry_words(&entry);
            for variant in variants {
                let normalized = normalize_token(&variant, false);
                if normalized.is_empty() {
                    continue;
                }

                if normalized.len() >= min_match_length {
                    bad_word_set.insert(normalized);
                    continue;
                }

                if normalized.len() >= 2 && is_short {
                    short_acronym_set.insert(normalized.clone());
                    if is_aggressive {
                        aggressive_short_acronym_set.insert(normalized);
                    }
                }
            }
        }

        for word in db.whitelist {
            let normalized = normalize_token(&word, false);
            if !normalized.is_empty() {
                whitelist_set.insert(normalized);
            }
        }

        for (abbr, _) in db.slang_map {
            let normalized = normalize_token(&abbr, false);
            if normalized.len() >= min_match_length {
                bad_word_set.insert(normalized);
            }
        }
    }

    fn ingest_legacy_external(
        path: &Path,
        min_match_length: usize,
        bad_word_set: &mut HashSet<String>,
    ) {
        let Ok(raw) = fs::read_to_string(path) else {
            return;
        };
        Self::ingest_legacy_external_from_str(&raw, min_match_length, bad_word_set);
    }

    fn ingest_legacy_external_from_str(
        raw: &str,
        min_match_length: usize,
        bad_word_set: &mut HashSet<String>,
    ) {
        let Ok(entries) = serde_json::from_str::<Vec<RawLegacyEntry>>(raw) else {
            return;
        };

        for entry in entries {
            let is_english = match entry.lang.as_deref() {
                Some(lang) => lang.eq_ignore_ascii_case("en"),
                None => true,
            };
            if !is_english {
                continue;
            }

            let severity = entry.severity.unwrap_or(3).clamp(1, 4);
            if severity < 2 {
                continue;
            }

            let Some(word) = entry.word else {
                continue;
            };
            let normalized = normalize_token(&word, false);
            if normalized.len() >= min_match_length {
                bad_word_set.insert(normalized);
            }
        }
    }

    fn load_clean_lexicon_assets(path: &Path, limit: usize) -> (Vec<String>, Option<BloomFilter>) {
        let bounded_limit = limit.clamp(0, MAX_CLEAN_LEXICON_LIMIT);
        if bounded_limit == 0 || !path.exists() {
            return (Vec::new(), None);
        }

        let Ok(raw_text) = fs::read_to_string(path) else {
            return (Vec::new(), None);
        };

        let per_bucket_limit = ((bounded_limit + 25) / 26) + 512;
        let mut buckets: Vec<Vec<String>> = vec![Vec::new(); 26];
        let mut bloom = BloomFilter::new(DEFAULT_BLOOM_BITS, DEFAULT_BLOOM_HASHES);
        let mut top_seen = HashSet::new();

        for line in raw_text.lines() {
            let Some(normalized) = normalize_lexicon_word(line) else {
                continue;
            };

            bloom.add(&normalized);
            let Some(idx) = letter_bucket_index(&normalized) else {
                continue;
            };
            if buckets[idx].len() >= per_bucket_limit || top_seen.contains(&normalized) {
                continue;
            }

            buckets[idx].push(normalized.clone());
            top_seen.insert(normalized);
        }

        let mut top_words = Vec::with_capacity(bounded_limit);
        let mut progressed = true;
        let mut cursor = 0usize;

        while top_words.len() < bounded_limit && progressed {
            progressed = false;
            for bucket in &buckets {
                if top_words.len() >= bounded_limit {
                    break;
                }
                if cursor < bucket.len() {
                    top_words.push(bucket[cursor].clone());
                    progressed = true;
                }
            }
            cursor += 1;
        }

        (top_words, Some(bloom))
    }

    fn build_bad_word_indexes(
        bad_word_set: &HashSet<String>,
        min_match_length: usize,
    ) -> (
        HashMap<usize, HashMap<(u8, u8), Vec<String>>>,
        HashMap<usize, HashMap<u8, Vec<String>>>,
    ) {
        let mut by_len_edge = HashMap::<usize, HashMap<(u8, u8), Vec<String>>>::new();
        let mut skeletons = HashMap::<usize, HashMap<u8, Vec<String>>>::new();

        for word in bad_word_set {
            let bytes = word.as_bytes();
            if bytes.len() < min_match_length {
                continue;
            }

            by_len_edge
                .entry(bytes.len())
                .or_default()
                .entry((bytes[0], bytes[bytes.len() - 1]))
                .or_default()
                .push(word.clone());

            let skeleton = consonant_skeleton(word);
            if skeleton.is_empty() || skeleton.len() < min_match_length.saturating_sub(1) {
                continue;
            }

            let head = skeleton.as_bytes()[0];
            skeletons
                .entry(skeleton.len())
                .or_default()
                .entry(head)
                .or_default()
                .push(skeleton);
        }

        (by_len_edge, skeletons)
    }

    fn load_decision_model(path: &Path, base: DecisionModel) -> DecisionModel {
        let Ok(raw) = fs::read_to_string(path) else {
            return base;
        };
        Self::load_decision_model_from_str(&raw, base)
    }

    fn load_decision_model_from_str(raw: &str, mut model: DecisionModel) -> DecisionModel {
        let Ok(parsed) = serde_json::from_str::<DecisionModelFile>(raw) else {
            return model;
        };

        if let Some(review) = parsed.review_threshold {
            model.review_threshold = review.clamp(0.0, 1.0);
        }
        if let Some(block) = parsed.block_threshold {
            model.block_threshold = block.clamp(0.0, 1.0);
        }
        if model.block_threshold < model.review_threshold {
            std::mem::swap(&mut model.block_threshold, &mut model.review_threshold);
        }

        if let Some(features) = parsed.features {
            apply_feature(&mut model.features.bias, features.bias);
            apply_feature(&mut model.features.native_exact, features.native_exact);
            apply_feature(&mut model.features.exact, features.exact);
            apply_feature(&mut model.features.fuzzy_strong, features.fuzzy_strong);
            apply_feature(&mut model.features.fuzzy_weak, features.fuzzy_weak);
            apply_feature(&mut model.features.skeleton, features.skeleton);
            apply_feature(&mut model.features.obfuscated, features.obfuscated);
            apply_feature(&mut model.features.hard_separator, features.hard_separator);
            apply_feature(&mut model.features.leet, features.leet);
            apply_feature(&mut model.features.digit, features.digit);
            apply_feature(&mut model.features.hyphen_only, features.hyphen_only);
            apply_feature(&mut model.features.apostrophe, features.apostrophe);
            apply_feature(&mut model.features.alpha_only, features.alpha_only);
            apply_feature(&mut model.features.likely_clean, features.likely_clean);
            apply_feature(&mut model.features.short_token, features.short_token);
            apply_feature(&mut model.features.long_token, features.long_token);
        }

        model
    }

    fn collect_surface_signals(text: &str) -> SurfaceSignals {
        let source = text.to_string();
        let normalized = source.nfkc().collect::<String>();

        let has_digit = normalized.chars().any(|ch| ch.is_ascii_digit());
        let has_leet = normalized
            .chars()
            .any(|ch| matches!(ch, '@' | '$' | '!' | '|' | '+'));
        let has_letters = normalized.chars().any(|ch| ch.is_ascii_alphabetic());
        let has_dot_slash = normalized.chars().any(|ch| matches!(ch, '.' | '/' | '\\'));
        let has_hard_separator = normalized
            .chars()
            .any(|ch| matches!(ch, '#' | '@' | '$' | '_'))
            || source
                .chars()
                .any(|ch| matches!(ch, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'))
            || (has_dot_slash && has_letters);
        let has_hyphen = normalized.chars().any(|ch| ch == '-');
        let has_apostrophe = normalized.chars().any(|ch| ch == '\'' || ch == '\u{2019}');
        let alpha_only =
            !normalized.is_empty() && normalized.chars().all(|ch| ch.is_ascii_alphabetic());
        let hyphen_only = has_hyphen && !has_digit && !has_leet && !has_hard_separator;

        SurfaceSignals {
            digit: has_digit,
            leet: has_leet,
            hard_separator: has_hard_separator,
            hyphen_only,
            apostrophe: has_apostrophe,
            alpha_only,
        }
    }

    fn collect_context_guards(text: &str) -> ContextGuards {
        let normalized = text.nfkc().collect::<String>().trim().to_string();
        let letters = normalized
            .chars()
            .filter(|ch| ch.is_ascii_alphabetic())
            .count();
        let digits = normalized.chars().filter(|ch| ch.is_ascii_digit()).count();
        let punct = normalized
            .chars()
            .filter(|ch| !ch.is_ascii_alphanumeric() && !ch.is_whitespace())
            .count();

        let date_like = is_date_like(&normalized);
        let numeric_noise = (digits >= 3 && letters == 0) || (digits >= 4 && digits >= letters * 2);
        let code_like = normalized.len() >= 4
            && digits > 0
            && punct > 0
            && normalized
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '/' | '-'));
        let math_like =
            has_math_function_signal(&normalized) || is_math_expression_like(&normalized);

        ContextGuards {
            date_like,
            numeric_noise,
            code_like,
            math_like,
        }
    }

    fn contains_link_signal(normalized: &str) -> bool {
        let lowered = normalized.to_ascii_lowercase();
        if lowered.contains("http://")
            || lowered.contains("https://")
            || lowered.contains("t.me/")
            || lowered.contains("bit.ly/")
            || lowered.contains("tinyurl.com/")
            || lowered.contains("wa.me/")
        {
            return true;
        }

        let bytes = lowered.as_bytes();
        let mut idx = 0usize;
        while idx < bytes.len() {
            if bytes[idx] == b'@' {
                let mut run = 0usize;
                let mut j = idx + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    run += 1;
                    j += 1;
                }
                if run >= 4 {
                    return true;
                }
                idx = j;
                continue;
            }
            idx += 1;
        }

        false
    }

    fn contains_money_signal(normalized: &str) -> bool {
        if normalized
            .chars()
            .any(|ch| matches!(ch, '$' | '€' | '£' | '₽' | '₴'))
        {
            return true;
        }

        let tokens = ascii_tokens(&normalized.to_ascii_lowercase());
        tokens
            .iter()
            .any(|token| MONEY_TOKENS.contains(&token.as_str()))
    }

    fn contains_urgency_signal(normalized: &str) -> bool {
        let tokens = ascii_tokens(&normalized.to_ascii_lowercase());
        tokens
            .iter()
            .any(|token| URGENCY_TOKENS.contains(&token.as_str()))
    }

    fn is_semantic_trigger_token(token: &str) -> bool {
        SEMANTIC_TRIGGER_TOKENS.contains(&token)
    }

    fn contains_semantic_trigger_signal(normalized: &str) -> bool {
        let lowered = normalized.to_ascii_lowercase();
        let tokens = ascii_tokens(&lowered);
        if tokens
            .iter()
            .any(|token| Self::is_semantic_trigger_token(token.as_str()))
        {
            return true;
        }
        Self::contains_high_risk_phrase(&lowered)
    }

    fn contains_high_risk_phrase(text: &str) -> bool {
        let lowered = text.nfkc().collect::<String>().to_ascii_lowercase();
        let padded = format!(" {} ", lowered);
        HIGH_RISK_PHRASES.iter().any(|phrase| {
            let needle = format!(" {} ", phrase);
            padded.contains(&needle)
        })
    }
}

fn resolve_profanity_root() -> Option<PathBuf> {
    let mut candidates = Vec::<PathBuf>::new();

    if let Ok(raw) = std::env::var("OMEGA_PROFANITY_ROOT") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("profanity-destroyer"));
        let mut cursor = cwd;
        for _ in 0..4 {
            let Some(parent) = cursor.parent() else {
                break;
            };
            let parent_buf = parent.to_path_buf();
            candidates.push(parent_buf.join("profanity-destroyer"));
            cursor = parent_buf;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut cursor = exe_dir.to_path_buf();
            candidates.push(cursor.join("profanity-destroyer"));
            for _ in 0..6 {
                let Some(parent) = cursor.parent() else {
                    break;
                };
                let parent_buf = parent.to_path_buf();
                candidates.push(parent_buf.join("profanity-destroyer"));
                cursor = parent_buf;
            }
        }
    }

    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("profanity-destroyer"),
    );

    let mut seen = HashSet::<String>::new();
    for candidate in candidates {
        let key = candidate.to_string_lossy().to_string();
        if !seen.insert(key) {
            continue;
        }
        if candidate
            .join("src")
            .join("database")
            .join("moderation-db.json")
            .exists()
        {
            return Some(candidate);
        }
    }

    None
}

fn bool_to_f32(v: bool) -> f32 {
    if v {
        1.0
    } else {
        0.0
    }
}

fn apply_feature(dst: &mut f32, src: Option<f32>) {
    if let Some(value) = src {
        *dst = value;
    }
}

fn sigmoid(x: f32) -> f32 {
    if x >= 0.0 {
        let z = (-x).exp();
        1.0 / (1.0 + z)
    } else {
        let z = x.exp();
        z / (1.0 + z)
    }
}

fn is_short_acronym_entry(category: &str, tags: &HashSet<String>) -> bool {
    SHORT_ACRONYM_TAGS.contains(&category)
        || tags
            .iter()
            .any(|tag| SHORT_ACRONYM_TAGS.contains(&tag.as_str()))
}

fn entry_words(entry: &RawDbEntry) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(match_field) = &entry.match_field {
        out.extend(expand_match_variants(match_field));
    }

    if let Some(word) = &entry.word {
        match word {
            serde_json::Value::String(s) => out.extend(expand_match_variants(s)),
            serde_json::Value::Array(items) => {
                for item in items {
                    if let serde_json::Value::String(s) = item {
                        out.extend(expand_match_variants(s));
                    }
                }
            }
            _ => {}
        }
    }

    out
}

fn expand_match_variants(raw: &str) -> Vec<String> {
    raw.split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_lexicon_word(raw: &str) -> Option<String> {
    let compact = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_lowercase())
        .collect::<String>();
    if compact.len() < 3 || compact.len() > 24 {
        return None;
    }
    Some(compact)
}

fn letter_bucket_index(word: &str) -> Option<usize> {
    let first = *word.as_bytes().first()?;
    if !first.is_ascii_lowercase() {
        return None;
    }
    Some((first - b'a') as usize)
}

fn consonant_skeleton(text: &str) -> String {
    let compact = text
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    if compact.is_empty() {
        return String::new();
    }
    let skeleton = compact
        .chars()
        .filter(|ch| !matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u'))
        .collect::<String>();
    if skeleton.len() >= 2 {
        skeleton
    } else {
        compact
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

fn ascii_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn is_date_like(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    let mut part_len = 0usize;
    let mut separators = 0usize;
    let mut last_was_sep = false;

    for ch in text.chars() {
        if ch.is_ascii_digit() {
            if part_len >= 4 {
                return false;
            }
            part_len += 1;
            last_was_sep = false;
            continue;
        }

        if matches!(ch, '.' | '/' | '-') {
            if part_len == 0 || last_was_sep {
                return false;
            }
            separators += 1;
            if separators > 3 {
                return false;
            }
            part_len = 0;
            last_was_sep = true;
            continue;
        }

        return false;
    }

    separators >= 1 && part_len >= 1 && part_len <= 4 && !last_was_sep
}

fn has_math_function_signal(normalized: &str) -> bool {
    let lowered = normalized.to_ascii_lowercase();
    for term in [
        "sin", "cos", "tan", "cot", "sec", "csc", "log", "ln", "sqrt",
    ] {
        let mut cursor = 0usize;
        while let Some(rel_idx) = lowered[cursor..].find(term) {
            let idx = cursor + rel_idx;
            let before_ok = idx == 0 || !lowered.as_bytes()[idx - 1].is_ascii_alphabetic();
            let after = idx + term.len();
            let after_ok =
                after >= lowered.len() || !lowered.as_bytes()[after].is_ascii_alphabetic();
            if before_ok && after_ok {
                return true;
            }
            cursor = idx + 1;
        }
    }
    false
}

fn is_math_expression_like(normalized: &str) -> bool {
    if normalized.is_empty() {
        return false;
    }

    let mut has_operator = false;
    for ch in normalized.chars() {
        let allowed = ch.is_ascii_digit()
            || matches!(
                ch,
                '+' | '-' | '*' | '/' | '^' | '(' | ')' | '.' | ',' | '='
            )
            || ch.is_whitespace();
        if !allowed {
            return false;
        }
        if matches!(ch, '+' | '-' | '*' | '/' | '^' | '=' | '(' | ')') {
            has_operator = true;
        }
    }
    has_operator
}

#[cfg(test)]
mod tests {
    use crate::{dfa_fast_path::DfaEngine, simd_preprocessor::SimdBuffer};

    use super::JsParityEngine;

    #[test]
    fn detects_masked_obfuscated_slur() {
        let patterns = ["nigga", "nigger"];
        let dfa = DfaEngine::new(&patterns);
        let parity = JsParityEngine::new(&patterns);

        let text = "n###gga";
        let native_raw_hit = dfa.scan(text);

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(text);

        let analysis = parity.analyze(
            text,
            native_raw_hit,
            buffer.strict_candidates(),
            buffer.collapsed_candidates(),
            buffer.merged_candidates(),
        );

        assert!(analysis.matched);
        assert!(analysis.is_profane);
    }

    #[test]
    fn detects_short_acronym_ng() {
        let patterns = ["nigga", "nigger"];
        let dfa = DfaEngine::new(&patterns);
        let parity = JsParityEngine::new(&patterns);

        let text = "ng";
        let native_raw_hit = dfa.scan(text);

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(text);

        let analysis = parity.analyze(
            text,
            native_raw_hit,
            buffer.strict_candidates(),
            buffer.collapsed_candidates(),
            buffer.merged_candidates(),
        );

        assert!(analysis.matched);
        assert!(analysis.is_profane);
    }

    #[test]
    fn detects_pusssie_variant() {
        let patterns = ["pussy"];
        let dfa = DfaEngine::new(&patterns);
        let parity = JsParityEngine::new(&patterns);

        let text = "pusssie";
        let native_raw_hit = dfa.scan(text);

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(text);

        let analysis = parity.analyze(
            text,
            native_raw_hit,
            buffer.strict_candidates(),
            buffer.collapsed_candidates(),
            buffer.merged_candidates(),
        );

        assert!(analysis.matched);
        assert!(analysis.is_profane);
    }

    #[test]
    fn allows_clean_text() {
        let patterns = ["nigga", "nigger"];
        let dfa = DfaEngine::new(&patterns);
        let parity = JsParityEngine::new(&patterns);

        let text = "hello friendly world";
        let native_raw_hit = dfa.scan(text);

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(text);

        let analysis = parity.analyze(
            text,
            native_raw_hit,
            buffer.strict_candidates(),
            buffer.collapsed_candidates(),
            buffer.merged_candidates(),
        );

        assert!(!analysis.matched);
        assert!(!analysis.is_profane);
    }

    #[test]
    fn does_not_skip_lexical_stage_when_trigger_word_present() {
        let patterns = ["nigga", "nigger"];
        let parity = JsParityEngine::new(&patterns);

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text("just die");

        let reason = parity.lexical_skip_reason(buffer.strict_candidates());
        assert!(reason.is_none());
    }

    #[test]
    fn enables_vector_fallback_for_trigger_words() {
        let patterns = ["nigga", "nigger"];
        let parity = JsParityEngine::new(&patterns);
        let analysis = parity.analyze("just die", false, &[], &[], &[]);

        assert!(parity.should_run_vector_fallback("just die", analysis.surface));
    }

    #[test]
    fn blocks_high_risk_phrase_even_without_raw_hit() {
        let patterns = ["nigga", "nigger"];
        let parity = JsParityEngine::new(&patterns);
        let dfa = DfaEngine::new(&patterns);
        let text = "just end yourself finally";

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(text);
        let analysis = parity.analyze(
            text,
            dfa.scan(text),
            buffer.strict_candidates(),
            buffer.collapsed_candidates(),
            buffer.merged_candidates(),
        );

        assert!(analysis.matched);
        assert!(analysis.is_profane);
    }
}
