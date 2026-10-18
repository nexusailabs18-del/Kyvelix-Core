use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::protocols::tls_fingerprint::TLSFingerprint;
use crate::generators::header_gen::HeaderGenerator;
use crate::generators::user_agent_gen::UserAgentGenerator;
use tracing::info;

pub struct TLSHandler;

impl TLSHandler {
    pub async fn handle_connect(
        client: &mut TcpStream,
        target: &str,
    ) -> anyhow::Result<()> {
        // Generate random TLS fingerprint for THIS connection
        let fingerprint = TLSFingerprint::random();
        let header_gen = HeaderGenerator::new();
        let ua_gen = UserAgentGenerator::new();
        
        let headers = header_gen.generate_headers();
        let ua = ua_gen.generate_ua();
        
        info!("🔐 TLS Fingerprint: {} ciphers", fingerprint.cipher_suites.len());
        info!("📝 Cipher order: {:?}", fingerprint.cipher_suites);
        info!("📝 User-Agent: {}", ua);
        info!("📝 Headers count: {}", headers.len());
        
        // Connect to target
        let mut upstream = TcpStream::connect(target).await?;
        
        // Send 200 to client
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
        
        // Relay bytes (TLS passes through for now)
        let (c, u) = tokio::io::copy_bidirectional(client, &mut upstream).await?;
        info!("✅ Relayed: {} up, {} down", c, u);
        
        Ok(())
    }
}
