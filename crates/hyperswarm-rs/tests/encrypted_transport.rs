//! Integration test: Encrypted stream round-trip
//!
//! This test verifies that two peers can:
//! 1. Establish a Noise XX encrypted stream
//! 2. Send encrypted payloads in both directions
//! 3. Verify payload integrity

mod common;

use bytes::Bytes;
use hyperswarm::transport::EncryptedStream;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_encrypted_stream_round_trip() {
    // Create two UDP sockets for peer-to-peer communication
    let socket1 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind socket1"));
    let socket2 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind socket2"));
    
    let addr1 = socket1.local_addr().expect("Failed to get addr1");
    let addr2 = socket2.local_addr().expect("Failed to get addr2");
    
    println!("Peer 1 bound to: {}", addr1);
    println!("Peer 2 bound to: {}", addr2);
    
    // Create encrypted streams
    let mut stream1 = EncryptedStream::new(socket1.clone(), addr2)
        .await
        .expect("Failed to create stream1");
    let mut stream2 = EncryptedStream::new(socket2.clone(), addr1)
        .await
        .expect("Failed to create stream2");
    
    // Spawn handshake tasks concurrently
    let handshake1 = tokio::spawn(async move {
        let result = stream1.handshake_initiator(None).await;
        (stream1, result)
    });
    
    let handshake2 = tokio::spawn(async move {
        let result = stream2.handshake_responder().await;
        (stream2, result)
    });
    
    // Wait for both handshakes with timeout
    let results = tokio::time::timeout(
        Duration::from_secs(3),
        async {
            tokio::join!(handshake1, handshake2)
        }
    )
    .await
    .expect("Handshake timed out");
    
    let (stream1, result1) = results.0.expect("Handshake task 1 failed");
    let (stream2, result2) = results.1.expect("Handshake task 2 failed");
    
    result1.expect("Handshake 1 failed");
    result2.expect("Handshake 2 failed");
    
    let mut stream1 = stream1;
    let mut stream2 = stream2;
    
    println!("✓ Handshake completed successfully");
    
    // Test data exchange: Peer 1 -> Peer 2
    let message1 = Bytes::from("Hello from Peer 1!");
    println!("Peer 1 sending: {:?}", String::from_utf8_lossy(&message1));
    
    stream1.send(message1.clone()).await.expect("Failed to send from peer 1");
    let received1 = stream2.recv().await.expect("Failed to receive at peer 2");
    
    println!("Peer 2 received: {:?}", String::from_utf8_lossy(&received1));
    assert_eq!(received1, Bytes::from("Hello from Peer 1!"));
    
    // Test data exchange: Peer 2 -> Peer 1 (reverse direction)
    let message2 = Bytes::from("Hello back from Peer 2!");
    println!("Peer 2 sending: {:?}", String::from_utf8_lossy(&message2));
    
    stream2.send(message2.clone()).await.expect("Failed to send from peer 2");
    let received2 = stream1.recv().await.expect("Failed to receive at peer 1");
    
    println!("Peer 1 received: {:?}", String::from_utf8_lossy(&received2));
    assert_eq!(received2, Bytes::from("Hello back from Peer 2!"));
    
    println!("✓ Encrypted stream round-trip test passed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_encrypted_stream_multiple_messages() {
    // Create two UDP sockets
    let socket1 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind socket1"));
    let socket2 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind socket2"));
    
    let addr1 = socket1.local_addr().expect("Failed to get addr1");
    let addr2 = socket2.local_addr().expect("Failed to get addr2");
    
    // Create and handshake encrypted streams
    let mut stream1 = EncryptedStream::new(socket1, addr2).await.expect("Failed to create stream1");
    let mut stream2 = EncryptedStream::new(socket2, addr1).await.expect("Failed to create stream2");
    
    let handshake1 = tokio::spawn(async move {
        let result = stream1.handshake_initiator(None).await;
        (stream1, result)
    });
    let handshake2 = tokio::spawn(async move {
        let result = stream2.handshake_responder().await;
        (stream2, result)
    });
    
    let results = tokio::time::timeout(
        Duration::from_secs(3),
        async { tokio::join!(handshake1, handshake2) }
    )
    .await
    .expect("Handshake timed out");
    
    let (stream1, result1) = results.0.expect("Task 1 failed");
    let (stream2, result2) = results.1.expect("Task 2 failed");
    
    result1.expect("Handshake 1 failed");
    result2.expect("Handshake 2 failed");
    
    let mut stream1 = stream1;
    let mut stream2 = stream2;
    
    // Send multiple messages in sequence
    for i in 0..5 {
        let message = Bytes::from(format!("Message {}", i));
        stream1.send(message.clone()).await.expect("Failed to send");
        let received = stream2.recv().await.expect("Failed to receive");
        assert_eq!(received, message, "Message {} mismatch", i);
    }
    
    println!("✓ Multiple encrypted messages test passed");
}
