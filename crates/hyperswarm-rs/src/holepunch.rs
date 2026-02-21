//! UDP holepunching scaffold.
//!
//! Hyperswarm establishes direct peer-to-peer connectivity across NATs.
//! The typical flow is:
//!
//! 1) **Probe**: each peer sends outbound UDP packets to candidate addresses
//!    to create NAT bindings and learn which candidates are viable.
//! 2) **Exchange candidates**: peers exchange observed endpoints (via relay/DHT)
//!    so both sides know where to punch.
//! 3) **Punch**: both peers simultaneously send packets to each other to
//!    open the mapping and confirm reachability.
//!
//! # Security
//! Punch packets are authenticated with a Blake2s MAC keyed on a pre-shared
//! `session_key`.  Both peers must call [`HolepunchSession::new`] with the same
//! key (derived from the topic or exchanged via the DHT relay).  Packets that
//! fail MAC verification are silently ignored.

use blake2::{Blake2sMac256, digest::{Mac, KeyInit}};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration};

#[derive(Clone, Debug)]
pub struct Candidate {
    pub addr: SocketAddr,
    pub kind: CandidateKind,
}

#[derive(Clone, Debug)]
pub enum CandidateKind {
    /// Private LAN address.
    Lan,
    /// Public address observed by a third party.
    Wan,
    /// Relay / rendezvous.
    Relay,
}

#[derive(thiserror::Error, Debug)]
pub enum HolepunchError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("timeout")]
    Timeout,
    #[error("no viable candidates")]
    NoViableCandidates,
    #[error("authentication failed")]
    AuthenticationFailed,
}

const PROBE_MESSAGE: &[u8] = b"HYPERSWARM_PROBE";
const PUNCH_MESSAGE: &[u8] = b"HYPERSWARM_PUNCH";
const PUNCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Size of the Blake2s MAC tag appended to every punch packet (bytes).
const PUNCH_MAC_SIZE: usize = 32;
/// How long to wait between punch retransmissions while waiting for a response.
const PUNCH_RETRY_INTERVAL: Duration = Duration::from_millis(200);

pub struct HolepunchSession {
    socket: Arc<UdpSocket>,
    /// Pre-shared secret used to authenticate punch packets.
    ///
    /// Both the initiator and the responder must use the same key (typically
    /// derived from the shared topic or exchanged through the DHT relay).
    session_key: [u8; 32],
}

impl HolepunchSession {
    /// Create a new holepunch session with a bound UDP socket.
    ///
    /// `session_key` is a 32-byte pre-shared secret used to authenticate punch
    /// packets.  Both the initiating and responding peers must supply the same
    /// key.  A good source for this key is the topic hash shared via the DHT.
    pub async fn new(bind_addr: SocketAddr, session_key: [u8; 32]) -> Result<Self, HolepunchError> {
        let socket = UdpSocket::bind(bind_addr).await?;
        Ok(Self {
            socket: Arc::new(socket),
            session_key,
        })
    }

    // ---- MAC helpers --------------------------------------------------------

    /// Compute the Blake2s MAC tag for a punch packet.
    ///
    /// MAC = Blake2sMac256(key = session_key, msg = PUNCH_MESSAGE)
    fn compute_punch_mac(&self) -> [u8; 32] {
        let mut mac = <Blake2sMac256 as KeyInit>::new_from_slice(&self.session_key)
            .expect("session_key is exactly 32 bytes, which is valid for Blake2sMac256");
        Mac::update(&mut mac, PUNCH_MESSAGE);
        Mac::finalize(mac).into_bytes().into()
    }

    /// Build an authenticated punch packet: `PUNCH_MESSAGE || mac_tag`.
    fn build_punch_packet(&self) -> Vec<u8> {
        let mac = self.compute_punch_mac();
        let mut packet = Vec::with_capacity(PUNCH_MESSAGE.len() + PUNCH_MAC_SIZE);
        packet.extend_from_slice(PUNCH_MESSAGE);
        packet.extend_from_slice(&mac);
        packet
    }

    /// Verify an authenticated punch packet using a constant-time MAC check.
    fn verify_punch_packet(&self, data: &[u8]) -> bool {
        if data.len() != PUNCH_MESSAGE.len() + PUNCH_MAC_SIZE {
            return false;
        }
        if &data[..PUNCH_MESSAGE.len()] != PUNCH_MESSAGE {
            return false;
        }
        let mut mac = <Blake2sMac256 as KeyInit>::new_from_slice(&self.session_key)
            .expect("session_key is exactly 32 bytes, which is valid for Blake2sMac256");
        Mac::update(&mut mac, PUNCH_MESSAGE);
        // verify_slice performs a constant-time comparison.
        mac.verify_slice(&data[PUNCH_MESSAGE.len()..]).is_ok()
    }

    /// Initiate a holepunch attempt to a remote peer.
    pub async fn initiate(&mut self, remote_candidates: Vec<Candidate>) -> Result<SocketAddr, HolepunchError> {
        if remote_candidates.is_empty() {
            return Err(HolepunchError::NoViableCandidates);
        }

        // Probe all candidates to create NAT bindings
        self.probe(&remote_candidates).await?;

        // Try to establish connection with each candidate
        for candidate in &remote_candidates {
            // Send punch message
            if let Ok(established_addr) = self.punch_to(candidate.addr).await {
                return Ok(established_addr);
            }
        }

        Err(HolepunchError::Timeout)
    }

    /// Respond to a remote initiation.
    pub async fn respond(&mut self, remote_candidates: Vec<Candidate>) -> Result<SocketAddr, HolepunchError> {
        if remote_candidates.is_empty() {
            return Err(HolepunchError::NoViableCandidates);
        }

        // Probe all candidates
        self.probe(&remote_candidates).await?;

        // Listen for incoming punch messages and respond
        match timeout(PUNCH_TIMEOUT, self.recv_and_respond()).await {
            Ok(Ok(addr)) => Ok(addr),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HolepunchError::Timeout),
        }
    }

    /// Send probe packets to candidates.
    pub async fn probe(&mut self, candidates: &[Candidate]) -> Result<(), HolepunchError> {
        let mut success_count = 0usize;
        let mut last_error: Option<std::io::Error> = None;

        for candidate in candidates {
            // Send probe message to create NAT binding
            match self.socket.send_to(PROBE_MESSAGE, candidate.addr).await {
                Ok(_) => {
                    success_count += 1;
                }
                Err(e) => {
                    tracing::debug!("Probe attempt unsuccessful for candidate {}: {}", candidate.addr, e);
                    last_error = Some(e);
                }
            }
        }

        if success_count == 0 {
            if let Some(e) = last_error {
                return Err(HolepunchError::Io(e));
            } else {
                // Defensive fallback: no successful probes and no captured error.
                return Err(HolepunchError::NoViableCandidates);
            }
        }
        Ok(())
    }

    /// Attempt to punch through to a specific address.
    ///
    /// Sends an authenticated punch packet and retransmits every
    /// [`PUNCH_RETRY_INTERVAL`] until the peer responds with a valid
    /// authenticated punch packet or the 2-second deadline expires.
    ///
    /// Returns [`HolepunchError::AuthenticationFailed`] if a packet arrives from
    /// the expected peer address but fails the MAC check (wrong session key).
    async fn punch_to(&self, addr: SocketAddr) -> Result<SocketAddr, HolepunchError> {
        let punch_packet = self.build_punch_packet();

        // Buffer large enough for authenticated punch packet (PUNCH_MESSAGE + MAC).
        let mut buf = vec![0u8; PUNCH_MESSAGE.len() + PUNCH_MAC_SIZE + 16];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);

        // Send the first punch immediately.
        self.socket.send_to(&punch_packet, addr).await?;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(HolepunchError::Timeout);
            }

            // Use tokio::select! so the retransmit timer fires independently of
            // how many invalid/unauthenticated packets arrive on the socket.
            // Without this a flood of junk packets could starve the retry timer.
            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, from_addr)) => {
                            if from_addr == addr {
                                if self.verify_punch_packet(&buf[..len]) {
                                    return Ok(addr);
                                } else if buf[..len].starts_with(PUNCH_MESSAGE) {
                                    // Packet has the PUNCH_MESSAGE prefix but the
                                    // MAC is wrong — the peer is using a different
                                    // session key.
                                    return Err(HolepunchError::AuthenticationFailed);
                                }
                                // Other packets from the expected peer (e.g. probes)
                                // are silently ignored.
                            }
                            // Packets from other addresses are also ignored.
                        }
                        Err(e) => return Err(HolepunchError::Io(e)),
                    }
                }
                _ = tokio::time::sleep(PUNCH_RETRY_INTERVAL) => {
                    // Retry interval elapsed — retransmit and loop.
                    self.socket.send_to(&punch_packet, addr).await?;
                }
            }
        }
    }

    /// Receive an authenticated punch packet and respond in kind.
    async fn recv_and_respond(&self) -> Result<SocketAddr, HolepunchError> {
        let punch_packet = self.build_punch_packet();
        let mut buf = vec![0u8; PUNCH_MESSAGE.len() + PUNCH_MAC_SIZE + 16];
        
        loop {
            let (len, from_addr) = self.socket.recv_from(&mut buf).await?;
            
            if self.verify_punch_packet(&buf[..len]) {
                // Respond with our own authenticated punch message.
                self.socket.send_to(&punch_packet, from_addr).await?;
                return Ok(from_addr);
            }
            // Ignore unauthenticated or unexpected packets.
        }
    }

    /// Get the local address of this session
    pub fn local_addr(&self) -> Result<SocketAddr, HolepunchError> {
        Ok(self.socket.local_addr()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SESSION_KEY: [u8; 32] = [0x42u8; 32];

    #[tokio::test]
    async fn test_holepunch_session_creation() {
        let bind_addr = "127.0.0.1:0".parse().unwrap();
        let session = HolepunchSession::new(bind_addr, TEST_SESSION_KEY).await;
        assert!(session.is_ok());
    }

    #[tokio::test]
    async fn test_probe_candidates() {
        let bind_addr = "127.0.0.1:0".parse().unwrap();
        let mut session = HolepunchSession::new(bind_addr, TEST_SESSION_KEY).await.unwrap();

        let candidates = vec![
            Candidate {
                addr: "127.0.0.1:8001".parse().unwrap(),
                kind: CandidateKind::Lan,
            },
            Candidate {
                addr: "127.0.0.1:8002".parse().unwrap(),
                kind: CandidateKind::Wan,
            },
        ];

        let result = session.probe(&candidates).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_local_holepunch() {
        // Create two sessions
        let session1 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_SESSION_KEY)
            .await
            .unwrap();
        let session2 = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_SESSION_KEY)
            .await
            .unwrap();

        let addr1 = session1.local_addr().unwrap();
        let addr2 = session2.local_addr().unwrap();

        // Test that both sessions can get their addresses
        assert_ne!(addr1.port(), 0);
        assert_ne!(addr2.port(), 0);
    }

    #[tokio::test]
    async fn test_punch_mac_valid() {
        let session = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_SESSION_KEY)
            .await
            .unwrap();

        let packet = session.build_punch_packet();
        assert!(session.verify_punch_packet(&packet), "valid packet should pass MAC check");
    }

    #[tokio::test]
    async fn test_punch_mac_wrong_key_rejected() {
        let key_a = [0x01u8; 32];
        let key_b = [0x02u8; 32];

        let session_a = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), key_a)
            .await
            .unwrap();
        let session_b = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), key_b)
            .await
            .unwrap();

        // A packet built with key_a must be rejected by a session using key_b.
        let packet = session_a.build_punch_packet();
        assert!(
            !session_b.verify_punch_packet(&packet),
            "packet from a different key should fail MAC check"
        );
    }

    #[tokio::test]
    async fn test_punch_mac_truncated_packet_rejected() {
        let session = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_SESSION_KEY)
            .await
            .unwrap();

        // PUNCH_MESSAGE alone (without the MAC) must be rejected.
        assert!(
            !session.verify_punch_packet(PUNCH_MESSAGE),
            "plain PUNCH_MESSAGE without MAC should be rejected"
        );
    }

    #[tokio::test]
    async fn test_punch_mac_tampered_payload_rejected() {
        let session = HolepunchSession::new("127.0.0.1:0".parse().unwrap(), TEST_SESSION_KEY)
            .await
            .unwrap();

        let mut packet = session.build_punch_packet();
        // Flip a bit in the MAC portion.
        let mac_start = PUNCH_MESSAGE.len();
        packet[mac_start] ^= 0xFF;
        assert!(
            !session.verify_punch_packet(&packet),
            "tampered MAC should be rejected"
        );
    }
}
