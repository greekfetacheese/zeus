//! Railgun Scanner + Poseidon Merkle Tree state management.
//!
//! Responsibilities (aligned with Kohaku Railgun indexer patterns):
//! - Sync on-chain events (Shield, Transact, Nullified, etc.).
//! - Maintain local Poseidon Merkle tree of commitments (for ZK proofs).
//! - Track **unspent** owned notes (`owned_notes` vec = current unspent only).
//! - On Nullified events: remove notes from the unspent list (no separate "spent" filter at query time).
//! - Decrypt notes belonging to us (shield + private transact via Waku).
//! - Provide per-`TokenData` private balances and note selection for unshield.
//!
//! Primary (and only) source of unspent notes: the `owned_notes` vector.
//! Notes are removed from this list when their nullifier is seen in a Nullified event or via mark_nullified().
//!
//! Encrypted private transfer notes come via Waku (separate from this on-chain scanner).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use anyhow::{Result, anyhow};
use redb::{Database, ReadableDatabase, TableDefinition};
use zeus_eth::utils::{client::RpcClient, get_logs_for};
use zeus_railgun_shared::RailgunKeys;

use crate::contracts::deployment_block;
use crate::{
   contracts::{RailgunSmartWallet, railgun_address},
   merkle::PoseidonMerkleTree,
   note::{Note, TokenData, compute_nullifier, decrypt_note_v2},
};

/// A single decrypted note that we own, plus its position in the tree.
#[derive(Debug, Clone)]
pub struct OwnedNote {
   pub note: Note,
   pub leaf_index: u64,
   pub nullifier: U256,
   pub commitment: U256,
}

const SCANNER_OWNED_NOTES_TABLE: TableDefinition<&str, &[u8]> =
   TableDefinition::new("railgun_scanner_owned_notes");

const SCANNER_META_TABLE: TableDefinition<&str, &[u8]> =
   TableDefinition::new("railgun_scanner_meta");

/// Internal state (protected by mutex for thread safety).
///
/// Data model (Kohaku-aligned):
/// - `owned_notes`: the source of truth for **currently unspent** notes.
///   When we see a Nullified event (or call mark_nullified), we remove the note.
/// The `owned_notes` vec is the sole source of truth for currently spendable notes (Kohaku style).
///
/// Grouping by TokenData: not done yet. With typical private wallet note counts
/// (usually < 50-100), a linear scan in private_balances() is fine.
/// If we ever reach thousands of notes we can add `HashMap<TokenData, Vec<OwnedNote>>`.
struct RailgunScannerInner {
   /// Viewing private key used for decryption.
   viewing_private: [u8; 32],

   /// Nullifying key (Poseidon hash of viewing private).
   nullifying_key: U256,

   /// Spending private key (used to derive nullifiers for spends we make).
   #[allow(dead_code)]
   spending_private: [u8; 32], // kept for future spend / nullifier signing in builders

   /// Local Poseidon Merkle tree of all commitments.
   pub merkle_tree: PoseidonMerkleTree,

   /// Notes we have successfully decrypted and that belong to us.
   /// This vec contains ONLY unspent notes (notes are removed on nullification).
   pub owned_notes: Vec<OwnedNote>,

   /// Last block we have fully synced up to.
   pub last_synced_block: u64,

   /// Chain ID we are scanning on.
   pub chain_id: u64,

   /// Railgun contract address on this chain.
   pub railgun_address: Address,
}

/// The Railgun scanner maintains private state.
///
/// Thread-safe: can be cheaply cloned and used across threads.
#[derive(Clone)]
pub struct RailgunScanner {
   inner: Arc<Mutex<RailgunScannerInner>>,
}

impl RailgunScanner {
   /// Create a new scanner for a given set of Railgun keys.
   pub fn new(keys: &RailgunKeys, chain_id: u64) -> Result<Self> {
      let viewing_private = keys.viewing_private.unlock(|b| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(b);
         arr
      });

      // Compute nullifying key the same way the TS engine does (Poseidon of viewing priv)
      let nullifying_key = crate::note::compute_nullifying_key_from_viewing(&viewing_private)?;

      let spending_private = keys.spending_private.unlock(|b| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(b);
         arr
      });

      // Use known address if available; fall back to ZERO for unknown/test chains.
      // The broadcaster path will error later if a real contract address is required.
      let addr = railgun_address(chain_id).unwrap_or(Address::ZERO);

      let inner = RailgunScannerInner {
         viewing_private,
         nullifying_key,
         spending_private,
         merkle_tree: PoseidonMerkleTree::new()?,
         owned_notes: Vec::new(),
         last_synced_block: 0,
         chain_id,
         railgun_address: addr,
      };

      Ok(Self {
         inner: Arc::new(Mutex::new(inner)),
      })
   }

   // ===================== Public accessors (thread-safe) =====================

   pub fn chain_id(&self) -> u64 {
      self.read_inner(|inner| inner.chain_id)
   }

   pub fn railgun_address(&self) -> Address {
      self.read_inner(|inner| inner.railgun_address)
   }

   pub fn owned_notes_len(&self) -> usize {
      self.read_inner(|inner| inner.owned_notes.len())
   }

   pub fn last_synced_block(&self) -> u64 {
      let last_synced = self.read_inner(|inner| inner.last_synced_block);

      let block = if last_synced == 0 {
         deployment_block(self.chain_id()).unwrap_or(0)
      } else {
         last_synced
      };

      block
   }

   /// Expose a clone of the merkle tree (for advanced use / testing).
   pub fn merkle_tree(&self) -> PoseidonMerkleTree {
      self.read_inner(|inner| inner.merkle_tree.clone())
   }

   /// Current Merkle root (convenience for builders that need the root for txs).
   pub fn merkle_root(&self) -> alloy_primitives::U256 {
      self.read_inner(|inner| inner.merkle_tree.root())
   }

   // ===================== Merkle tree redb persistence (delegates to PoseidonMerkleTree) =====================

   /// Load the Poseidon Merkle tree from the given redb database.
   pub fn load_merkle_tree(&self, db: &Database, tree_id: &str) -> Result<()> {
      let tree = PoseidonMerkleTree::load(db, tree_id)?;
      {
         let mut inner = self.inner.lock().unwrap();
         inner.merkle_tree = tree;
      }
      Ok(())
   }

   /// Save the current Poseidon Merkle tree to the given redb database.
   pub fn save_merkle_tree(&self, db: &Database, tree_id: &str) -> Result<()> {
      let tree = {
         let inner = self.inner.lock().unwrap();
         inner.merkle_tree.clone()
      };
      tree.save(db, tree_id)
   }

   // ===================== Unified scanner state persistence in redb =====================

   /// Load full scanner state (nullifiers + owned notes + last_synced_block) from redb.
   /// Call this after load_merkle_tree when using a single redb file.
   pub fn load_state(&self, db: &Database, tree_id: &str) -> Result<()> {
      let notes = load_owned_notes_redb(db, tree_id)?;
      let last_opt = load_last_block_redb(db, tree_id);
      {
         let mut inner = self.inner.lock().unwrap();
         inner.owned_notes = notes;
         if let Some(b) = last_opt {
            if b > 0 {
               inner.last_synced_block = b;
            }
         }
      }
      Ok(())
   }

   /// Save full scanner state (nullifiers + owned notes + last block) to redb.
   /// Use together with save_merkle_tree for complete persistence in one file.
   pub fn save_state(&self, db: &Database, tree_id: &str) -> Result<()> {
      let (notes, last) = {
         let inner = self.inner.lock().unwrap();
         (inner.owned_notes.clone(), inner.last_synced_block)
      };
      save_owned_notes_redb(db, tree_id, &notes)?;
      save_last_block_redb(db, tree_id, last)?;
      Ok(())
   }

   /// Convenience: open a single redb database file and return a fully loaded (or fresh) scanner.
   /// This is the recommended way to get started with unified persistence.
   ///
   /// Note: The returned scanner does **not** hold the Database. Keep the db around
   /// if you want to call save_merkle_tree / save_state later.
   pub fn open(db_path: &str, keys: &RailgunKeys, chain_id: u64, tree_id: &str) -> Result<Self> {
      let db = Database::create(db_path)?;
      let scanner = Self::new(keys, chain_id)?;
      // Best effort load
      let _ = scanner.load_merkle_tree(&db, tree_id);
      let _ = scanner.load_state(&db, tree_id);
      Ok(scanner)
   }

   /// Internal helper: read access to inner state.
   fn read_inner<F, R>(&self, f: F) -> R
   where
      F: FnOnce(&RailgunScannerInner) -> R,
   {
      let guard = self.inner.lock().unwrap();
      f(&guard)
   }

   /// Internal helper: mutable access to inner state.
   fn write_inner<F, R>(&self, f: F) -> R
   where
      F: FnOnce(&mut RailgunScannerInner) -> R,
   {
      let mut guard = self.inner.lock().unwrap();
      f(&mut guard)
   }

   /// Total private balance across all tokens (sum of all unspent owned notes).
   /// Prefer private_balances() or private_balance_for() for per-token info.
   pub fn private_balance(&self) -> U256 {
      self.read_inner(|inner| {
         inner.owned_notes.iter().map(|n| n.note.value).fold(U256::ZERO, |a, b| a + b)
      })
   }

   /// Private balances broken down by TokenData (ERC20 address + type + sub-id).
   /// This matches the Kohaku RailgunProvider / BalanceEntry model (per AssetId).
   pub fn private_balances(&self) -> HashMap<TokenData, U256> {
      self.read_inner(|inner| {
         let mut map: HashMap<TokenData, U256> = HashMap::new();
         for n in &inner.owned_notes {
            *map.entry(n.note.token_data.clone()).or_insert(U256::ZERO) += n.note.value;
         }
         map
      })
   }

   /// Private balance for one specific token.
   pub fn private_balance_for(&self, token: &TokenData) -> U256 {
      self.read_inner(|inner| {
         inner
            .owned_notes
            .iter()
            .filter(|n| &n.note.token_data == token)
            .map(|n| n.note.value)
            .fold(U256::ZERO, |a, b| a + b)
      })
   }

   /// Total number of unspent notes across all tokens.
   pub fn unspent_note_count(&self) -> usize {
      self.read_inner(|inner| inner.owned_notes.len())
   }

   /// Unspent notes for a specific token (used for note selection in unshield).
   pub fn unspent_notes_for(&self, token: &TokenData) -> Vec<OwnedNote> {
      self.read_inner(|inner| {
         inner
            .owned_notes
            .iter()
            .filter(|n| &n.note.token_data == token)
            .cloned()
            .collect()
      })
   }

   /// All currently unspent notes we control.
   pub fn unspent_notes(&self) -> Vec<OwnedNote> {
      self.read_inner(|inner| inner.owned_notes.clone())
   }

   /// Remove a note from our unspent list because its nullifier was observed on-chain
   /// (or because we just spent it in a local unshield).
   ///
   /// This is the Kohaku-style model: the `owned_notes` vec itself is the set of current unspent notes.
   pub fn mark_nullified(&self, nullifier: U256) {
      self.write_inner(|inner| {
         inner.owned_notes.retain(|n| n.nullifier != nullifier);
      });
   }

   /// Mark multiple nullifiers at once (remove the corresponding notes from unspent).
   pub fn mark_nullified_many(&self, nullifiers: &[U256]) {
      for n in nullifiers {
         self.mark_nullified(*n);
      }
   }

   /// Sync the Merkle tree and unspent notes from on-chain events.
   ///
   /// Fetches Shield/Transact/Nullified/Unshield logs.
   /// - Inserts new commitments into the local Merkle tree.
   /// - On Nullified events: removes matching notes from `owned_notes` (Kohaku style).
   ///
   /// Recommended after a successful shield/unshield tx to update balances correctly.
   /// This is the safe way (no optimistic updates that could be wrong if tx reverts).
   pub async fn sync_from_block(
      &self,
      client: RpcClient,
      from_block: u64,
      to_block: Option<u64>,
   ) -> Result<()> {
      let (contract, chain_id) = self.read_inner(|inner| (inner.railgun_address, inner.chain_id));

      let client_chain_id = client.get_chain_id().await?;
      if client_chain_id != chain_id {
         return Err(anyhow!(
            "Client chain ID {} does not match scanner chain ID {}",
            client_chain_id,
            chain_id
         ));
      }

      let latest = if let Some(tb) = to_block {
         tb
      } else {
         client.get_block_number().await? as u64
      };

      if from_block > latest {
         return Ok(());
      }

      // TODO: Make these configurable
      let concurrency = 1;
      let block_range = 50_000;

      // Build filter for Railgun events (current contract signatures)
      let target_address = vec![contract];
      let events = vec![
         "Shield(uint256,uint256,(uint8,address,uint256)[],(bytes32[3],bytes32)[],uint256[])",
         "Transact(uint256,uint256,bytes32[],(bytes32[4],bytes32,bytes32,bytes,bytes)[])",
         "Unshield(address,(uint8,address,uint256),uint256,uint256)",
         "Nullified(uint16,bytes32[])",
      ];

      let logs = get_logs_for(
         client,
         target_address,
         events,
         from_block,
         Some(latest),
         concurrency,
         block_range,
      )
      .await?;

      let mut new_commitments: Vec<U256> = Vec::new();

      for log in logs {
         // Shield: contains preimages. We must compute the actual leaf = poseidon(npk, tokenID, value)
         if let Ok(decoded) =
            <RailgunSmartWallet::Shield as alloy_sol_types::SolEvent>::decode_log(&log.inner)
         {
            for preimage in decoded.data.commitments {
               let token_data = crate::note::TokenData {
                  token_type: match preimage.token.tokenType {
                     0 => crate::note::TokenType::ERC20,
                     1 => crate::note::TokenType::ERC721,
                     _ => crate::note::TokenType::ERC1155,
                  },
                  token_address: preimage.token.tokenAddress.to_string(),
                  token_sub_id: preimage.token.tokenSubID,
               };

               if let Ok(token_hash) = crate::note::compute_token_hash(&token_data) {
                  if let Ok(leaf) = crate::note::compute_commitment(
                     U256::from_be_slice(&preimage.npk[..]),
                     token_hash,
                     U256::from(preimage.value),
                  ) {
                     new_commitments.push(leaf);
                  }
               }
            }
            continue;
         }

         // Transact: hash[] are the pre-computed leaves (already hashed by the contract)
         if let Ok(decoded) =
            <RailgunSmartWallet::Transact as alloy_sol_types::SolEvent>::decode_log(&log.inner)
         {
            for h in decoded.data.hash {
               new_commitments.push(U256::from_be_slice(&h[..]));
            }
            continue;
         }

         // Nullified: on-chain spend of one or more notes (from Transact / unshield).
         // Remove matching notes from our unspent list (Kohaku `handle_nullified_event` pattern).
         // This is how private balances are correctly reduced after a successful spend.
         if let Ok(decoded) =
            <RailgunSmartWallet::Nullified as alloy_sol_types::SolEvent>::decode_log(&log.inner)
         {
            for n in decoded.data.nullifier {
               let nullifier = U256::from_be_slice(&n[..]);
               self.mark_nullified(nullifier);
            }
            continue;
         }
      }

      if !new_commitments.is_empty() {
         self.write_inner(|inner| {
            let _ = inner.merkle_tree.insert_batch(&new_commitments);
         });
      }

      self.write_inner(|inner| {
         inner.last_synced_block = latest;
      });
      Ok(())
   }

   /// Try to decrypt a received encrypted note using our viewing key.
   ///
   /// This is called when we receive a note payload (usually from Waku broadcaster).
   /// `ciphertext` and `nonce` come from the encrypted note (V2 format).
   /// `blinded_receiver_viewing_key` is sent along with the note.
   pub fn try_decrypt_received_note(
      &self,
      ciphertext: &[u8],
      nonce: &[u8],
      blinded_receiver_viewing_key: [u8; 32],
      _sender_random: Option<[u8; 32]>,
      expected_commitment: Option<U256>,
      leaf_index_hint: Option<u64>,
   ) -> Result<Option<OwnedNote>> {
      // Snapshot keys + tree length without holding lock for long
      let (view_priv, null_key, tree_len) = self.read_inner(|inner| {
         (
            inner.viewing_private,
            inner.nullifying_key,
            inner.merkle_tree.len(),
         )
      });

      let shared_key =
         crate::note::derive_shared_symmetric_key(&view_priv, &blinded_receiver_viewing_key)?;

      let nonce12: &[u8; 12] = nonce.try_into().map_err(|_| anyhow!("nonce must be 12 bytes"))?;
      let decrypted = match decrypt_note_v2(ciphertext, nonce12, &shared_key) {
         Ok(n) => n,
         Err(_) => return Ok(None),
      };

      let token_hash = crate::note::compute_token_hash(&decrypted.token_data)?;
      let commitment = crate::note::compute_commitment(
         decrypted.note_public_key,
         token_hash,
         decrypted.value,
      )?;

      if let Some(expected) = expected_commitment {
         if commitment != expected {
            return Ok(None);
         }
      }

      let leaf_index = leaf_index_hint.unwrap_or(tree_len as u64);
      let nullifier = compute_nullifier(null_key, leaf_index)?;

      // Avoid adding duplicate note (nullifier already known as unspent)
      let already_have =
         self.read_inner(|inner| inner.owned_notes.iter().any(|n| n.nullifier == nullifier));
      if already_have {
         return Ok(None);
      }

      let owned = OwnedNote {
         note: decrypted,
         leaf_index,
         nullifier,
         commitment,
      };

      self.write_inner(|inner| {
         inner.owned_notes.push(owned.clone());
      });

      Ok(Some(owned))
   }

   /// Add a note we created ourselves (e.g. after a successful shield).
   /// We already know the plaintext.
   pub fn add_own_shielded_note(&self, note: Note, leaf_index: u64) -> Result<OwnedNote> {
      self.write_inner(|inner| {
         let nullifier = compute_nullifier(inner.nullifying_key, leaf_index)?;
         // Avoid duplicates (Kohaku style: the list is the current unspent set)
         if inner.owned_notes.iter().any(|n| n.nullifier == nullifier) {
            // already have it, return existing
            if let Some(existing) = inner.owned_notes.iter().find(|n| n.nullifier == nullifier) {
               return Ok(existing.clone());
            }
         }
         let owned = OwnedNote {
            note: note.clone(),
            leaf_index,
            nullifier,
            commitment: note.commitment,
         };
         inner.owned_notes.push(owned.clone());
         let _ = inner.merkle_tree.insert(note.commitment);
         Ok(owned)
      })
   }
}

// ===================== Redb persistence for scanner state (nullifiers + owned notes + meta) =====================

fn serialize_owned_notes_for_redb(notes: &[OwnedNote]) -> Vec<u8> {
   // Reuse the existing binary format
   let mut b = Vec::new();
   b.extend_from_slice(&(notes.len() as u64).to_le_bytes());
   for on in notes {
      b.extend_from_slice(&on.leaf_index.to_le_bytes());
      b.extend_from_slice(&on.nullifier.to_be_bytes::<32>());
      b.extend_from_slice(&on.commitment.to_be_bytes::<32>());
      let nb = on.note.to_bytes();
      b.extend_from_slice(&(nb.len() as u32).to_le_bytes());
      b.extend_from_slice(&nb);
   }
   b
}

fn deserialize_owned_notes_from_redb(data: &[u8]) -> Result<Vec<OwnedNote>> {
   if data.len() < 8 {
      return Ok(Vec::new());
   }
   let len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
   let mut notes = Vec::with_capacity(len);
   let mut offset = 8;
   for _ in 0..len {
      if offset + 72 > data.len() {
         break;
      }
      let li = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
      offset += 8;
      let mut nul = [0u8; 32];
      nul.copy_from_slice(&data[offset..offset + 32]);
      offset += 32;
      let mut com = [0u8; 32];
      com.copy_from_slice(&data[offset..offset + 32]);
      offset += 32;
      let nl = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
      offset += 4;
      if offset + nl > data.len() {
         break;
      }
      let note = Note::from_bytes(&data[offset..offset + nl])?;
      offset += nl;
      notes.push(OwnedNote {
         note,
         leaf_index: li,
         nullifier: U256::from_be_bytes(nul),
         commitment: U256::from_be_bytes(com),
      });
   }
   Ok(notes)
}

fn load_owned_notes_redb(db: &Database, tree_id: &str) -> Result<Vec<OwnedNote>> {
   let read_txn = db.begin_read()?;
   let table = match read_txn.open_table(SCANNER_OWNED_NOTES_TABLE) {
      Ok(t) => t,
      Err(_) => return Ok(Vec::new()),
   };
   match table.get(tree_id)? {
      Some(value) => deserialize_owned_notes_from_redb(value.value()),
      None => Ok(Vec::new()),
   }
}

fn save_owned_notes_redb(db: &Database, tree_id: &str, notes: &[OwnedNote]) -> Result<()> {
   let write_txn = db.begin_write()?;
   {
      let mut table = write_txn.open_table(SCANNER_OWNED_NOTES_TABLE)?;
      let bytes = serialize_owned_notes_for_redb(notes);
      table.insert(tree_id, bytes.as_slice())?;
   }
   write_txn.commit()?;
   Ok(())
}

fn load_last_block_redb(db: &Database, tree_id: &str) -> Option<u64> {
   let read_txn = match db.begin_read() {
      Ok(t) => t,
      Err(_) => return None,
   };
   let table = match read_txn.open_table(SCANNER_META_TABLE) {
      Ok(t) => t,
      Err(_) => return None,
   };
   match table.get(tree_id) {
      Ok(Some(value)) => {
         let b: &[u8] = value.value();
         if b.len() >= 8 {
            Some(u64::from_le_bytes(b[0..8].try_into().unwrap()))
         } else {
            None
         }
      }
      _ => None,
   }
}

fn save_last_block_redb(db: &Database, tree_id: &str, block: u64) -> Result<()> {
   let write_txn = db.begin_write()?;
   {
      let mut table = write_txn.open_table(SCANNER_META_TABLE)?;
      table.insert(tree_id, &block.to_le_bytes()[..])?;
   }
   write_txn.commit()?;
   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use secure_types::SecureArray;

   fn dummy_seed() -> SecureArray<u8, 64> {
      let mut seed = [0u8; 64];
      for (i, b) in seed.iter_mut().enumerate() {
         *b = (i % 251) as u8;
      }
      SecureArray::from_slice(&seed).unwrap()
   }

   #[tokio::test]
   async fn test_scanner_creation_and_basic_state() {
      let seed = dummy_seed();
      let keys = RailgunKeys::new(seed, 0).unwrap();

      let scanner = RailgunScanner::new(&keys, 1).unwrap();

      assert_eq!(scanner.chain_id(), 1);
      assert_eq!(scanner.owned_notes_len(), 0);
      assert_eq!(scanner.private_balance(), U256::ZERO);
   }

   #[test]
   fn test_scanner_load_save_state_file() {
      let seed = dummy_seed();
      let keys = RailgunKeys::new(seed, 0).unwrap();

      let db_path = "/tmp/zeus-railgun-unified-test.redb";
      let tree_id = "test";
      let _ = std::fs::remove_file(db_path);

      let db = redb::Database::create(db_path).unwrap();
      let scanner = RailgunScanner::new(&keys, 1).unwrap();

      scanner.write_inner(|inner| {
         inner.last_synced_block = 98765;
      });

      scanner.save_merkle_tree(&db, tree_id).unwrap();
      scanner.save_state(&db, tree_id).unwrap();

      let scanner2 = RailgunScanner::new(&keys, 1).unwrap();
      scanner2.load_merkle_tree(&db, tree_id).unwrap();
      scanner2.load_state(&db, tree_id).unwrap();

      assert_eq!(scanner2.last_synced_block(), 98765);
      assert_eq!(scanner2.owned_notes_len(), 0);

      let _ = std::fs::remove_file(db_path);
   }
}
