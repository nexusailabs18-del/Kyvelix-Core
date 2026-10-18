use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use std::time::{Instant, Duration};

pub struct RateLimiter {
    limits: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    max_requests: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            limits: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window,
        }
    }

    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut limits = self.limits.write();
        let reqs = limits.entry(key.to_string()).or_default();
        reqs.retain(|t| now.duration_since(*t) < self.window);
        if reqs.len() < self.max_requests as usize {
            reqs.push(now);
            true
        } else {
            false
        }
    }
}
