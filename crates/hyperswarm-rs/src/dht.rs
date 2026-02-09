//! DHT client scaffold.
//!
//! Hyperswarm uses a Kademlia-like DHT with KRPC messages over UDP.
//! This module provides a minimal client interface for:
//! - bootstrapping into the routing table
//! - announcing on a topic
//! - looking up peers for a topic

use std::net::SocketAddr;

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
    Protocol(String),
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
#[derive(Clone)]
pub struct DhtClient {
    // TODO: socket, routing table, tx manager, etc.
}

impl DhtClient {
    pub async fn new(_config: DhtConfig) -> Result<Self, DhtError> {
        // TODO: bind UDP, generate node id, start receive loop.
        Err(DhtError::Unimplemented)
    }

    /// Join the DHT and populate the routing table from bootstrap nodes.
    pub async fn bootstrap(&self) -> Result<(), DhtError> {
        // TODO: send ping/find_node to bootstrap nodes.
        Err(DhtError::Unimplemented)
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

    // ---- low-level helpers (scaffold) ----

    #[allow(dead_code)]
    async fn send_krpc(&self, _to: SocketAddr, _msg: protocol::KrpcMessage) -> Result<(), DhtError> {
        Err(DhtError::Unimplemented)
    }

    #[allow(dead_code)]
    async fn recv_krpc(&self) -> Result<(SocketAddr, protocol::KrpcMessage), DhtError> {
        Err(DhtError::Unimplemented)
    }
}
