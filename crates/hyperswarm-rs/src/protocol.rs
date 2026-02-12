//! Wire protocol definitions (scaffold).
//!
//! Hyperswarm's discovery layer uses KRPC-style messages over UDP.
//! This module defines message types and (de)serialization helpers.

use serde::{Deserialize, Serialize};
use serde_bencode::{de, ser};

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
    #[error("bencode encode error: {0}")]
    BencodeEncode(String),
    #[error("bencode decode error: {0}")]
    BencodeDecode(String),
}

pub fn encode_krpc(msg: &KrpcMessage) -> Result<Vec<u8>, ProtocolError> {
    ser::to_bytes(msg).map_err(|e| ProtocolError::BencodeEncode(e.to_string()))
}

pub fn decode_krpc(data: &[u8]) -> Result<KrpcMessage, ProtocolError> {
    de::from_bytes(data).map_err(|e| ProtocolError::BencodeDecode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_krpc_ping_encode_decode() {
        let msg = KrpcMessage {
            t: vec![1, 2],
            y: KrpcMessageType::Query,
            q: Some(KrpcQueryKind::Ping),
            a: Some(KrpcArgs {
                id: Some(vec![0; 20]),
                ..Default::default()
            }),
            r: None,
            e: None,
        };

        // Encode
        let encoded = encode_krpc(&msg).expect("Failed to encode");
        assert!(!encoded.is_empty());

        // Decode
        let decoded = decode_krpc(&encoded).expect("Failed to decode");
        assert_eq!(decoded.t, msg.t);
        match decoded.y {
            KrpcMessageType::Query => {},
            _ => panic!("Expected Query type"),
        }
        assert!(decoded.q.is_some());
    }

    #[test]
    fn test_krpc_response_encode_decode() {
        let msg = KrpcMessage {
            t: vec![3, 4],
            y: KrpcMessageType::Response,
            q: None,
            a: None,
            r: Some(KrpcResponse {
                id: Some(vec![1; 20]),
                ..Default::default()
            }),
            e: None,
        };

        // Encode
        let encoded = encode_krpc(&msg).expect("Failed to encode");
        
        // Decode
        let decoded = decode_krpc(&encoded).expect("Failed to decode");
        assert_eq!(decoded.t, msg.t);
        match decoded.y {
            KrpcMessageType::Response => {},
            _ => panic!("Expected Response type"),
        }
        assert!(decoded.r.is_some());
    }
}
