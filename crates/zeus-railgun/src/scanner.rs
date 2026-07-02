//! Railgun Scanner + Poseidon Merkle Tree state management.
//!
//! Responsibilities:
//! - Sync on-chain events (Shield, Transact, Nullifiers, etc.) using an RPC client.
//! - Maintain a local Poseidon Merkle tree of all commitments.
//! - Track spent nullifiers.
//! - Attempt to decrypt notes that belong to us (using viewing key + blinded keys).
//! - Provide private balance and list of owned unspent notes.
//!
//! The actual encrypted note payloads for received private transfers are delivered
//! via the Waku broadcaster (separate from on-chain events). This scanner focuses
//! on the on-chain state (tree + nullifiers) and provides hooks for adding
//! candidate notes (from shield or from Waku messages).

use std::collections::{HashMap, HashSet};
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

/// Tables for scanner state in the same redb file as the merkle tree
const SCANNER_NULLIFIERS_TABLE: TableDefinition<&str, &[u8]> =
   TableDefinition::new("railgun_scanner_nullifiers");

const SCANNER_OWNED_NOTES_TABLE: TableDefinition<&str, &[u8]> =
   TableDefinition::new("railgun_scanner_owned_notes");

const SCANNER_META_TABLE: TableDefinition<&str, &[u8]> =
   TableDefinition::new("railgun_scanner_meta");

/// Internal state (protected by mutex for thread safety).
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
   pub owned_notes: Vec<OwnedNote>,

   /// Set of spent nullifiers we know about.
   pub spent_nullifiers: HashSet<U256>,

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
         spent_nullifiers: HashSet::new(),
         last_synced_block: 0,
         chain_id,
         railgun_address: addr,
      };

      Ok(Self {
         inner: Arc::new(Mutex::new(inner)),
      })
   }

   /// Load the scanner state from a simple binary file (legacy).
   ///
   /// Prefer the unified redb path (`load_state` + single Database) for new code.
   /// If the file does not exist or is empty, the scanner keeps its current (empty) state.
   #[deprecated(since = "0.1.0", note = "use `load_state` instead")]
   pub fn load_state_from_file(&self, path: &str) -> Result<()> {
      let data = match std::fs::read(path) {
         Ok(d) => d,
         Err(_) => return Ok(()),
      };

      if data.is_empty() {
         return Ok(());
      }

      let (nulls, notes, last) = deserialize_scanner_state(&data)?;
      {
         let mut inner = self.inner.lock().unwrap();
         inner.spent_nullifiers = nulls;
         inner.owned_notes = notes;
         if last > 0 {
            inner.last_synced_block = last;
         }
      }
      Ok(())
   }

   /// Save the full scanner state (nullifiers + owned notes + last block) to a binary file.
   /// Use together with `save_merkle_tree` (redb) for complete persistence.
   #[deprecated(since = "0.1.0", note = "use `save_state` instead")]
   pub fn save_state_to_file(&self, path: &str) -> Result<()> {
      let (nulls, owned, last) = {
         let inner = self.inner.lock().unwrap();
         (
            inner.spent_nullifiers.clone(),
            inner.owned_notes.clone(),
            inner.last_synced_block,
         )
      };
      let bytes = serialize_scanner_state(&nulls, &owned, last);
      std::fs::write(path, &bytes)?;
      Ok(())
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
      let nulls = load_nullifiers_redb(db, tree_id)?;
      let notes = load_owned_notes_redb(db, tree_id)?;
      let last_opt = load_last_block_redb(db, tree_id);
      {
         let mut inner = self.inner.lock().unwrap();
         inner.spent_nullifiers = nulls;
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
      let (nulls, notes, last) = {
         let inner = self.inner.lock().unwrap();
         (
            inner.spent_nullifiers.clone(),
            inner.owned_notes.clone(),
            inner.last_synced_block,
         )
      };
      save_nullifiers_redb(db, tree_id, &nulls)?;
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
         inner
            .owned_notes
            .iter()
            .map(|n| n.note.value)
            .fold(U256::ZERO, |a, b| a + b)
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
   /// Matches Kohaku's `handle_nullified_event` + `notes.retain(...)`.
   pub fn mark_nullified(&self, nullifier: U256) {
      self.write_inner(|inner| {
         let before = inner.owned_notes.len();
         inner.owned_notes.retain(|n| n.nullifier != nullifier);
         if inner.owned_notes.len() != before {
            // also record in spent set for any legacy code (will be removed)
            inner.spent_nullifiers.insert(nullifier);
         }
      });
   }

   /// Mark multiple nullifiers at once (e.g. from a Transact that spends several notes).
   pub fn mark_nullified_many(&self, nullifiers: &[U256]) {
      for n in nullifiers {
         self.mark_nullified(*n);
      }
   }

   /// Sync the Merkle tree and nullifier set from on-chain events.
   ///
   /// This is the core "scanner" function.
   /// It fetches historical logs and updates the tree + spent set.
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
                  token_address:preimage.token.tokenAddress.to_string(),
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

         // Nullified
         if let Ok(decoded) =
            <RailgunSmartWallet::Nullified as alloy_sol_types::SolEvent>::decode_log(&log.inner)
         {
            for n in decoded.data.nullifier {
               self.write_inner(|inner| {
                  inner.spent_nullifiers.insert(U256::from_be_slice(&n[..]));
               });
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

      // Check spent + push under lock
      let already_spent = self.read_inner(|inner| inner.spent_nullifiers.contains(&nullifier));
      if already_spent {
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

   /// Mark a nullifier as spent (after we broadcast a spend transaction).
   pub fn mark_nullifier_spent(&self, nullifier: U256) {
      self.write_inner(|inner| {
         inner.spent_nullifiers.insert(nullifier);
      });
   }

}

// ===================== Simple binary serialization for scanner state =====================

fn serialize_scanner_state(
   nullifiers: &std::collections::HashSet<U256>,
   owned: &[OwnedNote],
   last_block: u64,
) -> Vec<u8> {
   let mut b = Vec::new();
   b.extend_from_slice(&last_block.to_le_bytes());
   let null_vec: Vec<U256> = nullifiers.iter().copied().collect();
   b.extend_from_slice(&(null_vec.len() as u64).to_le_bytes());
   for n in &null_vec {
      b.extend_from_slice(&n.to_be_bytes::<32>());
   }
   b.extend_from_slice(&(owned.len() as u64).to_le_bytes());
   for on in owned {
      b.extend_from_slice(&on.leaf_index.to_le_bytes());
      b.extend_from_slice(&on.nullifier.to_be_bytes::<32>());
      b.extend_from_slice(&on.commitment.to_be_bytes::<32>());
      let nb = on.note.to_bytes();
      b.extend_from_slice(&(nb.len() as u32).to_le_bytes());
      b.extend_from_slice(&nb);
   }
   b
}

fn deserialize_scanner_state(
   data: &[u8],
) -> Result<(
   std::collections::HashSet<U256>,
   Vec<OwnedNote>,
   u64,
)> {
   if data.len() < 8 {
      return Ok((Default::default(), vec![], 0));
   }
   let mut offset = 0;
   let last_block = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
   offset += 8;
   let nlen = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as usize;
   offset += 8;
   let mut nulls = std::collections::HashSet::with_capacity(nlen);
   for _ in 0..nlen {
      if offset + 32 > data.len() {
         break;
      }
      let mut buf = [0u8; 32];
      buf.copy_from_slice(&data[offset..offset + 32]);
      nulls.insert(U256::from_be_bytes(buf));
      offset += 32;
   }
   if offset + 8 > data.len() {
      return Ok((nulls, vec![], last_block));
   }
   let olen = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as usize;
   offset += 8;
   let mut notes = Vec::with_capacity(olen);
   for _ in 0..olen {
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
   Ok((nulls, notes, last_block))
}

// ===================== Redb persistence for scanner state (nullifiers + owned notes + meta) =====================

fn serialize_nullifiers(set: &HashSet<U256>) -> Vec<u8> {
   let v: Vec<U256> = set.iter().copied().collect();
   let mut b = Vec::with_capacity(8 + v.len() * 32);
   b.extend_from_slice(&(v.len() as u64).to_le_bytes());
   for x in &v {
      b.extend_from_slice(&x.to_be_bytes::<32>());
   }
   b
}

fn deserialize_nullifiers(data: &[u8]) -> Result<HashSet<U256>> {
   if data.len() < 8 {
      return Ok(HashSet::new());
   }
   let n = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
   let mut s = HashSet::with_capacity(n);
   let mut offset = 8;
   for _ in 0..n {
      if offset + 32 > data.len() {
         break;
      }
      let mut buf = [0u8; 32];
      buf.copy_from_slice(&data[offset..offset + 32]);
      s.insert(U256::from_be_bytes(buf));
      offset += 32;
   }
   Ok(s)
}

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

fn load_nullifiers_redb(db: &Database, tree_id: &str) -> Result<HashSet<U256>> {
   let read_txn = db.begin_read()?;
   let table = match read_txn.open_table(SCANNER_NULLIFIERS_TABLE) {
      Ok(t) => t,
      Err(_) => return Ok(HashSet::new()),
   };
   match table.get(tree_id)? {
      Some(value) => deserialize_nullifiers(value.value()),
      None => Ok(HashSet::new()),
   }
}

fn save_nullifiers_redb(db: &Database, tree_id: &str, set: &HashSet<U256>) -> Result<()> {
   let write_txn = db.begin_write()?;
   {
      let mut table = write_txn.open_table(SCANNER_NULLIFIERS_TABLE)?;
      let bytes = serialize_nullifiers(set);
      table.insert(tree_id, bytes.as_slice())?;
   }
   write_txn.commit()?;
   Ok(())
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
   fn test_scanner_state_ser_de() {
      let mut nulls = std::collections::HashSet::new();
      nulls.insert(U256::from(42u64));
      nulls.insert(U256::from(43u64));

      let owned: Vec<OwnedNote> = vec![];
      let bytes = serialize_scanner_state(&nulls, &owned, 123);
      let (loaded_nulls, loaded_owned, last) = deserialize_scanner_state(&bytes).unwrap();

      assert_eq!(last, 123);
      assert_eq!(loaded_nulls.len(), 2);
      assert!(loaded_owned.is_empty());
      assert!(loaded_nulls.contains(&U256::from(42u64)));
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
         inner.spent_nullifiers.insert(U256::from(777u64));
      });

      scanner.save_merkle_tree(&db, tree_id).unwrap();
      scanner.save_state(&db, tree_id).unwrap();

      let scanner2 = RailgunScanner::new(&keys, 1).unwrap();
      scanner2.load_merkle_tree(&db, tree_id).unwrap();
      scanner2.load_state(&db, tree_id).unwrap();

      assert_eq!(scanner2.last_synced_block(), 98765);
      assert!(scanner2.read_inner(|i| i.spent_nullifiers.contains(&U256::from(777u64))));
      assert_eq!(scanner2.owned_notes_len(), 0);

      let _ = std::fs::remove_file(db_path);
   }
}
