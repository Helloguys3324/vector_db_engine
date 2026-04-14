use moderation_engine::ModerationEngine;
use std::time::Instant;

fn main() {
    println!("🚀 Launching HFT Moderation Core (L1 + L2 Hybrid)...");

    // In a real module we'd pull these patterns from a database or JSON file
    let bad_words = vec!["fuck", "scam", "crypto double", "send funds", "bitcoin giveaway"];
    
    // We are mocking creating the L2 session since models aren't physically present in this local repo
    // To run this actually, you would provide paths to "models/model_quantized.onnx" etc.
    let engine = ModerationEngine::new(&bad_words, 1024, "mock.onnx", "mock.json");
    
    // Test payload 1: Obvious malicious Fast Path trigger
    let msg1 = "Hey friend, this is totally not a scam!";
    let start = Instant::now();
    let l1_fail = engine.check_payload(msg1);
    let duration = start.elapsed();
    
    if l1_fail {
        println!("🚨 Msg1 blocked by L1 Fast Path in {:?}", duration);
    } else {
        println!("✅ Msg1 passed L1 (moved to queue) in {:?}", duration);
    }
    
    // Test payload 2: Tricky text
    let msg2 = "Hi, how are you today?";
    let start = Instant::now();
    let l1_fail2 = engine.check_payload(msg2);
    let duration2 = start.elapsed();
    
    if l1_fail2 {
        println!("🚨 Msg2 blocked in {:?}", duration2);
    } else {
        println!("✅ Msg2 passed L1 (moved to queue) in {:?}", duration2);
    }
    
    println!("\nSystems operational. Engine ready for TCP/gRPC inbound traffic.");
}
