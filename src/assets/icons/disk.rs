use crate::core::persisted::{
   NFT_ICON_X64, NFT_ICON_X250, NFT_IMAGE_SVG, PersistedTree, TOKEN_ICON_X32, tree_dir,
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

/// Write an NFT's art: two raster renderings, or the vector source. Empty renderings are skipped
/// rather than written blank, and the other form is removed first so a token directory never holds
/// both (a stale pair would otherwise shadow newly stored vector art).
pub fn save_nft_icon(
   chain_id: u64,
   collection: Address,
   token_id: U256,
   data: &NftIconData,
) -> Result<(), anyhow::Error> {
   let dir = nft_icon_dir(chain_id, collection, token_id)?;
   std::fs::create_dir_all(&dir)?;

   match data {
      NftIconData::Raster { x64, x250 } => {
         remove_file_if_present(&dir, NFT_IMAGE_SVG);

         if !x64.is_empty() {
            std::fs::write(dir.join(NFT_ICON_X64), x64)?;
         }
         if !x250.is_empty() {
            std::fs::write(dir.join(NFT_ICON_X250), x250)?;
         }
      }
      NftIconData::Svg(svg) => {
         remove_file_if_present(&dir, NFT_ICON_X64);
         remove_file_if_present(&dir, NFT_ICON_X250);

         if !svg.is_empty() {
            std::fs::write(dir.join(NFT_IMAGE_SVG), svg)?;
         }
      }
   }

   Ok(())
}

fn remove_file_if_present(dir: &std::path::Path, name: &str) {
   let path = dir.join(name);
   if path.exists() {
      if let Err(e) = std::fs::remove_file(path) {
         tracing::warn!("Failed to remove {name}: {e}");
      }
   }
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
/// A token directory is kept only if it holds something non-empty — a rendering or the vector
/// source — so a half-written directory does not surface as an icon with a blank picture.
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

            // Vector art wins when both are present, and a directory holding neither is skipped so a
            // half-written one does not surface as an icon with a blank picture.
            let data = match read(NFT_IMAGE_SVG).filter(|svg| !svg.is_empty()) {
               Some(svg) => NftIconData::Svg(svg),
               None => NftIconData::Raster {
                  x64: read(NFT_ICON_X64).unwrap_or_default(),
                  x250: read(NFT_ICON_X250).unwrap_or_default(),
               },
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

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::persisted::{NFT_ICON_X64, NFT_ICON_X250, NFT_IMAGE_SVG};

   fn raster(x64: Vec<u8>, x250: Vec<u8>) -> NftIconData {
      NftIconData::Raster { x64, x250 }
   }

   /// Round-trips both storage forms through the real filesystem, including the cleanup that stops a
   /// re-downloaded token from serving its previous art.
   ///
   /// Ignored by default: the persisted tree is resolved from the process working directory
   /// (`data/nft_icons/…`, see `persisted::data_dir`), so this moves the process into a scratch
   /// directory for the duration. **Run it on its own** — any test running concurrently would
   /// follow it there.
   #[test]
   #[ignore = "moves the process working directory; run alone"]
   fn nft_icons_round_trip_through_the_disk() {
      let previous = std::env::current_dir().unwrap();
      let scratch = std::env::temp_dir().join(format!("zeus_nft_icons_{}", std::process::id()));
      let _ = std::fs::remove_dir_all(&scratch);
      std::fs::create_dir_all(&scratch).unwrap();
      std::env::set_current_dir(&scratch).unwrap();

      // Everything below runs inside `scratch`. The outcome is caught so the working directory is
      // restored even when an assertion fires, rather than leaving the rest of the run pointing
      // into a temp path that is about to be deleted.
      let outcome = std::panic::catch_unwind(|| {
         let collection = Address::from([0xbc; 20]);
         let token = U256::from(7);
         let key = (collection, 1, token);

         assert!(
            load_downloaded_nft_icons().is_empty(),
            "nothing saved yet"
         );

         save_nft_icon(1, collection, token, &raster(vec![1, 2], vec![3])).unwrap();
         let dir = nft_icon_dir(1, collection, token).unwrap();

         assert!(
            dir.join(NFT_ICON_X64).exists(),
            "thumbnail written"
         );
         assert!(
            dir.join(NFT_ICON_X250).exists(),
            "detail copy written"
         );
         assert_eq!(
            load_downloaded_nft_icons().get(&key),
            Some(&raster(vec![1, 2], vec![3])),
            "both renderings come back"
         );

         // The same token re-downloaded as vector art must not leave the renderings behind: the
         // loader prefers the SVG, so they would be invisible dead weight.
         let svg = b"<svg/>".to_vec();
         save_nft_icon(
            1,
            collection,
            token,
            &NftIconData::Svg(svg.clone()),
         )
         .unwrap();

         assert!(
            !dir.join(NFT_ICON_X64).exists(),
            "stale thumbnail removed"
         );
         assert!(
            !dir.join(NFT_ICON_X250).exists(),
            "stale detail copy removed"
         );
         assert!(
            dir.join(NFT_IMAGE_SVG).exists(),
            "vector art written"
         );
         assert_eq!(
            load_downloaded_nft_icons().get(&key),
            Some(&NftIconData::Svg(svg))
         );

         // ...and back again, so neither direction can leave the other form behind.
         save_nft_icon(1, collection, token, &raster(vec![9], vec![8])).unwrap();

         assert!(
            !dir.join(NFT_IMAGE_SVG).exists(),
            "stale vector art removed"
         );
         assert_eq!(
            load_downloaded_nft_icons().get(&key),
            Some(&raster(vec![9], vec![8]))
         );

         delete_nft_icon(1, collection, token).unwrap();
         assert!(load_downloaded_nft_icons().get(&key).is_none());
      });

      std::env::set_current_dir(previous).unwrap();
      let _ = std::fs::remove_dir_all(&scratch);

      if let Err(panic) = outcome {
         std::panic::resume_unwind(panic);
      }
   }
}
