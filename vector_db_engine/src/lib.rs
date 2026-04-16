pub mod dfa_fast_path;
pub mod disruptor;
pub mod js_parity;
pub mod l2_semantic;
pub mod simd_preprocessor;

use dfa_fast_path::DfaEngine;
use js_parity::JsParityEngine;
use l2_semantic::SemanticEngine;
use simd_preprocessor::SimdBuffer;

/// The primary engine structure holding our Fast-Path and pointers to the Smart-Path.
pub struct ModerationEngine {
    dfa: DfaEngine,
    parity: JsParityEngine,
    semantic: SemanticEngine,
}

impl ModerationEngine {
    pub async fn new(
        patterns: &[&str],
        model_path: &str,
        tokenizer_path: &str,
        qdrant_url: &str,
        collection_name: &str,
    ) -> Self {
        let semantic = SemanticEngine::new(model_path, tokenizer_path, qdrant_url, collection_name)
            .await
            .expect("Failed to initialize L2 Semantic Engine");
        let parity = JsParityEngine::new(patterns);

        let profanity_seed_limit = std::env::var("OMEGA_PROFANITY_VECTOR_SEED_LIMIT")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .map(|limit| limit.clamp(0, 25_000))
            .unwrap_or(2_000);
        if profanity_seed_limit > 0 {
            let seed_terms = parity.profanity_seed_terms(profanity_seed_limit);
            if !seed_terms.is_empty() {
                match semantic.bootstrap_profanity_lexicon(&seed_terms).await {
                    Ok(inserted) => {
                        println!(
                            "🧠 Seeded {} profanity vectors for 80% semantic matching.",
                            inserted
                        );
                    }
                    Err(err) => {
                        eprintln!("⚠️ Failed to seed profanity vectors: {}", err);
                    }
                }
            }
        }

        Self {
            dfa: DfaEngine::new(patterns),
            parity,
            semantic,
        }
    }

    /// Dynamically train the Neural Network from the chat interface
    pub async fn train_payload(&self, text: &str) -> Result<(), String> {
        self.semantic.train_payload(text).await
    }

    /// Process a string through the hybrid L1 (DFA) + L2 (ONNX + Qdrant) path.
    /// Returns `true` if it violated L1 or matched a scam vector in L2.
    #[inline(always)]
    pub async fn check_payload(&self, payload: &str) -> bool {
        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(payload);

        if self
            .parity
            .should_skip_lexical_stage(buffer.strict_candidates())
        {
            return false;
        }

        // Step 1: Evaluate lexical pipeline in the same order as JS detector.
        let native_raw_hit = self.dfa.scan(payload);

        let analysis = self.parity.analyze(
            payload,
            native_raw_hit,
            buffer.strict_candidates(),
            buffer.collapsed_candidates(),
            buffer.merged_candidates(),
        );
        if analysis.matched {
            return analysis.is_profane;
        }

        // Step 2: Probe semantic profanity similarity (>= 0.80) for unresolved obfuscated tokens.
        let profanity_candidates = self.parity.profanity_vector_candidates(
            payload,
            buffer.merged_candidates(),
            analysis.surface,
        );
        if !profanity_candidates.is_empty()
            && self
                .semantic
                .scan_profanity_candidates(&profanity_candidates)
                .await
        {
            return true;
        }

        // Step 3: Run broad vector fallback only when lexical path does not match
        // and the same JS-like gate conditions are met.
        if self
            .parity
            .should_run_vector_fallback(payload, analysis.surface)
        {
            return self.semantic.scan_semantic(payload).await;
        }

        false
    }
}
