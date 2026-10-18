use rand::Rng;

pub struct UserAgentGenerator {
    platforms: Vec<String>,
    browsers: Vec<String>,
    versions: Vec<String>,
}

impl UserAgentGenerator {
    pub fn new() -> Self {
        Self {
            platforms: vec![
                "Windows NT 10.0; Win64; x64".to_string(),
                "Macintosh; Intel Mac OS X 10_15_7".to_string(),
                "X11; Linux x86_64".to_string(),
            ],
            browsers: vec![
                "Chrome".to_string(),
                "Firefox".to_string(),
                "Edge".to_string(),
                "Opera".to_string(),
            ],
            versions: vec![
                "120.0.0.0".to_string(),
                "119.0.0.0".to_string(),
                "118.0.0.0".to_string(),
            ],
        }
    }
    
    pub fn generate_ua(&self) -> String {
        let mut rng = rand::thread_rng();
        let platform = &self.platforms[rng.gen_range(0..self.platforms.len())];
        let browser = &self.browsers[rng.gen_range(0..self.browsers.len())];
        let version = &self.versions[rng.gen_range(0..self.versions.len())];
        
        match browser.as_str() {
            "Chrome" => format!("Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36", platform, version),
            "Firefox" => format!("Mozilla/5.0 ({}; rv:109.0) Gecko/20100101 Firefox/121.0", platform),
            "Edge" => format!("Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36 Edg/{}", platform, version, version),
            "Opera" => format!("Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36 OPR/{}", platform, version, version),
            _ => format!("Mozilla/5.0 ({}) AppleWebKit/537.36", platform),
        }
    }
    
    pub fn generate_millions(&self, count: u64) -> Vec<String> {
        (0..count).map(|_| self.generate_ua()).collect()
    }
}
