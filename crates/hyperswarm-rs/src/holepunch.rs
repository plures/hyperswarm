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
    #[error("not implemented")]
    Unimplemented,
}

pub struct HolepunchSession {
    // TODO: local socket, candidate set, state machine, timers.
}

impl HolepunchSession {
    pub fn new() -> Self {
        Self {}
    }

    /// Initiate a holepunch attempt to a remote peer.
    pub async fn initiate(&mut self, _remote_candidates: Vec<Candidate>) -> Result<SocketAddr, HolepunchError> {
        // TODO: probe candidates; perform simultaneous punch.
        Err(HolepunchError::Unimplemented)
    }

    /// Respond to a remote initiation.
    pub async fn respond(&mut self, _remote_candidates: Vec<Candidate>) -> Result<SocketAddr, HolepunchError> {
        // TODO: accept probe/punch packets and confirm chosen path.
        Err(HolepunchError::Unimplemented)
    }

    /// Send probe packets to candidates.
    pub async fn probe(&mut self, _candidates: &[Candidate]) -> Result<(), HolepunchError> {
        Err(HolepunchError::Unimplemented)
    }
}
