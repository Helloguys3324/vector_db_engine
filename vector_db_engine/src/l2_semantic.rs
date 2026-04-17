use ndarray::Array2;
use ort::session::{builder::GraphOptimizationLevel, Session};
use qdrant_client::prelude::*;
use qdrant_client::qdrant::{
    Condition, CreateCollection, Distance, Filter, PointStruct, SearchPoints, VectorParams,
    VectorsConfig,
};
use std::sync::Mutex;
use tokenizers::Tokenizer;

const EMBEDDING_DIM: usize = 384;
const GENERAL_SEMANTIC_THRESHOLD: f32 = 0.65;
const DEFAULT_PROFANITY_SEMANTIC_THRESHOLD: f32 = 0.80;
const PROFANITY_SEED_CATEGORY: &str = "seed_profanity";
const LIVE_TRAINED_PROFANITY_CATEGORY: &str = "live_trained_profanity";
const BOOTSTRAP_PLACEHOLDER_CATEGORY: &str = "bootstrap_placeholder";

/// L2 Semantic Smart-Path via gRPC Qdrant
pub struct SemanticEngine {
    session: Mutex<Session>,
    expects_token_type_ids: bool,
    tokenizer: Tokenizer,
    qdrant: QdrantClient,
    collection_name: String,
}

impl SemanticEngine {
    pub async fn new(
        model_path: &str,
        tokenizer_path: &str,
        qdrant_url: &str,
        collection_name: &str,
    ) -> Result<Self, String> {
        let _ = ort::init().with_name("antigravity-l2").commit(); // Returns Result, we ignore if already initialized

        let session = Session::builder()
            .map_err(|err| err.to_string())?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|err| err.to_string())?
            .with_intra_threads(4)
            .map_err(|err| err.to_string())?
            .commit_from_file(model_path)
            .map_err(|err| err.to_string())?;
        let expects_token_type_ids = session
            .inputs()
            .iter()
            .any(|input| input.name().eq_ignore_ascii_case("token_type_ids"));

        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|err| err.to_string())?;

        let qdrant = QdrantClient::from_url(qdrant_url)
            .build()
            .map_err(|err| err.to_string())?;

        // Assure collection exists.
        if !qdrant
            .collection_exists(collection_name)
            .await
            .unwrap_or(false)
        {
            qdrant
                .create_collection(&CreateCollection {
                    collection_name: collection_name.to_string(),
                    vectors_config: Some(VectorsConfig {
                        config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                            VectorParams {
                                size: EMBEDDING_DIM as u64,
                                distance: Distance::Cosine.into(),
                                ..Default::default()
                            },
                        )),
                    }),
                    ..Default::default()
                })
                .await
                .map_err(|err| err.to_string())?;

            println!("✅ Created Qdrant collection '{}'", collection_name);

            // Imbue a mock vector to prevent empty vector panic.
            let mut payload_map = std::collections::HashMap::new();
            payload_map.insert("category", BOOTSTRAP_PLACEHOLDER_CATEGORY.into());

            let _ = qdrant
                .upsert_points(
                    collection_name,
                    None,
                    vec![PointStruct::new(
                        1,
                        vec![0.5f32; EMBEDDING_DIM],
                        payload_map.into(),
                    )],
                    None,
                )
                .await;
        }

        Ok(Self {
            session: Mutex::new(session),
            expects_token_type_ids,
            tokenizer,
            qdrant,
            collection_name: collection_name.to_string(),
        })
    }

    pub async fn scan_semantic(&self, text: &str) -> bool {
        let Ok(query_vector) = self.embed_text(text) else {
            return false;
        };
        self.search_vector(query_vector, GENERAL_SEMANTIC_THRESHOLD, None)
            .await
    }

    pub async fn scan_profanity_candidates(&self, candidates: &[String]) -> bool {
        if candidates.is_empty() {
            return false;
        }

        let threshold = profanity_similarity_threshold();
        let filter = Some(Filter::any([
            Condition::matches("category", PROFANITY_SEED_CATEGORY.to_string()),
            Condition::matches("category", LIVE_TRAINED_PROFANITY_CATEGORY.to_string()),
        ]));

        for candidate in candidates.iter().take(8) {
            let Ok(query_vector) = self.embed_text(candidate) else {
                continue;
            };
            if self
                .search_vector(query_vector, threshold, filter.clone())
                .await
            {
                return true;
            }
        }

        false
    }

    /// Seeds Qdrant with known profane words so obfuscated variants can be
    /// detected semantically (for example: "pusssie" -> "pussy").
    pub async fn bootstrap_profanity_lexicon(&self, words: &[String]) -> Result<usize, String> {
        if words.is_empty() {
            return Ok(0);
        }

        let mut normalized = words
            .iter()
            .filter_map(|word| normalize_seed_word(word))
            .collect::<Vec<_>>();
        normalized.sort_unstable();
        normalized.dedup();

        if normalized.is_empty() {
            return Ok(0);
        }

        let mut inserted = 0usize;
        let mut batch = Vec::<PointStruct>::new();
        for word in normalized {
            let query_vector = self.embed_text(&word)?;
            let mut payload = std::collections::HashMap::new();
            payload.insert("text", word.clone().into());
            payload.insert("category", PROFANITY_SEED_CATEGORY.into());
            payload.insert("source", "js_parity_seed".into());

            batch.push(PointStruct::new(
                stable_seed_id(&word),
                query_vector,
                payload.into(),
            ));
            inserted += 1;

            if batch.len() >= 64 {
                self.qdrant
                    .upsert_points(
                        self.collection_name.clone(),
                        None,
                        std::mem::take(&mut batch),
                        None,
                    )
                    .await
                    .map_err(|err| err.to_string())?;
            }
        }

        if !batch.is_empty() {
            self.qdrant
                .upsert_points(self.collection_name.clone(), None, batch, None)
                .await
                .map_err(|err| err.to_string())?;
        }

        Ok(inserted)
    }

    /// Extends the vector database on-the-fly dynamically.
    pub async fn train_payload(&self, text: &str) -> Result<(), String> {
        let query_vector = self.embed_text(text)?;

        // Generate dynamic UUID payload.
        let uuid_string = uuid::Uuid::new_v4().to_string();

        let point = PointStruct {
            id: Some(qdrant_client::qdrant::PointId {
                point_id_options: Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(
                    uuid_string,
                )),
            }),
            vectors: Some(query_vector.into()),
            payload: [
                ("text".to_string(), text.into()),
                (
                    "category".to_string(),
                    LIVE_TRAINED_PROFANITY_CATEGORY.into(),
                ),
            ]
            .into_iter()
            .collect(),
        };

        self.qdrant
            .upsert_points(self.collection_name.clone(), None, vec![point], None)
            .await
            .map_err(|err| err.to_string())?;

        Ok(())
    }

    fn embed_text(&self, text: &str) -> Result<Vec<f32>, String> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|err| err.to_string())?;
        let input_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();
        let seq_len = input_ids.len();

        if seq_len == 0 {
            return Err("Empty sequence".to_string());
        }

        let input_ids_i64: Vec<i64> = input_ids.iter().map(|&id| id as i64).collect();
        let attention_mask_i64: Vec<i64> = attention_mask.iter().map(|&m| m as i64).collect();

        let tensor_inputs =
            Array2::from_shape_vec((1, seq_len), input_ids_i64).map_err(|err| err.to_string())?;
        let tensor_mask = Array2::from_shape_vec((1, seq_len), attention_mask_i64)
            .map_err(|err| err.to_string())?;

        let inputs = if self.expects_token_type_ids {
            let token_type_ids_i64 = vec![0i64; seq_len];
            let tensor_token_type = Array2::from_shape_vec((1, seq_len), token_type_ids_i64)
                .map_err(|err| err.to_string())?;

            ort::inputs![
                "input_ids" => ort::value::Tensor::from_array(tensor_inputs).map_err(|err| err.to_string())?,
                "attention_mask" => ort::value::Tensor::from_array(tensor_mask).map_err(|err| err.to_string())?,
                "token_type_ids" => ort::value::Tensor::from_array(tensor_token_type).map_err(|err| err.to_string())?
            ]
        } else {
            ort::inputs![
                "input_ids" => ort::value::Tensor::from_array(tensor_inputs).map_err(|err| err.to_string())?,
                "attention_mask" => ort::value::Tensor::from_array(tensor_mask).map_err(|err| err.to_string())?
            ]
        };

        let mut session_guard = self
            .session
            .lock()
            .map_err(|_| "Failed to lock ONNX session".to_string())?;
        let outputs = session_guard.run(inputs).map_err(|err| err.to_string())?;

        let embedding_tuple = outputs["last_hidden_state"]
            .try_extract_tensor::<f32>()
            .map_err(|err| err.to_string())?;
        let emb_slice = embedding_tuple.1;

        let mut query_vector = vec![0.0f32; EMBEDDING_DIM];
        let copy_len = std::cmp::min(EMBEDDING_DIM, emb_slice.len());
        query_vector[..copy_len].copy_from_slice(&emb_slice[..copy_len]);

        Ok(query_vector)
    }

    async fn search_vector(
        &self,
        query_vector: Vec<f32>,
        score_threshold: f32,
        filter: Option<Filter>,
    ) -> bool {
        let score_threshold = score_threshold.clamp(0.0, 1.0);

        if let Ok(search_result) = self
            .qdrant
            .search_points(&SearchPoints {
                collection_name: self.collection_name.clone(),
                vector: query_vector,
                filter,
                limit: 1,
                with_payload: Some(true.into()),
                score_threshold: Some(score_threshold),
                ..Default::default()
            })
            .await
        {
            if let Some(closest_match) = search_result.result.first() {
                return closest_match.score >= score_threshold;
            }
        }

        false
    }
}

fn profanity_similarity_threshold() -> f32 {
    std::env::var("OMEGA_PROFANITY_VECTOR_THRESHOLD")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .map(|value| value.clamp(0.5, 0.99))
        .unwrap_or(DEFAULT_PROFANITY_SEMANTIC_THRESHOLD)
}

fn normalize_seed_word(word: &str) -> Option<String> {
    let normalized = word.trim().to_ascii_lowercase();
    if normalized.len() < 4 || normalized.len() > 24 {
        return None;
    }
    if !normalized.chars().all(|ch| ch.is_ascii_lowercase()) {
        return None;
    }
    Some(normalized)
}

fn stable_seed_id(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}
