#[path = "../tokens/mod.rs"]
mod tokens;

use zeus_token_list::TokenIconData;
use std::{ path::PathBuf, io::{BufWriter, Write, Cursor} };

// Extract all the icon data from the images into a single token-icons.json file
fn main() -> Result<(), anyhow::Error> {
    let current_dir = std::env::current_dir()?;
    let images_dir = current_dir.join("icons");


    let mut icons: Vec<TokenIconData> = Vec::new();
    let chains = [
        ("ethereum", 1),
        ("base", 8453),
        ("optimism", 10),
        ("arbitrum", 42161),
        ("bnb", 56),
    ];

    for chain in chains {
        let dir = images_dir.join(chain.0);
        get_icon_data(&dir, chain.1, &mut icons)?;
    }

    let string = serde_json::to_string(&icons)?;
    std::fs::write("token-icons.json", string)?;

    println!("Token icons extracted successfully!");
    Ok(())
}

fn get_icon_data(
    directory: &PathBuf,
    chain_id: u64,
    icons: &mut Vec<TokenIconData>,
) -> Result<(), anyhow::Error> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_stem() {
                if let Some(address_str) = file_name.to_str() {
                    let img = image::open(&path)?;
                    let mut write_buffer = Vec::new();

                    {
                        let mut cursor = Cursor::new(&mut write_buffer);
                        let mut buf_writer = BufWriter::new(&mut cursor);
                        img.write_to(&mut buf_writer, image::ImageFormat::Png)?;
                        buf_writer.flush()?;
                    } 

                    icons.push(TokenIconData::new(address_str.to_string(), chain_id, write_buffer));
                }
            }
        }
    }

    Ok(())
}