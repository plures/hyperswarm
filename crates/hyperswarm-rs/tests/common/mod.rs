//! Common test utilities for integration tests

use hyperswarm::dht::{DhtClient, DhtConfig};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

/// Create a DHT client bound to localhost with OS-assigned port
pub async fn create_test_dht_client() -> Result<DhtClient, Box<dyn std::error::Error>> {
    let config = DhtConfig {
        bootstrap: vec![], // No external bootstrap for local tests
        bind_port: 0, // OS-assigned port
    };
    
    Ok(DhtClient::new(config).await?)
}

/// Create a UDP socket bound to localhost with OS-assigned port
pub async fn create_test_socket() -> Result<(std::sync::Arc<UdpSocket>, SocketAddr), Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    let local_addr = socket.local_addr()?;
    Ok((std::sync::Arc::new(socket), local_addr))
}

/// Wait for a short period to allow async operations to complete
pub async fn wait_for_setup() {
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}
