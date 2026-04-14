use ndarray::Array2;
use ort::session::{builder::GraphOptimizationLevel, Session};
use std::sync::Mutex;
use tokenizers::Tokenizer;
use qdrant_client::prelude::*;
use qdrant_client::qdrant::{PointStruct, SearchPoints, VectorsConfig, VectorParams, Distance};

/// L2 Semantic Smart-Path via gRPC Qdrant
pub struct SemanticEngine {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    qdrant: QdrantClient,
    collection_name: String,
}

impl SemanticEngine {
    pub async fn new(model_path: &str, tokenizer_path: &str, qdrant_url: &str, collection_name: &str) -> ort::Result<Self> {
        let _ = ort::init()
            .with_name("antigravity-l2")
            .commit(); // Returns Result, we ignore if already initialized

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(model_path)?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .expect("Failed to load tokenizer.json");

        let qdrant = QdrantClient::from_url(qdrant_url).build().expect("Failed to connect to Qdrant");

        // Assure collection exists
        if !qdrant.collection_exists(collection_name).await.unwrap_or(false) {
            qdrant
                .create_collection(&CreateCollection {
                    collection_name: collection_name.to_string(),
                    vectors_config: Some(VectorsConfig {
                        config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                            VectorParams {
                                size: 384, // MiniLM standard dimensionality
                                distance: Distance::Cosine.into(),
                                ..Default::default()
                            },
                        )),
                    }),
                    ..Default::default()
                })
                .await.expect("Failed to create collection");

            println!("✅ Created Qdrant collection '{}'", collection_name);

            // Imbue a mock vector to prevent empty vector panic
            let mut payload_map = std::collections::HashMap::new();
            payload_map.insert("category", "crypto_scam".into());

            let _ = qdrant.upsert_points(collection_name, None, vec![
                PointStruct::new(
                    1,
                    vec![0.5f32; 384],
                    payload_map.into()
                )
            ], None).await;
        }

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            qdrant,
            collection_name: collection_name.to_string(),
        })
    }

    pub async fn scan_semantic(&self, text: &str) -> bool {
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
        
        let inputs = ort::inputs![
            "input_ids" => ort::value::Tensor::from_array(tensor_inputs).unwrap(),
            "attention_mask" => ort::value::Tensor::from_array(tensor_mask).unwrap()
        ];

        let mut query_vector = vec![0.0f32; 384];

        if let Ok(outputs) = self.session.lock().unwrap().run(inputs) {
            if let Ok(embedding_tuple) = outputs["last_hidden_state"].try_extract_tensor::<f32>() {
                let emb_slice = embedding_tuple.1;
                // Grab the CLS token (first 384 elements of the pool)
                let copy_len = std::cmp::min(384, emb_slice.len());
                query_vector[..copy_len].copy_from_slice(&emb_slice[..copy_len]);
            } else {
                return false;
            }
        } else {
            return false;
        }

        // Qdrant gRPC Vector Search
        if let Ok(search_result) = self.qdrant.search_points(&SearchPoints {
            collection_name: self.collection_name.clone(),
            vector: query_vector,
            limit: 1,
            with_payload: Some(true.into()),
            ..Default::default()
        }).await {
            if let Some(closest_match) = search_result.result.first() {
                return closest_match.score > 0.65; // 65% Loose associational threshold
            }
        }

        false
    }

    /// Extends the vector database on-the-fly dynamically
    pub async fn train_payload(&self, text: &str) -> Result<(), String> {
        let encoding = self.tokenizer.encode(text, true).map_err(|e| e.to_string())?;
        let input_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();
        let seq_len = input_ids.len();

        if seq_len == 0 { return Err("Empty sequence".to_string()); }
        
        let input_ids_i64: Vec<i64> = input_ids.iter().map(|&id| id as i64).collect();
        let attention_mask_i64: Vec<i64> = attention_mask.iter().map(|&m| m as i64).collect();
        
        let tensor_inputs = Array2::from_shape_vec((1, seq_len), input_ids_i64).map_err(|e| e.to_string())?;
        let tensor_mask = Array2::from_shape_vec((1, seq_len), attention_mask_i64).map_err(|e| e.to_string())?;
        
        let inputs = ort::inputs![
            "input_ids" => ort::value::Tensor::from_array(tensor_inputs).map_err(|e| e.to_string())?,
            "attention_mask" => ort::value::Tensor::from_array(tensor_mask).map_err(|e| e.to_string())?
        ];

        let mut query_vector = vec![0.0f32; 384];

        if let Ok(outputs) = self.session.lock().unwrap().run(inputs) {
            if let Ok(embedding_tuple) = outputs["last_hidden_state"].try_extract_tensor::<f32>() {
                let emb_slice = embedding_tuple.1;
                let copy_len = std::cmp::min(384, emb_slice.len());
                query_vector[..copy_len].copy_from_slice(&emb_slice[..copy_len]);
            } else {
                return Err("Failed to extract ONNX tensor".to_string());
            }
        } else {
            return Err("Failed to run ONNX Session".to_string());
        }

        // Generate dynamic UUID payload
        let uuid_string = uuid::Uuid::new_v4().to_string();
        
        let point = PointStruct {
            id: Some(qdrant_client::qdrant::PointId {
                point_id_options: Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(uuid_string)),
            }),
            vectors: Some(query_vector.into()),
            payload: [
                ("text".to_string(), text.into()),
                ("category".to_string(), "live_trained_scam".into()),
            ].into_iter().collect(),
        };

        if let Err(e) = self.qdrant.upsert_points(self.collection_name.clone(), None, vec![point]).await {
            return Err(e.to_string());
        }

        Ok(())
    }
}
