use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use parking_lot::RwLock;
use serde_json::Value;

pub struct HeaderPool {
    headers: Arc<RwLock<Vec<HashMap<String, String>>>>,
    current_index: Arc<RwLock<usize>>,
}

impl HeaderPool {
    pub fn new() -> Self {
        Self {
            headers: Arc::new(RwLock::new(Vec::new())),
            current_index: Arc::new(RwLock::new(0)),
        }
    }
    
    pub fn load_from_files(&self, dir: &str) -> anyhow::Result<usize> {
        let mut loaded = 0;
        
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().extension().map_or(false, |e| e == "json") {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        if let Ok(data) = serde_json::from_str::<Value>(&content) {
                            if let Some(items) = data.as_array() {
                                for item in items {
                                    let mut header_map = HashMap::new();
                                    if let Some(obj) = item.as_object() {
                                        for (k, v) in obj {
                                            if let Some(s) = v.as_str() {
                                                header_map.insert(k.clone(), s.to_string());
                                            }
                                        }
                                    }
                                    self.headers.write().push(header_map);
                                    loaded += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(loaded)
    }
    
    pub fn get_next(&self) -> Option<HashMap<String, String>> {
        let headers = self.headers.read();
        if headers.is_empty() {
            return None;
        }
        
        let mut idx = self.current_index.write();
        let header = headers[*idx % headers.len()].clone();
        *idx += 1;
        Some(header)
    }
    
    pub fn total_loaded(&self) -> usize {
        self.headers.read().len()
    }
}
