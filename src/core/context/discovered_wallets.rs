use crate::core::{
   context::{ZeusCtx, data_dir},
   serde_hashmap,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeus_bip32::{BIP32_HARDEN, DerivationPath};
use zeus_eth::alloy_primitives::{Address, B256, U256};
use zeus_wallet::SecureHDWallet;

pub const DISCOVERED_WALLETS_FILE: &str = "discovered_wallets.json";

/// HMAC-SHA256 digest of an Ethereum address (privacy-preserving on-disk identifier)
pub type HashedAddress = B256;

#[derive(Debug, Clone)]
pub struct DiscoveredWallet {
   pub address: Address,
   pub path: DerivationPath,
   pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashedDiscoveredWallet {
   pub path: DerivationPath,
   pub index: u32,
}

fn default_concurrency() -> usize {
   2
}

fn default_batch_size() -> usize {
   20
}

/// In-memory discovered wallets derived from a `SecureHDWallet`.
///
/// Contains plaintext addresses and is never written to disk as-is.
#[derive(Debug, Clone)]
pub struct DiscoveredWallets {
   pub balances: HashMap<(u64, Address), U256>,
   pub master_wallet_address: Option<Address>,
   pub wallets: Vec<DiscoveredWallet>,
   /// Current index, starting from [BIP32_HARDEN]
   pub index: u32,
   /// Number of concurrent requests
   pub concurrency: usize,
   /// Batch size
   pub batch_size: usize,
}

/// On-disk form of discovered wallets. Addresses are HMAC-SHA256 hashed
/// (keyed by a username-derived seed via [ZeusCtx::hash_addresses]) so the file
/// does not leak the master or child wallet addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashedDiscoveredWallets {
   #[serde(with = "serde_hashmap")]
   pub balances: HashMap<(u64, HashedAddress), U256>,
   pub master_wallet_address: Option<HashedAddress>,
   pub wallets: Vec<HashedDiscoveredWallet>,
   /// Current index, starting from [BIP32_HARDEN]
   pub index: u32,
   /// Number of concurrent requests
   #[serde(default = "default_concurrency")]
   pub concurrency: usize,
   /// Batch size
   #[serde(default = "default_batch_size")]
   pub batch_size: usize,
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

   /// Load hashed discovered wallets from disk, verify the master address hash,
   /// and re-derive child addresses into an in-memory [DiscoveredWallets].
   pub fn load_from_file(
      ctx: ZeusCtx,
   ) -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(DISCOVERED_WALLETS_FILE);
      let data = std::fs::read(dir)?;
      let hashed: HashedDiscoveredWallets = serde_json::from_slice(&data)?;
      Ok(Self::from_hashed(
         ctx,
         hashed,
      ))
   }

   /// Persist a hashed, privacy-preserving copy to disk.
   pub fn save(&self, ctx: ZeusCtx) -> Result<(), anyhow::Error> {
      let hashed = self.to_hashed(ctx);
      let db = serde_json::to_string(&hashed)?;
      let dir = data_dir()?.join(DISCOVERED_WALLETS_FILE);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn to_hashed(&self, ctx: ZeusCtx) -> HashedDiscoveredWallets {
      // Batch-hash master + every balance owner in one credential unlock
      let mut to_hash = Vec::with_capacity(self.balances.len() + 1);
      let has_master = self.master_wallet_address.is_some();
      if let Some(addr) = self.master_wallet_address {
         to_hash.push(addr);
      }

      let balance_start = to_hash.len();
      let balance_entries: Vec<(u64, U256)> = self
         .balances
         .iter()
         .map(|((chain, address), balance)| {
            to_hash.push(*address);
            (*chain, *balance)
         })
         .collect();

      let hashes = ctx.hash_addresses(to_hash);

      let master_wallet_address = if has_master { Some(hashes[0]) } else { None };

      let balances = balance_entries
         .into_iter()
         .enumerate()
         .map(|(i, (chain, balance))| ((chain, hashes[balance_start + i]), balance))
         .collect();

      let wallets = self
         .wallets
         .iter()
         .map(|w| HashedDiscoveredWallet {
            path: w.path.clone(),
            index: w.index,
         })
         .collect();

      HashedDiscoveredWallets {
         balances,
         master_wallet_address,
         wallets,
         index: self.index,
         concurrency: self.concurrency,
         batch_size: self.batch_size,
      }
   }

   /// Build in-memory wallets from the hashed on-disk form.
   ///
   /// - Verifies the hashed master wallet address matches `master_address`
   /// - Rebuilds child addresses via [Self::rediscover_wallets]
   /// - Remaps balances from hashed address keys back to real addresses
   ///
   /// On verification / corruption failure returns a fresh [DiscoveredWallets]
   /// bound to `master_address`.
   pub fn from_hashed(
      ctx: ZeusCtx,
      hashed: HashedDiscoveredWallets,
   ) -> Self {
      let hd_wallet = ctx.read(|ctx| ctx.vault.get_hd_wallet());
      let master_address = hd_wallet.master_wallet.address();
      let master_hash = ctx.hash_addresses(vec![master_address])[0];

      if let Some(stored_master) = hashed.master_wallet_address {
         if stored_master != master_hash {
            tracing::warn!("Discovered wallets master address hash mismatch, resetting");
            let mut discovered = Self::new();
            discovered.master_wallet_address = Some(master_address);
            return discovered;
         }
      }

      let mut discovered = Self {
         balances: HashMap::new(),
         master_wallet_address: Some(master_address),
         wallets: hashed
            .wallets
            .iter()
            .map(|w| DiscoveredWallet {
               // Placeholder — overwritten by rediscover_wallets
               address: Address::ZERO,
               path: w.path.clone(),
               index: w.index,
            })
            .collect(),
         index: hashed.index,
         concurrency: hashed.concurrency,
         batch_size: hashed.batch_size,
      };

      if discovered.is_corrupted() {
         tracing::warn!("Discovered wallets index is corrupted, resetting");
         let mut fresh = Self::new();
         fresh.master_wallet_address = Some(master_address);
         return fresh;
      }

      discovered.rediscover_wallets(hd_wallet);

      // Remap balances: HMAC'd address → real derived address
      let wallet_addrs: Vec<Address> = discovered.wallets.iter().map(|w| w.address).collect();
      let hashed_addrs = ctx.hash_addresses(wallet_addrs.clone());

      let hash_to_address: HashMap<HashedAddress, Address> =
         hashed_addrs.into_iter().zip(wallet_addrs).collect();

      for ((chain, hashed_addr), balance) in hashed.balances {
         if let Some(address) = hash_to_address.get(&hashed_addr) {
            discovered.balances.insert((chain, *address), balance);
         }
      }

      discovered
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
   /// This is needed to make sure even if the json file is corrupted somehow
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
