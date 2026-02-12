# hyperswarm-rs (WIP)

Rust implementation scaffold of **Hyperswarm** (P2P discovery via DHT + NAT holepunching) for **PluresDB**.

- Repo: <https://github.com/plures/hyperswarm>
- Tracking: plures/pluresdb#70

## Status

This crate is currently under **active development**. The DHT client module has a minimal working implementation.

### Implemented
- ✅ DHT client with KRPC protocol support (ping, find_node)
- ✅ Bencode encoding/decoding for KRPC messages
- ✅ Basic routing table (simplified k-buckets)
- ✅ Bootstrap functionality with mainline DHT nodes
- ✅ UDP socket handling with Tokio

### TODO
- ⏳ Complete topic announce/lookup
- ⏳ Full k-bucket routing table implementation
- ⏳ UDP holepunching
- ⏳ Noise protocol encryption
- ⏳ Connection multiplexing
- ⏳ Interop testing with JS Hyperswarm

## Usage

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

See `examples/dht_bootstrap.rs` for a complete example.

## Architecture (planned)

- `dht` — KRPC-over-UDP client + Kademlia-style routing table
  - ✅ bootstrap
  - ⏳ topic announce
  - ⏳ peer lookup
- `discovery` — orchestrates per-topic lifecycle and connection attempts
- `holepunch` — UDP holepunch coordination (probe → exchange → punch)
- `transport` — encrypted stream transport using Noise (XX handshake)
- `protocol` — wire format definitions (KRPC message types + serialization)

## PluresDB integration

PluresDB sync needs a robust peer discovery + transport layer to replicate state between devices without relying on centralized infra.

Hyperswarm-rs is intended to be used by the PluresDB sync module as:

1. Derive a `Topic` from a shared secret / collection key
2. `join(topic)` to announce + discover peers
3. Establish encrypted connections (Noise) to exchange replication messages

## Development notes

- The JS Hyperswarm implementation is the reference for protocol details and interop testing.
- Uses `serde_bencode` for KRPC message encoding/decoding (BEP 5).
- Early versions may use QUIC (`quinn`) for a stream abstraction while the raw UDP framing evolves.

## Testing

```bash
cargo test
cargo run --example dht_bootstrap
```

## License

AGPL-3.0 (matches upstream project licensing expectations; confirm when wiring interop).
