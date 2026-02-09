//! Wire protocol definitions (scaffold).
//!
//! Hyperswarm's discovery layer uses KRPC-style messages over UDP.
//! This module defines message types and (de)serialization helpers.

use serde::{Deserialize, Serialize};

/// KRPC message envelope.
///
/// NOTE: Real KRPC is bencode-based. This scaffold uses serde-friendly
/// representations; the on-the-wire encoding will be implemented later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrpcMessage {
    pub t: Vec<u8>, // transaction id
    pub y: KrpcMessageType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<KrpcQueryKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<KrpcArgs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<KrpcResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<(i64, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KrpcMessageType {
    Query,
    Response,
    Error,
}

/// Query kinds: ping, find_node, get_peers, announce_peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KrpcQueryKind {
    Ping,
    FindNode,
    GetPeers,
    AnnouncePeer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KrpcArgs {
    /// Node id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Vec<u8>>,
    /// Target node id (find_node).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Vec<u8>>,
    /// Info-hash / topic (get_peers/announce_peer).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_hash: Option<Vec<u8>>,
    /// Announced port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Token from get_peers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KrpcResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Vec<u8>>,
    /// Compact node info.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<u8>>,
    /// Peer values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<Vec<u8>>>,
    /// Token for announce_peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<Vec<u8>>,
}

#[derive(thiserror::Error, Debug)]
pub enum ProtocolError {
    #[error("not implemented")]
    Unimplemented,
}

pub fn encode_krpc(_msg: &KrpcMessage) -> Result<Vec<u8>, ProtocolError> {
    // TODO: implement bencode encoding.
    Err(ProtocolError::Unimplemented)
}

pub fn decode_krpc(_data: &[u8]) -> Result<KrpcMessage, ProtocolError> {
    // TODO: implement bencode decoding.
    Err(ProtocolError::Unimplemented)
}
