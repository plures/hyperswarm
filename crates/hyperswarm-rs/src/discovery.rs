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
        
        // Announce our presence on the DHT for this topic
        // Use port 0 to indicate we're interested but not listening on a specific port
        dht.announce(topic, 0).await?;
        
        // Perform initial lookup to find peers
        let peers = dht.lookup(topic).await?;
        
        tracing::debug!("Joined topic with {} peers found", peers.len());
        
        // TODO: periodically re-announce and lookup; connect to peers.
        let _ = self.config.max_peers;
        
        Ok(())
    }

    pub async fn leave(&self, _dht: &dht::DhtClient, topic: Topic) -> Result<(), DiscoveryError> {
        self.topics.write().await.remove(&topic);
        // TODO: stop tasks for this topic.
        Ok(())
    }
}
