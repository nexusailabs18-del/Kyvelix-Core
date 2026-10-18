use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use parking_lot::RwLock;
use std::collections::HashMap;
use tracing::info;

pub struct BackgroundGenerator {
    total_generated: Arc<AtomicU64>,
    generated_data: Arc<RwLock<HashMap<String, Vec<String>>>>,
    is_running: Arc<RwLock<bool>>,
}

impl BackgroundGenerator {
    pub fn new() -> Self {
        Self {
            total_generated: Arc::new(AtomicU64::new(0)),
            generated_data: Arc::new(RwLock::new(HashMap::new())),
            is_running: Arc::new(RwLock::new(false)),
        }
    }
    
    pub async fn start(&self) {
        let mut running = self.is_running.write();
        *running = true;
        drop(running);
        
        let total = self.total_generated.clone();
        let data = self.generated_data.clone();
        let running = self.is_running.clone();
        
        tokio::spawn(async move {
            info!("🧠 Background AI generation started!");
            
            while *running.read() {
                // Generate TLS patterns using AI
                let tls_pattern = generate_tls_with_tinyllama();
                let headers = generate_headers_with_phi2();
                let cookies = generate_cookies_with_mistral();
                
                // Store generated data
                data.write().entry("tls".to_string()).or_default().push(tls_pattern);
                data.write().entry("headers".to_string()).or_default().push(headers);
                data.write().entry("cookies".to_string()).or_default().push(cookies);
                
                total.fetch_add(3, Ordering::SeqCst);
                
                // Small delay to avoid overwhelming
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });
    }
    
    pub fn stop(&self) {
        *self.is_running.write() = false;
    }
    
    pub fn get_total(&self) -> u64 {
        self.total_generated.load(Ordering::SeqCst)
    }
    
    pub fn get_generated(&self, category: &str) -> Vec<String> {
        self.generated_data.read().get(category).cloned().unwrap_or_default()
    }
}

fn generate_tls_with_tinyllama() -> String {
    // Use TinyLlama via Python for fast TLS generation
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(r#"
from llama_cpp import Llama
import random
llm = Llama(model_path="models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=64, n_threads=12)
browsers = ["Chrome", "Firefox", "Safari", "Edge"]
browser = random.choice(browsers)
output = llm(f"Generate TLS cipher order for {browser}", max_tokens=20)
print(output['choices'][0]['text'].strip()[:50])
"#)
        .output();
    
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => format!("Random-{}", rand::random::<u32>()),
    }
}

fn generate_headers_with_phi2() -> String {
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(r#"
from llama_cpp import Llama
llm = Llama(model_path="models/text/phi-2.Q4_K_M.gguf", n_ctx=64, n_threads=12)
output = llm("Generate realistic HTTP headers", max_tokens=20)
print(output['choices'][0]['text'].strip()[:50])
"#)
        .output();
    
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

fn generate_cookies_with_mistral() -> String {
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(r#"
from llama_cpp import Llama
llm = Llama(model_path="models/text/mistral-7b-v0.1.Q4_K_M.gguf", n_ctx=64, n_threads=12)
output = llm("Generate realistic session cookie", max_tokens=20)
print(output['choices'][0]['text'].strip()[:50])
"#)
        .output();
    
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}
