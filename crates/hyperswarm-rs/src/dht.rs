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
}

/// Basic routing table for storing known nodes
struct RoutingTable {
    nodes: Vec<NodeInfo>,
}

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
        if self.nodes.len() > 100 {
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
        })
    }

    /// Join the DHT and populate the routing table from bootstrap nodes.
    pub async fn bootstrap(&self) -> Result<(), DhtError> {
        // Default mainline DHT bootstrap nodes
        let bootstrap_nodes = vec![
            "router.bittorrent.com:6881",
            "dht.transmissionbt.com:6881",
            "router.utorrent.com:6881",
        ];
        
        for node_addr in bootstrap_nodes {
            // Try to resolve and ping each bootstrap node
            if let Ok(addr) = tokio::net::lookup_host(node_addr).await?.next().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "No address found")
            }) {
                // Send ping to bootstrap node
                let _ = self.ping(addr).await;
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
                // Each node is 26 bytes: 20-byte ID + 4-byte IP + 2-byte port
                for chunk in nodes_data.chunks(26) {
                    if chunk.len() == 26 {
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

    /// Announce our presence for `topic`.
    ///
    /// In KRPC terms this often maps to `announce_peer` / topic announce.
    pub async fn announce(&self, _topic: Topic, _port: u16) -> Result<(), DhtError> {
        // TODO: locate closest nodes then announce to them.
        Err(DhtError::Unimplemented)
    }

    /// Lookup peers for `topic`.
    pub async fn lookup(&self, _topic: Topic) -> Result<Vec<PeerAddress>, DhtError> {
        // TODO: iterative get_peers traversal.
        Err(DhtError::Unimplemented)
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
        let mut buf = vec![0u8; 2048];
        let (len, addr) = self.socket.recv_from(&mut buf).await?;
        buf.truncate(len);
        let msg = protocol::decode_krpc(&buf)?;
        Ok((addr, msg))
    }

    async fn recv_response(&self, tx_id: &[u8]) -> Result<protocol::KrpcMessage, DhtError> {
        // Simple implementation: receive messages until we find matching transaction ID
        // In a full implementation, this would use a proper request/response matcher
        for _ in 0..10 {
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
}
