//! Common test utilities for integration tests

// Note: These helper functions are available for future use in integration tests
#[allow(dead_code)]
pub async fn create_test_dht_client() -> Result<hyperswarm::dht::DhtClient, Box<dyn std::error::Error>> {
    use hyperswarm::dht::DhtConfig;
    
    let config = DhtConfig {
        bootstrap: vec![], // No external bootstrap for local tests
        bind_port: 0, // OS-assigned port
    };
    
    Ok(hyperswarm::dht::DhtClient::new(config).await?)
}

#[allow(dead_code)]
pub async fn create_test_socket() -> Result<(std::sync::Arc<tokio::net::UdpSocket>, std::net::SocketAddr), Box<dyn std::error::Error>> {
    use tokio::net::UdpSocket;
    
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    let local_addr = socket.local_addr()?;
    Ok((std::sync::Arc::new(socket), local_addr))
}

#[allow(dead_code)]
pub async fn wait_for_setup() {
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}
