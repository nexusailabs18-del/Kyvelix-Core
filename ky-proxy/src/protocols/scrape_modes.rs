use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum ScrapeMode {
    // Content Types
    FullPage,           // Everything
    TextOnly,           // Just HTML/text
    ImagesOnly,         // Just images
    VideosOnly,         // Just videos
    AudioOnly,          // Just audio
    Documents,          // PDF, DOC, etc.
    DataOnly,           // JSON, XML, APIs
    
    // Specific Resources
    CSSOnly,            // Stylesheets
    JavaScriptOnly,     // JS files
    FontsOnly,          // Font files
    
    // Combinations
    TextAndImages,      // HTML + images
    TextAndCSS,         // HTML + CSS (for layout)
    TextAndJS,          // HTML + JS (for dynamic content)
    MediaOnly,          // Images + Videos + Audio
    
    // Special Modes
    APICalls,           // API endpoints only
    WebScraping,        // Optimized for scraping (no media)
    MinimalHTML,        // Bare HTML structure
    Everything,         // No filtering
    Custom,             // Custom rules
}

pub struct ScrapeModeFilter {
    mode: ScrapeMode,
    blocked_patterns: HashSet<String>,
    allowed_patterns: HashSet<String>,
}

impl ScrapeModeFilter {
    pub fn new(mode: ScrapeMode) -> Self {
        let mut filter = Self {
            mode: mode.clone(),
            blocked_patterns: HashSet::new(),
            allowed_patterns: HashSet::new(),
        };
        filter.configure();
        filter
    }
    
    fn configure(&mut self) {
        match self.mode {
            ScrapeMode::FullPage | ScrapeMode::Everything => {
                // No blocking - get everything
            }
            
            ScrapeMode::TextOnly => {
                self.blocked = vec![
                    // Images
                    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".bmp", ".ico",
                    // Videos
                    ".mp4", ".webm", ".avi", ".mov", ".mkv", ".flv",
                    // Audio
                    ".mp3", ".wav", ".ogg", ".flac", ".aac",
                    // Fonts
                    ".woff", ".woff2", ".ttf", ".otf", ".eot",
                    // Documents
                    ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx",
                    // Styles
                    ".css", ".scss", ".sass", ".less",
                    // Scripts
                    ".js", ".mjs", ".ts", ".jsx", ".tsx",
                ];
                self.allowed = vec![".html", ".htm", ".php", ".asp", ".aspx", ".jsp", ".txt", ".md"];
            }
            
            ScrapeMode::ImagesOnly => {
                self.blocked = vec![
                    ".html", ".htm", ".css", ".js", ".mp4", ".mp3", ".woff", ".ttf",
                    ".pdf", ".doc", ".txt", ".json", ".xml"
                ];
                self.allowed = vec![".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".bmp"];
            }
            
            ScrapeMode::VideosOnly => {
                self.blocked = vec![
                    ".html", ".css", ".js", ".jpg", ".png", ".mp3", ".woff", ".ttf"
                ];
                self.allowed = vec![".mp4", ".webm", ".avi", ".mov", ".mkv", ".flv"];
            }
            
            ScrapeMode::AudioOnly => {
                self.blocked = vec![
                    ".html", ".css", ".js", ".jpg", ".png", ".mp4", ".woff", ".ttf"
                ];
                self.allowed = vec![".mp3", ".wav", ".ogg", ".flac", ".aac", ".m4a"];
            }
            
            ScrapeMode::Documents => {
                self.blocked = vec![
                    ".html", ".css", ".js", ".jpg", ".png", ".mp4", ".mp3"
                ];
                self.allowed = vec![".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".txt"];
            }
            
            ScrapeMode::DataOnly => {
                self.blocked = vec![
                    ".html", ".css", ".js", ".jpg", ".png", ".mp4", ".mp3", ".woff"
                ];
                self.allowed = vec![".json", ".xml", ".csv", ".yaml", ".yml", ".api"];
            }
            
            ScrapeMode::CSSOnly => {
                self.blocked = vec![
                    ".html", ".js", ".jpg", ".png", ".mp4", ".mp3", ".woff"
                ];
                self.allowed = vec![".css", ".scss", ".sass", ".less"];
            }
            
            ScrapeMode::JavaScriptOnly => {
                self.blocked = vec![
                    ".html", ".css", ".jpg", ".png", ".mp4", ".mp3"
                ];
                self.allowed = vec![".js", ".mjs", ".ts", ".jsx", ".tsx"];
            }
            
            ScrapeMode::FontsOnly => {
                self.blocked = vec![
                    ".html", ".css", ".js", ".jpg", ".png", ".mp4"
                ];
                self.allowed = vec![".woff", ".woff2", ".ttf", ".otf", ".eot"];
            }
            
            ScrapeMode::TextAndImages => {
                self.blocked = vec![
                    ".mp4", ".webm", ".mp3", ".woff", ".ttf", ".css", ".js", ".pdf"
                ];
                self.allowed = vec![".html", ".htm", ".jpg", ".jpeg", ".png", ".gif", ".webp"];
            }
            
            ScrapeMode::TextAndCSS => {
                self.blocked = vec![
                    ".js", ".jpg", ".png", ".mp4", ".mp3", ".woff", ".ttf"
                ];
                self.allowed = vec![".html", ".htm", ".css", ".scss"];
            }
            
            ScrapeMode::TextAndJS => {
                self.blocked = vec![
                    ".css", ".jpg", ".png", ".mp4", ".mp3", ".woff", ".ttf"
                ];
                self.allowed = vec![".html", ".htm", ".js", ".mjs"];
            }
            
            ScrapeMode::MediaOnly => {
                self.blocked = vec![
                    ".html", ".css", ".js", ".woff", ".ttf", ".pdf"
                ];
                self.allowed = vec![
                    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg",
                    ".mp4", ".webm", ".avi", ".mov",
                    ".mp3", ".wav", ".ogg", ".flac"
                ];
            }
            
            ScrapeMode::APICalls => {
                self.blocked = vec![
                    ".html", ".css", ".js", ".jpg", ".png", ".mp4", ".mp3", ".woff"
                ];
                self.allowed = vec![".json", ".xml", "api", "graphql", "rest"];
            }
            
            ScrapeMode::WebScraping => {
                // Optimized for web scraping
                self.blocked = vec![
                    // All media
                    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg",
                    ".mp4", ".webm", ".avi", ".mov",
                    ".mp3", ".wav", ".ogg",
                    // Fonts
                    ".woff", ".woff2", ".ttf", ".otf",
                    // Heavy documents
                    ".pdf", ".doc", ".docx", ".xls", ".xlsx",
                    // Analytics
                    "analytics", "tracking", "pixel",
                ];
                self.allowed = vec![
                    ".html", ".htm", ".js", ".css", ".json", ".xml"
                ];
            }
            
            ScrapeMode::MinimalHTML => {
                self.blocked = vec![
                    ".jpg", ".png", ".gif", ".webp", ".svg",
                    ".mp4", ".webm", ".mp3", ".woff", ".ttf",
                    ".css", ".js", ".pdf", ".ico"
                ];
                self.allowed = vec![".html", ".htm"];
            }
            
            ScrapeMode::Custom => {
                // Will be set dynamically
            }
        }
    }
    
    pub fn should_block(&self, url: &str) -> bool {
        let lowercase = url.to_lowercase();
        
        // Check blocked patterns
        if self.blocked_patterns.iter().any(|pattern| lowercase.contains(pattern)) {
            return true;
        }
        
        // If we have allowed patterns, check those
        if !self.allowed_patterns.is_empty() {
            let has_allowed = self.allowed_patterns.iter()
                .any(|pattern| lowercase.contains(pattern));
            
            if !has_allowed {
                return true;
            }
        }
        
        false
    }
    
    pub fn set_custom_rules(&mut self, blocked: Vec<String>, allowed: Vec<String>) {
        self.mode = ScrapeMode::Custom;
        self.blocked_patterns = blocked.into_iter().collect();
        self.allowed_patterns = allowed.into_iter().collect();
    }
    
    pub fn get_mode(&self) -> &ScrapeMode {
        &self.mode
    }
    
    pub fn description(&self) -> String {
        match self.mode {
            ScrapeMode::FullPage => "Full page - everything".to_string(),
            ScrapeMode::TextOnly => "Text only - HTML/txt".to_string(),
            ScrapeMode::ImagesOnly => "Images only - jpg/png/gif/webp".to_string(),
            ScrapeMode::VideosOnly => "Videos only - mp4/webm/avi".to_string(),
            ScrapeMode::AudioOnly => "Audio only - mp3/wav/ogg".to_string(),
            ScrapeMode::Documents => "Documents - pdf/doc/xls".to_string(),
            ScrapeMode::DataOnly => "Data - json/xml/csv".to_string(),
            ScrapeMode::CSSOnly => "CSS only - stylesheets".to_string(),
            ScrapeMode::JavaScriptOnly => "JavaScript only".to_string(),
            ScrapeMode::FontsOnly => "Fonts only - woff/ttf".to_string(),
            ScrapeMode::TextAndImages => "Text + Images".to_string(),
            ScrapeMode::TextAndCSS => "Text + CSS".to_string(),
            ScrapeMode::TextAndJS => "Text + JavaScript".to_string(),
            ScrapeMode::MediaOnly => "All media - images/videos/audio".to_string(),
            ScrapeMode::APICalls => "API calls - json/xml/graphql".to_string(),
            ScrapeMode::WebScraping => "Web scraping optimized".to_string(),
            ScrapeMode::MinimalHTML => "Minimal HTML only".to_string(),
            ScrapeMode::Everything => "Everything - no filtering".to_string(),
            ScrapeMode::Custom => "Custom rules".to_string(),
        }
    }
}
