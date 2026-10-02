//! The NFT catalog: which NFTs the user tracks, plus cached collection metadata.
//!
//! Split from [`crate::core::context::WalletPortfolio`] on purpose. This store is the analogue of
//! [`crate::core::context::CurrencyDB`] — the set of assets the *app* knows about, chain-scoped and
//! shared by every wallet. What a *particular* wallet holds lives in its portfolio, keyed by
//! `(chain, owner)`. So ownership is deliberately **not** stored here: an ERC-721 id has exactly
//! one owner at a time, and a stored owner would go stale the moment the token is transferred.
//! Ask the chain instead (`zeus_eth::nft::verify_ownership`).
//!
//! Like `CurrencyDB` this is its own sealed file with its own AAD rather than a field inside
//! `wallet_state.data`: it is a cache-sized store that grows with what the user tracks, and
//! `wallet_state.data` is re-encrypted and rewritten on unrelated saves (contacts, balances,
//! approvals).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::persisted::{PersistedFile, file_path};
use crate::core::{WalletStateKey, serde_hashmap};
use crate::utils::write_private_atomic;

use zeus_eth::{
   alloy_primitives::{Address, U256},
   nft::{NftCollection, NftToken},
};

/// Bound ciphertext to this logical slot (AAD).
const NFT_DB_AAD: &[u8] = b"zeus-nft-db-v1";

/// Collection metadata per chain.
type CollectionMap = HashMap<Address, NftCollection>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NftDB {
   /// Cached collection metadata (`name` / `symbol` / `standard`) per chain, so a row can be
   /// labelled without an RPC round trip.
   #[serde(default, with = "serde_hashmap")]
   pub collections: HashMap<u64, CollectionMap>,

   /// Tracked NFTs per chain.
   ///
   /// A flat list, not a map keyed by collection: every [`NftToken`] already carries its own chain
   /// id and collection address, so grouping them would repeat those two fields on every entry.
   /// Dedupe is by token identity (`PartialEq`/`Hash` on the token itself).
   #[serde(default, with = "serde_hashmap")]
   pub nfts: HashMap<u64, Vec<NftToken>>,
}

impl NftDB {
   pub fn new() -> Self {
      Self::default()
   }

   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let dir = Self::dir()?;
      let sealed = std::fs::read(&dir)?;
      let db: NftDB = key.open_json(&sealed, NFT_DB_AAD)?;
      Ok(db)
   }

   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = key.seal_json(self, NFT_DB_AAD)?;
      let dir = Self::dir()?;
      write_private_atomic(&dir, &sealed)?;
      Ok(())
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::NftDb)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   /// Cache (or refresh) a collection's metadata.
   pub fn insert_collection(&mut self, chain_id: u64, collection: NftCollection) {
      self
         .collections
         .entry(chain_id)
         .or_default()
         .insert(collection.address, collection);
   }

   pub fn get_collection(&self, chain_id: u64, address: Address) -> Option<NftCollection> {
      self.collections.get(&chain_id)?.get(&address).cloned()
   }

   /// Every cached collection on a chain, address-ordered so the UI list is stable.
   pub fn get_collections(&self, chain_id: u64) -> Vec<NftCollection> {
      let Some(collections) = self.collections.get(&chain_id) else {
         return Vec::new();
      };
      let mut collections: Vec<NftCollection> = collections.values().cloned().collect();
      collections.sort_by_key(|collection| collection.address);
      collections
   }

   /// Start tracking `token`. Tracking the same token twice is a no-op, and the existing entry wins
   /// so a newer `metadata_uri` cannot regress until the caller removes it.
   pub fn insert_nft(&mut self, token: NftToken) {
      let nfts = self.nfts.entry(token.chain_id).or_default();
      if !nfts.contains(&token) {
         nfts.push(token);
      }
   }

   /// One tracked token. Distinct from [`Self::contains_nft`]: the collection can be cached while the
   /// individual token is not.
   pub fn get_nft(&self, chain_id: u64, collection: Address, token_id: U256) -> Option<NftToken> {
      self
         .get_nfts_of_collection(chain_id, collection)
         .into_iter()
         .find(|token| token.token_id == token_id)
   }

   pub fn contains_nft(&self, chain_id: u64, collection: Address, token_id: U256) -> bool {
      self.get_nft(chain_id, collection, token_id).is_some()
   }

   /// Tracked NFTs on `chain_id`, in insertion order.
   pub fn get_nfts(&self, chain_id: u64) -> Vec<NftToken> {
      self.nfts.get(&chain_id).cloned().unwrap_or_default()
   }

   /// Tracked NFTs of one collection on `chain_id`.
   pub fn get_nfts_of_collection(&self, chain_id: u64, collection: Address) -> Vec<NftToken> {
      self
         .get_nfts(chain_id)
         .into_iter()
         .filter(|token| token.collection == collection)
         .collect()
   }

   /// Stop tracking one token. Returns whether it was there.
   pub fn remove_nft(&mut self, chain_id: u64, collection: Address, token_id: U256) -> bool {
      let Some(nfts) = self.nfts.get_mut(&chain_id) else {
         return false;
      };
      let before = nfts.len();
      nfts.retain(|token| !(token.collection == collection && token.token_id == token_id));
      let removed = nfts.len() != before;
      if nfts.is_empty() {
         self.nfts.remove(&chain_id);
      }
      removed
   }

   /// Forget a collection: its metadata and every token tracked from it.
   pub fn remove_collection(&mut self, chain_id: u64, address: Address) {
      if let Some(nfts) = self.nfts.get_mut(&chain_id) {
         nfts.retain(|token| token.collection != address);
         if nfts.is_empty() {
            self.nfts.remove(&chain_id);
         }
      }

      if let Some(collections) = self.collections.get_mut(&chain_id) {
         collections.remove(&address);
         if collections.is_empty() {
            self.collections.remove(&chain_id);
         }
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::nft::NftStandard;

   fn token(collection: Address, token_id: u64) -> NftToken {
      NftToken {
         chain_id: 1,
         collection,
         token_id: U256::from(token_id),
         standard: NftStandard::Erc721,
         metadata_uri: None,
      }
   }

   fn collection(address: Address, symbol: &str) -> NftCollection {
      NftCollection {
         chain_id: 1,
         address,
         standard: NftStandard::Erc721,
         name: Some("BoredApeYachtClub".to_string()),
         symbol: Some(symbol.to_string()),
      }
   }

   #[test]
   fn seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let bayc = Address::from([0xbc; 20]);

      let mut db = NftDB::new();
      db.insert_collection(1, collection(bayc, "BAYC"));
      db.insert_nft(token(bayc, 1));
      db.insert_nft(token(bayc, 2));

      let sealed = key.seal_json(&db, NFT_DB_AAD).unwrap();
      let loaded: NftDB = key.open_json(&sealed, NFT_DB_AAD).unwrap();

      assert_eq!(
         loaded.get_collection(1, bayc).unwrap().symbol.as_deref(),
         Some("BAYC")
      );
      assert_eq!(loaded.get_nfts(1).len(), 2);
      assert!(loaded.contains_nft(1, bayc, U256::from(2)));

      // A sibling store sealed with a different AAD must not open this blob.
      assert!(key.open_json::<NftDB>(&sealed, b"zeus-currency-db-v1").is_err());
   }

   /// Tracking the same token twice must not double it in the picker.
   #[test]
   fn inserting_the_same_token_is_a_no_op() {
      let bayc = Address::from([0xbc; 20]);
      let mut db = NftDB::new();

      db.insert_nft(token(bayc, 1));
      db.insert_nft(token(bayc, 1));

      assert_eq!(db.get_nfts(1).len(), 1);
   }

   #[test]
   fn collections_and_tokens_are_scoped_per_chain_and_collection() {
      let bayc = Address::from([0xbc; 20]);
      let azuki = Address::from([0xed; 20]);
      let mut db = NftDB::new();

      db.insert_nft(token(bayc, 1));
      db.insert_nft(token(bayc, 2));
      db.insert_nft(token(azuki, 9));

      assert_eq!(db.get_nfts(1).len(), 3);
      assert_eq!(
         db.get_nfts(10).len(),
         0,
         "other chains stay empty"
      );
      assert_eq!(db.get_nfts_of_collection(1, bayc).len(), 2);
      assert!(db.contains_nft(1, azuki, U256::from(9)));
      assert!(!db.contains_nft(1, azuki, U256::from(1)));
   }

   /// Deleting a collection drops its tokens and its metadata, and leaves its neighbours alone.
   #[test]
   fn removing_a_collection_takes_its_tokens_with_it() {
      let bayc = Address::from([0xbc; 20]);
      let azuki = Address::from([0xed; 20]);
      let mut db = NftDB::new();

      db.insert_collection(1, collection(bayc, "BAYC"));
      db.insert_collection(1, collection(azuki, "AZUKI"));
      db.insert_nft(token(bayc, 1));
      db.insert_nft(token(azuki, 9));

      db.remove_collection(1, bayc);

      assert!(db.get_collection(1, bayc).is_none());
      assert!(db.get_nfts_of_collection(1, bayc).is_empty());
      assert_eq!(
         db.get_nfts(1).len(),
         1,
         "the other collection survives"
      );
      assert_eq!(db.get_collections(1).len(), 1);
   }

   #[test]
   fn removing_one_token_leaves_the_rest() {
      let bayc = Address::from([0xbc; 20]);
      let mut db = NftDB::new();

      db.insert_nft(token(bayc, 1));
      db.insert_nft(token(bayc, 2));

      assert!(db.remove_nft(1, bayc, U256::from(1)));
      assert!(
         !db.remove_nft(1, bayc, U256::from(1)),
         "already gone"
      );
      assert_eq!(db.get_nfts(1), vec![token(bayc, 2)]);
   }
}
