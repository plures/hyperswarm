//! Peer discovery coordinator scaffold.
//!
//! Coordinates the announce/lookup lifecycle across multiple topics and
//! triggers connection establishment (holepunch + encrypted transport).

use std::collections::HashSet;

use tokio::sync::RwLock;

use crate::{dht, Topic};

#[derive(Clone, Debug)]
pub struct DiscoveryConfig {
    pub max_peers: usize,
}

#[derive(thiserror::Error, Debug)]
pub enum DiscoveryError {
    #[error("dht: {0}")]
    Dht(#[from] dht::DhtError),
    #[error("not implemented")]
    Unimplemented,
}

pub struct DiscoveryManager {
    config: DiscoveryConfig,
    topics: RwLock<HashSet<Topic>>,
}

impl DiscoveryManager {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            config,
            topics: RwLock::new(HashSet::new()),
        }
    }

    pub async fn join(&self, dht: &dht::DhtClient, topic: Topic) -> Result<(), DiscoveryError> {
        self.topics.write().await.insert(topic);
        // TODO: announce periodically; run iterative lookups; connect to peers.
        let _ = (dht, self.config.max_peers);
        Err(DiscoveryError::Unimplemented)
    }

    pub async fn leave(&self, _dht: &dht::DhtClient, topic: Topic) -> Result<(), DiscoveryError> {
        self.topics.write().await.remove(&topic);
        // TODO: stop tasks for this topic.
        Ok(())
    }
}
