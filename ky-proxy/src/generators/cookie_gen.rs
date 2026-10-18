use rand::Rng;

pub struct CookieGenerator {
    domains: Vec<String>,
    cookie_names: Vec<String>,
}

impl CookieGenerator {
    pub fn new() -> Self {
        Self {
            domains: vec![".example.com".to_string(), ".amazon.com".to_string(), ".google.com".to_string(), ".facebook.com".to_string()],
            cookie_names: vec!["session_id".to_string(), "user_token".to_string(), "csrf_token".to_string(), "tracking_id".to_string(), "visit_id".to_string()],
        }
    }
    
    pub fn generate_cookie(&self) -> (String, String, String) {
        let mut rng = rand::thread_rng();
        let domain = &self.domains[rng.gen_range(0..self.domains.len())];
        let name = &self.cookie_names[rng.gen_range(0..self.cookie_names.len())];
        let value: String = (0..32).map(|_| rng.gen_range(0..16)).map(|b| format!("{:x}", b)).collect();
        
        (domain.clone(), name.clone(), value)
    }
    
    pub fn generate_cookie_header(&self) -> String {
        let mut rng = rand::thread_rng();
        let num_cookies = rng.gen_range(2..5);
        let mut cookies = Vec::new();
        
        for _ in 0..num_cookies {
            let (_, name, value) = self.generate_cookie();
            cookies.push(format!("{}={}", name, value));
        }
        
        cookies.join("; ")
    }
}
