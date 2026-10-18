use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct PersistentSession {
    pub session_id: String,
    pub ip: String,
    pub cookies: Vec<String>,
}

pub struct SessionPersistence {
    path: String,
}

impl SessionPersistence {
    pub fn new(path: &str) -> Self {
        Self { path: path.to_string() }
    }
    
    pub fn save(&self, sessions: &HashMap<String, PersistentSession>) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(sessions)?;
        fs::write(&self.path, json)?;
        Ok(())
    }
    
    pub fn load(&self) -> anyhow::Result<HashMap<String, PersistentSession>> {
        if !Path::new(&self.path).exists() {
            return Ok(HashMap::new());
        }
        let content = fs::read_to_string(&self.path)?;
        let sessions = serde_json::from_str(&content)?;
        Ok(sessions)
    }
}
