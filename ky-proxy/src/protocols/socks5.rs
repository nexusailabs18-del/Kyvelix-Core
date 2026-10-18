use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, error};

pub struct Socks5Proxy {
    address: String,
}

impl Socks5Proxy {
    pub fn new(address: &str) -> Self {
        Self {
            address: address.to_string(),
        }
    }
    
    pub async fn start(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.address).await?;
        info!("🧦 SOCKS5 Proxy listening on {}", self.address);
        
        loop {
            let (socket, addr) = listener.accept().await?;
            info!("📥 SOCKS5 connection from {}", addr);
            
            tokio::spawn(async move {
                if let Err(e) = handle_socks5(socket).await {
                    error!("SOCKS5 error: {}", e);
                }
            });
        }
    }
}

async fn handle_socks5(mut socket: TcpStream) -> anyhow::Result<()> {
    socket.set_nodelay(true)?;
    
    let mut buffer = [0; 512];
    
    // Read greeting
    let n = socket.read(&mut buffer).await?;
    if n < 3 {
        return Ok(());
    }
    
    if buffer[0] != 0x05 {
        return Ok(());
    }
    
    // Send no-auth response
    socket.write_all(&[0x05, 0x00]).await?;
    
    // Read request
    let n = socket.read(&mut buffer).await?;
    if n < 4 {
        return Ok(());
    }
    
    // Parse target address
    let atyp = buffer[3];
    let target = match atyp {
        0x01 => {
            format!("{}.{}.{}.{}:{}", buffer[4], buffer[5], buffer[6], buffer[7], 
                (buffer[8] as u16) << 8 | buffer[9] as u16)
        }
        0x03 => {
            let len = buffer[4] as usize;
            let domain = String::from_utf8_lossy(&buffer[5..5+len]).to_string();
            let port = (buffer[5+len] as u16) << 8 | buffer[6+len] as u16;
            format!("{}:{}", domain, port)
        }
        _ => return Ok(()),
    };
    
    info!("🎯 SOCKS5 target: {}", target);
    
    // Connect to target
    let mut upstream = match TcpStream::connect(&target).await {
        Ok(upstream) => upstream,
        Err(e) => {
            error!("Failed to connect: {}", e);
            socket.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
            return Ok(());
        }
    };
    
    upstream.set_nodelay(true)?;
    
    // Send success response
    socket.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
    
    info!("🔄 Starting data relay for {}", target);
    
    // Use copy_bidirectional - handles large data efficiently
    let (client_bytes, upstream_bytes) = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await?;
    
    info!("✅ Transfer complete: Client→Target: {} bytes, Target→Client: {} bytes", 
        client_bytes, upstream_bytes);
    
    Ok(())
}
