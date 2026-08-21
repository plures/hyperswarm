//! Managed direct UDP connections.
//!
//! Discovery yields untrusted socket addresses. This module owns the bounded
//! direct-connection lifecycle for those addresses: one UDP receiver task
//! demultiplexes packets by peer, and each admitted peer receives a dedicated
//! Noise stream. DHT lookup never authenticates a peer; authentication happens
//! only during the Noise handshake.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use tokio::{
    net::UdpSocket,
    sync::{mpsc, Mutex as AsyncMutex},
    task::AbortHandle,
};

use crate::{
    transport::{EncryptedStream, TransportError},
    Topic,
};

const PACKET_QUEUE_CAPACITY: usize = 32;
const MAX_PACKET_SIZE: usize = 65_535;

#[derive(thiserror::Error, Debug)]
pub enum ConnectionError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("connection capacity reached")]
    CapacityReached,
    #[error("connection to {0} is already active or pending")]
    AlreadyRegistered(SocketAddr),
    #[error("incoming connection queue closed")]
    IncomingQueueClosed,
}

struct PendingConnection {
    remote_addr: SocketAddr,
    receiver: mpsc::Receiver<Bytes>,
}

struct ConnectionManagerInner {
    socket: Arc<UdpSocket>,
    max_peers: usize,
    routes: Mutex<HashMap<SocketAddr, mpsc::Sender<Bytes>>>,
    incoming_tx: mpsc::Sender<PendingConnection>,
}

/// Owns the UDP packet dispatcher and a bounded set of direct connections.
///
/// The manager deliberately does not choose peers, retry failed connections,
/// or interpret discovery results. Callers supply the peer address and topic;
/// this layer performs only the resulting network effects.
pub struct ConnectionManager {
    inner: Arc<ConnectionManagerInner>,
    incoming_rx: AsyncMutex<mpsc::Receiver<PendingConnection>>,
    dispatcher: AbortHandle,
}

impl ConnectionManager {
    pub async fn bind(port: u16, max_peers: usize) -> Result<Self, ConnectionError> {
        let socket = Arc::new(UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await?);
        let (incoming_tx, incoming_rx) = mpsc::channel(max_peers.max(1));
        let inner = Arc::new(ConnectionManagerInner {
            socket: Arc::clone(&socket),
            max_peers,
            routes: Mutex::new(HashMap::new()),
            incoming_tx,
        });
        let dispatcher = tokio::spawn(run_dispatcher(Arc::clone(&inner))).abort_handle();

        Ok(Self {
            inner,
            incoming_rx: AsyncMutex::new(incoming_rx),
            dispatcher,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, ConnectionError> {
        Ok(self.inner.socket.local_addr()?)
    }

    /// Initiate a topic-bound, optionally identity-pinned connection.
    pub async fn connect(
        &self,
        topic: Topic,
        remote_addr: SocketAddr,
        expected_remote_static_key: Option<[u8; 32]>,
    ) -> Result<ManagedConnection, ConnectionError> {
        let receiver = self.register(remote_addr)?;
        let mut stream = EncryptedStream::new_with_inbound_packets(
            Arc::clone(&self.inner.socket),
            remote_addr,
            receiver,
        )
        .await?;

        if let Err(error) = stream
            .handshake_initiator_with_payload(&topic.0, expected_remote_static_key)
            .await
        {
            self.unregister(remote_addr);
            return Err(error.into());
        }

        Ok(ManagedConnection {
            inner: Arc::clone(&self.inner),
            remote_addr,
            stream,
        })
    }

    /// Accept the next inbound connection for `topic`.
    ///
    /// The first Noise payload carries the topic and is authenticated by the
    /// completed handshake. A mismatched topic is rejected before a stream is
    /// exposed to the caller.
    pub async fn accept(&self, topic: Topic) -> Result<ManagedConnection, ConnectionError> {
        let pending = self
            .incoming_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or(ConnectionError::IncomingQueueClosed)?;
        let remote_addr = pending.remote_addr;
        let mut stream = EncryptedStream::new_with_inbound_packets(
            Arc::clone(&self.inner.socket),
            remote_addr,
            pending.receiver,
        )
        .await?;

        if let Err(error) = stream
            .handshake_responder_with_expected_payload(&topic.0)
            .await
        {
            self.unregister(remote_addr);
            return Err(error.into());
        }

        Ok(ManagedConnection {
            inner: Arc::clone(&self.inner),
            remote_addr,
            stream,
        })
    }

    pub fn shutdown(&self) {
        self.dispatcher.abort();
        self.inner
            .routes
            .lock()
            .expect("connection routes lock poisoned")
            .clear();
    }

    fn register(&self, remote_addr: SocketAddr) -> Result<mpsc::Receiver<Bytes>, ConnectionError> {
        let mut routes = self
            .inner
            .routes
            .lock()
            .expect("connection routes lock poisoned");
        if routes.contains_key(&remote_addr) {
            return Err(ConnectionError::AlreadyRegistered(remote_addr));
        }
        if routes.len() >= self.inner.max_peers {
            return Err(ConnectionError::CapacityReached);
        }

        let (sender, receiver) = mpsc::channel(PACKET_QUEUE_CAPACITY);
        routes.insert(remote_addr, sender);
        Ok(receiver)
    }

    fn unregister(&self, remote_addr: SocketAddr) {
        self.inner
            .routes
            .lock()
            .expect("connection routes lock poisoned")
            .remove(&remote_addr);
    }
}

impl Drop for ConnectionManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// An established managed encrypted stream. Dropping it releases the peer's
/// bounded routing slot, so reconnects do not depend on a stale in-memory map.
pub struct ManagedConnection {
    inner: Arc<ConnectionManagerInner>,
    remote_addr: SocketAddr,
    stream: EncryptedStream,
}

impl ManagedConnection {
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    pub fn remote_static_key(&self) -> Option<[u8; 32]> {
        self.stream.remote_static_key()
    }

    pub async fn send(&mut self, data: Bytes) -> Result<(), ConnectionError> {
        self.stream.send(data).await.map_err(Into::into)
    }

    pub async fn recv(&mut self) -> Result<Bytes, ConnectionError> {
        self.stream.recv().await.map_err(Into::into)
    }
}

impl Drop for ManagedConnection {
    fn drop(&mut self) {
        self.inner
            .routes
            .lock()
            .expect("connection routes lock poisoned")
            .remove(&self.remote_addr);
    }
}

async fn run_dispatcher(inner: Arc<ConnectionManagerInner>) {
    let mut buffer = vec![0_u8; MAX_PACKET_SIZE];

    loop {
        let Ok((length, remote_addr)) = inner.socket.recv_from(&mut buffer).await else {
            break;
        };
        let packet = Bytes::copy_from_slice(&buffer[..length]);
        let pending = {
            let mut routes = inner
                .routes
                .lock()
                .expect("connection routes lock poisoned");
            if let Some(sender) = routes.get(&remote_addr) {
                // A full or closed per-peer queue must fail the transient
                // route rather than silently dropping an authenticated packet.
                // The stream then observes its channel closed on the next
                // receive and the slot becomes available for a clean retry.
                let route_failed = sender.try_send(packet).is_err();
                if route_failed {
                    routes.remove(&remote_addr);
                }
                None
            } else if routes.len() >= inner.max_peers {
                None
            } else {
                let (sender, receiver) = mpsc::channel(PACKET_QUEUE_CAPACITY);
                if sender.try_send(packet).is_err() {
                    None
                } else {
                    routes.insert(remote_addr, sender);
                    Some(PendingConnection {
                        remote_addr,
                        receiver,
                    })
                }
            }
        };

        if let Some(pending) = pending {
            if inner.incoming_tx.send(pending).await.is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, sync::Arc, time::Duration};

    use super::*;
    use tokio::net::UdpSocket;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn managed_connections_authenticate_exchange_and_reconnect() {
        let initiator = ConnectionManager::bind(0, 2)
            .await
            .expect("initiator should bind");
        let responder = Arc::new(
            ConnectionManager::bind(0, 2)
                .await
                .expect("responder should bind"),
        );
        let topic = Topic::from_key(b"managed-connection-test-topic");
        let responder_addr = SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            responder
                .local_addr()
                .expect("responder should expose an address")
                .port(),
        );

        let accepting = Arc::clone(&responder);
        let accept = tokio::spawn(async move { accepting.accept(topic).await });
        let mut client = initiator
            .connect(topic, responder_addr, None)
            .await
            .expect("initiator should complete a managed handshake");
        let mut server = tokio::time::timeout(Duration::from_secs(2), accept)
            .await
            .expect("responder should accept promptly")
            .expect("responder task should not panic")
            .expect("responder handshake should succeed");

        client
            .send(Bytes::from_static(b"first managed payload"))
            .await
            .expect("client should encrypt and send");
        assert_eq!(
            server.recv().await.expect("server should decrypt payload"),
            Bytes::from_static(b"first managed payload")
        );
        assert_eq!(
            client.remote_static_key(),
            Some(server.stream.local_static_pubkey())
        );

        drop(client);
        drop(server);

        let accepting = Arc::clone(&responder);
        let accept = tokio::spawn(async move { accepting.accept(topic).await });
        let mut client = initiator
            .connect(topic, responder_addr, None)
            .await
            .expect("released routes should allow reconnect");
        let mut server = tokio::time::timeout(Duration::from_secs(2), accept)
            .await
            .expect("responder should accept reconnect promptly")
            .expect("responder task should not panic")
            .expect("responder reconnect should succeed");
        client
            .send(Bytes::from_static(b"reconnected"))
            .await
            .expect("reconnected client should send");
        assert_eq!(
            server
                .recv()
                .await
                .expect("reconnected server should receive"),
            Bytes::from_static(b"reconnected")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accept_rejects_a_mismatched_topic() {
        let initiator = ConnectionManager::bind(0, 1)
            .await
            .expect("initiator should bind");
        let responder = Arc::new(
            ConnectionManager::bind(0, 1)
                .await
                .expect("responder should bind"),
        );
        let expected_topic = Topic::from_key(b"expected-topic");
        let wrong_topic = Topic::from_key(b"wrong-topic");
        let responder_addr = SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            responder
                .local_addr()
                .expect("responder should expose an address")
                .port(),
        );

        let accepting = Arc::clone(&responder);
        let accept = tokio::spawn(async move { accepting.accept(expected_topic).await });
        let connecting =
            tokio::spawn(async move { initiator.connect(wrong_topic, responder_addr, None).await });

        let result = tokio::time::timeout(Duration::from_secs(2), accept)
            .await
            .expect("topic mismatch should be rejected promptly")
            .expect("responder task should not panic");
        assert!(matches!(
            result,
            Err(ConnectionError::Transport(TransportError::TopicMismatch))
        ));
        connecting.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn overloaded_packet_queue_closes_the_route_instead_of_dropping_data() {
        let manager = ConnectionManager::bind(0, 1)
            .await
            .expect("manager should bind");
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("packet source should bind");
        let source_addr = sender
            .local_addr()
            .expect("packet source should have an address");
        let receiver = manager
            .register(source_addr)
            .expect("test route should register");
        let manager_addr = manager
            .local_addr()
            .expect("manager should have an address");
        let destination = SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            manager_addr.port(),
        );

        for _ in 0..=PACKET_QUEUE_CAPACITY {
            sender
                .send_to(b"packet", destination)
                .await
                .expect("packet should be sent locally");
        }

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let route_is_open = manager
                    .inner
                    .routes
                    .lock()
                    .expect("connection routes lock poisoned")
                    .contains_key(&source_addr);
                if !route_is_open {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("queue overload should close the route promptly");

        let mut receiver = receiver;
        while receiver.recv().await.is_some() {}
    }
}
