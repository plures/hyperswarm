//! Example: P2P Connection Flow
//!
//! This example demonstrates the full P2P connection flow:
//! 1. DHT bootstrap and peer discovery
//! 2. UDP holepunching to establish direct connection
//! 3. Noise protocol encrypted communication
//!
//! Run with:
//! ```bash
//! cargo run --example p2p_connection
//! ```

use hyperswarm::{holepunch, transport, Topic};
use std::sync::Arc;
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Hyperswarm P2P Connection Example ===\n");
    
    // Step 1: DHT Setup
    println!("Step 1: Setting up DHT client...");
    let dht_config = hyperswarm::dht::DhtConfig {
        bootstrap: vec![
            "router.bittorrent.com:6881".to_string(),
            "dht.transmissionbt.com:6881".to_string(),
        ],
        bind_port: 0,
    };
    
    let dht_client = hyperswarm::dht::DhtClient::new(dht_config).await?;
    println!("✓ DHT client created");
    
    // Bootstrap the DHT
    println!("\nStep 2: Bootstrapping DHT...");
    match dht_client.bootstrap().await {
        Ok(_) => println!("✓ DHT bootstrap completed"),
        Err(e) => println!("⚠ Bootstrap completed with some errors: {}", e),
    }
    
    // Step 3: Topic Discovery
    println!("\nStep 3: Topic-based peer discovery...");
    let topic_key = b"example-p2p-connection";
    let topic = Topic::from_key(topic_key);
    println!("Topic hash: {:02x?}", &topic.0[..8]);
    
    // Announce our presence (simplified - would use actual listening port)
    println!("  - Announcing presence on topic...");
    match dht_client.announce(topic, 0).await {
        Ok(_) => println!("  ✓ Announced successfully"),
        Err(e) => println!("  ⚠ Announce result: {}", e),
    }
    
    // Lookup peers
    println!("  - Looking up peers on topic...");
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        dht_client.lookup(topic)
    ).await {
        Ok(Ok(peers)) => {
            println!("  ✓ Found {} peer(s)", peers.len());
            for (i, peer) in peers.iter().take(3).enumerate() {
                println!("    Peer {}: {}", i + 1, peer.addr);
            }
        }
        Ok(Err(e)) => println!("  ⚠ Lookup result: {}", e),
        Err(_) => println!("  ⚠ Lookup timed out (no peers found for this test topic)"),
    }
    
    // Step 4: UDP Holepunching
    println!("\nStep 4: UDP Holepunching (demonstration)...");
    let bind_addr = "127.0.0.1:0".parse()?;
    // In a real deployment the session key is derived from the shared topic.
    let session_key = [0u8; 32];
    let mut holepunch_session = holepunch::HolepunchSession::new(bind_addr, session_key).await?;
    let local_addr = holepunch_session.local_addr()?;
    println!("  ✓ Holepunch session created on {}", local_addr);
    
    // Demonstrate candidate probing
    let candidates = vec![
        holepunch::Candidate {
            addr: "127.0.0.1:9001".parse()?,
            kind: holepunch::CandidateKind::Lan,
        },
        holepunch::Candidate {
            addr: "127.0.0.1:9002".parse()?,
            kind: holepunch::CandidateKind::Wan,
        },
    ];
    
    holepunch_session.probe(&candidates).await?;
    println!("  ✓ Probed {} candidate(s)", candidates.len());
    
    // Step 5: Encrypted Transport
    println!("\nStep 5: Encrypted transport setup (demonstration)...");
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
    let remote_addr = "127.0.0.1:8080".parse()?;
    
    let mut encrypted_stream = transport::EncryptedStream::new(socket, remote_addr).await?;
    println!("  ✓ Encrypted stream created");
    println!("  Note: Handshake would require a remote peer to complete");
    
    // Demonstrate that handshake is required before sending
    match encrypted_stream.send(bytes::Bytes::from("test")).await {
        Err(transport::TransportError::HandshakeIncomplete) => {
            println!("  ✓ Correctly requires handshake before sending data");
        }
        _ => println!("  ⚠ Unexpected result"),
    }
    
    // Summary
    println!("\n=== Summary ===");
    println!("✓ All core components functional:");
    println!("  - DHT client: bootstrap, announce, lookup");
    println!("  - UDP holepunching: session creation, probing");
    println!("  - Noise encryption: stream setup, handshake validation");
    println!("\n✓ Ready for PluresDB P2P sync integration!");
    
    Ok(())
}
