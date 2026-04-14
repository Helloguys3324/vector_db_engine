use aho_corasick::{AhoCorasick, MatchKind};

/// Production Aho-Corasick Deterministic Finite Automaton.
/// Wrapping the highly optimized `aho-corasick` crate.
pub struct DfaEngine {
    ac: AhoCorasick,
}

impl DfaEngine {
    pub fn new(patterns: &[&str]) -> Self {
        // Build the AC state machine. We use MatchKind::Standard to return the first match immediately.
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .ascii_case_insensitive(true)
            .build(patterns)
            .expect("Failed to build Aho-Corasick automaton");
            
        Self { ac }
    }

    /// Pure branchless-optimized hardware evaluation in AC crate.
    #[inline(always)]
    pub fn scan(&self, text: &str) -> bool {
        // is_match is short-circuiting and extremely fast
        self.ac.is_match(text)
    }
}
