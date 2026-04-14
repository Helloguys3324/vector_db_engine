pub mod simd_preprocessor;
pub mod dfa_fast_path;
pub mod disruptor;
pub mod l2_semantic;

use simd_preprocessor::SimdBuffer;
use dfa_fast_path::DfaEngine;
use disruptor::HandoffQueue;

/// The primary engine structure holding our Fast-Path and pointers to the Smart-Path.
pub struct ModerationEngine {
    dfa: DfaEngine,
    queue: HandoffQueue,
}

impl ModerationEngine {
    pub fn new(patterns: &[&str], queue_capacity: usize, model_path: &str, tokenizer_path: &str) -> Self {
        Self {
            dfa: DfaEngine::new(patterns),
            queue: HandoffQueue::new(queue_capacity),
        }
    }

    /// Process a string. Returns `true` if it violated L1 (Fast Path) logic immediately.
    /// If it passes L1, it is handed off to L2 via the lock-free disruptor ring buffer.
    /// This function has zero heap allocations on the hot path.
    #[inline(always)]
    pub fn check_payload(&self, payload: &str) -> bool {
        // Step 1: Normalize through SIMD
        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(payload);
        
        let processed_view = buffer.as_str();

        // Step 2: Branchless DFA
        if self.dfa.scan(processed_view) {
            return true; // Malicious detected in Fast Path
        }

        // Step 3: Handoff to Semantic Path
        self.queue.enqueue(processed_view);

        false // Passed L1, pending L2 processing
    }
}
