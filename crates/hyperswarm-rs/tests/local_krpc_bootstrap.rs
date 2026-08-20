//! Deterministic localhost KRPC fixture for the production DHT client.
//!
//! This exercises bootstrap, `get_peers`, and `announce_peer` over a real UDP
//! socket without depending on the public DHT or manually populating a client
//! routing table.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use hyperswarm::{
    dht::{DhtClient, DhtConfig},
    protocol::{
        decode_krpc, encode_krpc, KrpcMessage, KrpcMessageType, KrpcQueryKind, KrpcResponse,
    },
    Hyperswarm, SwarmConfig, Topic,
};
use tokio::{net::UdpSocket, sync::Mutex, task::JoinHandle};

const FIXTURE_NODE_ID: [u8; 20] = [7; 20];
const FIXTURE_TOKEN: &[u8] = b"local-krpc-token";

struct LocalKrpcBootstrap {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl LocalKrpcBootstrap {
    async fn start() -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("local KRPC fixture should bind");
        let addr = socket
            .local_addr()
            .expect("local KRPC fixture should have an address");
        let peers = Arc::new(Mutex::new(HashMap::<Vec<u8>, Vec<SocketAddr>>::new()));
        let task = tokio::spawn(run_fixture(socket, peers));

        Self { addr, task }
    }
}

impl Drop for LocalKrpcBootstrap {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run_fixture(socket: UdpSocket, peers: Arc<Mutex<HashMap<Vec<u8>, Vec<SocketAddr>>>>) {
    let mut buffer = [0_u8; 2048];

    loop {
        let Ok((length, source)) = socket.recv_from(&mut buffer).await else {
            break;
        };
        let Ok(request) = decode_krpc(&buffer[..length]) else {
            continue;
        };
        let Some(query) = request.q else {
            continue;
        };
        let args = request.a.unwrap_or_default();

        let response = match query {
            KrpcQueryKind::Ping => response(request.t, KrpcResponse::default()),
            KrpcQueryKind::GetPeers => {
                let announced = match args.info_hash.as_ref() {
                    Some(topic) => peers.lock().await.get(topic).cloned().unwrap_or_default(),
                    None => Vec::new(),
                };
                let values = announced.into_iter().map(compact_ipv4_peer).collect();
                response(
                    request.t,
                    KrpcResponse {
                        values: Some(values),
                        token: Some(FIXTURE_TOKEN.to_vec()),
                        ..Default::default()
                    },
                )
            }
            KrpcQueryKind::AnnouncePeer => {
                if args.token.as_deref() != Some(FIXTURE_TOKEN) {
                    error_response(request.t, "fixture token was not returned by get_peers")
                } else if let (Some(topic), Some(port)) = (args.info_hash, args.port) {
                    peers
                        .lock()
                        .await
                        .entry(topic)
                        .or_default()
                        .push(SocketAddr::new(source.ip(), port));
                    response(request.t, KrpcResponse::default())
                } else {
                    error_response(request.t, "announce_peer missing topic or port")
                }
            }
            KrpcQueryKind::FindNode => response(request.t, KrpcResponse::default()),
        };

        let encoded = encode_krpc(&response).expect("fixture responses must encode");
        socket
            .send_to(&encoded, source)
            .await
            .expect("fixture response must send");
    }
}

fn response(transaction_id: Vec<u8>, mut body: KrpcResponse) -> KrpcMessage {
    body.id = Some(FIXTURE_NODE_ID.to_vec());
    KrpcMessage {
        t: transaction_id,
        y: KrpcMessageType::Response,
        q: None,
        a: None,
        r: Some(body),
        e: None,
    }
}

fn error_response(transaction_id: Vec<u8>, message: &str) -> KrpcMessage {
    KrpcMessage {
        t: transaction_id,
        y: KrpcMessageType::Error,
        q: None,
        a: None,
        r: None,
        e: Some((203, message.to_string())),
    }
}

fn compact_ipv4_peer(peer: SocketAddr) -> Vec<u8> {
    let IpAddr::V4(ip) = peer.ip() else {
        panic!("the local fixture only stores IPv4 peers");
    };
    let mut compact = ip.octets().to_vec();
    compact.extend_from_slice(&peer.port().to_be_bytes());
    compact
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn announce_then_lookup_uses_real_local_krpc_bootstrap() {
    let fixture = LocalKrpcBootstrap::start().await;
    let config = || DhtConfig {
        bootstrap: vec![fixture.addr.to_string()],
        bind_port: 0,
    };
    let announcing_client = DhtClient::new(config())
        .await
        .expect("announcing client should start");
    let lookup_client = DhtClient::new(config())
        .await
        .expect("lookup client should start");
    let topic = Topic::from_key(b"deterministic-local-krpc-bootstrap");
    let announced_addr = announcing_client
        .local_addr()
        .expect("announcing client should have an address");

    announcing_client
        .announce(topic, announced_addr.port())
        .await
        .expect("announce should complete through the local bootstrap");

    let peers = lookup_client
        .lookup(topic)
        .await
        .expect("lookup should complete through the local bootstrap");

    assert_eq!(peers.len(), 1, "lookup must return the announced peer");
    assert_eq!(
        peers[0].addr,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), announced_addr.port())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_announces_the_bound_udp_port() {
    let fixture = LocalKrpcBootstrap::start().await;
    let topic = Topic::from_key(b"discovery-announces-bound-udp-port");
    let swarm = Hyperswarm::new(SwarmConfig {
        bootstrap: vec![fixture.addr.to_string()],
        port: 0,
        max_peers: 1,
    })
    .await
    .expect("swarm should start against the local bootstrap");

    swarm
        .join(topic)
        .await
        .expect("discovery join should announce through the local bootstrap");

    let verifier = DhtClient::new(DhtConfig {
        bootstrap: vec![fixture.addr.to_string()],
        bind_port: 0,
    })
    .await
    .expect("verifier should start");
    let peers = verifier
        .lookup(topic)
        .await
        .expect("verifier lookup should complete");

    assert_eq!(
        peers.len(),
        1,
        "discovery must publish a usable peer record"
    );
    assert_eq!(peers[0].addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(
        peers[0].addr.port(),
        0,
        "a DHT peer record cannot advertise port zero"
    );
}
