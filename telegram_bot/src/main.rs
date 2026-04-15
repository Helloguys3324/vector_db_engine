use dotenvy::dotenv;
use moderation_engine::ModerationEngine;
use std::env;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::time::{sleep, Duration};
use unicode_normalization::UnicodeNormalization;
fn sanitize_text(raw_input: &str) -> String {
    raw_input.chars()
        // 1. Вырезаем все невидимые символы (Zero-Width, RTL overrides и т.д.)
        .filter(|c| !matches!(*c, 
            '\u{200B}'..='\u{200F}' | // Zero-width spaces & marks
            '\u{202A}'..='\u{202E}' | // Text direction overrides
            '\u{2060}'..='\u{2064}' | // Invisible math operators
            '\u{FEFF}'                // Byte order mark
        ))
        // 2. Принудительно в нижний регистр (flat_map нужен, т.к. 1 символ может дать 2 в low-case)
        .flat_map(|c| c.to_lowercase())
        // 3. Анти-Тайпсквоттинг: переводим визуально похожую кириллицу в латиницу (Homoglyphs)
        .map(|c| match c {
            'о' => 'o',
            'е' => 'e',
            'а' => 'a',
            'с' => 'c',
            'р' => 'p',
            'х' => 'x',
            'у' => 'y',
            'і' => 'i',
            _ => c,
        })
        .collect::<String>()
        // 4. Нормализация NFKD: разбивает хитрые юникод-конструкции на базовые буквы
        .nfkd()
        // 5. Опционально: можно отфильтровать всю диакритику (если пишут whóre)
        .filter(|c| !c.is_mark())
        .collect()
}
#[tokio::main]
async fn main() {
    // 1. Load the core environment configs from D:\gemini\.env
    dotenv().ok();

    let bot_token = env::var("BOT_TOKEN").expect("BOT_TOKEN environment variable is required");

    let bot = Bot::new(bot_token);

    println!("🚀 Launching native Rust Moderation Bot...");

    // 2. Load the exactly replicated `en.json` and `naughty-words` dictionary into RAM
    let dict_content = std::fs::read_to_string("rust_dict.txt")
        .expect("Failed to read rust_dict.txt. Ensure the python script compiled it first.");

    let bad_words: Vec<&str> = dict_content
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    println!(
        "📚 Loaded {} profane patterns into L1 memory.",
        bad_words.len()
    );

    let engine = Arc::new(
        ModerationEngine::new(
            &bad_words,
            "vector_db_engine/models/model_quantized.onnx",
            "vector_db_engine/models/tokenizer.json",
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
                let clean_text = sanitize_text(text);
                // HOT PATH: Execute lexical DFA + SIMD buffer + Neural ONNX routing + Qdrant gRPC
                if engine.check_payload(&clean_text).await {
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
