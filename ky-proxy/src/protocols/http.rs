use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, error};
use crate::protocols::tls_fingerprint::TLSFingerprint;
use crate::protocols::tls_terminator::TLSTerminator;
use crate::generators::header_gen::HeaderGenerator;
use crate::generators::user_agent_gen::UserAgentGenerator;
use crate::generators::geolocation_gen::GeolocationGenerator;
use crate::generators::cookie_gen::CookieGenerator;
use crate::generators::behavior_gen::BehaviorGenerator;
use crate::generators::header_pool::HeaderPool;
use crate::core::auth::ProxyAuth;
use crate::core::session::SessionManager;
use crate::core::rate_limiter::RateLimiter;
use crate::ai::model::AIModels;
use std::sync::Arc;

pub struct HttpProxy {
    address: String,
    auth: Arc<ProxyAuth>,
    sessions: Arc<SessionManager>,
    rate_limiter: Arc<RateLimiter>,
}

impl HttpProxy {
    pub fn new(address: &str) -> Self {
        Self {
            address: address.to_string(),
            auth: Arc::new(ProxyAuth::new()),
            sessions: Arc::new(SessionManager::new()),
            rate_limiter: Arc::new(RateLimiter::new(100, std::time::Duration::from_secs(60))),
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.address).await?;
        info!("🌐 HTTP Proxy listening on {}", self.address);

        loop {
            let (socket, addr) = listener.accept().await?;
            info!("📥 New connection from {}", addr);

            let auth = self.auth.clone();
            let sessions = self.sessions.clone();
            let rate_limiter = self.rate_limiter.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(socket, auth, sessions, rate_limiter).await {
                    error!("Connection error: {}", e);
                }
            });
        }
    }
}

async fn handle_connection(
    mut socket: TcpStream,
    auth: Arc<ProxyAuth>,
    _sessions: Arc<SessionManager>,
    rate_limiter: Arc<RateLimiter>,
) -> anyhow::Result<()> {
    socket.set_nodelay(true)?;

    let mut buffer = [0; 65536];
    let n = socket.read(&mut buffer).await?;
    if n == 0 { return Ok(()); }

    let request = String::from_utf8_lossy(&buffer[..n]).to_string();
    let first_line = request.lines().next().unwrap_or("");

    if !auth.authenticate(&request) {
        info!("🔒 Auth failed");
        socket.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n").await?;
        return Ok(());
    }

    if !rate_limiter.allow(&first_line.to_string()) {
        info!("⏳ Rate limited");
        socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n").await?;
        return Ok(());
    }

    // Check AI models
    let ai = AIModels::new();
    let available = ai.check_models();
    info!("🧠 AI Models: {} available", available.len());
    for model in &available {
        info!("   {}", model);
    }
    
    info!("📝 Request: {}", first_line);

    if first_line.starts_with("CONNECT") {
        handle_connect_with_tls(&mut socket, &request).await?;
    } else {
        forward_http_request(&mut socket, &request).await?;
    }

    Ok(())
}

async fn handle_connect_with_tls(socket: &mut TcpStream, request: &str) -> anyhow::Result<()> {
    let target = request.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("");
    info!("🔗 CONNECT to {}", target);
    
    match TcpStream::connect(target).await {
        Ok(mut upstream) => {
            upstream.set_nodelay(true)?;
            socket.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
            let (c, u) = tokio::io::copy_bidirectional(socket, &mut upstream).await?;
            info!("✅ HTTPS Tunnel: {} up, {} down", c, u);
        }
        Err(e) => {
            error!("CONNECT failed: {}", e);
            socket.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await?;
        }
    }
    Ok(())
}

async fn forward_http_request(socket: &mut TcpStream, request: &str) -> anyhow::Result<()> {
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 { return Ok(()); }

    let method = parts[0];
    let url = parts[1];
    let (host, port, path) = parse_url(url)?;
    let address = format!("{}:{}", host, port);

    match TcpStream::connect(&address).await {
        Ok(mut upstream) => {
            upstream.set_nodelay(true)?;

            let header_pool = HeaderPool::new();
            let loaded = header_pool.load_from_files("generated_clean").unwrap_or(0);
            if loaded > 0 {
                info!("📚 Loaded {} generated headers", loaded);
            }
            let header_gen = HeaderGenerator::new();
            let ua_gen = UserAgentGenerator::new();
            let geo_gen = GeolocationGenerator::new();
            let cookie_gen = CookieGenerator::new();
            let behavior_gen = BehaviorGenerator::new();

            let headers = header_gen.generate_headers();
            let ua = ua_gen.generate_ua();
            let (country, city, lat, lon, _tz, _) = geo_gen.generate_location();
            let cookies = cookie_gen.generate_cookie_header();
            let behavior = behavior_gen.generate_behavior();

            let new_first_line = format!("{} {} HTTP/1.1", method, path);
            let new_request = format!(
                "{}\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: {}\r\nAccept-Language: {}\r\nAccept-Encoding: gzip, deflate, br\r\nCache-Control: max-age=0\r\nCookie: {}\r\nX-Geo-Country: {}\r\nX-Geo-City: {}\r\nX-Geo-Lat: {}\r\nX-Geo-Lon: {}\r\nX-Behavior-Mouse: {}px/s\r\nX-Behavior-Dwell: {}s\r\nConnection: close\r\n\r\n",
                new_first_line, host, ua,
                headers.get("Accept").unwrap_or(&"*/*".to_string()),
                headers.get("Accept-Language").unwrap_or(&"en-US".to_string()),
                cookies, country, city, lat, lon,
                behavior.mouse_speed, behavior.dwell_time
            );

            upstream.write_all(new_request.as_bytes()).await?;

            let mut response_buffer = [0u8; 65536];
            let mut total = 0u64;
            loop {
                match upstream.read(&mut response_buffer).await {
                    Ok(0) => break,
                    Ok(n) => {
                        socket.write_all(&response_buffer[..n]).await?;
                        total += n as u64;
                    }
                    Err(_) => break,
                }
            }
            info!("✅ Response: {} bytes", total);
        }
        Err(e) => {
            error!("Failed: {}", e);
            socket.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await?;
        }
    }
    Ok(())
}

fn parse_url(url: &str) -> anyhow::Result<(String, u16, String)> {
    let without_protocol = url.trim_start_matches("http://").trim_start_matches("https://");
    let (host_part, path) = if let Some(pos) = without_protocol.find('/') {
        (&without_protocol[..pos], &without_protocol[pos..])
    } else {
        (without_protocol, "/")
    };
    let (host, port) = if let Some(pos) = host_part.find(':') {
        (&host_part[..pos], host_part[pos+1..].parse::<u16>().unwrap_or(80))
    } else {
        (host_part, 80)
    };
    Ok((host.to_string(), port, path.to_string()))
}
