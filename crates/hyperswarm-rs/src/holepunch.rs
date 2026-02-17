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
}

const PROBE_MESSAGE: &[u8] = b"HYPERSWARM_PROBE";
const PUNCH_MESSAGE: &[u8] = b"HYPERSWARM_PUNCH";
const PUNCH_TIMEOUT: Duration = Duration::from_secs(10);

pub struct HolepunchSession {
    socket: Arc<UdpSocket>,
}

impl HolepunchSession {
    /// Create a new holepunch session with a bound UDP socket
    pub async fn new(bind_addr: SocketAddr) -> Result<Self, HolepunchError> {
        let socket = UdpSocket::bind(bind_addr).await?;
        Ok(Self {
            socket: Arc::new(socket),
        })
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
        for candidate in candidates {
            // Send probe message to create NAT binding
            let _ = self.socket.send_to(PROBE_MESSAGE, candidate.addr).await;
        }
        Ok(())
    }

    /// Attempt to punch through to a specific address
    async fn punch_to(&self, addr: SocketAddr) -> Result<SocketAddr, HolepunchError> {
        // Send punch message
        self.socket.send_to(PUNCH_MESSAGE, addr).await?;

        // Wait for acknowledgment
        let mut buf = vec![0u8; 1024];
        match timeout(Duration::from_secs(2), self.socket.recv_from(&mut buf)).await {
            Ok(Ok((len, from_addr))) => {
                if &buf[..len] == PUNCH_MESSAGE && from_addr == addr {
                    Ok(addr)
                } else {
                    Err(HolepunchError::Timeout)
                }
            }
            _ => Err(HolepunchError::Timeout),
        }
    }

    /// Receive punch message and respond
    async fn recv_and_respond(&self) -> Result<SocketAddr, HolepunchError> {
        let mut buf = vec![0u8; 1024];
        
        loop {
            let (len, from_addr) = self.socket.recv_from(&mut buf).await?;
            
            if &buf[..len] == PUNCH_MESSAGE {
                // Respond with punch message
                self.socket.send_to(PUNCH_MESSAGE, from_addr).await?;
                return Ok(from_addr);
            }
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

    #[tokio::test]
    async fn test_holepunch_session_creation() {
        let bind_addr = "127.0.0.1:0".parse().unwrap();
        let session = HolepunchSession::new(bind_addr).await;
        assert!(session.is_ok());
    }

    #[tokio::test]
    async fn test_probe_candidates() {
        let bind_addr = "127.0.0.1:0".parse().unwrap();
        let mut session = HolepunchSession::new(bind_addr).await.unwrap();

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
        let session1 = HolepunchSession::new("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let session2 = HolepunchSession::new("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();

        let addr1 = session1.local_addr().unwrap();
        let addr2 = session2.local_addr().unwrap();

        // Test that both sessions can get their addresses
        assert_ne!(addr1.port(), 0);
        assert_ne!(addr2.port(), 0);
    }
}
