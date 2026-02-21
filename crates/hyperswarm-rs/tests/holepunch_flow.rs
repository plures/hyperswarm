//! Integration test: Holepunch simulation
//!
//! This test verifies the holepunch flow on localhost:
//! 1. Probe phase: send probe packets to create NAT bindings
//! 2. Exchange phase: peers exchange candidate addresses
//! 3. Punch phase: establish direct connection

mod common;

use hyperswarm::holepunch::{Candidate, CandidateKind, HolepunchSession};
use std::time::Duration;

/// Shared session key used for all authenticated holepunch tests.
const TEST_KEY: [u8; 32] = [0xABu8; 32];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_holepunch_initiate_and_respond() {
    // Create two holepunch sessions
    let mut session1 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_KEY)
        .await
        .expect("Failed to create session1");
    let mut session2 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_KEY)
        .await
        .expect("Failed to create session2");
    
    let addr1 = session1.local_addr().expect("Failed to get addr1");
    let addr2 = session2.local_addr().expect("Failed to get addr2");
    
    println!("Session 1 bound to: {}", addr1);
    println!("Session 2 bound to: {}", addr2);
    
    // Each peer has candidates for the other
    let candidates_for_1 = vec![
        Candidate {
            addr: addr1,
            kind: CandidateKind::Lan,
        },
    ];
    
    let candidates_for_2 = vec![
        Candidate {
            addr: addr2,
            kind: CandidateKind::Lan,
        },
    ];
    
    // Start the responder first to ensure it's listening
    let respond_task = tokio::spawn(async move {
        session2.respond(candidates_for_1).await
    });
    
    // Small delay to ensure responder is ready
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Then start the initiator
    let initiate_task = tokio::spawn(async move {
        session1.initiate(candidates_for_2).await
    });
    
    // Wait for both to complete with timeout
    let results = tokio::time::timeout(
        Duration::from_secs(3),
        async { tokio::join!(initiate_task, respond_task) }
    )
    .await
    .expect("Holepunch timed out");
    
    let initiate_result = results.0.expect("Initiate task failed");
    let respond_result = results.1.expect("Respond task failed");
    
    let established_addr1 = initiate_result.expect("Initiate failed");
    let established_addr2 = respond_result.expect("Respond failed");
    
    println!("Session 1 established connection to: {}", established_addr1);
    println!("Session 2 established connection from: {}", established_addr2);
    
    // Verify the addresses match
    assert_eq!(established_addr1, addr2, "Initiator should connect to session 2");
    assert_eq!(established_addr2, addr1, "Responder should connect from session 1");
    
    println!("✓ Holepunch initiate and respond test passed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_holepunch_probe_phase() {
    // Test the probe phase independently
    let mut session = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_KEY)
        .await
        .expect("Failed to create session");
    
    let local_addr = session.local_addr().expect("Failed to get local address");
    println!("Session bound to: {}", local_addr);
    
    // Create some test candidates
    let candidates = vec![
        Candidate {
            addr: "127.0.0.1:9001".parse().unwrap(),
            kind: CandidateKind::Lan,
        },
        Candidate {
            addr: "127.0.0.1:9002".parse().unwrap(),
            kind: CandidateKind::Wan,
        },
        Candidate {
            addr: "127.0.0.1:9003".parse().unwrap(),
            kind: CandidateKind::Relay,
        },
    ];
    
    // Probe should complete without error (even if targets don't exist)
    let result = session.probe(&candidates).await;
    assert!(result.is_ok(), "Probe phase should succeed");
    
    println!("✓ Holepunch probe phase test passed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_holepunch_with_multiple_candidates() {
    // Test holepunch with multiple candidate addresses
    let mut session1 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_KEY)
        .await
        .expect("Failed to create session1");
    let mut session2 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_KEY)
        .await
        .expect("Failed to create session2");
    
    let addr1 = session1.local_addr().expect("Failed to get addr1");
    let addr2 = session2.local_addr().expect("Failed to get addr2");
    
    // Session 2 has multiple candidates (simulating different network paths)
    let candidates_for_2 = vec![
        Candidate {
            addr: "127.0.0.1:9999".parse().unwrap(), // Wrong address
            kind: CandidateKind::Wan,
        },
        Candidate {
            addr: addr2, // Correct address
            kind: CandidateKind::Lan,
        },
    ];
    
    let candidates_for_1 = vec![
        Candidate {
            addr: addr1,
            kind: CandidateKind::Lan,
        },
    ];
    
    // Start responder first
    let respond_task = tokio::spawn(async move {
        session2.respond(candidates_for_1).await
    });
    
    // Small delay to ensure responder is ready
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Then start initiator
    let initiate_task = tokio::spawn(async move {
        session1.initiate(candidates_for_2).await
    });
    
    // Wait for completion
    let results = tokio::time::timeout(
        Duration::from_secs(3),
        async { tokio::join!(initiate_task, respond_task) }
    )
    .await
    .expect("Holepunch timed out");
    
    let initiate_result = results.0.expect("Task failed");
    let respond_result = results.1.expect("Task failed");
    
    // Should successfully establish connection using the correct candidate
    let established_addr = initiate_result.expect("Initiate failed");
    let _ = respond_result.expect("Respond failed");
    
    assert_eq!(established_addr, addr2, "Should connect to correct candidate");
    
    println!("✓ Holepunch with multiple candidates test passed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_holepunch_timeout_with_no_candidates() {
    // Test that holepunch fails gracefully when no viable candidates
    let mut session = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_KEY)
        .await
        .expect("Failed to create session");
    
    let empty_candidates = vec![];
    
    let result = session.initiate(empty_candidates).await;
    assert!(result.is_err(), "Should fail with no candidates");
    
    println!("✓ Holepunch with no candidates test passed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_holepunch_mismatched_keys_fail() {
    // If the two sessions use different session keys the MAC verification will
    // fail on both sides, so neither peer will accept the other's punch packet.
    // The initiator should time out (no valid response from responder).

    let key_a = [0x11u8; 32];
    let key_b = [0x22u8; 32]; // different key

    let mut session1 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), key_a)
        .await
        .expect("Failed to create session1");
    let mut session2 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), key_b)
        .await
        .expect("Failed to create session2");

    let addr1 = session1.local_addr().expect("Failed to get addr1");
    let addr2 = session2.local_addr().expect("Failed to get addr2");

    let candidates_for_2 = vec![Candidate { addr: addr2, kind: CandidateKind::Lan }];
    let candidates_for_1 = vec![Candidate { addr: addr1, kind: CandidateKind::Lan }];

    // Run both sides concurrently; both should time out or fail.
    let respond_task = tokio::spawn(async move {
        session2.respond(candidates_for_1).await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let initiate_task = tokio::spawn(async move {
        session1.initiate(candidates_for_2).await
    });

    // Allow up to 12 seconds: the responder's PUNCH_TIMEOUT is 10 s.
    let results = tokio::time::timeout(
        std::time::Duration::from_secs(12),
        async { tokio::join!(initiate_task, respond_task) },
    )
    .await
    .expect("test timed out waiting for mismatched sessions to fail");

    let initiate_result = results.0.expect("initiate task panicked");
    let respond_result = results.1.expect("respond task panicked");

    assert!(
        initiate_result.is_err(),
        "initiator should fail when session keys don't match"
    );
    assert!(
        respond_result.is_err(),
        "responder should fail when session keys don't match"
    );

    println!("✓ Holepunch with mismatched keys correctly fails");
}
