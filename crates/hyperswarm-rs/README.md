# hyperswarm-rs

Rust implementation of **Hyperswarm** (P2P discovery via DHT + NAT holepunching) for **PluresDB**.

- Repo: <https://github.com/plures/hyperswarm>
- Tracking: plures/pluresdb#70, plures/hyperswarm#1

## Status

This crate has a deterministic local DHT and managed-connection foundation.
It is not yet validated as a public-DHT or NAT-traversal implementation.

### Implemented

- ✅ DHT client with KRPC protocol support (ping, find_node, get_peers, announce_peer)
- ✅ Bencode encoding/decoding for KRPC messages
- ✅ Basic routing table with node management
- ✅ Bootstrap functionality with mainline DHT nodes
- ✅ Topic-based peer announcement and lookup
- ✅ UDP holepunching with probe/punch protocol
- ✅ Noise XX protocol encryption for secure transport
- ✅ Bounded managed UDP connection lifecycle with packet demultiplexing
- ✅ Topic-bound authenticated stream admission through the public API
- ✅ Address verification to prevent spoofing attacks
- ✅ IPv6 support in DHT compact peer parsing (BEP 5)
- ✅ Integration test coverage
- ✅ Working examples demonstrating all features
- ✅ Peer authentication in Noise handshake (validates remote static key when provided)
- ✅ Authenticated holepunch punch packets (Blake2s MAC with shared session key)
- ✅ Retry logic in holepunch punch phase (retransmit every 200 ms)

### TODO (Production Readiness)

- ⏳ Full k-bucket routing table optimization
- ⏳ Iterative DHT traversal for wider peer discovery
- ⏳ Public-DHT and multi-machine NAT traversal validation
- ⏳ Long-lived node identities and discovery-to-connection policy integration
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

### Managed Direct Connection

Discovery returns untrusted addresses. The caller selects a candidate and the
manager binds the selected topic into the authenticated Noise handshake:

```rust,no_run
use hyperswarm::{dht::PeerAddress, Hyperswarm, Topic};

async fn connect_selected_peer(
    swarm: &Hyperswarm,
    topic: Topic,
    peer: PeerAddress,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = swarm.connect(topic, peer, None).await?;
    // `connection` is a managed encrypted stream; dropping it releases its slot.
    Ok(())
}
```

Pass an expected Noise static key instead of `None` when the caller has one
for this connection. Persistent node identities remain a separate production
readiness item. Peer selection, retries, and re-announcement remain
caller-owned policy rather than hidden transport behavior.

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

- **`discovery`** — Orchestrates per-topic announce/lookup lifecycle
  - ✅ join/leave topic management
  - ✅ Integration with DHT for announce/lookup

- **`connection`** — Bounded direct-stream lifecycle
  - ✅ Sole UDP packet receiver and per-peer demultiplexing
  - ✅ Topic-bound Noise handshake admission
  - ✅ Explicit caller-selected connect/accept APIs

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

### Retained two-process acceptance lab

`managed_connection_lab` is the release-adjacent acceptance harness for the
managed UDP connection path. It deliberately rejects loopback peers and
retains one JSON receipt per process. The harness proves direct encrypted
delivery, reconnect, and topic refusal across two processes; it does **not**
claim public-DHT discovery, NAT traversal, or a persistent node identity.

Build the same commit on both hosts, then run the server on a LAN-reachable
address:

```powershell
cargo build --release --example managed_connection_lab
target\release\examples\managed_connection_lab.exe --role server --port 42042 --topic "lab-shared-topic" --evidence C:\qa-evidence\hyperswarm-server.json
```

On the second host, use the server's non-loopback LAN address:

```powershell
target\release\examples\managed_connection_lab.exe --role client --peer 10.0.0.42:42042 --topic "lab-shared-topic" --evidence C:\qa-evidence\hyperswarm-client.json
```

Both terminals must report `LAB_RESULT passed`; retain their output and the
two receipts with the release evidence. The client starts no bootstrap/DHT
fixture, so this remains a focused direct-transport test suitable for a lab
network.

### Upstream tracking

Do not automatically rebase this crate on an upstream wrapper release.
Track HyperDHT as the protocol and interoperability reference, and review it
on a regular cadence before targeted updates. Track Iroh and rust-libp2p as
architectural alternatives, not drop-in dependencies: replacing the transport
requires an explicit compatibility and migration proposal.

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
