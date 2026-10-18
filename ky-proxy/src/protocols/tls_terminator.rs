use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, TlsAcceptor};
use rustls::{ClientConfig, ServerConfig, RootCertStore};
use std::sync::Arc;
use tracing::info;
use rcgen::generate_simple_self_signed;

pub struct TLSTerminator {
    server_config: Arc<ServerConfig>,
}

impl TLSTerminator {
    pub fn new() -> Self {
        let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let cert = generate_simple_self_signed(subject_alt_names).unwrap();
        
        let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());
        let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der())
        );
        
        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .unwrap();
        
        server_config.alpn_protocols = vec![b"http/1.1".to_vec()];
        
        Self { server_config: Arc::new(server_config) }
    }

    pub async fn handle_https(
        &self,
        client: &mut TcpStream,
        target: &str,
        hostname: &str,
    ) -> anyhow::Result<()> {
        let acceptor = TlsAcceptor::from(self.server_config.clone());
        let mut client_tls = acceptor.accept(&mut *client).await?;
        info!("✅ Client TLS accepted");
        
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        
        let mut client_config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        
        client_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        
        let connector = TlsConnector::from(Arc::new(client_config));
        let tcp = TcpStream::connect(target).await?;
        
        let server_name = rustls::pki_types::ServerName::try_from(hostname.to_string())?;
        let mut target_tls = connector.connect(server_name, tcp).await?;
        info!("✅ Target TLS established");
        
        let (c, u) = tokio::io::copy_bidirectional(&mut client_tls, &mut target_tls).await?;
        info!("✅ Relayed: {} up, {} down", c, u);
        
        Ok(())
    }
}
