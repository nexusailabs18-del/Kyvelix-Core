use rand::Rng;
use rand::SeedableRng;

#[derive(Debug, Clone)]
pub struct TLSFingerprint {
    pub tls_version: u16,
    pub cipher_suites: Vec<u16>,
    pub extensions: Vec<Extension>,
    pub alpn: Vec<String>,
    pub supported_groups: Vec<u16>,
    pub signature_algorithms: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct Extension {
    pub type_id: u16,
    pub data: Vec<u8>,
}

impl TLSFingerprint {
    pub fn chrome_120() -> Self {
        Self {
            tls_version: 0x0303,
            cipher_suites: vec![
                0x1301, 0x1302, 0x1303, 0xC02B, 0xC02F,
                0xC02C, 0xC030, 0xCCA9, 0xCCA8,
            ],
            extensions: vec![
                Extension { type_id: 0x0000, data: vec![] },
                Extension { type_id: 0x002b, data: vec![0x03, 0x04] },
            ],
            alpn: vec!["h2".to_string(), "http/1.1".to_string()],
            supported_groups: vec![0x001d, 0x0017, 0x001e],
            signature_algorithms: vec![0x0403, 0x0804, 0x0401],
        }
    }
    
    pub fn firefox_121() -> Self {
        Self {
            tls_version: 0x0303,
            cipher_suites: vec![0x1301, 0x1302, 0x1303, 0xC02B, 0xC02F],
            extensions: vec![
                Extension { type_id: 0x0000, data: vec![] },
                Extension { type_id: 0x002b, data: vec![0x03, 0x04] },
            ],
            alpn: vec!["h2".to_string(), "http/1.1".to_string()],
            supported_groups: vec![0x001d, 0x0017],
            signature_algorithms: vec![0x0403, 0x0804],
        }
    }
    
    pub fn safari_17() -> Self {
        Self {
            tls_version: 0x0303,
            cipher_suites: vec![
                0x1301, 0x1302, 0x1303,
                0xC02B, 0xC02F, 0xC02C, 0xC030,
                0xCCA9, 0xCCA8,
            ],
            extensions: vec![
                Extension { type_id: 0x0000, data: vec![] },
                Extension { type_id: 0x002b, data: vec![0x03, 0x04] },
                Extension { type_id: 0x0010, data: vec![] },
            ],
            alpn: vec!["h2".to_string(), "http/1.1".to_string()],
            supported_groups: vec![0x001d, 0x0017, 0x001e, 0x0018],
            signature_algorithms: vec![0x0403, 0x0804, 0x0401, 0x0503],
        }
    }
    
    pub fn edge_120() -> Self {
        Self {
            tls_version: 0x0303,
            cipher_suites: vec![
                0x1301, 0x1302, 0x1303,
                0xC02B, 0xC02F, 0xC02C, 0xC030,
                0xCCA9, 0xCCA8, 0x009C,
            ],
            extensions: vec![
                Extension { type_id: 0x0000, data: vec![] },
                Extension { type_id: 0x002b, data: vec![0x03, 0x04] },
                Extension { type_id: 0x000a, data: vec![] },
                Extension { type_id: 0x000d, data: vec![] },
            ],
            alpn: vec!["h2".to_string(), "http/1.1".to_string()],
            supported_groups: vec![0x001d, 0x0017, 0x001e],
            signature_algorithms: vec![0x0403, 0x0804, 0x0401, 0x0805],
        }
    }
    
    pub fn opera_100() -> Self {
        Self {
            tls_version: 0x0303,
            cipher_suites: vec![
                0x1301, 0x1302, 0x1303,
                0xC02B, 0xC02F, 0xC02C, 0xC030,
            ],
            extensions: vec![
                Extension { type_id: 0x0000, data: vec![] },
                Extension { type_id: 0x002b, data: vec![0x03, 0x04] },
            ],
            alpn: vec!["h2".to_string(), "http/1.1".to_string()],
            supported_groups: vec![0x001d, 0x0017],
            signature_algorithms: vec![0x0403, 0x0804],
        }
    }
    
    pub fn random() -> Self {
        let mut rng = rand::thread_rng();
        let variants = [
            Self::chrome_120(),
            Self::firefox_121(),
            Self::safari_17(),
            Self::edge_120(),
            Self::opera_100(),
        ];
        let mut fp = variants[rng.gen_range(0..variants.len())].clone();
        let len = fp.cipher_suites.len();
        let swap_idx = rng.gen_range(0..len);
        fp.cipher_suites.swap(0, swap_idx);
        fp
    }
    
    pub fn ai_generate(seed: u64) -> Self {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let variants = [
            Self::chrome_120(),
            Self::firefox_121(),
            Self::safari_17(),
            Self::edge_120(),
            Self::opera_100(),
        ];
        let mut fp = variants[rng.gen_range(0..variants.len())].clone();
        
        let len = fp.cipher_suites.len();
        let swaps = rng.gen_range(1..len);
        for _ in 0..swaps {
            let i = rng.gen_range(0..len);
            let j = rng.gen_range(0..len);
            fp.cipher_suites.swap(i, j);
        }
        
        fp
    }
    
    pub fn to_client_hello(&self) -> Vec<u8> {
        let mut hello = Vec::new();
        
        hello.extend_from_slice(&self.tls_version.to_be_bytes());
        
        let mut random = [0u8; 32];
        rand::thread_rng().fill(&mut random);
        hello.extend_from_slice(&random);
        
        hello.push(0x00);
        
        let suite_len = (self.cipher_suites.len() * 2) as u16;
        hello.extend_from_slice(&suite_len.to_be_bytes());
        for suite in &self.cipher_suites {
            hello.extend_from_slice(&suite.to_be_bytes());
        }
        
        hello.push(0x01);
        hello.push(0x00);
        
        for ext in &self.extensions {
            hello.extend_from_slice(&ext.type_id.to_be_bytes());
            let ext_len = ext.data.len() as u16;
            hello.extend_from_slice(&ext_len.to_be_bytes());
            hello.extend_from_slice(&ext.data);
        }
        
        hello
    }
}
