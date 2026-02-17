//! Encrypted transport scaffold.
//!
//! Hyperswarm uses end-to-end encryption. This module provides an encrypted
//! stream abstraction on top of UDP using Noise XX handshake pattern.

use bytes::Bytes;
use snow::{Builder, HandshakeState, TransportState};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const MAX_MESSAGE_SIZE: usize = 65535;

#[derive(thiserror::Error, Debug)]
pub enum TransportError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("noise: {0}")]
    Noise(String),
    #[error("handshake not complete")]
    HandshakeIncomplete,
    #[error("invalid message")]
    InvalidMessage,
}

/// An encrypted stream wrapper using Noise protocol.
pub struct EncryptedStream {
    socket: Arc<UdpSocket>,
    remote_addr: SocketAddr,
    state: Arc<Mutex<StreamState>>,
}

enum StreamState {
    Handshaking(HandshakeState),
    Established(TransportState),
}

impl EncryptedStream {
    /// Create a new encrypted stream in handshake mode
    pub async fn new(socket: Arc<UdpSocket>, remote_addr: SocketAddr) -> Result<Self, TransportError> {
        Ok(Self {
            socket,
            remote_addr,
            state: Arc::new(Mutex::new(StreamState::Handshaking(
                Self::create_handshake_state()?,
            ))),
        })
    }

    /// Create a Noise handshake state
    fn create_handshake_state() -> Result<HandshakeState, TransportError> {
        // Generate static keypair for this session
        let builder = Builder::new(NOISE_PARAMS.parse().map_err(|e| TransportError::Noise(format!("{:?}", e)))?);
        
        // Generate a keypair and build initiator with it
        let keypair = builder.generate_keypair()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        Builder::new(NOISE_PARAMS.parse().map_err(|e| TransportError::Noise(format!("{:?}", e)))?)
            .local_private_key(&keypair.private)
            .build_initiator()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))
    }

    /// Perform Noise XX handshake as initiator
    pub async fn handshake_initiator(&mut self, _remote_static_pubkey: Option<[u8; 32]>) -> Result<(), TransportError> {
        // Extract the handshake state temporarily
        let handshake = {
            let mut state = self.state.lock().await;
            match &*state {
                StreamState::Established(_) => return Ok(()),
                StreamState::Handshaking(_) => {
                    // We need to extract it, so replace with a dummy value temporarily
                    match std::mem::replace(&mut *state, StreamState::Handshaking(Self::create_handshake_state()?)) {
                        StreamState::Handshaking(h) => h,
                        _ => unreachable!(),
                    }
                }
            }
        };
        
        let mut handshake = handshake;
        
        // -> e
        let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
        let len = handshake
            .write_message(&[], &mut buf)
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        self.socket.send_to(&buf[..len], self.remote_addr).await?;

        // <- e, ee, s, es
        let (recv_len, _) = self.socket.recv_from(&mut buf).await?;
        
        let _ = handshake
            .read_message(&buf[..recv_len], &mut [])
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // -> s, se
        let len = handshake
            .write_message(&[], &mut buf)
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        self.socket.send_to(&buf[..len], self.remote_addr).await?;

        // Transition to transport mode
        let transport = handshake
            .into_transport_mode()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        // Update state
        let mut state = self.state.lock().await;
        *state = StreamState::Established(transport);
        
        Ok(())
    }

    /// Perform Noise XX handshake as responder
    pub async fn handshake_responder(&mut self) -> Result<(), TransportError> {
        // Generate static keypair for responder
        let builder = Builder::new(
            NOISE_PARAMS.parse().map_err(|e| TransportError::Noise(format!("{:?}", e)))?
        );

        let keypair = builder
            .generate_keypair()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // Create responder handshake state with keys, reusing the same builder
        let mut handshake = builder
            .local_private_key(&keypair.private)
            .build_responder()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // <- e
        let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
        let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
        let (recv_len, _) = self.socket.recv_from(&mut buf).await?;
        let _ = handshake
            .read_message(&buf[..recv_len], &mut [])
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // -> e, ee, s, es
        let len = handshake
            .write_message(&[], &mut buf)
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        self.socket.send_to(&buf[..len], self.remote_addr).await?;

        // <- s, se
        let (recv_len, _) = self.socket.recv_from(&mut buf).await?;
        let _ = handshake
            .read_message(&buf[..recv_len], &mut [])
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // Transition to transport mode
        let transport = handshake
            .into_transport_mode()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        let mut state = self.state.lock().await;
        *state = StreamState::Established(transport);
        
        Ok(())
    }

    /// Send encrypted data
    pub async fn send(&mut self, data: Bytes) -> Result<(), TransportError> {
        let mut state = self.state.lock().await;
        
        match &mut *state {
            StreamState::Established(transport) => {
                let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
                let len = transport
                    .write_message(&data, &mut buf)
                    .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
                
                self.socket.send_to(&buf[..len], self.remote_addr).await?;
                Ok(())
            }
            StreamState::Handshaking(_) => Err(TransportError::HandshakeIncomplete),
        }
    }

    /// Receive encrypted data
    pub async fn recv(&mut self) -> Result<Bytes, TransportError> {
        let mut state = self.state.lock().await;
        
        match &mut *state {
            StreamState::Established(transport) => {
                let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
                // Only accept packets from the expected remote_addr
                let len = loop {
                    let (len, addr) = self.socket.recv_from(&mut buf).await?;
                    if addr == self.remote_addr {
                        break len;
                    }
                    // Ignore packets from unexpected peers and wait for the correct one
                };
                
                let mut plaintext = vec![0u8; MAX_MESSAGE_SIZE];
                let plaintext_len = transport
                    .read_message(&buf[..len], &mut plaintext)
                    .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
                
                Ok(Bytes::copy_from_slice(&plaintext[..plaintext_len]))
            }
            StreamState::Handshaking(_) => Err(TransportError::HandshakeIncomplete),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::net::UdpSocket;

    #[tokio::test]
    async fn test_encrypted_stream_creation() {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let remote_addr = "127.0.0.1:8080".parse().unwrap();
        
        let stream = EncryptedStream::new(socket, remote_addr).await;
        assert!(stream.is_ok());
    }

    #[tokio::test]
    async fn test_noise_handshake_state_creation() {
        let state = EncryptedStream::create_handshake_state();
        assert!(state.is_ok());
    }

    #[tokio::test]
    async fn test_send_recv_without_handshake() {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let remote_addr = "127.0.0.1:8080".parse().unwrap();
        
        let mut stream = EncryptedStream::new(socket, remote_addr).await.unwrap();
        
        // Sending without handshake should fail
        let result = stream.send(Bytes::from("test")).await;
        assert!(result.is_err());
    }
}
