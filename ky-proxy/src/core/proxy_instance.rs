use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyInstance {
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub country: String,
    pub city: String,
    pub asn: String,
    pub user_agent: String,
    pub tls_fingerprint: String,
    pub headers: HashMap<String, String>,
    pub cookies: Vec<Cookie>,
    pub created_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
    pub success_rate: f64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: Option<DateTime<Utc>>,
}

impl ProxyInstance {
    pub fn new(id: String, ip: String, port: u16) -> Self {
        Self {
            id,
            ip,
            port,
            country: String::new(),
            city: String::new(),
            asn: String::new(),
            user_agent: String::new(),
            tls_fingerprint: String::new(),
            headers: HashMap::new(),
            cookies: Vec::new(),
            created_at: Utc::now(),
            last_used: None,
            success_rate: 100.0,
            is_active: true,
        }
    }
    
    pub fn mark_used(&mut self) {
        self.last_used = Some(Utc::now());
    }
    
    pub fn update_success_rate(&mut self, success: bool) {
        if success {
            self.success_rate = (self.success_rate * 0.9) + 10.0;
        } else {
            self.success_rate = self.success_rate * 0.7;
        }
        
        if self.success_rate < 10.0 {
            self.is_active = false;
        }
    }
}
