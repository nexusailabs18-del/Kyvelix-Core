use crate::core::proxy_instance::ProxyInstance;
use dashmap::DashMap;
use std::sync::Arc;

pub struct ProxyPool {
    proxies: Arc<DashMap<String, ProxyInstance>>,
}

impl ProxyPool {
    pub fn new() -> Self {
        Self {
            proxies: Arc::new(DashMap::new()),
        }
    }
    
    pub fn add_proxy(&self, proxy: ProxyInstance) {
        self.proxies.insert(proxy.id.clone(), proxy);
    }
    
    pub fn get_random_proxy(&self) -> Option<ProxyInstance> {
        self.proxies.iter().next().map(|entry| entry.value().clone())
    }
    
    pub fn get_active_count(&self) -> usize {
        self.proxies.len()
    }
}
