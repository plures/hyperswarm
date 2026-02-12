//! Example: DHT Bootstrap
//!
//! This example demonstrates how to create a DHT client and bootstrap
//! it with mainline DHT bootstrap nodes.
//!
//! Run with:
//! ```bash
//! cargo run --example dht_bootstrap
//! ```

use hyperswarm::dht::{DhtClient, DhtConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating DHT client...");
    
    let config = DhtConfig {
        bootstrap: vec![
            "router.bittorrent.com:6881".to_string(),
            "dht.transmissionbt.com:6881".to_string(),
        ],
        bind_port: 0, // Let OS choose a port
    };
    
    let client = DhtClient::new(config).await?;
    println!("DHT client created successfully!");
    
    println!("\nBootstrapping DHT...");
    match client.bootstrap().await {
        Ok(_) => println!("Bootstrap completed successfully!"),
        Err(e) => println!("Bootstrap completed with some errors: {}", e),
    }
    
    println!("\nDHT client is ready.");
    
    Ok(())
}
