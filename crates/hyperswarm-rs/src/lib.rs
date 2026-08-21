//! Hyperswarm-rs: Rust implementation of Hyperswarm P2P discovery
//!
//! Enables peers to find each other via a DHT and establish encrypted
//! connections through NATs using UDP holepunching.
//!
//! Status: scaffold / work-in-progress (PluresDB sync prerequisite).

pub mod connection;
pub mod dht;
pub mod discovery;
pub mod holepunch;
pub mod protocol;
pub mod transport;

pub struct Hyperswarm {
    dht: dht::DhtClient,
    discovery: discovery::DiscoveryManager,
    connections: connection::ConnectionManager,
}

/// Configuration for [`Hyperswarm`].
#[derive(Clone, Debug)]
pub struct SwarmConfig {
    /// Bootstrap nodes in `host:port` form.
    pub bootstrap: Vec<String>,
    /// Local direct-connection UDP port to bind. `0` means random.
    pub port: u16,
    /// Upper bound on concurrent peer connections.
    pub max_peers: usize,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            bootstrap: vec![
                "node1.hyperdht.org:49737".into(),
                "node2.hyperdht.org:49737".into(),
                "node3.hyperdht.org:49737".into(),
            ],
            port: 0,
            max_peers: 64,
        }
    }
}

/// A topic to announce / lookup on the DHT.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Topic(pub [u8; 32]);

impl Topic {
    /// Derive a topic from a shared secret key.
    pub fn from_key(key: &[u8]) -> Self {
        use blake2::{Blake2b512, Digest};

        let mut hasher = Blake2b512::new();
        hasher.update(key);
        let result = hasher.finalize();

        let mut topic = [0u8; 32];
        topic.copy_from_slice(&result[..32]);
        Topic(topic)
    }
}

impl Hyperswarm {
    pub async fn new(config: SwarmConfig) -> Result<Self, SwarmError> {
        let connections = connection::ConnectionManager::bind(config.port, config.max_peers)
            .await
            .map_err(|e| SwarmError::Connection(e.to_string()))?;
        let dht = dht::DhtClient::new(dht::DhtConfig {
            bootstrap: config.bootstrap.clone(),
            // Discovery keeps its KRPC socket separate from the stream
            // manager so a KRPC response can never consume encrypted payload.
            bind_port: 0,
        })
        .await
        .map_err(|e| SwarmError::Dht(e.to_string()))?;

        let discovery = discovery::DiscoveryManager::new();

        Ok(Self {
            dht,
            discovery,
            connections,
        })
    }

    pub async fn join(&self, topic: Topic) -> Result<(), SwarmError> {
        let advertised_port = self
            .connections
            .local_addr()
            .map_err(|e| SwarmError::Connection(e.to_string()))?
            .port();
        self.discovery
            .join(&self.dht, topic, advertised_port)
            .await
            .map_err(|e| SwarmError::Dht(e.to_string()))
    }

    pub async fn leave(&self, topic: Topic) -> Result<(), SwarmError> {
        self.discovery
            .leave(&self.dht, topic)
            .await
            .map_err(|e| SwarmError::Dht(e.to_string()))
    }

    /// Return discovery candidates. The addresses are unauthenticated until a
    /// caller completes [`Self::connect`].
    pub async fn lookup(&self, topic: Topic) -> Result<Vec<dht::PeerAddress>, SwarmError> {
        self.dht
            .lookup(topic)
            .await
            .map_err(|e| SwarmError::Dht(e.to_string()))
    }

    /// Initiate one managed direct connection selected by the caller.
    pub async fn connect(
        &self,
        topic: Topic,
        peer: dht::PeerAddress,
        expected_remote_static_key: Option<[u8; 32]>,
    ) -> Result<connection::ManagedConnection, SwarmError> {
        self.connections
            .connect(topic, peer.addr, expected_remote_static_key)
            .await
            .map_err(|e| SwarmError::Connection(e.to_string()))
    }

    /// Accept the next managed connection for `topic`.
    pub async fn accept(&self, topic: Topic) -> Result<connection::ManagedConnection, SwarmError> {
        self.connections
            .accept(topic)
            .await
            .map_err(|e| SwarmError::Connection(e.to_string()))
    }

    /// Direct-connection listener address. The IP may be unspecified when the
    /// manager listens on all interfaces; discovery records the usable port.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, SwarmError> {
        self.connections
            .local_addr()
            .map_err(|e| SwarmError::Connection(e.to_string()))
    }

    /// Wait until all pending DHT operations complete.
    pub async fn flush(&self) -> Result<(), SwarmError> {
        self.dht
            .flush()
            .await
            .map_err(|e| SwarmError::Dht(e.to_string()))
    }

    pub async fn destroy(self) -> Result<(), SwarmError> {
        self.connections.shutdown();
        self.dht
            .shutdown()
            .await
            .map_err(|e| SwarmError::Dht(e.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    #[error("DHT error: {0}")]
    Dht(String),
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Transport error: {0}")]
    Transport(String),
}
