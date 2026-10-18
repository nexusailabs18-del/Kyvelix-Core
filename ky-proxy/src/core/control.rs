use std::sync::Arc;
use parking_lot::RwLock;

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    // Toggle features
    pub tls_spoofing: bool,
    pub random_headers: bool,
    pub random_ua: bool,
    pub random_cookies: bool,
    pub random_geo: bool,
    pub random_behavior: bool,
    pub smart_filtering: bool,
    pub ai_generation: bool,
    pub rate_limiting: bool,
    pub auth_required: bool,
    
    // Scraping modes
    pub scrape_mode: ScrapeMode,
    
    // Rate limit settings
    pub max_requests_per_min: u32,
    
    // TLS settings
    pub browser_type: BrowserType,
}

#[derive(Debug, Clone)]
pub enum ScrapeMode {
    Full,       // Everything
    TextOnly,   // Only HTML
    ImagesOnly, // Only images
    Minimal,    // Bare HTML
    Custom,     // Custom rules
}

#[derive(Debug, Clone)]
pub enum BrowserType {
    Chrome,
    Firefox,
    Safari,
    Edge,
    Opera,
    Random,
}

impl ProxyConfig {
    pub fn default() -> Self {
        Self {
            tls_spoofing: true,
            random_headers: true,
            random_ua: true,
            random_cookies: true,
            random_geo: true,
            random_behavior: true,
            smart_filtering: true,
            ai_generation: true,
            rate_limiting: true,
            auth_required: true,
            scrape_mode: ScrapeMode::Full,
            max_requests_per_min: 10,
            browser_type: BrowserType::Random,
        }
    }
}

pub struct ProxyController {
    config: Arc<RwLock<ProxyConfig>>,
}

impl ProxyController {
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(ProxyConfig::default())),
        }
    }
    
    pub fn get_config(&self) -> ProxyConfig {
        self.config.read().clone()
    }
    
    pub fn update(&self, new_config: ProxyConfig) {
        *self.config.write() = new_config;
    }
    
    pub fn toggle(&self, feature: &str) -> bool {
        let mut config = self.config.write();
        match feature {
            "tls" => config.tls_spoofing = !config.tls_spoofing,
            "headers" => config.random_headers = !config.random_headers,
            "ua" => config.random_ua = !config.random_ua,
            "cookies" => config.random_cookies = !config.random_cookies,
            "geo" => config.random_geo = !config.random_geo,
            "behavior" => config.random_behavior = !config.random_behavior,
            "filter" => config.smart_filtering = !config.smart_filtering,
            "ai" => config.ai_generation = !config.ai_generation,
            "rate" => config.rate_limiting = !config.rate_limiting,
            "auth" => config.auth_required = !config.auth_required,
            _ => return false,
        }
        true
    }
}
