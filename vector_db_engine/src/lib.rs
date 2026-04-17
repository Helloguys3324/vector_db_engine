pub mod dfa_fast_path;
pub mod disruptor;
pub mod js_parity;
pub mod l2_semantic;
pub mod simd_preprocessor;

use dfa_fast_path::DfaEngine;
use js_parity::JsParityEngine;
use l2_semantic::SemanticEngine;
use simd_preprocessor::{normalize_token, Candidate, SimdBuffer};

/// The primary engine structure holding our Fast-Path and pointers to the Smart-Path.
pub struct ModerationEngine {
    dfa: DfaEngine,
    parity: JsParityEngine,
    semantic: Option<SemanticEngine>,
    trace_enabled: bool,
}

impl ModerationEngine {
    pub async fn new(
        patterns: &[&str],
        model_path: &str,
        tokenizer_path: &str,
        qdrant_url: &str,
        collection_name: &str,
    ) -> Self {
        let semantic = match SemanticEngine::new(
            model_path,
            tokenizer_path,
            qdrant_url,
            collection_name,
        )
        .await
        {
            Ok(engine) => Some(engine),
            Err(err) => {
                eprintln!(
                    "⚠️ Failed to initialize L2 Semantic Engine. Running L1-only mode. Details: {}",
                    err
                );
                None
            }
        };
        let parity = JsParityEngine::new(patterns);
        let trace_enabled = moderation_trace_enabled();

        let profanity_seed_limit = std::env::var("OMEGA_PROFANITY_VECTOR_SEED_LIMIT")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .map(|limit| limit.clamp(0, 25_000))
            .unwrap_or(2_000);
        if profanity_seed_limit > 0 {
            let seed_terms = parity.profanity_seed_terms(profanity_seed_limit);
            if !seed_terms.is_empty() {
                if let Some(semantic) = &semantic {
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
                } else {
                    eprintln!("⚠️ Skipping profanity seed bootstrap because L2 is disabled.");
                }
            }
        }

        Self {
            dfa: DfaEngine::new(patterns),
            parity,
            semantic,
            trace_enabled,
        }
    }

    /// Dynamically train the Neural Network from the chat interface
    pub async fn train_payload(&self, text: &str) -> Result<(), String> {
        if let Some(semantic) = &self.semantic {
            semantic.train_payload(text).await
        } else {
            Err("L2 semantic engine is disabled (missing model/tokenizer).".to_string())
        }
    }

    /// Process a string through the hybrid L1 (DFA) + L2 (ONNX + Qdrant) path.
    /// Returns `true` if it violated L1 or matched a scam vector in L2.
    #[inline(always)]
    pub async fn check_payload(&self, payload: &str) -> bool {
        if self.trace_enabled {
            println!("\n=== [moderation-trace] incoming payload ===");
            println!("[input] '{}'", shorten_for_log(payload, 500));
            log_word_stages(payload);
        }

        let mut buffer = SimdBuffer::new();
        buffer.normalize_adversarial_text(payload);

        if self.trace_enabled {
            println!(
                "[simd] normalized_with_spaces='{}'",
                shorten_for_log(buffer.as_str(), 500)
            );
            println!(
                "[simd] strict(count={}): {}",
                buffer.strict_candidates().len(),
                format_candidates(buffer.strict_candidates())
            );
            println!(
                "[simd] collapsed(count={}): {}",
                buffer.collapsed_candidates().len(),
                format_candidates(buffer.collapsed_candidates())
            );
            println!(
                "[simd] merged(count={}): {}",
                buffer.merged_candidates().len(),
                format_candidates(buffer.merged_candidates())
            );
        }

        let skip_reason = self.parity.lexical_skip_reason(buffer.strict_candidates());
        if let Some(reason) = skip_reason {
            if self.trace_enabled {
                println!("[l1-skip] true ({})", reason);
                println!("[final] decision=ALLOW reason=lexical_skip");
            }
            return false;
        }
        if self.trace_enabled {
            println!("[l1-skip] false");
        }

        // Step 1: Evaluate lexical pipeline in the same order as JS detector.
        let native_raw_hit = self.dfa.scan(payload) || self.dfa.scan(buffer.as_str());
        if self.trace_enabled {
            println!("[dfa] native_raw_hit={}", native_raw_hit);
        }

        let analysis = self.parity.analyze(
            payload,
            native_raw_hit,
            buffer.strict_candidates(),
            buffer.collapsed_candidates(),
            buffer.merged_candidates(),
        );
        if self.trace_enabled {
            println!(
                "[parity] matched={} is_profane={} decision={:?} score={:.4} linear={:.4}",
                analysis.matched,
                analysis.is_profane,
                analysis.decision,
                analysis.score,
                analysis.linear
            );
            println!(
                "[parity.surface] digit={} leet={} hard_separator={} hyphen_only={} apostrophe={} alpha_only={}",
                analysis.surface.digit,
                analysis.surface.leet,
                analysis.surface.hard_separator,
                analysis.surface.hyphen_only,
                analysis.surface.apostrophe,
                analysis.surface.alpha_only
            );
        }
        if analysis.matched {
            if self.trace_enabled {
                println!(
                    "[final] decision={} reason=lexical_match",
                    if analysis.is_profane {
                        "BLOCK"
                    } else {
                        "ALLOW"
                    }
                );
            }
            return analysis.is_profane;
        }

        if let Some(context_reason) = self
            .parity
            .contextual_whitelist_phrase_reason(payload, buffer.strict_candidates())
        {
            if self.trace_enabled {
                println!(
                    "[pre-l2.context] matched contextual whitelist phrase '{}'",
                    context_reason
                );
                println!("[final] decision=BLOCK reason=contextual_whitelist_phrase");
            }
            return true;
        }

        // Step 2: Probe semantic profanity similarity (>= 0.80) for unresolved obfuscated tokens.
        let profanity_candidates = self.parity.profanity_vector_candidates(
            payload,
            buffer.merged_candidates(),
            analysis.surface,
        );
        if self.trace_enabled {
            println!(
                "[l2.profanity] candidates(count={}): {:?}",
                profanity_candidates.len(),
                profanity_candidates
            );
        }
        if let Some(semantic) = &self.semantic {
            if !profanity_candidates.is_empty()
                && semantic
                    .scan_profanity_candidates(&profanity_candidates)
                    .await
            {
                if self.trace_enabled {
                    println!("[l2.profanity] semantic_hit=true");
                    println!("[final] decision=BLOCK reason=semantic_profanity");
                }
                return true;
            }
            if self.trace_enabled && !profanity_candidates.is_empty() {
                println!("[l2.profanity] semantic_hit=false");
            }
        } else if self.trace_enabled {
            println!("[l2.profanity] skipped (L2 disabled)");
        }

        // Step 3: Run broad vector fallback only when lexical path does not match
        // and the same JS-like gate conditions are met.
        let should_vector_fallback = self
            .parity
            .should_run_vector_fallback(payload, analysis.surface);
        if self.trace_enabled {
            println!("[l2.fallback] enabled={}", should_vector_fallback);
        }
        if should_vector_fallback {
            if let Some(semantic) = &self.semantic {
                let semantic_hit = semantic.scan_semantic(payload).await;
                if self.trace_enabled {
                    println!("[l2.fallback] semantic_hit={}", semantic_hit);
                    println!(
                        "[final] decision={} reason=semantic_fallback",
                        if semantic_hit { "BLOCK" } else { "ALLOW" }
                    );
                }
                return semantic_hit;
            }
            if self.trace_enabled {
                println!("[l2.fallback] skipped (L2 disabled)");
            }
        }

        if self.trace_enabled {
            println!("[final] decision=ALLOW reason=no_lexical_or_semantic_hit");
        }

        false
    }
}

fn moderation_trace_enabled() -> bool {
    std::env::var("OMEGA_TRACE_WORD_PIPELINE")
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

fn shorten_for_log(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("…");
            break;
        }
        out.push(ch);
    }
    out.replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn log_word_stages(payload: &str) {
    let mut count = 0usize;
    for (idx, raw) in payload.split_whitespace().enumerate() {
        count += 1;
        let strict = normalize_token(raw, false);
        let collapsed = normalize_token(raw, true);
        println!(
            "[word:{}] raw='{}' -> strict='{}' -> collapsed='{}'",
            idx + 1,
            shorten_for_log(raw, 80),
            shorten_for_log(&strict, 80),
            shorten_for_log(&collapsed, 80)
        );
    }
    if count == 0 {
        println!("[word] no whitespace tokens extracted");
    }
}

fn format_candidates(candidates: &[Candidate]) -> String {
    if candidates.is_empty() {
        return "[]".to_string();
    }
    let preview = candidates
        .iter()
        .take(64)
        .map(|candidate| {
            if candidate.obfuscated {
                format!("{}*", candidate.text)
            } else {
                candidate.text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    if candidates.len() > 64 {
        format!("[{}, ... +{} more]", preview, candidates.len() - 64)
    } else {
        format!("[{}]", preview)
    }
}
