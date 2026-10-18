use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::net::TcpStream;

pub struct ConnectionPool {
    connections: Arc<RwLock<HashMap<String, Vec<TcpStream>>>>,
    max_per_host: usize,
}

impl ConnectionPool {
    pub fn new(max_per_host: usize) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            max_per_host,
        }
    }
    
    pub fn get(&self, host: &str) -> Option<TcpStream> {
        self.connections.write().get_mut(host)?.pop()
    }
    
    pub fn put(&self, host: &str, stream: TcpStream) {
        let mut conns = self.connections.write();
        let entry = conns.entry(host.to_string()).or_default();
        if entry.len() < self.max_per_host {
            entry.push(stream);
        }
    }
}
