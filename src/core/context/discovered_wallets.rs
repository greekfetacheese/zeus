use crate::core::serde_hashmap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeus_bip32::{BIP32_HARDEN, DerivationPath};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_wallet::SecureHDWallet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredWallet {
   pub address: Address,
   pub path: DerivationPath,
   pub index: u32,
}

fn default_concurrency() -> usize {
   2
}

fn default_batch_size() -> usize {
   20
}

fn default_index() -> u32 {
   BIP32_HARDEN
}

/// Discovered HD child wallets and balances (persisted inside the encrypted vault).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredWallets {
   #[serde(default, with = "serde_hashmap")]
   pub balances: HashMap<(u64, Address), U256>,
   #[serde(default)]
   pub master_wallet_address: Option<Address>,
   #[serde(default)]
   pub wallets: Vec<DiscoveredWallet>,
   /// Current index, starting from [BIP32_HARDEN]
   #[serde(default = "default_index")]
   pub index: u32,
   /// Number of concurrent requests
   #[serde(default = "default_concurrency")]
   pub concurrency: usize,
   /// Batch size
   #[serde(default = "default_batch_size")]
   pub batch_size: usize,
}

impl Default for DiscoveredWallets {
   fn default() -> Self {
      Self::new()
   }
}

impl DiscoveredWallets {
   pub fn new() -> Self {
      Self {
         balances: HashMap::new(),
         master_wallet_address: None,
         wallets: Vec::new(),
         index: BIP32_HARDEN,
         concurrency: default_concurrency(),
         batch_size: default_batch_size(),
      }
   }

   /// Make sure that the current index is correct based on the wallets length
   pub fn is_corrupted(&self) -> bool {
      let start = BIP32_HARDEN;
      let wallets_len = self.wallets.len() as u32;
      let should_end = start + wallets_len;
      let current_index = self.index;

      if should_end == current_index {
         return false;
      }

      true
   }

   /// Rediscover the wallets from the master wallet
   ///
   /// This is needed to make sure even if persisted data is corrupted somehow
   /// we dont show any wrong wallets in the UI
   pub fn rediscover_wallets(&mut self, master: SecureHDWallet) {
      let len = self.wallets.len();

      let mut index = BIP32_HARDEN;

      for i in 0..len {
         if let Ok(wallet) = master.derive_child_at("".into(), index) {
            self.wallets[i].address = wallet.address();
            self.wallets[i].path = wallet.derivation_path();
            self.wallets[i].index = index;
            index += 1;
         }
      }
   }

   pub fn add_wallet(&mut self, address: Address, path: DerivationPath, index: u32) {
      self.wallets.push(DiscoveredWallet {
         address,
         path,
         index,
      });
   }
}
