use ky_proxy::core::proxy_factory::ProxyFactory;
use ky_proxy::protocols::http::HttpProxy;
use ky_proxy::protocols::socks5::Socks5Proxy;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("🚀 Starting ky-proxy...");
    
    let factory = ProxyFactory::new();
    info!("Generating 1000 proxies...");
    factory.generate_millions(1000);
    info!("✅ Generated {} proxies", factory.get_total_generated());
    
    // Start HTTP proxy on 8080
    let http_proxy = HttpProxy::new("127.0.0.1:8081");
    info!("🌐 HTTP Proxy: 127.0.0.1:8081");
    
    // Start SOCKS5 proxy on 1080
    let socks5_proxy = Socks5Proxy::new("127.0.0.1:1080");
    info!("🧦 SOCKS5 Proxy: 127.0.0.1:1080");
    
    // Run both
    tokio::try_join!(
        http_proxy.start(),
        socks5_proxy.start(),
    )?;
    
    Ok(())
}
