use rand::Rng;
use rand::SeedableRng;

pub struct AITLSGenerator {
    cipher_suite_pool: Vec<u16>,
    extension_pool: Vec<u16>,
    alpn_pool: Vec<String>,
    group_pool: Vec<u16>,
    signature_pool: Vec<u16>,
}

impl AITLSGenerator {
    pub fn new() -> Self {
        Self {
            cipher_suite_pool: vec![
                0x1301, 0x1302, 0x1303, 0xC02B, 0xC02F,
                0xC02C, 0xC030, 0xCCA9, 0xCCA8, 0x009C,
            ],
            extension_pool: vec![
                0x0000, 0x002b, 0x000a, 0x000d, 0x0010,
                0x0023, 0x0017, 0x0015, 0x0012, 0x002d,
            ],
            alpn_pool: vec![
                "h2".to_string(),
                "http/1.1".to_string(),
                "h3".to_string(),
            ],
            group_pool: vec![
                0x001d, 0x0017, 0x001e, 0x0018,
            ],
            signature_pool: vec![
                0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0603,
            ],
        }
    }
    
    pub fn generate_fingerprint(&self, seed: u64) -> Vec<u8> {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        
        let mut hello = Vec::new();
        
        // TLS version
        hello.extend_from_slice(&0x0303u16.to_be_bytes());
        
        // Random 32 bytes
        let mut random = [0u8; 32];
        rng.fill(&mut random);
        hello.extend_from_slice(&random);
        
        // Session ID (random length 0-32)
        let session_len = rng.gen_range(0..32);
        hello.push(session_len as u8);
        let mut session = vec![0u8; session_len];
        for byte in session.iter_mut() {
            *byte = rng.gen();
        }
        hello.extend_from_slice(&session);
        
        // Cipher suites (random selection and order)
        let cipher_count = rng.gen_range(5..self.cipher_suite_pool.len());
        hello.extend_from_slice(&((cipher_count * 2) as u16).to_be_bytes());
        
        let mut ciphers = self.cipher_suite_pool.clone();
        for i in (1..ciphers.len()).rev() {
            let j = rng.gen_range(0..=i);
            ciphers.swap(i, j);
        }
        
        for i in 0..cipher_count {
            hello.extend_from_slice(&ciphers[i].to_be_bytes());
        }
        
        // Compression methods
        hello.push(0x01);
        hello.push(0x00);
        
        // Extensions (random selection and order)
        let ext_count = rng.gen_range(5..self.extension_pool.len());
        
        let mut extensions = self.extension_pool.clone();
        for i in (1..extensions.len()).rev() {
            let j = rng.gen_range(0..=i);
            extensions.swap(i, j);
        }
        
        // Extensions total length (2 bytes per extension + 2 bytes type + 2 bytes length)
        hello.extend_from_slice(&((ext_count * 4) as u16).to_be_bytes());
        
        for i in 0..ext_count {
            hello.extend_from_slice(&extensions[i].to_be_bytes());
            hello.extend_from_slice(&0u16.to_be_bytes()); // Empty data
        }
        
        hello
    }
    
    pub fn generate_millions(&self, count: u64) -> Vec<Vec<u8>> {
        (0..count).map(|i| self.generate_fingerprint(i)).collect()
    }
    
    pub fn ai_generate(&self, prompt: &str) -> Vec<u8> {
        let seed = prompt.bytes().fold(0u64, |acc, b| acc.wrapping_add(b as u64));
        self.generate_fingerprint(seed)
    }
}
