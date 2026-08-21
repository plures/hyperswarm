//! Peer discovery coordinator scaffold.
//!
//! Coordinates the announce/lookup lifecycle across multiple topics.
//!
//! Discovery only publishes and returns peer addresses.  Selecting peers and
//! establishing connections are explicit caller-owned operations through the
//! connection manager, rather than hidden policy in this layer.

use std::collections::HashSet;

use tokio::sync::RwLock;

use crate::{dht, Topic};

#[derive(thiserror::Error, Debug)]
pub enum DiscoveryError {
    #[error("dht: {0}")]
    Dht(#[from] dht::DhtError),
    #[error("not implemented")]
    Unimplemented,
}

pub struct DiscoveryManager {
    topics: RwLock<HashSet<Topic>>,
}

impl DiscoveryManager {
    pub fn new() -> Self {
        Self {
            topics: RwLock::new(HashSet::new()),
        }
    }

    pub async fn join(
        &self,
        dht: &dht::DhtClient,
        topic: Topic,
        advertised_port: u16,
    ) -> Result<(), DiscoveryError> {
        self.topics.write().await.insert(topic);

        // Advertise the actual UDP port that accepts the subsequent direct
        // connection attempt. A port of zero produces an unusable peer record.
        dht.announce(topic, advertised_port).await?;

        // Perform initial lookup to find peers
        let peers = dht.lookup(topic).await?;

        tracing::debug!("Joined topic with {} peers found", peers.len());

        // Re-announcement scheduling and peer-selection policy are owned by
        // the caller's orchestration layer, not this discovery effect.
        Ok(())
    }

    pub async fn leave(&self, _dht: &dht::DhtClient, topic: Topic) -> Result<(), DiscoveryError> {
        self.topics.write().await.remove(&topic);
        // TODO: stop tasks for this topic.
        Ok(())
    }
}

impl Default for DiscoveryManager {
    fn default() -> Self {
        Self::new()
    }
}
