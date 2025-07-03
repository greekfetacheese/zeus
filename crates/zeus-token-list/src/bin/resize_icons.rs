use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

      resize_icons(&asset_dir)?;
   }

   eprintln!("Icon data resized successfully!");

   Ok(())
}

fn resize_icons(directory: &PathBuf) -> Result<(), anyhow::Error> {
   for entry in std::fs::read_dir(directory)? {
      let entry = entry?;
      let path = entry.path();
      if path.is_dir() {
         let logo_path = path.join("logo.png");
         let address = match path.file_name() {
            Some(name) => name.to_str().unwrap().to_string(),
            None => {
               eprintln!("No file name for {:?}", path);
               continue;
            }
         };

         if logo_path.exists() && logo_path.is_file() {
            let img = match image::open(&logo_path) {
               Ok(img) => img,
               Err(e) => {
                  eprintln!("Failed to open image for {}: {}", address, e);
                  continue;
               }
            };

            let resized_img = img.resize(32, 32, image::imageops::FilterType::Lanczos3);

            match resized_img.save(&logo_path) {
               Ok(_) => {}
               Err(e) => {
                  eprintln!("Img save Error for {}: {}", address, e);
                  continue;
               }
            }
         }
      }
   }

   Ok(())
}
