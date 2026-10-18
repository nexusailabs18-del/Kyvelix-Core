use rand::Rng;
use std::collections::HashMap;

pub struct HeaderGenerator {
    browsers: Vec<BrowserProfile>,
}

struct BrowserProfile {
    name: String,
    user_agents: Vec<String>,
    accept: String,
    accept_language: Vec<String>,
    accept_encoding: Vec<String>,
    cache_control: Vec<String>,
    sec_ch_ua: Vec<String>,
    sec_fetch: Vec<String>,
}

impl HeaderGenerator {
    pub fn new() -> Self {
        Self {
            browsers: vec![
                BrowserProfile {
                    name: "Chrome".to_string(),
                    user_agents: vec![
                        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
                        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
                    ],
                    accept: "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8".to_string(),
                    accept_language: vec!["en-US,en;q=0.9".to_string(), "en-GB,en;q=0.8".to_string()],
                    accept_encoding: vec!["gzip, deflate, br".to_string(), "gzip, deflate".to_string()],
                    cache_control: vec!["max-age=0".to_string(), "no-cache".to_string()],
                    sec_ch_ua: vec![
                        "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\"".to_string(),
                    ],
                    sec_fetch: vec!["document".to_string(), "navigate".to_string(), "same-origin".to_string()],
                },
            ],
        }
    }
    
    pub fn generate_headers(&self) -> HashMap<String, String> {
        let mut rng = rand::thread_rng();
        let browser = &self.browsers[rng.gen_range(0..self.browsers.len())];
        
        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), browser.user_agents[rng.gen_range(0..browser.user_agents.len())].clone());
        headers.insert("Accept".to_string(), browser.accept.clone());
        headers.insert("Accept-Language".to_string(), browser.accept_language[rng.gen_range(0..browser.accept_language.len())].clone());
        headers.insert("Accept-Encoding".to_string(), browser.accept_encoding[rng.gen_range(0..browser.accept_encoding.len())].clone());
        headers.insert("Cache-Control".to_string(), browser.cache_control[rng.gen_range(0..browser.cache_control.len())].clone());
        headers.insert("Sec-CH-UA".to_string(), browser.sec_ch_ua[rng.gen_range(0..browser.sec_ch_ua.len())].clone());
        
        headers
    }
}
