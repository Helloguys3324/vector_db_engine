pub mod simd_preprocessor;
pub mod dfa_fast_path;
pub mod disruptor;
pub mod l2_semantic;

use simd_preprocessor::SimdBuffer;
use dfa_fast_path::DfaEngine;
use l2_semantic::SemanticEngine;

/// The primary engine structure holding our Fast-Path and pointers to the Smart-Path.
pub struct ModerationEngine {
    dfa: DfaEngine,
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

        Self {
            dfa: DfaEngine::new(patterns),
            semantic,
        }
    }

    /// Process a string through the hybrid L1 (DFA) + L2 (ONNX + Qdrant) path.
    /// Returns `true` if it violated L1 or matched a scam vector in L2.
    #[inline(always)]
    pub async fn check_payload(&self, payload: &str) -> bool {
        // Step 1: Normalize through SIMD
        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(payload);
        
        let processed_view = buffer.as_str();

        // Step 2: Branchless DFA (Local O(N))
        if self.dfa.scan(processed_view) {
            return true; // Malicious detected in Fast Path
        }

        // Step 3: Semantic Path (Local ONNX + gRPC Qdrant)
        self.semantic.scan_semantic(processed_view).await
    }
}
