pub struct HTTP2Client {
    enabled: bool,
}

impl HTTP2Client {
    pub fn new() -> Self {
        Self { enabled: true }
    }
    
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    
    pub fn get_alpn(&self) -> Vec<Vec<u8>> {
        if self.enabled {
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        } else {
            vec![b"http/1.1".to_vec()]
        }
    }
}
