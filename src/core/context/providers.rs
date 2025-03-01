use crate::core::utils::data_dir;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

const PROVIDERS_FILE: &str = "providers.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcProviders {
   pub rpc: Vec<Rpc>,
}

impl RpcProviders {
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(PROVIDERS_FILE);
      let data = std::fs::read(dir)?;
      let providers = serde_json::from_slice(&data)?;
      Ok(providers)
   }

   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let providers = serde_json::to_vec(&self)?;
      let dir = data_dir()?.join(PROVIDERS_FILE);
      std::fs::write(dir, providers)?;
      Ok(())
   }

   // ! Need to change this, its very bad
   /// Get an rpc provider based on the chain_id
   pub fn get(&self, chain_id: u64) -> Result<Rpc, anyhow::Error> {
      let rpc_opt = self.rpc.iter().find(|rpc| rpc.chain_id == chain_id);
      if let Some(rpc) = rpc_opt {
         Ok(rpc.clone())
      } else {
         Err(anyhow!("Rpc for chain id {} not found", chain_id))
      }
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rpc {
   pub url: String,

   pub chain_id: u64,

   /// False if the rpc is added by the user from the GUI
   pub default: bool,
}

impl Rpc {
   pub fn new(url: impl Into<String>, chain_id: u64, default: bool) -> Self {
      Self {
         url: url.into(),
         chain_id,
         default,
      }
   }
}

impl Default for RpcProviders {
   fn default() -> Self {
      let eth = Rpc::new("https://eth.merkle.io", 1, true);
      let base = Rpc::new("https://base.llamarpc.com", 8453, true);
      let op = Rpc::new("https://optimism.llamarpc.com", 10, true);
      let arbitrum = Rpc::new("https://arbitrum.llamarpc.com", 42161, true);
      let bsc = Rpc::new("https://bsc.drpc.org", 56, true);

      RpcProviders {
         rpc: vec![eth, base, op, arbitrum, bsc],
      }
   }
}
