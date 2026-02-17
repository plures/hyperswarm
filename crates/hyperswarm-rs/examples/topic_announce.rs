//! Example: Topic Announce and Lookup
//!
//! This example demonstrates how to announce and lookup peers on a topic
//! using the DHT.
//!
//! Run with:
//! ```bash
//! cargo run --example topic_announce
//! ```

use hyperswarm::{Hyperswarm, SwarmConfig, Topic};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating Hyperswarm instance...");
    
    let config = SwarmConfig::default();
    let swarm = Hyperswarm::new(config).await?;
    
    println!("Hyperswarm instance created successfully!");
    
    // Create a topic from a key
    let topic_key = b"example-topic-12345";
    let topic = Topic::from_key(topic_key);
    println!("\nTopic: {:02x?}", &topic.0[..8]);
    
    println!("\nJoining topic (announce + lookup)...");
    match swarm.join(topic).await {
        Ok(_) => println!("Successfully joined topic!"),
        Err(e) => println!("Join completed with result: {}", e),
    }
    
    println!("\nFlushing pending operations...");
    swarm.flush().await?;
    
    println!("\nShutting down...");
    swarm.destroy().await?;
    
    println!("Done!");
    
    Ok(())
}
