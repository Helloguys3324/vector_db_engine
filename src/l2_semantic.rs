use ndarray::Array2;
use ort::session::{builder::GraphOptimizationLevel, Session};
use std::sync::Arc;
use tokenizers::Tokenizer;

/// L2 Semantic Smart-Path
pub struct SemanticEngine {
    session: Arc<Session>,
    tokenizer: Tokenizer,
    threshold: f32,
}

impl SemanticEngine {
    pub fn new(model_path: &str, tokenizer_path: &str, threshold: f32) -> ort::Result<Self> {
        let _ = ort::init()
            .with_name("antigravity-l2")
            .commit(); // Returns Result, we ignore if already initialized

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(model_path)?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .expect("Failed to load tokenizer.json");

        Ok(Self {
            session: Arc::new(session),
            tokenizer,
            threshold,
        })
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|&x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|&x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
    }

    pub fn scan_semantic(&self, text: &str) -> bool {
        let encoding = match self.tokenizer.encode(text, true) {
            Ok(enc) => enc,
            Err(_) => return false,
        };
        
        let input_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();
        let seq_len = input_ids.len();

        if seq_len == 0 { return false; }
        
        let input_ids_i64: Vec<i64> = input_ids.iter().map(|&id| id as i64).collect();
        let attention_mask_i64: Vec<i64> = attention_mask.iter().map(|&m| m as i64).collect();
        
        let tensor_inputs = Array2::from_shape_vec((1, seq_len), input_ids_i64).unwrap();
        let tensor_mask = Array2::from_shape_vec((1, seq_len), attention_mask_i64).unwrap();
        
        // In ORT 2.0, macro inputs macro returns the struct, no unwrap.
        // We pass the ndarray directly.
        let inputs = ort::inputs![
            "input_ids" => tensor_inputs,
            "attention_mask" => tensor_mask
        ].unwrap_or_else(|_| Default::default());

        if let Ok(outputs) = self.session.run(inputs) {
            if let Ok(embedding_tensor) = outputs["last_hidden_state"].try_extract_tensor::<f32>() {
                if let Some(emb_slice) = embedding_tensor.as_slice() {
                    let scam_centroid = vec![0.5f32; emb_slice.len()];
                    return Self::cosine_similarity(emb_slice, &scam_centroid) > self.threshold;
                }
            }
        }

        false
    }
}
