use crate::core::persisted::{
   NFT_ICON_SOURCE, NFT_ICON_X64, NFT_ICON_X250, NFT_IMAGE_SVG, PersistedTree, TOKEN_ICON_X32,
   tree_dir,
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

/// Write an NFT's art: two raster renderings. Empty renderings are skipped rather than written
/// blank.
///
/// A vector file left by an earlier version is removed: nothing reads it any more (art is rasterised
/// at ingest), so leaving it would be dead weight beside the renderings that replaced it.
pub fn save_nft_icon(
   chain_id: u64,
   collection: Address,
   token_id: U256,
   data: &NftIconData,
) -> Result<(), anyhow::Error> {
   let dir = nft_icon_dir(chain_id, collection, token_id)?;
   std::fs::create_dir_all(&dir)?;
   remove_file_if_present(&dir, NFT_IMAGE_SVG);

   if !data.x64.is_empty() {
      std::fs::write(dir.join(NFT_ICON_X64), &data.x64)?;
   }
   if !data.x250.is_empty() {
      std::fs::write(dir.join(NFT_ICON_X250), &data.x250)?;
   }
   if let Some(uri) = &data.source_uri {
      std::fs::write(dir.join(NFT_ICON_SOURCE), uri)?;
   }

   // A write is when the cache grows, so it is also when the budget is worth checking. A failure here
   // still leaves the write in place: pruning is housekeeping, not a condition of the fetch.
   if let Err(e) = prune_nft_icons() {
      tracing::warn!("Failed to prune the NFT icon cache: {e}");
   }

   Ok(())
}

/// Keep the whole NFT icon cache inside [`MAX_DISK_BYTES`].
pub fn prune_nft_icons() -> Result<usize, anyhow::Error> {
   prune_nft_icons_to_quota(&nft_icons_dir()?, MAX_DISK_BYTES)
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

/// The cached art for one token: the renderings and the URI they were read from.
///
/// `None` when the directory does not exist, holds nothing, or cannot be read — the three cases mean
/// the same thing to a caller: ask the chain.
pub fn load_nft_icon(chain_id: u64, collection: Address, token_id: U256) -> Option<NftIconData> {
   let dir = nft_icon_dir(chain_id, collection, token_id).ok()?;
   read_nft_icon_dir(&dir)
}

fn read_nft_icon_dir(dir: &std::path::Path) -> Option<NftIconData> {
   let read = |name: &str| std::fs::read(dir.join(name)).ok();

   let data = NftIconData {
      x64: read(NFT_ICON_X64).unwrap_or_default(),
      x250: read(NFT_ICON_X250).unwrap_or_default(),
      source_uri: read(NFT_ICON_SOURCE)
         .and_then(|bytes| String::from_utf8(bytes).ok())
         .map(|uri| uri.trim().to_string()),
   };

   (!data.is_empty()).then_some(data)
}

/// Record the URI a token's cached art was read from, without touching the renderings.
///
/// This is how art cached by a version that stored no URI gets its baseline: the bytes cannot be checked
/// retroactively, so the URI the contract reports now becomes the one a later change is compared against.
pub fn save_nft_icon_source(
   chain_id: u64,
   collection: Address,
   token_id: U256,
   uri: &str,
) -> Result<(), anyhow::Error> {
   let dir = nft_icon_dir(chain_id, collection, token_id)?;
   std::fs::create_dir_all(&dir)?;
   std::fs::write(dir.join(NFT_ICON_SOURCE), uri)?;
   Ok(())
}

/// The byte budget for `data/nft_icons/`.
///
/// The disk copy is the durable one — dropping it costs a re-download, where dropping the in-memory
/// copy costs a re-read — so this is loose enough to hold a large wallet's art and tight enough that a
/// cache never becomes "some fraction of the user's disk".
pub const MAX_DISK_BYTES: u64 = 256 * 1024 * 1024;

/// Keep `root` under `quota` bytes, deleting the least recently written token directories first.
///
/// Returns how many directories were removed. The newest art is the art most likely to be looked at
/// again, which is the whole reason to keep a cache. Takes the root as an argument so it can be tested
/// without a process working directory.
pub fn prune_nft_icons_to_quota(
   root: &std::path::Path,
   quota: u64,
) -> Result<usize, anyhow::Error> {
   let mut dirs: Vec<(std::path::PathBuf, u64, std::time::SystemTime)> = Vec::new();
   let mut total = 0u64;

   collect_token_dirs(root, &mut dirs, &mut total)?;

   if total <= quota {
      return Ok(0);
   }

   // Oldest first: an unwritable mtime sorts as the beginning of time, so it goes first.
   dirs.sort_by_key(|(_, _, written)| *written);

   let mut removed = 0;
   for (dir, size, _) in dirs {
      if total <= quota {
         break;
      }
      match std::fs::remove_dir_all(&dir) {
         Ok(()) => {
            total = total.saturating_sub(size);
            removed += 1;
         }
         Err(e) => tracing::warn!("Failed to prune {}: {e}", dir.display()),
      }
   }

   Ok(removed)
}

fn collect_token_dirs(
   root: &std::path::Path,
   out: &mut Vec<(std::path::PathBuf, u64, std::time::SystemTime)>,
   total: &mut u64,
) -> Result<(), anyhow::Error> {
   let chains = match std::fs::read_dir(root) {
      Ok(entries) => entries,
      Err(_) => return Ok(()),
   };

   for chain in chains.flatten().filter(|e| e.path().is_dir()) {
      let Ok(collections) = std::fs::read_dir(chain.path()) else {
         continue;
      };

      for collection in collections.flatten().filter(|e| e.path().is_dir()) {
         let Ok(tokens) = std::fs::read_dir(collection.path()) else {
            continue;
         };

         for token in tokens.flatten().filter(|e| e.path().is_dir()) {
            let dir = token.path();
            let (bytes, written) = dir_size_and_age(&dir);
            *total += bytes;
            out.push((dir, bytes, written));
         }
      }
   }

   Ok(())
}

/// `(bytes, most recent write)` for a token directory, both best-effort.
fn dir_size_and_age(dir: &std::path::Path) -> (u64, std::time::SystemTime) {
   let mut bytes = 0;
   let mut newest = std::time::SystemTime::UNIX_EPOCH;

   let Ok(entries) = std::fs::read_dir(dir) else {
      return (0, newest);
   };

   for entry in entries.flatten() {
      let Ok(meta) = entry.metadata() else {
         continue;
      };
      bytes += meta.len();
      if let Ok(written) = meta.modified() {
         newest = newest.max(written);
      }
   }

   (bytes, newest)
}

/// Load previously downloaded NFT images from `data/nft_icons/`.
///
/// A token directory is kept only if it holds a non-empty rendering, so a half-written directory
/// does not surface as an icon with a blank picture.
///
/// At most `limit` of them are read back, newest by write time. The in-memory copy is a bounded hot
/// cache — reading a whole archive into memory at startup is the very thing the bound exists to stop —
/// and anything past the limit stays on disk, to be read on demand when a view asks for it.
pub fn load_downloaded_nft_icons(limit: usize) -> HashMap<NftKey, NftIconData> {
   let mut found: Vec<(std::time::SystemTime, NftKey, NftIconData)> = Vec::new();

   let root = match nft_icons_dir() {
      Ok(dir) => dir,
      Err(e) => {
         tracing::warn!("Failed to resolve NFT icon dir: {e}");
         return HashMap::new();
      }
   };

   let Ok(chain_entries) = std::fs::read_dir(&root) else {
      return HashMap::new();
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

            // Only the renderings are read. A vector file left by an earlier version is ignored
            // rather than parsed, and the token then counts as having no art — so the fetch path
            // asks for it again and stores renderings this time, with no cache-format bump needed.
            let Some(data) = read_nft_icon_dir(&token_entry.path()) else {
               continue;
            };

            let written = dir_size_and_age(&token_entry.path()).1;
            found.push((written, (collection, chain_id, token_id), data));
         }
      }
   }

   found.sort_by_key(|(written, _, _)| *written);

   let total = found.len();
   // Keep the newest `limit`, still oldest-first: that is the order the cache evicts from.
   let excess = total.saturating_sub(limit);
   found.drain(..excess);

   #[cfg(feature = "dev")]
   tracing::info!(
      "Loaded {} of {total} downloaded NFT images from disk",
      found.len()
   );

   found.into_iter().map(|(_, key, data)| (key, data)).collect()
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::persisted::{NFT_ICON_SOURCE, NFT_ICON_X64, NFT_ICON_X250, NFT_IMAGE_SVG};

   fn raster(x64: Vec<u8>, x250: Vec<u8>) -> NftIconData {
      NftIconData {
         x64,
         x250,
         source_uri: None,
      }
   }

   /// The disk cache is kept under its quota, dropping the least recently written art first.
   ///
   /// The newest art is the art most likely to be looked at again, which is the whole point of keeping a
   /// cache at all; a directory whose mtime cannot be read sorts oldest and goes first.
   #[test]
   fn the_disk_cache_is_pruned_to_its_quota() {
      let root = std::env::temp_dir().join(format!("zeus_nft_prune_{}", std::process::id()));
      let _ = std::fs::remove_dir_all(&root);

      let write = |token: &str, bytes: usize| {
         let dir = root.join("1").join("0xabc").join(token);
         std::fs::create_dir_all(&dir).unwrap();
         std::fs::write(dir.join(NFT_ICON_X64), vec![7u8; bytes]).unwrap();
         // Distinct write times, so "oldest first" is decided by the clock rather than by tie-breaking.
         std::thread::sleep(std::time::Duration::from_millis(10));
         dir
      };

      let oldest = write("1", 100);
      let middle = write("2", 100);
      let newest = write("3", 100);

      // 300 bytes cached, a 150-byte quota: the two oldest have to go, and only those.
      let pruned = prune_nft_icons_to_quota(&root, 150).expect("prune");

      assert_eq!(pruned, 2, "two directories had to go");
      assert!(
         !oldest.exists() && !middle.exists(),
         "the oldest art is what went"
      );
      assert!(newest.exists(), "and the newest survives");

      // Nothing to do when the cache is already inside its budget.
      assert_eq!(
         prune_nft_icons_to_quota(&root, 150).expect("prune"),
         0
      );

      let _ = std::fs::remove_dir_all(&root);
   }

   /// Round-trips the renderings through the real filesystem, including the cleanup of a vector file
   /// left by an earlier version and the fact that such a file is no longer art.
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
            load_downloaded_nft_icons(64).is_empty(),
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
            load_downloaded_nft_icons(64).get(&key),
            Some(&raster(vec![1, 2], vec![3])),
            "both renderings come back"
         );

         // A vector file written by an earlier version must change nothing: it is not read, and the
         // next save clears it so a directory cannot hold a stale source beside its renderings.
         std::fs::write(dir.join(NFT_IMAGE_SVG), b"<svg/>").unwrap();
         assert_eq!(
            load_downloaded_nft_icons(64).get(&key),
            Some(&raster(vec![1, 2], vec![3])),
            "a legacy vector file is ignored"
         );

         save_nft_icon(1, collection, token, &raster(vec![9], vec![8])).unwrap();
         assert!(
            !dir.join(NFT_IMAGE_SVG).exists(),
            "stale vector file removed"
         );
         assert_eq!(
            load_downloaded_nft_icons(64).get(&key),
            Some(&raster(vec![9], vec![8]))
         );

         // A directory holding only a legacy vector file has no art to show any more — so the fetch
         // path asks for the token again rather than serving a source nothing will parse.
         let legacy_only = U256::from(8);
         let legacy_dir = nft_icon_dir(1, collection, legacy_only).unwrap();
         std::fs::create_dir_all(&legacy_dir).unwrap();
         std::fs::write(legacy_dir.join(NFT_IMAGE_SVG), b"<svg/>").unwrap();
         assert!(
            load_downloaded_nft_icons(64).get(&(collection, 1, legacy_only)).is_none(),
            "a vector-only directory is not art"
         );

         // The URI the art was read from is kept beside it: that is what lets a later change to the
         // metadata URI be noticed at all, and it must survive the round trip with the renderings.
         let with_uri = NftIconData {
            x64: vec![1, 2],
            x250: vec![3],
            source_uri: Some("ipfs://cid/7.json".to_string()),
         };
         save_nft_icon(1, collection, token, &with_uri).unwrap();
         assert!(
            nft_icon_dir(1, collection, token).unwrap().join(NFT_ICON_SOURCE).exists(),
            "the source URI is written beside the renderings"
         );
         assert_eq!(
            load_downloaded_nft_icons(64).get(&key),
            Some(&with_uri),
            "and read back with them"
         );

         delete_nft_icon(1, collection, token).unwrap();
         assert!(load_downloaded_nft_icons(64).get(&key).is_none());
      });

      std::env::set_current_dir(previous).unwrap();
      let _ = std::fs::remove_dir_all(&scratch);

      if let Err(panic) = outcome {
         std::panic::resume_unwind(panic);
      }
   }
}
