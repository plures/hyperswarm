//! Retained two-process managed-connection acceptance harness.
//!
//! Run `server` and `client` as separate processes, preferably on separate
//! hosts. The client refuses loopback/unspecified peers so a successful run
//! exercises a real interface rather than silently falling back to localhost.

use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    process,
    time::Duration,
};

use bytes::Bytes;
use hyperswarm::{dht::PeerAddress, Hyperswarm, SwarmConfig, Topic};
use serde_json::json;

const FIRST_PAYLOAD: &[u8] = b"hyperswarm-lab:first";
const RECONNECT_PAYLOAD: &[u8] = b"hyperswarm-lab:reconnect";
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(15);
const REFUSAL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug)]
enum Role {
    Server,
    Client,
}

#[derive(Debug)]
struct LabArgs {
    role: Role,
    topic: String,
    port: Option<u16>,
    peer: Option<SocketAddr>,
    evidence: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = match parse_args(env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("argument error: {error}\n\n{}", usage());
            process::exit(2);
        }
    };

    let role = match args.role {
        Role::Server => "server",
        Role::Client => "client",
    };
    let result = match args.role {
        Role::Server => run_server(&args).await,
        Role::Client => run_client(&args).await,
    };

    let (outcome, steps, error) = match result {
        Ok(steps) => ("passed", steps, None),
        Err(error) => ("failed", Vec::new(), Some(error)),
    };
    if let Err(error) = write_evidence(&args.evidence, role, outcome, &steps, error.as_deref()) {
        eprintln!("could not retain evidence: {error}");
        process::exit(1);
    }

    println!(
        "LAB_RESULT role={role} outcome={outcome} evidence={}",
        args.evidence.display()
    );
    if let Some(error) = error {
        eprintln!("LAB_FAILURE role={role} error={error}");
        process::exit(1);
    }
}

async fn run_server(args: &LabArgs) -> Result<Vec<&'static str>, String> {
    let port = args
        .port
        .ok_or_else(|| "server requires --port".to_string())?;
    if port == 0 {
        return Err("server --port must be non-zero so the client can target it".to_string());
    }
    let topic = Topic::from_key(args.topic.as_bytes());
    let swarm = start_swarm(port).await?;
    println!("LAB_READY role=server port={port}");

    let mut steps = Vec::new();
    receive_expected(&swarm, topic, FIRST_PAYLOAD).await?;
    println!("LAB_STEP server=first_delivery");
    steps.push("first_delivery");

    receive_expected(&swarm, topic, RECONNECT_PAYLOAD).await?;
    println!("LAB_STEP server=reconnect_delivery");
    steps.push("reconnect_delivery");

    // The client intentionally offers a different topic.  Keep this side on
    // the original topic so this is an actual contract-refusal check rather
    // than a second successful connection on the same alternate topic.
    let refusal = tokio::time::timeout(ACCEPT_TIMEOUT, swarm.accept(topic))
        .await
        .map_err(|_| "timed out waiting for the mismatched-topic attempt".to_string())?;
    match refusal {
        Err(error) if error.to_string().contains("topic handshake payload") => {
            println!("LAB_STEP server=topic_refusal");
            steps.push("topic_refusal");
        }
        Ok(_) => return Err("mismatched topic was admitted".to_string()),
        Err(error) => return Err(format!("unexpected topic-refusal error: {error}")),
    }

    Ok(steps)
}

async fn run_client(args: &LabArgs) -> Result<Vec<&'static str>, String> {
    let peer = args
        .peer
        .ok_or_else(|| "client requires --peer".to_string())?;
    if peer.ip().is_loopback() || peer.ip().is_unspecified() {
        return Err("client --peer must be a non-loopback, non-unspecified address".to_string());
    }
    let topic = Topic::from_key(args.topic.as_bytes());
    let peer = PeerAddress {
        addr: peer,
        node_id: None,
    };
    let mut steps = Vec::new();

    send_payload(topic, peer.clone(), FIRST_PAYLOAD).await?;
    println!("LAB_STEP client=first_delivery");
    steps.push("first_delivery");

    send_payload(topic, peer.clone(), RECONNECT_PAYLOAD).await?;
    println!("LAB_STEP client=reconnect_delivery");
    steps.push("reconnect_delivery");

    let refusal_swarm = start_swarm(0).await?;
    match tokio::time::timeout(
        REFUSAL_TIMEOUT,
        refusal_swarm.connect(wrong_topic(args), peer, None),
    )
    .await
    {
        Err(_) => {
            // The responder rejects after the first Noise message and does not
            // reveal a stream. The initiator sees that as a bounded refusal.
            println!("LAB_STEP client=topic_refusal_observed");
            steps.push("topic_refusal_observed");
        }
        Ok(Err(error)) => {
            println!("LAB_STEP client=topic_refusal_observed error={error}");
            steps.push("topic_refusal_observed");
        }
        Ok(Ok(_)) => return Err("mismatched topic established a stream".to_string()),
    }

    Ok(steps)
}

async fn start_swarm(port: u16) -> Result<Hyperswarm, String> {
    Hyperswarm::new(SwarmConfig {
        bootstrap: Vec::new(),
        port,
        max_peers: 4,
    })
    .await
    .map_err(|error| error.to_string())
}

async fn receive_expected(swarm: &Hyperswarm, topic: Topic, expected: &[u8]) -> Result<(), String> {
    let mut connection = tokio::time::timeout(ACCEPT_TIMEOUT, swarm.accept(topic))
        .await
        .map_err(|_| "timed out waiting for the managed connection".to_string())?
        .map_err(|error| error.to_string())?;
    let payload = tokio::time::timeout(ACCEPT_TIMEOUT, connection.recv())
        .await
        .map_err(|_| "timed out waiting for an encrypted payload".to_string())?
        .map_err(|error| error.to_string())?;
    if payload.as_ref() != expected {
        return Err(format!("unexpected payload: {:?}", payload));
    }
    if connection.remote_static_key().is_none() {
        return Err("managed connection did not expose a Noise peer key".to_string());
    }
    Ok(())
}

async fn send_payload(topic: Topic, peer: PeerAddress, payload: &[u8]) -> Result<(), String> {
    let swarm = start_swarm(0).await?;
    let mut connection = tokio::time::timeout(ACCEPT_TIMEOUT, swarm.connect(topic, peer, None))
        .await
        .map_err(|_| "timed out establishing the managed connection".to_string())?
        .map_err(|error| error.to_string())?;
    if connection.remote_static_key().is_none() {
        return Err("managed connection did not expose a Noise peer key".to_string());
    }
    connection
        .send(Bytes::copy_from_slice(payload))
        .await
        .map_err(|error| error.to_string())
}

fn wrong_topic(args: &LabArgs) -> Topic {
    Topic::from_key(format!("{}:mismatch", args.topic).as_bytes())
}

fn write_evidence(
    path: &Path,
    role: &str,
    outcome: &str,
    steps: &[&str],
    error: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let evidence = json!({
        "harness": "managed_connection_lab",
        "role": role,
        "outcome": outcome,
        "steps": steps,
        "error": error,
    });
    std::fs::write(path, format!("{}\n", evidence)).map_err(|error| error.to_string())
}

fn parse_args<I>(arguments: I) -> Result<LabArgs, String>
where
    I: IntoIterator<Item = String>,
{
    let mut values = arguments.into_iter();
    let mut role = None;
    let mut topic = None;
    let mut port = None;
    let mut peer = None;
    let mut evidence = None;

    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--role" => {
                role = Some(match value.as_str() {
                    "server" => Role::Server,
                    "client" => Role::Client,
                    _ => return Err(format!("unsupported role {value:?}")),
                });
            }
            "--topic" => topic = Some(value),
            "--port" => {
                port = Some(value.parse::<u16>().map_err(|error| error.to_string())?);
            }
            "--peer" => {
                peer = Some(
                    value
                        .parse::<SocketAddr>()
                        .map_err(|error| error.to_string())?,
                );
            }
            "--evidence" => evidence = Some(PathBuf::from(value)),
            _ => return Err(format!("unsupported argument {flag:?}")),
        }
    }

    Ok(LabArgs {
        role: role.ok_or_else(|| "missing --role".to_string())?,
        topic: topic.ok_or_else(|| "missing --topic".to_string())?,
        port,
        peer,
        evidence: evidence.ok_or_else(|| "missing --evidence".to_string())?,
    })
}

fn usage() -> &'static str {
    "Usage:\n  managed_connection_lab --role server --port 42042 --topic <shared-lab-topic> --evidence <path>\n  managed_connection_lab --role client --peer <non-loopback-ip:port> --topic <shared-lab-topic> --evidence <path>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_arguments() {
        let args = parse_args(
            [
                "--role",
                "server",
                "--port",
                "42042",
                "--topic",
                "lab",
                "--evidence",
                "server.json",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("server arguments should parse");

        assert!(matches!(args.role, Role::Server));
        assert_eq!(args.port, Some(42042));
    }

    #[test]
    fn rejects_loopback_peer_before_connecting() {
        let args = parse_args(
            [
                "--role",
                "client",
                "--peer",
                "127.0.0.1:42042",
                "--topic",
                "lab",
                "--evidence",
                "client.json",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("client arguments should parse");

        let result = tokio::runtime::Runtime::new()
            .expect("runtime should start")
            .block_on(run_client(&args));
        assert!(result.is_err());
        assert!(result
            .expect_err("loopback must be rejected")
            .contains("non-loopback"));
    }
}
