//! Public managed-connection acceptance through the Hyperswarm facade.
//!
//! This is deliberately local and deterministic: it proves that a caller can
//! select a discovered peer, establish a topic-bound authenticated stream, and
//! exchange encrypted data. Public-DHT/NAT behaviour is covered by the
//! separate lab acceptance gate.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use hyperswarm::{dht::PeerAddress, Hyperswarm, SwarmConfig, Topic};

fn local_config() -> SwarmConfig {
    SwarmConfig {
        bootstrap: Vec::new(),
        port: 0,
        max_peers: 2,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_peer_establishes_a_topic_bound_managed_stream() {
    let client = Hyperswarm::new(local_config())
        .await
        .expect("client swarm should start");
    let server = Arc::new(
        Hyperswarm::new(local_config())
            .await
            .expect("server swarm should start"),
    );
    let topic = Topic::from_key(b"hyperswarm-managed-public-api");
    let server_addr = server
        .local_addr()
        .expect("server should expose its managed listener");
    let selected_peer = PeerAddress {
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), server_addr.port()),
        node_id: None,
    };

    let accepting = Arc::clone(&server);
    let accept = tokio::spawn(async move { accepting.accept(topic).await });
    let mut outbound = client
        .connect(topic, selected_peer, None)
        .await
        .expect("caller-selected peer should complete the Noise handshake");
    let mut inbound = tokio::time::timeout(Duration::from_secs(2), accept)
        .await
        .expect("server should accept promptly")
        .expect("server accept task should not panic")
        .expect("matching topic should establish a managed stream");

    outbound
        .send(Bytes::from_static(b"managed public api"))
        .await
        .expect("outbound stream should encrypt and send");
    assert_eq!(
        inbound
            .recv()
            .await
            .expect("inbound stream should decrypt the payload"),
        Bytes::from_static(b"managed public api")
    );
    assert!(
        outbound.remote_static_key().is_some(),
        "the outbound stream must expose the Noise-authenticated peer key"
    );
}
