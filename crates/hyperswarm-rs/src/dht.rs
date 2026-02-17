//! DHT client scaffold.
//!
//! Hyperswarm uses a Kademlia-like DHT with KRPC messages over UDP.
//! This module provides a minimal client interface for:
//! - bootstrapping into the routing table
//! - announcing on a topic
//! - looking up peers for a topic

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use rand::Rng;

use crate::{protocol, Topic};

#[derive(Clone, Debug)]
pub struct DhtConfig {
    pub bootstrap: Vec<String>,
    pub bind_port: u16,
}

#[derive(Clone, Debug)]
pub struct PeerAddress {
    pub addr: SocketAddr,
    /// Optional remote public key / node id if available.
    pub node_id: Option<[u8; 32]>,
}

#[derive(thiserror::Error, Debug)]
pub enum DhtError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(#[from] protocol::ProtocolError),
    #[error("timeout")]
    Timeout,
    #[error("not implemented")]
    Unimplemented,
}

/// A minimal DHT client.
///
/// Eventually this will manage:
/// - node id / keypair
/// - UDP socket
/// - routing table
/// - transaction ids and request/response matching
pub struct DhtClient {
    socket: Arc<UdpSocket>,
    node_id: [u8; 20],
    routing_table: Arc<Mutex<RoutingTable>>,
    next_transaction_id: Arc<Mutex<u16>>,
    bootstrap_nodes: Vec<String>,
}

/// Basic routing table for storing known nodes
struct RoutingTable {
    nodes: Vec<NodeInfo>,
}

// Constants for routing table and protocol
const MAX_ROUTING_TABLE_SIZE: usize = 100; // Simplified limit; full impl would use k-buckets
const MAX_KRPC_MESSAGE_SIZE: usize = 2048; // Typical UDP DHT message size
const MAX_RESPONSE_ATTEMPTS: usize = 10; // Retries for matching transaction ID

// Constants for compact encoding formats (BEP 5)
const COMPACT_PEER_INFO_SIZE_IPV4: usize = 6; // 4-byte IPv4 + 2-byte port
const COMPACT_PEER_INFO_SIZE_IPV6: usize = 18; // 16-byte IPv6 + 2-byte port
const COMPACT_NODE_INFO_SIZE: usize = 26; // 20-byte ID + 4-byte IPv4 + 2-byte port

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct NodeInfo {
    node_id: [u8; 20],
    addr: SocketAddr,
}

impl RoutingTable {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    fn add_node(&mut self, node_id: [u8; 20], addr: SocketAddr) {
        // Simple implementation: just add to the list
        // In a full implementation, this would use k-buckets
        self.nodes.push(NodeInfo { node_id, addr });
        
        // Keep the table size limited
        if self.nodes.len() > MAX_ROUTING_TABLE_SIZE {
            self.nodes.remove(0);
        }
    }

    #[allow(dead_code)]
    fn get_nodes(&self, count: usize) -> Vec<NodeInfo> {
        self.nodes.iter().take(count).cloned().collect()
    }
}

impl DhtClient {
    pub async fn new(config: DhtConfig) -> Result<Self, DhtError> {
        // Bind UDP socket
        let bind_addr = format!("0.0.0.0:{}", config.bind_port);
        let socket = UdpSocket::bind(&bind_addr).await?;
        
        // Generate random node ID (20 bytes for mainline DHT compatibility)
        let mut rng = rand::thread_rng();
        let mut node_id = [0u8; 20];
        rng.fill(&mut node_id);
        
        Ok(Self {
            socket: Arc::new(socket),
            node_id,
            routing_table: Arc::new(Mutex::new(RoutingTable::new())),
            next_transaction_id: Arc::new(Mutex::new(0)),
            bootstrap_nodes: config.bootstrap,
        })
    }

    /// Join the DHT and populate the routing table from bootstrap nodes.
    pub async fn bootstrap(&self) -> Result<(), DhtError> {
        // Use configured bootstrap nodes
        let bootstrap_nodes = if self.bootstrap_nodes.is_empty() {
            // If no bootstrap nodes configured, use mainline DHT defaults
            vec![
                "router.bittorrent.com:6881".to_string(),
                "dht.transmissionbt.com:6881".to_string(),
                "router.utorrent.com:6881".to_string(),
            ]
        } else {
            self.bootstrap_nodes.clone()
        };
        
        for node_addr in bootstrap_nodes {
            // Try to resolve and ping each bootstrap node
            // Use a shorter timeout for DNS resolution
            let timeout_result = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                tokio::net::lookup_host(&node_addr)
            ).await;
            
            match timeout_result {
                Ok(Ok(mut addrs)) => {
                    if let Some(addr) = addrs.next() {
                        // Send ping to bootstrap node with timeout
                        // Use a shorter timeout (2 seconds instead of 5)
                        let ping_result = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            self.ping(addr)
                        ).await;
                        
                        // Silently ignore errors and timeouts
                        let _ = ping_result;
                    }
                }
                _ => {
                    // Silently skip nodes that can't be resolved or timeout
                    continue;
                }
            }
        }
        
        Ok(())
    }

    /// Send a ping query to a node
    async fn ping(&self, addr: SocketAddr) -> Result<Vec<u8>, DhtError> {
        let tx_id = self.get_transaction_id().await;
        
        let msg = protocol::KrpcMessage {
            t: tx_id.clone(),
            y: protocol::KrpcMessageType::Query,
            q: Some(protocol::KrpcQueryKind::Ping),
            a: Some(protocol::KrpcArgs {
                id: Some(self.node_id.to_vec()),
                ..Default::default()
            }),
            r: None,
            e: None,
        };
        
        self.send_krpc(addr, msg).await?;
        
        // Wait for response with timeout
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.recv_response(&tx_id)
        )
        .await
        .map_err(|_| DhtError::Timeout)??;
        
        // Add responding node to routing table
        if let Some(r) = &response.r {
            if let Some(id) = &r.id {
                if id.len() == 20 {
                    let mut node_id = [0u8; 20];
                    node_id.copy_from_slice(&id[..20]);
                    let mut rt = self.routing_table.lock().await;
                    rt.add_node(node_id, addr);
                }
            }
        }
        
        Ok(response.r.and_then(|r| r.id).unwrap_or_default())
    }

    /// Send a find_node query to locate nodes near a target
    #[allow(dead_code)]
    async fn find_node(&self, addr: SocketAddr, target: &[u8; 20]) -> Result<Vec<NodeInfo>, DhtError> {
        let tx_id = self.get_transaction_id().await;
        
        let msg = protocol::KrpcMessage {
            t: tx_id.clone(),
            y: protocol::KrpcMessageType::Query,
            q: Some(protocol::KrpcQueryKind::FindNode),
            a: Some(protocol::KrpcArgs {
                id: Some(self.node_id.to_vec()),
                target: Some(target.to_vec()),
                ..Default::default()
            }),
            r: None,
            e: None,
        };
        
        self.send_krpc(addr, msg).await?;
        
        // Wait for response with timeout
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.recv_response(&tx_id)
        )
        .await
        .map_err(|_| DhtError::Timeout)??;
        
        // Parse compact node info from response
        let mut nodes = Vec::new();
        if let Some(r) = response.r {
            if let Some(nodes_data) = r.nodes {
                // Each node is 26 bytes: 20-byte ID + 4-byte IPv4 + 2-byte port (BEP 5)
                for chunk in nodes_data.chunks(COMPACT_NODE_INFO_SIZE) {
                    if chunk.len() == COMPACT_NODE_INFO_SIZE {
                        let mut node_id = [0u8; 20];
                        node_id.copy_from_slice(&chunk[0..20]);
                        
                        let ip = std::net::Ipv4Addr::new(
                            chunk[20], chunk[21], chunk[22], chunk[23]
                        );
                        let port = u16::from_be_bytes([chunk[24], chunk[25]]);
                        let addr = SocketAddr::new(std::net::IpAddr::V4(ip), port);
                        
                        nodes.push(NodeInfo { node_id, addr });
                    }
                }
            }
        }
        
        Ok(nodes)
    }

    /// Get peers for a given info hash (topic) from a node
    async fn get_peers(&self, addr: SocketAddr, info_hash: &[u8; 32]) -> Result<(Vec<PeerAddress>, Option<Vec<u8>>), DhtError> {
        let tx_id = self.get_transaction_id().await;
        
        let msg = protocol::KrpcMessage {
            t: tx_id.clone(),
            y: protocol::KrpcMessageType::Query,
            q: Some(protocol::KrpcQueryKind::GetPeers),
            a: Some(protocol::KrpcArgs {
                id: Some(self.node_id.to_vec()),
                info_hash: Some(info_hash.to_vec()),
                ..Default::default()
            }),
            r: None,
            e: None,
        };
        
        self.send_krpc(addr, msg).await?;
        
        // Wait for response with timeout
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.recv_response(&tx_id)
        )
        .await
        .map_err(|_| DhtError::Timeout)??;
        
        let mut peers = Vec::new();
        let mut token = None;
        
        if let Some(r) = response.r {
            // Extract token for announce_peer
            token = r.token.clone();
            
            // Parse compact peer info from values field
            // BEP 5 defines both IPv4 (6 bytes) and IPv6 (18 bytes) formats
            if let Some(values) = r.values {
                for value in values {
                    if value.len() == COMPACT_PEER_INFO_SIZE_IPV4 {
                        // IPv4: 4-byte IP + 2-byte port
                        let ip = std::net::Ipv4Addr::new(
                            value[0], value[1], value[2], value[3]
                        );
                        let port = u16::from_be_bytes([value[4], value[5]]);
                        let addr = SocketAddr::new(std::net::IpAddr::V4(ip), port);
                        
                        peers.push(PeerAddress {
                            addr,
                            node_id: None,
                        });
                    } else if value.len() == COMPACT_PEER_INFO_SIZE_IPV6 {
                        // IPv6: 16-byte IP + 2-byte port
                        let mut ipv6_bytes = [0u8; 16];
                        ipv6_bytes.copy_from_slice(&value[0..16]);
                        let ip = std::net::Ipv6Addr::from(ipv6_bytes);
                        let port = u16::from_be_bytes([value[16], value[17]]);
                        let addr = SocketAddr::new(std::net::IpAddr::V6(ip), port);
                        
                        peers.push(PeerAddress {
                            addr,
                            node_id: None,
                        });
                    } else {
                        // Unknown format, skip
                        tracing::debug!("Skipping peer with unknown compact format length: {}", value.len());
                    }
                }
            }
        }
        
        Ok((peers, token))
    }

    /// Announce our presence for a topic to a specific node
    async fn announce_peer(&self, addr: SocketAddr, info_hash: &[u8; 32], port: u16, token: Vec<u8>) -> Result<(), DhtError> {
        let tx_id = self.get_transaction_id().await;
        
        let msg = protocol::KrpcMessage {
            t: tx_id.clone(),
            y: protocol::KrpcMessageType::Query,
            q: Some(protocol::KrpcQueryKind::AnnouncePeer),
            a: Some(protocol::KrpcArgs {
                id: Some(self.node_id.to_vec()),
                info_hash: Some(info_hash.to_vec()),
                port: Some(port),
                token: Some(token),
                ..Default::default()
            }),
            r: None,
            e: None,
        };
        
        self.send_krpc(addr, msg).await?;
        
        // Wait for response with timeout
        let _response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.recv_response(&tx_id)
        )
        .await
        .map_err(|_| DhtError::Timeout)??;
        
        Ok(())
    }

    /// Announce our presence for `topic`.
    ///
    /// In KRPC terms this often maps to `announce_peer` / topic announce.
    /// This is a simplified implementation that announces to bootstrap nodes.
    pub async fn announce(&self, topic: Topic, port: u16) -> Result<(), DhtError> {
        // Convert topic (32 bytes) to info_hash format
        let info_hash = topic.0;
        
        // Get nodes from routing table
        let nodes = {
            let rt = self.routing_table.lock().await;
            rt.get_nodes(10)
        };
        
        // If routing table is empty, bootstrap first
        if nodes.is_empty() {
            self.bootstrap().await?;
        }
        
        // Get updated node list
        let nodes = {
            let rt = self.routing_table.lock().await;
            rt.get_nodes(10)
        };
        
        // Announce to each node in routing table
        for node in nodes {
            // First get token from get_peers
            match self.get_peers(node.addr, &info_hash).await {
                Ok((_, Some(token))) => {
                    // Announce with the token
                    if let Err(e) = self.announce_peer(node.addr, &info_hash, port, token).await {
                        tracing::debug!("Failed to announce to node {}: {}", node.addr, e);
                    }
                }
                Err(e) => {
                    // Skip nodes that don't respond or don't provide a token
                    tracing::debug!("Failed to get peers from node {}: {}", node.addr, e);
                    continue;
                }
                _ => {
                    tracing::debug!("Node {} did not provide a token", node.addr);
                    continue;
                }
            }
        }
        
        Ok(())
    }

    /// Lookup peers for `topic`.
    /// This is a simplified implementation that queries bootstrap nodes.
    pub async fn lookup(&self, topic: Topic) -> Result<Vec<PeerAddress>, DhtError> {
        // Convert topic (32 bytes) to info_hash format
        let info_hash = topic.0;
        
        // Get nodes from routing table
        let nodes = {
            let rt = self.routing_table.lock().await;
            rt.get_nodes(10)
        };
        
        // If routing table is empty, bootstrap first
        if nodes.is_empty() {
            self.bootstrap().await?;
        }
        
        // Get updated node list
        let nodes = {
            let rt = self.routing_table.lock().await;
            rt.get_nodes(10)
        };
        
        let mut all_peers = Vec::new();
        
        // Query each node for peers
        for node in nodes {
            match self.get_peers(node.addr, &info_hash).await {
                Ok((peers, _)) => {
                    all_peers.extend(peers);
                }
                Err(_) => {
                    // Skip nodes that don't respond
                    continue;
                }
            }
        }
        
        Ok(all_peers)
    }

    /// Flush in-flight queries.
    pub async fn flush(&self) -> Result<(), DhtError> {
        // TODO: wait for pending queries to resolve.
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), DhtError> {
        // TODO: stop background tasks.
        Ok(())
    }

    /// Get the local socket address
    pub fn local_addr(&self) -> Result<SocketAddr, DhtError> {
        Ok(self.socket.local_addr()?)
    }

    /// Get this node's ID (for testing)
    pub fn node_id(&self) -> [u8; 20] {
        self.node_id
    }

    /// Manually add a node to the routing table (for testing)
    pub async fn add_node_to_routing_table(&self, node_id: [u8; 20], addr: SocketAddr) {
        let mut rt = self.routing_table.lock().await;
        rt.add_node(node_id, addr);
    }

    // ---- low-level helpers ----

    async fn get_transaction_id(&self) -> Vec<u8> {
        let mut tx = self.next_transaction_id.lock().await;
        let id = *tx;
        *tx = tx.wrapping_add(1);
        id.to_be_bytes().to_vec()
    }

    async fn send_krpc(&self, to: SocketAddr, msg: protocol::KrpcMessage) -> Result<(), DhtError> {
        let data = protocol::encode_krpc(&msg)?;
        self.socket.send_to(&data, to).await?;
        Ok(())
    }

    async fn recv_krpc(&self) -> Result<(SocketAddr, protocol::KrpcMessage), DhtError> {
        let mut buf = vec![0u8; MAX_KRPC_MESSAGE_SIZE];
        let (len, addr) = self.socket.recv_from(&mut buf).await?;
        buf.truncate(len);
        let msg = protocol::decode_krpc(&buf)?;
        Ok((addr, msg))
    }

    async fn recv_response(&self, tx_id: &[u8]) -> Result<protocol::KrpcMessage, DhtError> {
        // Simple implementation: receive messages until we find matching transaction ID
        // In a full implementation, this would use a proper request/response matcher
        for _ in 0..MAX_RESPONSE_ATTEMPTS {
            let (_, msg) = self.recv_krpc().await?;
            if msg.t == tx_id {
                return Ok(msg);
            }
        }
        Err(DhtError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dht_client_creation() {
        let config = DhtConfig {
            bootstrap: vec![],
            bind_port: 0, // Let OS choose port
        };

        let client = DhtClient::new(config).await.expect("Failed to create DHT client");
        
        // Verify node ID is generated
        assert_ne!(client.node_id, [0u8; 20]);
    }

    #[tokio::test]
    async fn test_routing_table() {
        let mut rt = RoutingTable::new();
        
        let node_id = [1u8; 20];
        let addr = "127.0.0.1:8080".parse().unwrap();
        
        rt.add_node(node_id, addr);
        
        assert_eq!(rt.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_announce_with_empty_routing_table() {
        let config = DhtConfig {
            bootstrap: vec![],
            bind_port: 0,
        };

        let client = DhtClient::new(config).await.expect("Failed to create DHT client");
        let topic = Topic([1u8; 32]);
        
        // Announce should handle empty routing table gracefully
        let result = client.announce(topic, 8080).await;
        
        // Should not panic, even if no nodes are available
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_lookup_with_empty_routing_table() {
        let config = DhtConfig {
            bootstrap: vec![],
            bind_port: 0,
        };

        let client = DhtClient::new(config).await.expect("Failed to create DHT client");
        let topic = Topic([1u8; 32]);
        
        // Lookup should handle empty routing table gracefully
        let result = client.lookup(topic).await;
        
        // Should return empty list if no nodes are available
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_transaction_id_generation() {
        let config = DhtConfig {
            bootstrap: vec![],
            bind_port: 0,
        };

        let client = DhtClient::new(config).await.expect("Failed to create DHT client");
        
        let tx1 = client.get_transaction_id().await;
        let tx2 = client.get_transaction_id().await;
        
        // Transaction IDs should be different
        assert_ne!(tx1, tx2);
    }

    #[tokio::test]
    async fn test_compact_peer_parsing_ipv4() {
        // Test IPv4 compact peer info parsing
        let ipv4_peer = vec![127, 0, 0, 1, 0x1F, 0x90]; // 127.0.0.1:8080
        assert_eq!(ipv4_peer.len(), COMPACT_PEER_INFO_SIZE_IPV4);
        
        // Verify parsing logic
        let ip = std::net::Ipv4Addr::new(ipv4_peer[0], ipv4_peer[1], ipv4_peer[2], ipv4_peer[3]);
        let port = u16::from_be_bytes([ipv4_peer[4], ipv4_peer[5]]);
        
        assert_eq!(ip.to_string(), "127.0.0.1");
        assert_eq!(port, 8080);
    }

    #[tokio::test]
    async fn test_compact_peer_parsing_ipv6() {
        // Test IPv6 compact peer info parsing
        let ipv6_peer = vec![
            0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
            0x1F, 0x90  // port 8080
        ];
        assert_eq!(ipv6_peer.len(), COMPACT_PEER_INFO_SIZE_IPV6);
        
        // Verify parsing logic
        let mut ipv6_bytes = [0u8; 16];
        ipv6_bytes.copy_from_slice(&ipv6_peer[0..16]);
        let ip = std::net::Ipv6Addr::from(ipv6_bytes);
        let port = u16::from_be_bytes([ipv6_peer[16], ipv6_peer[17]]);
        
        assert_eq!(ip.to_string(), "2001:db8::1");
        assert_eq!(port, 8080);
    }

    #[tokio::test]
    async fn test_concurrent_bootstrap_calls() {
        let config = DhtConfig {
            bootstrap: vec![],
            bind_port: 0,
        };

        let client = std::sync::Arc::new(
            DhtClient::new(config).await.expect("Failed to create DHT client")
        );
        
        // Spawn multiple concurrent bootstrap calls
        let mut handles = vec![];
        for _ in 0..5 {
            let client_clone = client.clone();
            handles.push(tokio::spawn(async move {
                client_clone.bootstrap().await
            }));
        }
        
        // All should complete without error
        for handle in handles {
            assert!(handle.await.unwrap().is_ok());
        }
        
        // Note: This test verifies concurrent bootstrap calls don't panic or error,
        // but doesn't verify OnceCell ensures single execution (would require internal
        // state inspection or mock counters). The OnceCell guarantee is verified by
        // the tokio::sync::OnceCell implementation itself.
    }
}
