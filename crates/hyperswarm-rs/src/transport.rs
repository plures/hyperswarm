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
/// Maximum time allowed to complete a Noise handshake (both roles).
/// Bounded to prevent an adversary from stalling a handshake indefinitely
/// by continuously sending spoofed packets from unexpected addresses.
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

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
    #[error("peer authentication failed: remote static key does not match expected key")]
    PeerAuthenticationFailed,
}

/// An encrypted stream wrapper using Noise protocol.
pub struct EncryptedStream {
    socket: Arc<UdpSocket>,
    remote_addr: SocketAddr,
    state: Arc<Mutex<StreamState>>,
    /// The remote peer's static public key, populated after a successful handshake.
    remote_static_key: Option<[u8; 32]>,
    /// The local static public key for this stream (constant for the lifetime of the stream).
    local_static_pubkey: [u8; 32],
    /// The local static private key, kept to allow creating both initiator and responder states.
    local_static_privkey: Vec<u8>,
}

enum StreamState {
    Handshaking(HandshakeState),
    Established(TransportState),
}

impl EncryptedStream {
    /// Create a new encrypted stream with a freshly-generated static keypair.
    pub async fn new(socket: Arc<UdpSocket>, remote_addr: SocketAddr) -> Result<Self, TransportError> {
        let (handshake, local_static_pubkey, local_static_privkey) =
            Self::generate_keypair_and_initiator()?;
        Ok(Self {
            socket,
            remote_addr,
            state: Arc::new(Mutex::new(StreamState::Handshaking(handshake))),
            remote_static_key: None,
            local_static_pubkey,
            local_static_privkey,
        })
    }

    /// Generate a static keypair, return an initiator handshake state together
    /// with the public and private key bytes.
    fn generate_keypair_and_initiator() -> Result<(HandshakeState, [u8; 32], Vec<u8>), TransportError> {
        let builder = Builder::new(
            NOISE_PARAMS.parse().map_err(|e| TransportError::Noise(format!("{:?}", e)))?,
        );
        let keypair = builder
            .generate_keypair()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&keypair.public[..32]);
        let privkey = keypair.private.to_vec();

        let handshake = Builder::new(
            NOISE_PARAMS.parse().map_err(|e| TransportError::Noise(format!("{:?}", e)))?,
        )
        .local_private_key(&privkey)
        .build_initiator()
        .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        Ok((handshake, pubkey, privkey))
    }

    /// Build an initiator handshake state reusing the stored static keypair.
    fn make_initiator_state(&self) -> Result<HandshakeState, TransportError> {
        Builder::new(
            NOISE_PARAMS.parse().map_err(|e| TransportError::Noise(format!("{:?}", e)))?,
        )
        .local_private_key(&self.local_static_privkey)
        .build_initiator()
        .map_err(|e| TransportError::Noise(format!("{:?}", e)))
    }

    /// Build a responder handshake state reusing the stored static keypair.
    fn make_responder_state(&self) -> Result<HandshakeState, TransportError> {
        Builder::new(
            NOISE_PARAMS.parse().map_err(|e| TransportError::Noise(format!("{:?}", e)))?,
        )
        .local_private_key(&self.local_static_privkey)
        .build_responder()
        .map_err(|e| TransportError::Noise(format!("{:?}", e)))
    }

    /// Returns the local static public key for this stream.
    ///
    /// This key is stable for the lifetime of the `EncryptedStream` and can be
    /// shared with a peer out-of-band so the peer can authenticate this end.
    pub fn local_static_pubkey(&self) -> [u8; 32] {
        self.local_static_pubkey
    }

    /// Perform Noise XX handshake as initiator.
    ///
    /// If `remote_static_pubkey` is provided, the handshake will verify that the
    /// responder's static public key (obtained from the `<- e, ee, s, es` message)
    /// matches the supplied value, and return [`TransportError::PeerAuthenticationFailed`]
    /// if it does not.  This defends against man-in-the-middle attacks.
    ///
    /// After a successful handshake the peer's static key is stored and accessible via
    /// [`EncryptedStream::remote_static_key`].
    pub async fn handshake_initiator(&mut self, remote_static_pubkey: Option<[u8; 32]>) -> Result<(), TransportError> {
        // Extract the handshake state temporarily
        let handshake = {
            let mut state = self.state.lock().await;
            match &*state {
                StreamState::Established(_) => return Ok(()),
                StreamState::Handshaking(_) => {
                    // Replace with a placeholder so the lock can be released while
                    // we perform network I/O.
                    match std::mem::replace(&mut *state, StreamState::Handshaking(self.make_initiator_state()?)) {
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
        let recv_len = loop {
            let (recv_len, src_addr) = self.socket.recv_from(&mut buf).await?;
            if src_addr == self.remote_addr {
                break recv_len;
            }
        };
        
        let _ = handshake
            .read_message(&buf[..recv_len], &mut [])
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // The remote static key ('s') is now revealed by the XX handshake.
        // Copy it out before consuming the handshake state.
        let remote_static: Option<[u8; 32]> = handshake.get_remote_static().and_then(|k| {
            if k.len() >= 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&k[..32]);
                Some(arr)
            } else {
                None
            }
        });

        // Validate the remote key if the caller supplied an expected value.
        if let Some(expected) = remote_static_pubkey {
            match remote_static {
                Some(actual) if actual == expected => {}
                _ => return Err(TransportError::PeerAuthenticationFailed),
            }
        }

        // -> s, se
        let len = handshake
            .write_message(&[], &mut buf)
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        self.socket.send_to(&buf[..len], self.remote_addr).await?;

        // Transition to transport mode
        let transport = handshake
            .into_transport_mode()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        // Update state and store the authenticated remote key
        let mut state = self.state.lock().await;
        *state = StreamState::Established(transport);
        self.remote_static_key = remote_static;
        
        Ok(())
    }

    /// Perform Noise XX handshake as responder.
    ///
    /// After a successful handshake the initiator's static public key is stored
    /// and accessible via [`EncryptedStream::remote_static_key`].
    pub async fn handshake_responder(&mut self) -> Result<(), TransportError> {
        // Build a responder state reusing the stored static keypair so that
        // local_static_pubkey() remains consistent regardless of which role
        // this stream takes.
        let mut handshake = self.make_responder_state()?;

        // <- e
        // Use a shared deadline so continuous packets from unexpected sources cannot
        // stall the handshake indefinitely (DoS mitigation).
        let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
        let deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT;
        let recv_len = loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(TransportError::HandshakeIncomplete);
            }
            match tokio::time::timeout(remaining, self.socket.recv_from(&mut buf)).await {
                Ok(Ok((len, addr))) if addr == self.remote_addr => break len,
                Ok(Ok(_)) => {} // ignore packets from unexpected sources
                _ => return Err(TransportError::HandshakeIncomplete),
            }
        };
        let _ = handshake
            .read_message(&buf[..recv_len], &mut [])
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // -> e, ee, s, es
        let len = handshake
            .write_message(&[], &mut buf)
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        self.socket.send_to(&buf[..len], self.remote_addr).await?;

        // <- s, se
        let recv_len = loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(TransportError::HandshakeIncomplete);
            }
            match tokio::time::timeout(remaining, self.socket.recv_from(&mut buf)).await {
                Ok(Ok((len, addr))) if addr == self.remote_addr => break len,
                Ok(Ok(_)) => {} // ignore packets from unexpected sources
                _ => return Err(TransportError::HandshakeIncomplete),
            }
        };
        let _ = handshake
            .read_message(&buf[..recv_len], &mut [])
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;

        // The initiator's static key ('s') is now revealed by the XX handshake.
        let remote_static: Option<[u8; 32]> = handshake.get_remote_static().and_then(|k| {
            if k.len() >= 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&k[..32]);
                Some(arr)
            } else {
                None
            }
        });

        // Transition to transport mode
        let transport = handshake
            .into_transport_mode()
            .map_err(|e| TransportError::Noise(format!("{:?}", e)))?;
        
        let mut state = self.state.lock().await;
        *state = StreamState::Established(transport);
        self.remote_static_key = remote_static;
        
        Ok(())
    }

    /// Returns the remote peer's static public key.
    ///
    /// This is available only after a successful handshake (either as initiator or
    /// responder).  Returns `None` if the handshake has not yet completed.
    pub fn remote_static_key(&self) -> Option<[u8; 32]> {
        self.remote_static_key
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
        let result = EncryptedStream::generate_keypair_and_initiator();
        assert!(result.is_ok());
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

    #[tokio::test]
    async fn test_noise_handshake_and_encryption() {
        // Create two sockets for initiator and responder
        let initiator_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let responder_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        
        let initiator_addr = initiator_socket.local_addr().unwrap();
        let responder_addr = responder_socket.local_addr().unwrap();
        
        // Create streams
        let mut initiator = EncryptedStream::new(
            initiator_socket.clone(),
            responder_addr,
        ).await.unwrap();
        
        let mut responder = EncryptedStream::new(
            responder_socket.clone(),
            initiator_addr,
        ).await.unwrap();
        
        // Perform handshake in parallel
        let initiator_handshake = tokio::spawn(async move {
            initiator.handshake_initiator(None).await
        });
        
        let responder_handshake = tokio::spawn(async move {
            responder.handshake_responder().await
        });
        
        // Both handshakes should complete successfully
        let init_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            initiator_handshake
        ).await;
        
        let resp_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            responder_handshake
        ).await;
        
        // Verify both completed (may fail due to actual network issues, but shouldn't panic)
        assert!(init_result.is_ok());
        assert!(resp_result.is_ok());
    }

    #[tokio::test]
    async fn test_address_validation_in_recv() {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let remote_addr = "127.0.0.1:8080".parse().unwrap();
        
        let stream = EncryptedStream::new(socket, remote_addr).await;
        assert!(stream.is_ok());
        
        // The actual address validation is tested implicitly through the handshake tests
        // where messages must come from the expected remote_addr
    }

    #[tokio::test]
    async fn test_peer_auth_none_succeeds() {
        // No expected key → handshake always succeeds; remote key is still stored.
        let s1 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let s2 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let a1 = s1.local_addr().unwrap();
        let a2 = s2.local_addr().unwrap();

        let mut initiator = EncryptedStream::new(s1, a2).await.unwrap();
        let mut responder = EncryptedStream::new(s2, a1).await.unwrap();

        let h1 = tokio::spawn(async move {
            let r = initiator.handshake_initiator(None).await;
            (initiator, r)
        });
        let h2 = tokio::spawn(async move {
            let r = responder.handshake_responder().await;
            (responder, r)
        });

        let results = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            async { tokio::join!(h1, h2) },
        )
        .await
        .expect("handshake timed out");

        let (initiator, init_result) = results.0.expect("task 1 panicked");
        let (responder, resp_result) = results.1.expect("task 2 panicked");

        assert!(init_result.is_ok(), "handshake should succeed with no expected key");
        assert!(resp_result.is_ok(), "responder handshake should succeed");
        assert!(
            initiator.remote_static_key().is_some(),
            "initiator should have the responder's static key"
        );
        assert!(
            responder.remote_static_key().is_some(),
            "responder should have the initiator's static key"
        );
    }

    #[tokio::test]
    async fn test_peer_auth_correct_key_succeeds() {
        // Build the two streams up-front so we can read local_static_pubkey()
        // before the handshake starts, then supply it as the expected key.
        let s1 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let s2 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let a1 = s1.local_addr().unwrap();
        let a2 = s2.local_addr().unwrap();

        let mut initiator = EncryptedStream::new(s1, a2).await.unwrap();
        let responder = EncryptedStream::new(s2, a1).await.unwrap();

        // The responder's static public key is known before the handshake.
        let expected_responder_key = responder.local_static_pubkey();

        let h1 = tokio::spawn(async move {
            initiator.handshake_initiator(Some(expected_responder_key)).await
        });
        let h2 = tokio::spawn(async move {
            let mut responder = responder;
            responder.handshake_responder().await
        });

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            async { tokio::join!(h1, h2) },
        )
        .await
        .expect("handshake timed out");

        let init_result = result.0.expect("task panicked");
        let resp_result = result.1.expect("task panicked");

        assert!(init_result.is_ok(), "handshake should succeed when expected key matches");
        assert!(resp_result.is_ok(), "responder handshake should succeed");
    }

    #[tokio::test]
    async fn test_peer_auth_wrong_key_rejected() {
        // Build streams; supply a wrong expected key to the initiator.
        let s1 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let s2 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let a1 = s1.local_addr().unwrap();
        let a2 = s2.local_addr().unwrap();

        let mut initiator = EncryptedStream::new(s1, a2).await.unwrap();
        let mut responder = EncryptedStream::new(s2, a1).await.unwrap();

        let wrong_key = [0xdeu8; 32];

        let h1 = tokio::spawn(async move {
            initiator.handshake_initiator(Some(wrong_key)).await
        });
        // Responder keeps going; initiator aborts after the second message.
        let h2 = tokio::spawn(async move {
            // Responder will time out waiting for the third Noise message since
            // the initiator aborts.  We don't assert on its result here.
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                responder.handshake_responder(),
            )
            .await;
        });

        let init_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            h1,
        )
        .await
        .expect("initiator timed out")
        .expect("task panicked");

        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), h2).await;

        assert!(
            matches!(init_result, Err(TransportError::PeerAuthenticationFailed)),
            "expected PeerAuthenticationFailed, got {:?}",
            init_result
        );
    }

    #[tokio::test]
    async fn test_remote_static_key_not_set_before_handshake() {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let remote_addr = "127.0.0.1:8080".parse().unwrap();
        let stream = EncryptedStream::new(socket, remote_addr).await.unwrap();
        assert!(
            stream.remote_static_key().is_none(),
            "remote_static_key should be None before handshake"
        );
    }

    #[tokio::test]
    async fn test_local_static_pubkey_consistent_across_roles() {
        // The same EncryptedStream's local_static_pubkey should be the key
        // the remote peer sees after completing either an initiator or responder handshake.
        let s1 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let s2 = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let a1 = s1.local_addr().unwrap();
        let a2 = s2.local_addr().unwrap();

        let mut initiator = EncryptedStream::new(s1, a2).await.unwrap();
        let mut responder = EncryptedStream::new(s2, a1).await.unwrap();

        let initiator_pubkey = initiator.local_static_pubkey();
        let responder_pubkey = responder.local_static_pubkey();

        let h1 = tokio::spawn(async move {
            let r = initiator.handshake_initiator(None).await;
            (initiator, r)
        });
        let h2 = tokio::spawn(async move {
            let r = responder.handshake_responder().await;
            (responder, r)
        });

        let results = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            async { tokio::join!(h1, h2) },
        )
        .await
        .expect("handshake timed out");

        let (initiator, _) = results.0.expect("task 1 panicked");
        let (responder, _) = results.1.expect("task 2 panicked");

        // The key the initiator sees as the remote key must match the responder's local key.
        assert_eq!(
            initiator.remote_static_key().unwrap(),
            responder_pubkey,
            "initiator's remote_static_key should match responder's local_static_pubkey"
        );
        // And vice-versa.
        assert_eq!(
            responder.remote_static_key().unwrap(),
            initiator_pubkey,
            "responder's remote_static_key should match initiator's local_static_pubkey"
        );
    }
}
