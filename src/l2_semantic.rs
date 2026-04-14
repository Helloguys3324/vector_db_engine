use ndarray::{Array2};
use ort::{GraphOptimizationLevel, Session};
use std::sync::Arc;
use tokenizers::Tokenizer;

/// L2 Semantic Smart-Path
/// Interfaces directly with ONNX Runtime to embed the text, and calculates cosine distance.
pub struct SemanticEngine {
    session: Arc<Session>,
    tokenizer: Tokenizer,
    threshold: f32,
}

impl SemanticEngine {
    /// Loads the specific `.onnx` quantized model and HF tokenizer from disk.
    pub fn new(model_path: &str, tokenizer_path: &str, threshold: f32) -> ort::Result<Self> {
        ort::init()
            .with_name("antigravity-l2")
            .with_execution_providers([ort::ExecutionProviderDispatch::CPU(Default::default())])
            .commit()?;

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

    /// Cosine similarity helper
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|&x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|&x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    /// Scans the semantic space using ONNX and HF Tokenizers.
    pub fn scan_semantic(&self, text: &str) -> bool {
        // 1. Tokenize the input using HuggingFace Tokenizer natively
        let encoding = match self.tokenizer.encode(text, true) {
            Ok(enc) => enc,
            Err(_) => return false,
        };
        
        let input_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();
        let seq_len = input_ids.len();

        if seq_len == 0 {
            return false; // Empty inputs are clean
        }
        
        // 2. Cast down to INT64 for ONNX graph expectations
        let input_ids_i64: Vec<i64> = input_ids.iter().map(|&id| id as i64).collect();
        let attention_mask_i64: Vec<i64> = attention_mask.iter().map(|&m| m as i64).collect();
        
        // 3. Create ndarray tensors from vectors
        let tensor_input_ids = Array2::from_shape_vec((1, seq_len), input_ids_i64).unwrap();
        let tensor_attention = Array2::from_shape_vec((1, seq_len), attention_mask_i64).unwrap();
        
        // 4. Fire ONNX Inference Session
        // (Names "input_ids" and "attention_mask" generally map perfectly to BERT models)
        let inputs = ort::inputs![
            "input_ids" => tensor_input_ids.view(),
            "attention_mask" => tensor_attention.view()
        ].unwrap();

        if let Ok(outputs) = self.session.run(inputs) {
            // "embeddings" or "last_hidden_state" or "pooler_output" depending on the model head
            // Typically quantized moderately sized LLMs return pooler_output for embedding matches
            if let Ok(embedding_tensor) = outputs["last_hidden_state"].try_extract_tensor::<f32>() {
                let emb_slice = embedding_tensor.as_slice().unwrap_or_default();
                
                // --- Here we would cross-check against hnsw_rs graphs! ---
                // For demonstration, comparing against a mock bad vector
                let scam_centroid = vec![0.5f32; emb_slice.len()];
                let similarity = Self::cosine_similarity(emb_slice, &scam_centroid);
                
                return similarity > self.threshold;
            }
        }

        false
    }
}
