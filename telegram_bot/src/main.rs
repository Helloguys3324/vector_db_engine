use dotenvy::dotenv;
use std::env;
use std::sync::Arc;
use teloxide::prelude::*;
use moderation_engine::ModerationEngine;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    // 1. Load the core environment configs from D:\gemini\.env
    dotenv().ok();
    
    let bot_token = env::var("BOT_TOKEN")
        .expect("BOT_TOKEN environment variable is required");
        
    let bot = Bot::new(bot_token);

    println!("🚀 Launching native Rust Moderation Bot...");

    // 2. Initialize the High-Frequency Trading vector_db_engine
    let bad_words = vec!["fuck", "scam", "crypto double", "send funds", "bitcoin giveaway"];
    let engine = Arc::new(ModerationEngine::new(
        &bad_words, 
        "vector_db_engine/models/model_quantized.onnx", 
        "vector_db_engine/models/tokenizer.json",
        "http://localhost:6334",
        "scam_patterns"
    ).await);
    
    let engine_mode = env::var("DETECTOR_MODE").unwrap_or_else(|_| "balanced".to_string());
    println!("Bot active (mode={})", engine_mode);

    // 3. Attach the Dispatcher
    let handler = Update::filter_message().endpoint(
        |bot: Bot, msg: Message, engine: Arc<ModerationEngine>| async move {
            // Only process text messages
            if let Some(text) = msg.text() {
                // HOT PATH: Execute lexical DFA + SIMD buffer + Neural ONNX routing + Qdrant gRPC
                if engine.check_payload(text).await {
                    
                    // 1. Fire-and-forget: Delete profane message
                    if let Err(e) = bot.delete_message(msg.chat.id, msg.id).await {
                        eprintln!("Failed to delete message: {}", e);
                    }
                    
                    // 2. Reply warning
                    if let Ok(warn_msg) = bot.send_message(msg.chat.id, "🚫 No profanity allowed!")
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
        }
    );

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![engine])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
