//! Encrypted transport scaffold.
//!
//! Hyperswarm uses end-to-end encryption. This module sketches an encrypted
//! stream abstraction on top of UDP/QUIC using Noise (XX handshake).

use bytes::Bytes;

#[derive(thiserror::Error, Debug)]
pub enum TransportError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("noise: {0}")]
    Noise(String),
    #[error("not implemented")]
    Unimplemented,
}

/// An encrypted stream wrapper (placeholder).
///
/// For early development we may use QUIC (via `quinn`) to provide a stream
/// interface; later we can support raw UDP + framing.
pub struct EncryptedStream {
    // TODO: socket/connection handle, cipher state, framing.
}

impl EncryptedStream {
    pub async fn handshake_initiator(&mut self, _remote_static_pubkey: Option<[u8; 32]>) -> Result<(), TransportError> {
        // TODO: Noise XX initiator.
        Err(TransportError::Unimplemented)
    }

    pub async fn handshake_responder(&mut self) -> Result<(), TransportError> {
        // TODO: Noise XX responder.
        Err(TransportError::Unimplemented)
    }

    pub async fn send(&mut self, _data: Bytes) -> Result<(), TransportError> {
        Err(TransportError::Unimplemented)
    }

    pub async fn recv(&mut self) -> Result<Bytes, TransportError> {
        Err(TransportError::Unimplemented)
    }
}
