use std::path::Path;
use std::process::Command;
use tracing::info;

pub struct AIModels {
    tinyllama_path: String,
    phi2_path: String,
    mistral_path: String,
    embedding_models: Vec<String>,
}

impl AIModels {
    pub fn new() -> Self {
        Self {
            tinyllama_path: "models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf".to_string(),
            phi2_path: "models/text/phi-2.Q4_K_M.gguf".to_string(),
            mistral_path: "models/text/mistral-7b-v0.1.Q4_K_M.gguf".to_string(),
            embedding_models: vec![
                "models/embedding/all-MiniLM-L6-v2".to_string(),
                "models/embedding/bge-small-en".to_string(),
                "models/embedding/gte-small".to_string(),
            ],
        }
    }
    
    pub fn check_models(&self) -> Vec<String> {
        let mut available = Vec::new();
        
        for (name, path) in [
            ("TinyLlama", &self.tinyllama_path),
            ("Phi-2", &self.phi2_path),
            ("Mistral 7B", &self.mistral_path),
        ] {
            if Path::new(path).exists() {
                available.push(format!("✅ {}", name));
            }
        }
        
        for path in &self.embedding_models {
            if Path::new(path).exists() {
                available.push(format!("✅ Embedding: {}", path.split('/').last().unwrap()));
            }
        }
        
        available
    }
    
    pub fn generate_with_model(&self, model: &str, prompt: &str) -> String {
        let model_path = match model {
            "tinyllama" => &self.tinyllama_path,
            "phi2" => &self.phi2_path,
            "mistral" => &self.mistral_path,
            _ => return String::new(),
        };
        
        if !Path::new(model_path).exists() {
            return String::new();
        }
        
        // Use llama.cpp to generate
        let output = Command::new("python3")
            .arg("-c")
            .arg(format!(
                r#"
from llama_cpp import Llama
llm = Llama(model_path="{}", n_ctx=256, n_threads=12)
output = llm("{}", max_tokens=50, temperature=0.7)
print(output['choices'][0]['text'])
"#,
                model_path, prompt
            ))
            .output();
        
        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(_) => String::new(),
        }
    }
    
    pub fn generate_tls_pattern(&self) -> String {
        // Use AI to generate TLS pattern
        self.generate_with_model(
            "tinyllama",
            "Generate random TLS cipher suite order for browser",
        )
    }
    
    pub fn generate_headers_ai(&self) -> String {
        self.generate_with_model(
            "phi2",
            "Generate realistic HTTP headers for Chrome browser",
        )
    }
}
