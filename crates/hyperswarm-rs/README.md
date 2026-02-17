# hyperswarm-rs

Rust implementation of **Hyperswarm** (P2P discovery via DHT + NAT holepunching) for **PluresDB**.

- Repo: <https://github.com/plures/hyperswarm>
- Tracking: plures/pluresdb#70, plures/hyperswarm#1

## Status

This crate is **feature-complete** for demonstration and development purposes. Core P2P functionality is implemented and tested.

**⚠️ Security Notice:** This implementation requires additional security hardening before production deployment:
- Noise handshake lacks peer authentication (no validation of remote static keys)
- Holepunch messages use unauthenticated plain text
- IPv4-only support (no IPv6)
- Limited retry logic and error handling

For production use in PluresDB, implement additional security measures including peer authentication, message authentication, and comprehensive security auditing.

### Implemented
- ✅ DHT client with KRPC protocol support (ping, find_node, get_peers, announce_peer)
- ✅ Bencode encoding/decoding for KRPC messages
- ✅ Basic routing table with node management
- ✅ Bootstrap functionality with mainline DHT nodes
- ✅ Topic-based peer announcement and lookup
- ✅ UDP holepunching with probe/punch protocol
- ✅ Noise XX protocol encryption for secure transport
- ✅ Address verification to prevent spoofing attacks
- ✅ Basic test coverage
- ✅ Working examples demonstrating all features

### TODO (Production Readiness)
- ⏳ Peer authentication in Noise handshake
- ⏳ Authenticated holepunch messages (HMAC)
- ⏳ IPv6 support in compact peer encoding
- ⏳ Full k-bucket routing table optimization
- ⏳ Iterative DHT traversal for wider peer discovery
- ⏳ Retry logic and timeout configuration
- ⏳ Comprehensive integration test coverage
- ⏳ Connection multiplexing
- ⏳ Interop testing with JS Hyperswarm
- ⏳ Security audit and penetration testing

## Usage

### Basic DHT Bootstrap

```rust
use hyperswarm::dht::{DhtClient, DhtConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = DhtConfig {
        bootstrap: vec!["router.bittorrent.com:6881".to_string()],
        bind_port: 0, // Let OS choose a port
    };
    
    let client = DhtClient::new(config).await?;
    client.bootstrap().await?;
    
    Ok(())
}
```

### Topic-based Peer Discovery

```rust
use hyperswarm::{Hyperswarm, SwarmConfig, Topic};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let swarm = Hyperswarm::new(SwarmConfig::default()).await?;
    
    let topic = Topic::from_key(b"my-app-topic");
    swarm.join(topic).await?;  // Announces and discovers peers
    
    Ok(())
}
```

### Examples

See the `examples/` directory for complete demonstrations:
- `dht_bootstrap.rs` — DHT client bootstrap
- `topic_announce.rs` — Topic announcement and peer lookup
- `p2p_connection.rs` — Full P2P connection flow demonstration

Run examples with:
```bash
cargo run --example dht_bootstrap
cargo run --example topic_announce
cargo run --example p2p_connection
```

## Architecture

- **`dht`** — KRPC-over-UDP client + Kademlia-style routing table
  - ✅ bootstrap — Connect to DHT network
  - ✅ announce — Announce presence for a topic
  - ✅ lookup — Find peers for a topic
  - ✅ ping / find_node / get_peers / announce_peer queries

- **`discovery`** — Orchestrates per-topic lifecycle and connection attempts
  - ✅ join/leave topic management
  - ✅ Integration with DHT for announce/lookup

- **`holepunch`** — UDP holepunch coordination
  - ✅ Session management
  - ✅ Candidate probing
  - ✅ Simultaneous punch initiation and response

- **`transport`** — Encrypted stream transport using Noise XX handshake
  - ✅ Handshake as initiator/responder
  - ✅ Encrypted send/receive
  - ✅ Session state management

- **`protocol`** — Wire format definitions
  - ✅ KRPC message types
  - ✅ Bencode serialization/deserialization

## PluresDB Integration

PluresDB sync needs a robust peer discovery + transport layer to replicate state between devices without relying on centralized infrastructure.

Hyperswarm-rs provides:

1. **Topic derivation** — Derive a `Topic` from a shared secret / collection key
2. **Peer discovery** — `join(topic)` to announce presence and discover peers
3. **Encrypted connections** — Establish Noise-encrypted connections to exchange replication messages
4. **NAT traversal** — UDP holepunching for direct peer-to-peer connectivity

### Integration Example

```rust
use hyperswarm::{Hyperswarm, SwarmConfig, Topic};

async fn setup_pluresdb_sync() -> Result<(), Box<dyn std::error::Error>> {
    // Create swarm instance
    let swarm = Hyperswarm::new(SwarmConfig::default()).await?;
    
    // Derive topic from PluresDB collection key
    let collection_key = b"pluresdb-collection-abc123";
    let topic = Topic::from_key(collection_key);
    
    // Join the swarm for this collection
    swarm.join(topic).await?;
    
    // ... establish connections and sync data ...
    
    Ok(())
}
```

## Development Notes

- The JS Hyperswarm implementation is the reference for protocol details
- Uses `serde_bencode` for KRPC message encoding/decoding (BEP 5)
- Uses `snow` crate for Noise protocol implementation
- Uses `tokio` for async I/O

## Testing

Run the test suite:
```bash
cargo test
```

Build the library:
```bash
cargo build
```

## License

AGPL-3.0 (matches upstream project licensing)
