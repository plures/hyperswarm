//! Integration tests for discovery without public-DHT access.
//!
//! The real localhost bootstrap/announce/lookup assertion lives in
//! `local_krpc_bootstrap.rs`. This file retains resilience coverage for a
//! client whose configured bootstrap cannot respond.

mod common;

use hyperswarm::dht::{DhtClient, DhtConfig};
use hyperswarm::Topic;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_announce_and_lookup_same_client() {
    // Test that a single client can announce and then lookup
    // Use unreachable bootstrap to avoid external network calls
    let config = DhtConfig {
        bootstrap: vec!["192.0.2.1:6881".to_string()], // Unreachable TEST-NET-1
        bind_port: 0,
    };
    
    let client = DhtClient::new(config).await.expect("Failed to create client");
    let addr = client.local_addr().expect("Failed to get address");
    
    let topic = Topic::from_key(b"self-test-topic");
    
    // Announce (will attempt bootstrap but timeout quickly)
    let announce_result = tokio::time::timeout(
        Duration::from_secs(3),
        client.announce(topic, addr.port())
    ).await;
    assert!(announce_result.is_ok(), "Announce should not hang");
    assert!(announce_result.unwrap().is_ok(), "Announce should complete without error");
    
    // Lookup (routing table is empty, will return empty list)
    let lookup_result = tokio::time::timeout(
        Duration::from_secs(3),
        client.lookup(topic)
    ).await;
    assert!(lookup_result.is_ok(), "Lookup should not hang");
    assert!(lookup_result.unwrap().is_ok(), "Lookup should complete without error");
    
    println!("✓ Announce and lookup on same client test passed");
}
