//! Integration test: Bootstrap failure resilience
//!
//! This test verifies that the DHT client handles bootstrap failures gracefully:
//! 1. Unreachable bootstrap nodes should timeout without panic
//! 2. Operations should continue to work (or fail gracefully) without bootstrap
//! 3. Mixed scenarios (some nodes reachable, some not)

mod common;

use hyperswarm::dht::{DhtClient, DhtConfig};
use hyperswarm::Topic;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn test_bootstrap_with_unreachable_nodes() {
    // Create a DHT client with unreachable bootstrap nodes
    let config = DhtConfig {
        bootstrap: vec![
            "192.0.2.1:6881".to_string(),     // TEST-NET-1 (unreachable)
            "198.51.100.1:6881".to_string(),  // TEST-NET-2 (unreachable)
            "203.0.113.1:6881".to_string(),   // TEST-NET-3 (unreachable)
        ],
        bind_port: 0,
    };
    
    let client = DhtClient::new(config).await.expect("Failed to create client");
    println!("Client created with unreachable bootstrap nodes");
    
    // Bootstrap should timeout gracefully without panic
    let bootstrap_result = tokio::time::timeout(
        Duration::from_secs(3),
        client.bootstrap()
    ).await;
    
    match bootstrap_result {
        Ok(Ok(_)) => println!("Bootstrap completed (no nodes responded)"),
        Ok(Err(e)) => println!("Bootstrap failed gracefully: {}", e),
        Err(_) => panic!("Bootstrap should not hang indefinitely"),
    }
    
    println!("✓ Bootstrap with unreachable nodes test passed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_announce_without_bootstrap() {
    // Test that announce works (or fails gracefully) without successful bootstrap
    let config = DhtConfig {
        bootstrap: vec!["192.0.2.1:6881".to_string()], // Unreachable node (TEST-NET-1)
        bind_port: 0,
    };
    
    let client = DhtClient::new(config).await.expect("Failed to create client");
    let addr = client.local_addr().expect("Failed to get address");
    
    let topic = Topic::from_key(b"test-no-bootstrap");
    
    // Announce should handle the failed bootstrap gracefully
    let announce_result = tokio::time::timeout(
        Duration::from_secs(3),
        client.announce(topic, addr.port())
    ).await;
    
    assert!(announce_result.is_ok(), "Announce should not hang");
    
    let result = announce_result.unwrap();
    assert!(result.is_ok(), "Announce should complete without panic");
    
    println!("✓ Announce without bootstrap test passed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_lookup_without_bootstrap() {
    // Test that lookup works (or fails gracefully) without successful bootstrap
    let config = DhtConfig {
        bootstrap: vec!["192.0.2.1:6881".to_string()], // Unreachable node (TEST-NET-1)
        bind_port: 0,
    };
    
    let client = DhtClient::new(config).await.expect("Failed to create client");
    
    let topic = Topic::from_key(b"test-lookup-no-bootstrap");
    
    // Lookup should handle the failed bootstrap gracefully
    let lookup_result = tokio::time::timeout(
        Duration::from_secs(3),
        client.lookup(topic)
    ).await;
    
    assert!(lookup_result.is_ok(), "Lookup should not hang");
    
    let result = lookup_result.unwrap();
    assert!(result.is_ok(), "Lookup should complete without panic");
    
    // Should return empty list since no nodes are available
    let peers = result.unwrap();
    assert_eq!(peers.len(), 0, "Should find no peers without successful bootstrap");
    
    println!("✓ Lookup without bootstrap test passed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_mixed_bootstrap_nodes() {
    // Test with a mix of reachable and unreachable nodes
    // Create a local DHT node to act as a "reachable" bootstrap node
    let bootstrap_config = DhtConfig {
        bootstrap: vec![],
        bind_port: 0,
    };
    let bootstrap_node = DhtClient::new(bootstrap_config).await.expect("Failed to create bootstrap node");
    let bootstrap_addr = bootstrap_node.local_addr().expect("Failed to get bootstrap address");
    
    // Create a client with mixed bootstrap nodes
    let config = DhtConfig {
        bootstrap: vec![
            format!("{}", bootstrap_addr),        // Reachable (our local node)
            "192.0.2.1:6881".to_string(),        // Unreachable
        ],
        bind_port: 0,
    };
    
    let client = DhtClient::new(config).await.expect("Failed to create client");
    
    // Bootstrap should complete (might timeout on unreachable nodes but succeed on reachable one)
    let bootstrap_result = tokio::time::timeout(
        Duration::from_secs(3),
        client.bootstrap()
    ).await;
    
    // Should not hang indefinitely
    assert!(bootstrap_result.is_ok(), "Bootstrap should complete with mixed nodes");
    
    println!("✓ Mixed bootstrap nodes test passed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_operations_during_bootstrap() {
    // Test that concurrent operations don't interfere with bootstrap
    let config = DhtConfig {
        bootstrap: vec![
            "192.0.2.1:6881".to_string(),  // Unreachable
        ],
        bind_port: 0,
    };
    
    let client = std::sync::Arc::new(DhtClient::new(config).await.expect("Failed to create client"));
    let addr = client.local_addr().expect("Failed to get address");
    
    let topic = Topic::from_key(b"concurrent-test");
    
    // Spawn multiple concurrent operations
    let client1 = client.clone();
    let bootstrap_task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), client1.bootstrap()).await
    });
    
    let client2 = client.clone();
    let announce_task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), client2.announce(topic, addr.port())).await
    });
    
    let client3 = client.clone();
    let lookup_task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), client3.lookup(topic)).await
    });
    
    // Wait for all tasks to complete
    let (bootstrap_result, announce_result, lookup_result) = tokio::join!(
        bootstrap_task,
        announce_task,
        lookup_task
    );
    
    // All should complete without panic
    assert!(bootstrap_result.is_ok(), "Bootstrap task should complete");
    assert!(announce_result.is_ok(), "Announce task should complete");
    assert!(lookup_result.is_ok(), "Lookup task should complete");
    
    println!("✓ Concurrent operations during bootstrap test passed");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_client_shutdown_after_failed_bootstrap() {
    // Test that client can shutdown cleanly after failed bootstrap
    let config = DhtConfig {
        bootstrap: vec!["192.0.2.1:6881".to_string()],
        bind_port: 0,
    };
    
    let client = DhtClient::new(config).await.expect("Failed to create client");
    
    // Try bootstrap (will fail)
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        client.bootstrap()
    ).await;
    
    // Shutdown should complete gracefully
    let shutdown_result = client.shutdown().await;
    assert!(shutdown_result.is_ok(), "Shutdown should succeed after failed bootstrap");
    
    println!("✓ Client shutdown after failed bootstrap test passed");
}
