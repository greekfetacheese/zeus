use zeus_railgun::RailgunEngine;
use zeus_railgun_shared::RailgunKeys;
use secure_types::SecureArray;
use rand::Rng;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let mut key = [0u8; 64];
    rand::thread_rng().try_fill(&mut key)?;
    let sec_seed = SecureArray::from_slice(&key)?;

    let keys = RailgunKeys::new(sec_seed, 0)?;
    let mut engine = RailgunEngine::new(keys, 1)?;

    engine.start_clients().await?;
    println!("Clients started");

    Ok(())
}