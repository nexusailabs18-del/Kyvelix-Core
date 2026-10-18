use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct WebSocketProxy;

impl WebSocketProxy {
    pub async fn handle(socket: &mut TcpStream, target: &str) -> anyhow::Result<()> {
        // WebSocket upgrade detection
        let mut buffer = [0; 4096];
        let n = socket.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..n]);
        
        if request.contains("Upgrade: websocket") {
            // Forward WebSocket connection
            let mut upstream = TcpStream::connect(target).await?;
            upstream.write_all(&buffer[..n]).await?;
            
            let (c, u) = tokio::io::copy_bidirectional(socket, &mut upstream).await?;
            tracing::info!("✅ WebSocket: {} up, {} down", c, u);
        }
        
        Ok(())
    }
}
