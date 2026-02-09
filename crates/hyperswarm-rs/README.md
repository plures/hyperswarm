# hyperswarm-rs (WIP)

Rust implementation scaffold of **Hyperswarm** (P2P discovery via DHT + NAT holepunching) for **PluresDB**.

- Repo: <https://github.com/plures/hyperswarm>
- Tracking: plures/pluresdb#70

## Status

This crate is currently a **scaffold**: public API shape + module boundaries are in place, but core networking/protocol logic is `todo!/unimplemented!`.

## Architecture (planned)

- `dht` — KRPC-over-UDP client + Kademlia-style routing table
  - bootstrap
  - topic announce
  - peer lookup
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
- Early versions may use QUIC (`quinn`) for a stream abstraction while the raw UDP framing evolves.

## License

AGPL-3.0 (matches upstream project licensing expectations; confirm when wiring interop).
