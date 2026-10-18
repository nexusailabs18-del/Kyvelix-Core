use crate::core::proxy_instance::ProxyInstance;
use crate::generators::ip_generator::IpGenerator;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use dashmap::DashMap;

pub struct ProxyFactory {
    ip_generator: IpGenerator,
    active_proxies: Arc<DashMap<String, ProxyInstance>>,
    total_generated: AtomicU64,
}

impl ProxyFactory {
    pub fn new() -> Self {
        Self {
            ip_generator: IpGenerator::new(),
            active_proxies: Arc::new(DashMap::new()),
            total_generated: AtomicU64::new(0),
        }
    }
    
    pub fn generate_proxy(&self) -> ProxyInstance {
        let id = self.total_generated.fetch_add(1, Ordering::SeqCst);
        let (ip, country, asn) = self.ip_generator.generate_ip();
        
        let mut proxy = ProxyInstance::new(
            format!("proxy-{}", id),
            ip,
            8080 + (id % 1000) as u16,
        );
        
        proxy.country = country;
        proxy.asn = asn;
        
        // Add to active proxies
        self.active_proxies.insert(proxy.id.clone(), proxy.clone());
        
        proxy
    }
    
    pub fn generate_millions(&self, count: u64) {
        for _ in 0..count {
            self.generate_proxy();
        }
    }
    
    pub fn get_active_count(&self) -> usize {
        self.active_proxies.len()
    }
    
    pub fn get_total_generated(&self) -> u64 {
        self.total_generated.load(Ordering::SeqCst)
    }
    
    pub fn get_random_proxy(&self) -> Option<ProxyInstance> {
        self.active_proxies.iter().next().map(|entry| entry.value().clone())
    }
}
