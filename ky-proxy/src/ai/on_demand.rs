use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use parking_lot::RwLock;
use std::collections::HashMap;
use tracing::info;

pub struct OnDemandAI {
    is_running: Arc<AtomicBool>,
    cache: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl OnDemandAI {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn start(&self) {
        self.is_running.store(true, Ordering::SeqCst);
        info!("🧠 On-demand AI generation started!");
    }
    
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        info!("🧠 On-demand AI generation stopped!");
    }
    
    pub fn generate_when_needed(&self, category: &str, prompt: &str) -> String {
        // Only generate when needed
        if !self.is_running.load(Ordering::SeqCst) {
            return String::new();
        }
        
        // Check cache first
        if let Some(cached) = self.cache.read().get(category) {
            if let Some(item) = cached.first() {
                return item.clone();
            }
        }
        
        // Generate new
        let output = std::process::Command::new("python3")
            .arg("-c")
            .arg(format!(
                r#"
from llama_cpp import Llama
llm = Llama(model_path="models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=32, n_threads=12, verbose=False)
out = llm("{}", max_tokens=10)
print(out['choices'][0]['text'].strip())
"#,
                prompt
            ))
            .output();
        
        let result = match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(_) => String::new(),
        };
        
        // Cache result
        self.cache.write().entry(category.to_string()).or_default().push(result.clone());
        
        result
    }
}
