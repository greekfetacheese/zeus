use alloy_primitives::Address;
use image::codecs::png::PngEncoder;
use std::{io::Cursor, path::PathBuf, str::FromStr};

use bincode::{config::standard, encode_to_vec};
use serde::{Deserialize, Serialize};
use zeus_token_list::TokenData;

#[derive(Clone, Serialize, Deserialize)]
struct TokenInfo {
   name: String,
   #[serde(rename = "type")]
   type2: String,
   symbol: String,
   decimals: u8,
   website: String,
   description: String,
   explorer: String,
   status: String,
   #[serde(rename = "id")]
   address: String,
}

fn main() -> Result<(), anyhow::Error> {
   let current_dir = std::env::current_dir()?;
   let crates = current_dir.join("crates");
   let zeus_token_list = crates.join("zeus-token-list");
   let data_dir = zeus_token_list.join("token_data");
   eprintln!("Directory: {:?}", data_dir);

   let chains = [1, 10, 56, 8453, 42161];

   let mut icon_data: Vec<TokenData> = Vec::new();

   for chain in chains {
      let dir = if chain == 1 {
         data_dir.join("ethereum")
      } else if chain == 10 {
         data_dir.join("optimism")
      } else if chain == 56 {
         data_dir.join("binance")
      } else if chain == 8453 {
         data_dir.join("base")
      } else if chain == 42161 {
         data_dir.join("arbitrum")
      } else {
         continue;
      };

      let asset_dir = dir.join("assets");
      if !asset_dir.exists() {
         eprintln!("Assets directory does not exist: {:?}", asset_dir);
         continue;
      }

      make_token_data(&asset_dir, chain, &mut icon_data)?;
   }

   let binary_data = encode_to_vec(&icon_data, standard())?;
   let output_path = zeus_token_list.join("token_data.data");
   std::fs::write(&output_path, &binary_data)?;

   eprintln!("Token data saved successfully!");

   Ok(())
}

fn make_token_data(
   directory: &PathBuf,
   chain_id: u64,
   icons: &mut Vec<TokenData>,
) -> Result<(), anyhow::Error> {
   for entry in std::fs::read_dir(directory)? {
      let entry = entry?;
      let path = entry.path();
      if path.is_dir() {
         let logo_path = path.join("logo.png");
         let info_path = path.join("info.json");

         if !logo_path.exists() || !logo_path.is_file() {
            eprintln!("No logo for {:?}", path);
            continue;
         }

         if !info_path.exists() || !info_path.is_file() {
            eprintln!("No info.json for {:?}", path);
            continue;
         }

         let info_data = std::fs::read_to_string(&info_path)?;
         let info = serde_json::from_str::<TokenInfo>(&info_data)?;

         // Avoid tokens with invalid addresses
         match Address::from_str(&info.address) {
            Ok(_) => {}
            Err(_) => {
               eprintln!("Invalid Ethereum address for {}", info.address);
               continue;
            }
         }

         let img = match image::open(&logo_path) {
            Ok(img) => img,
            Err(e) => {
               eprintln!("Failed to open image for {}: {}", info.address, e);
               continue;
            }
         };

         let mut write_buffer = Vec::new();

         {
            let mut cursor = Cursor::new(&mut write_buffer);
            let encoder = PngEncoder::new_with_quality(
               &mut cursor,
               image::codecs::png::CompressionType::Best,
               image::codecs::png::FilterType::Sub,
            );
            img.write_with_encoder(encoder)?;
         }

         icons.push(TokenData::new(
            chain_id,
            info.address,
            info.name,
            info.symbol,
            info.decimals,
            write_buffer,
         ));
      }
   }

   Ok(())
}
