use dotenvy::dotenv;
use moderation_engine::ModerationEngine;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::time::{sleep, Duration};

fn parse_bad_words(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn load_bad_words() -> Vec<String> {
    let mut candidates = Vec::<PathBuf>::new();

    if let Ok(raw) = env::var("OMEGA_RUST_DICT_PATH") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }

    candidates.push(PathBuf::from("rust_dict.txt"));

    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("rust_dict.txt"));
        }
    }

    for path in candidates {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let words = parse_bad_words(&raw);
            println!(
                "📚 Loaded {} profane patterns into L1 memory (source: {}).",
                words.len(),
                path.display()
            );
            return words;
        }
    }

    eprintln!(
        "⚠️ rust_dict.txt was not found. Starting with empty DFA dictionary; JS parity dictionary remains active."
    );
    Vec::new()
}

fn resolve_l2_asset_path(env_var: &str, file_name: &str) -> String {
    let mut candidates = Vec::<PathBuf>::new();

    if let Ok(raw) = env::var(env_var) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }

    candidates.push(
        PathBuf::from("vector_db_engine")
            .join("models")
            .join(file_name),
    );
    candidates.push(PathBuf::from("models").join(file_name));

    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join(file_name));
            candidates.push(exe_dir.join("models").join(file_name));
            candidates.push(
                exe_dir
                    .join("vector_db_engine")
                    .join("models")
                    .join(file_name),
            );
        }
    }

    for path in &candidates {
        if path.exists() {
            println!("📦 Using {} from {}", file_name, path.display());
            return path.to_string_lossy().to_string();
        }
    }

    let fallback = candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| Path::new(file_name).to_path_buf());
    eprintln!(
        "⚠️ {} was not found via {}. L2 may run in degraded mode.",
        file_name, env_var
    );
    fallback.to_string_lossy().to_string()
}

#[tokio::main]
async fn main() {
    // 1. Load core environment configs from .env in the current working directory
    dotenv().ok();

    let bot_token = env::var("BOT_TOKEN").expect("BOT_TOKEN environment variable is required");

    let bot = Bot::new(bot_token);

    println!("🚀 Launching native Rust Moderation Bot...");

    // 2. Load lexical dictionary for DFA fast-path.
    let bad_words_owned = load_bad_words();
    let bad_words: Vec<&str> = bad_words_owned.iter().map(String::as_str).collect();
    let model_path = resolve_l2_asset_path("OMEGA_MODEL_PATH", "model_quantized.onnx");
    let tokenizer_path = resolve_l2_asset_path("OMEGA_TOKENIZER_PATH", "tokenizer.json");

    let engine = Arc::new(
        ModerationEngine::new(
            &bad_words,
            &model_path,
            &tokenizer_path,
            "http://localhost:6334",
            "scam_patterns",
        )
        .await,
    );

    let engine_mode = env::var("DETECTOR_MODE").unwrap_or_else(|_| "balanced".to_string());
    println!("Bot active (mode={})", engine_mode);

    // 3. Attach the Dispatcher
    let handler = Update::filter_message().endpoint(
        |bot: Bot, msg: Message, engine: Arc<ModerationEngine>| async move {
            // Only process text messages
            if let Some(text) = msg.text() {
                // --- DYNAMIC TRAINING COMMAND ---
                if text.starts_with("/train ") {
                    let pattern = text.strip_prefix("/train ").unwrap().trim();
                    if pattern.is_empty() {
                        return respond(());
                    }

                    match engine.train_payload(pattern).await {
                        Ok(_) => {
                            let _ = bot
                                .send_message(
                                    msg.chat.id,
                                    "✅ Neural Network updated with new pattern!",
                                )
                                .reply_to_message_id(msg.id)
                                .await;
                        }
                        Err(err) => {
                            let _ = bot
                                .send_message(msg.chat.id, format!("❌ Training error: {}", err))
                                .reply_to_message_id(msg.id)
                                .await;
                        }
                    }
                    return respond(());
                }
                // HOT PATH: Execute lexical DFA + SIMD buffer + Neural ONNX routing + Qdrant gRPC
                if engine.check_payload(text).await {
                    // 1. Fire-and-forget: Delete profane message
                    if let Err(e) = bot.delete_message(msg.chat.id, msg.id).await {
                        eprintln!("Failed to delete message: {}", e);
                    }

                    // 2. Reply warning
                    if let Ok(warn_msg) = bot
                        .send_message(msg.chat.id, "🚫 No profanity allowed!")
                        .reply_to_message_id(msg.id)
                        .await
                    {
                        // 3. Spawn background detached coroutine to delete warning 5s later
                        tokio::spawn(async move {
                            sleep(Duration::from_secs(5)).await;
                            let _ = bot.delete_message(warn_msg.chat.id, warn_msg.id).await;
                        });
                    }
                }
            }
            respond(())
        },
    );

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![engine])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
