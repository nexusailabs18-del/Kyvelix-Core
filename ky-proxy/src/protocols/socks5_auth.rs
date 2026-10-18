use std::collections::HashMap;

pub struct Socks5Auth {
    users: HashMap<String, String>,
}

impl Socks5Auth {
    pub fn new() -> Self {
        let mut users = HashMap::new();
        users.insert("admin".to_string(), "kyprox123".to_string());
        users.insert("scraper".to_string(), "scrape2024".to_string());
        Self { users }
    }
    
    pub fn authenticate(&self, username: &str, password: &str) -> bool {
        self.users.get(username).map_or(false, |pw| pw == password)
    }
}
