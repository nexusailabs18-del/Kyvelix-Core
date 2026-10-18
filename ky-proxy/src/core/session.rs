use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use std::time::{Instant, Duration};

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

struct Session {
    ip: String,
    last_used: Instant,
    cookies: Vec<String>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn bind(&self, session_id: &str, ip: &str) {
        self.sessions.write().insert(session_id.to_string(), Session {
            ip: ip.to_string(),
            last_used: Instant::now(),
            cookies: Vec::new(),
        });
    }

    pub fn get_ip(&self, session_id: &str) -> Option<String> {
        self.sessions.read().get(session_id).map(|s| s.ip.clone())
    }

    pub fn cleanup(&self, timeout: Duration) {
        let now = Instant::now();
        self.sessions.write().retain(|_, s| now.duration_since(s.last_used) < timeout);
    }
}
