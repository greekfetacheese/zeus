use crate::core::persisted::{
   NFT_ICON_X64, NFT_ICON_X250, PersistedTree, TOKEN_ICON_X32, tree_dir,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use zeus_eth::alloy_primitives::{Address, U256};

use super::{NftIconData, NftKey};

fn token_icons_dir() -> Result<PathBuf, anyhow::Error> {
   tree_dir(PersistedTree::TokenIcons)
}

fn icon_dir(chain_id: u64, address: Address) -> Result<PathBuf, anyhow::Error> {
   Ok(token_icons_dir()?.join(chain_id.to_string()).join(format!("{address:#x}")))
}

pub fn save_token_icon(chain_id: u64, address: Address, x32: &[u8]) -> Result<(), anyhow::Error> {
   let dir = icon_dir(chain_id, address)?;
   std::fs::create_dir_all(&dir)?;
   std::fs::write(dir.join(TOKEN_ICON_X32), x32)?;
   Ok(())
}

/// Remove a downloaded token icon directory if it exists.
pub fn delete_token_icon(chain_id: u64, address: Address) -> Result<(), anyhow::Error> {
   let dir = icon_dir(chain_id, address)?;
   if dir.exists() {
      std::fs::remove_dir_all(&dir)?;
   }
   Ok(())
}

/// Load previously downloaded token icons from `data/token_icons/`.
///
/// Baked-in icons are merged by the caller and take priority.
pub fn load_downloaded_icons() -> HashMap<(Address, u64), Vec<u8>> {
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

         let x32_path = token_entry.path().join(TOKEN_ICON_X32);
         let Ok(x32) = std::fs::read(x32_path) else {
            continue;
         };

         if x32.is_empty() {
            continue;
         }

         map.insert((address, chain_id), x32);
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Loaded {} downloaded token icons from disk",
      map.len()
   );

   map
}

fn nft_icons_dir() -> Result<PathBuf, anyhow::Error> {
   tree_dir(PersistedTree::NftIcons)
}

/// `nft_icons/{chain}/{collection}/{tokenId}`.
///
/// The token id is a decimal directory name because it is a `uint256` — the persisted-path
/// validator rejects any other spelling, so there is exactly one directory per token.
fn nft_icon_dir(
   chain_id: u64,
   collection: Address,
   token_id: U256,
) -> Result<PathBuf, anyhow::Error> {
   Ok(nft_icons_dir()?
      .join(chain_id.to_string())
      .join(format!("{collection:#x}"))
      .join(token_id.to_string()))
}

/// Write both renderings of an NFT image, so the grid and the detail view never go back to the
/// network for a picture we already have. Empty renderings are skipped rather than written blank.
pub fn save_nft_icon(
   chain_id: u64,
   collection: Address,
   token_id: U256,
   data: &NftIconData,
) -> Result<(), anyhow::Error> {
   let dir = nft_icon_dir(chain_id, collection, token_id)?;
   std::fs::create_dir_all(&dir)?;

   if !data.x64.is_empty() {
      std::fs::write(dir.join(NFT_ICON_X64), &data.x64)?;
   }
   if !data.x250.is_empty() {
      std::fs::write(dir.join(NFT_ICON_X250), &data.x250)?;
   }

   Ok(())
}

/// Remove a downloaded NFT image directory if it exists.
pub fn delete_nft_icon(
   chain_id: u64,
   collection: Address,
   token_id: U256,
) -> Result<(), anyhow::Error> {
   let dir = nft_icon_dir(chain_id, collection, token_id)?;
   if dir.exists() {
      std::fs::remove_dir_all(&dir)?;
   }
   Ok(())
}

/// Load previously downloaded NFT images from `data/nft_icons/`.
///
/// A token directory is kept only if at least one rendering is present and non-empty, so a
/// half-written directory does not surface as an icon with a blank picture.
pub fn load_downloaded_nft_icons() -> HashMap<NftKey, NftIconData> {
   let mut map = HashMap::new();

   let root = match nft_icons_dir() {
      Ok(dir) => dir,
      Err(e) => {
         tracing::warn!("Failed to resolve NFT icon dir: {e}");
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

      let Ok(chain_id) = chain_entry.file_name().to_string_lossy().parse::<u64>() else {
         continue;
      };

      let Ok(collection_entries) = std::fs::read_dir(chain_entry.path()) else {
         continue;
      };

      for collection_entry in collection_entries.flatten() {
         if !collection_entry.path().is_dir() {
            continue;
         }

         let name = collection_entry.file_name().to_string_lossy().to_string();
         let Ok(collection) = Address::from_str(&name) else {
            continue;
         };

         let Ok(token_entries) = std::fs::read_dir(collection_entry.path()) else {
            continue;
         };

         for token_entry in token_entries.flatten() {
            if !token_entry.path().is_dir() {
               continue;
            }

            let Ok(token_id) = token_entry.file_name().to_string_lossy().parse::<U256>() else {
               continue;
            };

            let read = |name: &str| std::fs::read(token_entry.path().join(name)).ok();
            let data = NftIconData {
               x64: read(NFT_ICON_X64).unwrap_or_default(),
               x250: read(NFT_ICON_X250).unwrap_or_default(),
            };

            if data.is_empty() {
               continue;
            }

            map.insert((collection, chain_id, token_id), data);
         }
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Loaded {} downloaded NFT images from disk",
      map.len()
   );

   map
}
