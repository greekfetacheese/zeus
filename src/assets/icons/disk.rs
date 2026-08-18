use crate::core::context::data_dir;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;

const DIR_NAME: &str = "token_icons";

fn token_icons_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(DIR_NAME);
   if !dir.exists() {
      std::fs::create_dir_all(&dir)?;
   }
   Ok(dir)
}

fn icon_dir(chain_id: u64, address: Address) -> Result<PathBuf, anyhow::Error> {
   Ok(token_icons_dir()?.join(chain_id.to_string()).join(format!("{address:#x}")))
}

pub fn save_token_icon(
   chain_id: u64,
   address: Address,
   x32: &[u8],
   x24: &[u8],
) -> Result<(), anyhow::Error> {
   let dir = icon_dir(chain_id, address)?;
   std::fs::create_dir_all(&dir)?;
   std::fs::write(dir.join("x32.png"), x32)?;
   std::fs::write(dir.join("x24.png"), x24)?;
   Ok(())
}

/// Load previously downloaded token icons from `data/token_icons/`.
///
/// Baked-in icons are merged by the caller and take priority.
pub fn load_downloaded_icons() -> HashMap<(Address, u64), (Vec<u8>, Vec<u8>)> {
   let mut map = HashMap::new();
   let root = match token_icons_dir() {
      Ok(dir) => dir,
      Err(e) => {
         tracing::warn!("Failed to resolve token icon dir: {e}");
         return map;
      }
   };

   let Ok(chain_entries) = std::fs::read_dir(&root) else {
      return map;
   };

   for chain_entry in chain_entries.flatten() {
      if !chain_entry.path().is_dir() {
         continue;
      }

      let chain_id = match chain_entry.file_name().to_string_lossy().parse::<u64>() {
         Ok(id) => id,
         Err(_) => continue,
      };

      let Ok(token_entries) = std::fs::read_dir(chain_entry.path()) else {
         continue;
      };

      for token_entry in token_entries.flatten() {
         if !token_entry.path().is_dir() {
            continue;
         }

         let addr_str = token_entry.file_name().to_string_lossy().to_string();
         let Ok(address) = Address::from_str(&addr_str) else {
            continue;
         };

         let x32_path = token_entry.path().join("x32.png");
         let x24_path = token_entry.path().join("x24.png");
         let (Ok(x32), Ok(x24)) = (std::fs::read(x32_path), std::fs::read(x24_path)) else {
            continue;
         };
         
         if x32.is_empty() || x24.is_empty() {
            continue;
         }
         
         map.insert((address, chain_id), (x32, x24));
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Loaded {} downloaded token icons from disk",
      map.len()
   );

   map
}
