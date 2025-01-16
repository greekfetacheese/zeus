#[path = "../tokens/mod.rs"]
mod tokens;

use std::time::Duration;
use std::path::PathBuf;
use tokens::*;
use reqwest;
use tokio;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let ethereum: Vec<UniswapToken> = serde_json::from_str(ETHEREUM)?;
    let base: Vec<UniswapToken> = serde_json::from_str(BASE)?;
    let op: Vec<UniswapToken> = serde_json::from_str(OPTIMISM)?;
    let arbitrum: Vec<UniswapToken> = serde_json::from_str(ARBITRUM)?;
    let bnb: Vec<UniswapToken> = serde_json::from_str(BINANCE_SMART_CHAIN)?;


    let current_dir = std::env::current_dir()?;
    let save_dir = current_dir.join("icons");

    let eth_path = save_dir.join("ethereum");
    let base_path = save_dir.join("base");
    let op_path = save_dir.join("optimism");
    let arbitrum_path = save_dir.join("arbitrum");
    let bnb_path = save_dir.join("bnb");

    std::fs::create_dir_all(&save_dir.join("ethereum"))?;
    std::fs::create_dir_all(&save_dir.join("base"))?;
    std::fs::create_dir_all(&save_dir.join("optimism"))?;
    std::fs::create_dir_all(&save_dir.join("arbitrum"))?;
    std::fs::create_dir_all(&save_dir.join("bnb"))?;

    for token in ethereum {
        if let Err(e) = download_img(token, &eth_path).await {
            println!("Error downloading token image {:?}", e);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    for token in base {
        if let Err(e) = download_img(token, &base_path).await {
            println!("Error downloading token image {:?}", e);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    for token in op {
        if let Err(e) = download_img(token, &op_path).await {
            println!("Error downloading token image {:?}", e);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    for token in arbitrum {
        if let Err(e) = download_img(token, &arbitrum_path).await {
            println!("Error downloading token image {:?}", e);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    for token in bnb {
        if let Err(e) = download_img(token, &bnb_path).await {
            println!("Error downloading token image {:?}", e);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    println!("Finished downloading all images");

    Ok(())
}

async fn download_img(token: UniswapToken, path: &PathBuf) -> Result<(), anyhow::Error> {
    let file_name = format!("{}.png", token.address);
    if path.join(&file_name).exists() {
        println!("Image already exists for {} Skipping...", token.address);
        return Ok(());
    }

    println!("Downloading image from: {}", token.logo_uri);
    let response = reqwest::get(token.logo_uri).await?;
    let bytes = response.bytes().await?;
    let img = image::load_from_memory(&bytes)?;

    let full_path = path.join(&file_name);
    img.save(full_path)?;
    println!("Saved image as {}", file_name);
    Ok(())
}