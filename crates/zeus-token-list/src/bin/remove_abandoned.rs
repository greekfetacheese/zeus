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

      let mut active = 0;
      let mut removed = 0;

      for entry in std::fs::read_dir(&asset_dir)? {
         let entry = entry?;
         let path = entry.path();

         if path.is_dir() {
            let info_path = path.join("info.json");
            if info_path.exists() && info_path.is_file() {
               let data = std::fs::read_to_string(&info_path)?;
               match serde_json::from_str::<TokenInfo>(&data) {
                  Ok(info) => {
                     if info.status == "abandoned" {
                        removed += 1;
                        std::fs::remove_dir_all(path)?;
                     } else {
                        active += 1;
                     }
                  }
                  Err(e) => eprintln!("Failed to parse {}: {}", info_path.display(), e),
               }
            } else {
               eprintln!("No info.json found in {:?}", path);
            }
         }
      }
      eprintln!("ChainId: {chain}, Active: {active}, Removed: {removed}");
   }

   Ok(())
}
