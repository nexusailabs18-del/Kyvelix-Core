use std::process::Command;

fn main() {
    println!("🧪 Testing AI Models for TLS Fingerprint Generation\n");
    
    // Check available models
    let models = [
        ("TinyLlama", "models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"),
        ("Phi-2", "models/text/phi-2.Q4_K_M.gguf"),
        ("Mistral 7B", "models/text/mistral-7b-v0.1.Q4_K_M.gguf"),
    ];
    
    for (name, path) in models.iter() {
        let exists = std::path::Path::new(path).exists();
        let size = if exists {
            let metadata = std::fs::metadata(path).unwrap();
            metadata.len() / (1024 * 1024)
        } else {
            0
        };
        
        if exists {
            println!("✅ {}: {} MB", name, size);
        } else {
            println!("❌ {}: Not found", name);
        }
    }
    
    println!("\n=== Testing AI Integration Points ===");
    println!("1. TinyLlama (638MB) - Fast, basic TLS patterns");
    println!("2. Phi-2 (1.7GB) - Better quality, more variations");
    println!("3. Mistral 7B (4.1GB) - Best quality, most realistic");
    
    println!("\n=== Recommended Usage ===");
    println!("- TinyLlama: Generate cipher suite orderings");
    println!("- Phi-2: Generate extension combinations");
    println!("- Mistral 7B: Generate complete realistic fingerprints");
}
