use std::path::Path;

use tracing::{info, warn};
use zeus_eth::types::ChainId;

use crate::chains::{TokenInfo, assets_dir_for_chain};

/// Step 1: drop Trust Wallet entries whose `status` is `abandoned` or `spam`.
pub fn remove_garbage(work_dir: &Path, chains: &[ChainId]) -> anyhow::Result<()> {
   for chain in chains {
      let asset_dir = assets_dir_for_chain(work_dir, *chain)?;
      if !asset_dir.exists() {
         warn!("Assets directory does not exist: {:?}", asset_dir);
         continue;
      }

      let mut active = 0;
      let mut removed_abandoned = 0;
      let mut removed_spam = 0;

      for entry in std::fs::read_dir(&asset_dir)? {
         let entry = entry?;
         let path = entry.path();

         if !path.is_dir() {
            continue;
         }

         let info_path = path.join("info.json");
         if info_path.exists() && info_path.is_file() {
            let data = std::fs::read_to_string(&info_path)?;
            match serde_json::from_str::<TokenInfo>(&data) {
               Ok(info) => {
                  if info.status == "abandoned" {
                     removed_abandoned += 1;
                     std::fs::remove_dir_all(path)?;
                  } else if info.status == "spam" {
                     removed_spam += 1;
                     std::fs::remove_dir_all(path)?;
                  } else {
                     active += 1;
                  }
               }
               Err(e) => warn!("Failed to parse {}: {}", info_path.display(), e),
            }
         } else {
            warn!("No info.json found in {:?}", path);
         }
      }

      info!(
         "ChainId: {}, Active: {active}, Removed abandoned: {removed_abandoned}, Removed spam: {removed_spam}",
         chain.id()
      );
   }

   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use serde_json::json;
   use zeus_eth::types::ChainId;

   fn write_token(dir: &Path, address: &str, status: &str) {
      let token_dir = dir.join(address);
      std::fs::create_dir_all(&token_dir).unwrap();
      let info = json!({
         "name": "Test",
         "type": "ERC20",
         "symbol": "TST",
         "decimals": 18,
         "website": "https://example.com",
         "description": "test",
         "explorer": "https://example.com",
         "status": status,
         "id": address,
      });
      std::fs::write(token_dir.join("info.json"), info.to_string()).unwrap();
   }

   #[test]
   fn drops_abandoned_and_spam_keeps_active() {
      let tmp = tempfile::tempdir().unwrap();
      let assets = tmp.path().join("blockchains").join("ethereum").join("assets");
      std::fs::create_dir_all(&assets).unwrap();

      write_token(&assets, "0xactive", "active");
      write_token(&assets, "0xabandoned", "abandoned");
      write_token(&assets, "0xspam", "spam");

      remove_garbage(tmp.path(), &[ChainId::Ethereum]).unwrap();

      assert!(assets.join("0xactive").exists());
      assert!(!assets.join("0xabandoned").exists());
      assert!(!assets.join("0xspam").exists());
   }
}
