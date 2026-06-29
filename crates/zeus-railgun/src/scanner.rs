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

use std::collections::HashSet;

use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use alloy_rpc_types::Filter;
use anyhow::{Result, anyhow};
use zeus_eth::utils::client::RpcClient;

use crate::{
   RailgunKeys,
   contracts::{RailgunSmartWallet, railgun_address},
   merkle::PoseidonMerkleTree,
   note::{Note, compute_nullifier, decrypt_note_v2},
};

/// A single decrypted note that we own, plus its position in the tree.
#[derive(Debug, Clone)]
pub struct OwnedNote {
   pub note: Note,
   pub leaf_index: u64,
   pub nullifier: U256,
   pub commitment: U256,
}

/// The Railgun scanner maintains private state.
pub struct RailgunScanner {
   /// Viewing private key used for decryption.
   viewing_private: [u8; 32],

   /// Nullifying key (Poseidon hash of viewing private).
   nullifying_key: U256,

   /// Spending private key (used to derive nullifiers for spends we make).
   spending_private: [u8; 32], // kept as bytes for now

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

      let addr = railgun_address(chain_id)
         .ok_or_else(|| anyhow!("No known Railgun contract for chain {}", chain_id))?;

      Ok(Self {
         viewing_private,
         nullifying_key,
         spending_private,
         merkle_tree: PoseidonMerkleTree::new()?,
         owned_notes: Vec::new(),
         spent_nullifiers: HashSet::new(),
         last_synced_block: 0,
         chain_id,
         railgun_address: addr,
      })
   }

   /// Load the scanner state (spent nullifiers + owned notes + last_synced_block)
   /// from a simple binary file.
   ///
   /// If the file does not exist or is empty, the scanner keeps its current (empty) state.
   /// Use together with `load_merkle_tree` for full persistence.
   pub fn load_state_from_file(&mut self, path: &str) -> Result<()> {
      let data = match std::fs::read(path) {
         Ok(d) => d,
         Err(_) => return Ok(()),
      };

      if data.is_empty() {
         return Ok(());
      }

      let (nulls, notes, last) = deserialize_scanner_state(&data)?;
      self.spent_nullifiers = nulls;
      self.owned_notes = notes;

      if last > 0 {
         self.last_synced_block = last;
      }

      Ok(())
   }

   /// Save the full scanner state (nullifiers + owned notes + last block) to a binary file.
   /// Use together with `save_merkle_tree` (redb) for complete persistence.
   pub fn save_state_to_file(&self, path: &str) -> Result<()> {
      let bytes = serialize_scanner_state(
         &self.spent_nullifiers,
         &self.owned_notes,
         self.last_synced_block,
      );
      std::fs::write(path, &bytes)?;
      Ok(())
   }

   /// Get current private balance (sum of unspent owned notes, in the base unit of the token).
   /// Note: This is a simplified version — real implementation groups by token.
   pub fn private_balance(&self) -> U256 {
      self
         .owned_notes
         .iter()
         .filter(|n| !self.spent_nullifiers.contains(&n.nullifier))
         .map(|n| n.note.value)
         .fold(U256::ZERO, |a, b| a + b)
   }

   /// Number of unspent notes we currently control.
   pub fn unspent_note_count(&self) -> usize {
      self
         .owned_notes
         .iter()
         .filter(|n| !self.spent_nullifiers.contains(&n.nullifier))
         .count()
   }

   /// Sync the Merkle tree and nullifier set from on-chain events.
   ///
   /// This is the core "scanner" function.
   /// It fetches historical logs and updates the tree + spent set.
   pub async fn sync_from_block(
      &mut self,
      client: RpcClient,
      from_block: u64,
      to_block: Option<u64>,
   ) -> Result<()> {
      let contract = self.railgun_address;

      let latest = if let Some(tb) = to_block {
         tb
      } else {
         client.get_block_number().await? as u64
      };

      if from_block > latest {
         return Ok(());
      }

      // Build filter for Railgun events (current contract signatures)
      let filter =
         Filter::new().address(contract).from_block(from_block).to_block(latest).events([
            "Shield(uint256,uint256,(uint8,address,uint256)[],(bytes32[3],bytes32)[],uint256[])",
            "Transact(uint256,uint256,bytes32[],(bytes32[4],bytes32,bytes32,bytes,bytes)[])",
            "Unshield(address,(uint8,address,uint256),uint256,uint256)",
            "Nullified(uint16,bytes32[])",
         ]);

      let logs = client.get_logs(&filter).await?;

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
                  token_address: format!("0x{:x}", preimage.token.tokenAddress),
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
               self.spent_nullifiers.insert(U256::from_be_slice(&n[..]));
            }
            continue;
         }
      }

      if !new_commitments.is_empty() {
         self.merkle_tree.insert_batch(&new_commitments)?;
      }

      self.last_synced_block = latest;
      Ok(())
   }

   /// Try to decrypt a received encrypted note using our viewing key.
   ///
   /// This is called when we receive a note payload (usually from Waku broadcaster).
   /// `ciphertext` and `nonce` come from the encrypted note (V2 format).
   /// `blinded_receiver_viewing_key` is sent along with the note.
   pub fn try_decrypt_received_note(
      &mut self,
      ciphertext: &[u8],
      nonce: &[u8],
      blinded_receiver_viewing_key: [u8; 32],
      _sender_random: Option<[u8; 32]>,
      expected_commitment: Option<U256>,
      leaf_index_hint: Option<u64>,
   ) -> Result<Option<OwnedNote>> {
      let shared_key = crate::note::derive_shared_symmetric_key(
         &self.viewing_private,
         &blinded_receiver_viewing_key,
      )?;

      let nonce12: &[u8; 12] = nonce.try_into().map_err(|_| anyhow!("nonce must be 12 bytes"))?;
      let decrypted = match decrypt_note_v2(ciphertext, nonce12, &shared_key) {
         Ok(n) => n,
         Err(_) => return Ok(None), // not for us or bad data
      };

      // Compute the commitment
      let token_hash = crate::note::compute_token_hash(&decrypted.token_data)?;
      let commitment = crate::note::compute_commitment(
         decrypted.note_public_key,
         token_hash,
         decrypted.value,
      )?;

      // If we were given an expected commitment, verify
      if let Some(expected) = expected_commitment {
         if commitment != expected {
            return Ok(None);
         }
      }

      // Compute nullifier
      let leaf_index = leaf_index_hint.unwrap_or(self.merkle_tree.len() as u64);
      let nullifier = compute_nullifier(self.nullifying_key, leaf_index)?;

      // Check if already spent
      if self.spent_nullifiers.contains(&nullifier) {
         return Ok(None);
      }

      // Verify the commitment exists in our tree (best effort)
      // In production we would also check the Merkle proof, but for now we trust the tree.

      let owned = OwnedNote {
         note: decrypted,
         leaf_index,
         nullifier,
         commitment,
      };

      self.owned_notes.push(owned.clone());
      Ok(Some(owned))
   }

   /// Add a note we created ourselves (e.g. after a successful shield).
   /// We already know the plaintext.
   pub fn add_own_shielded_note(&mut self, note: Note, leaf_index: u64) -> Result<OwnedNote> {
      let commitment = note.commitment;
      let nullifier = compute_nullifier(self.nullifying_key, leaf_index)?;

      let owned = OwnedNote {
         note,
         leaf_index,
         nullifier,
         commitment,
      };

      // Make sure it's in the tree (caller should have inserted via sync or manually)
      self.owned_notes.push(owned.clone());
      Ok(owned)
   }

   /// Mark a nullifier as spent (after we broadcast a spend transaction).
   pub fn mark_nullifier_spent(&mut self, nullifier: U256) {
      self.spent_nullifiers.insert(nullifier);
   }

   /// Get all currently unspent owned notes.
   pub fn unspent_notes(&self) -> Vec<&OwnedNote> {
      self
         .owned_notes
         .iter()
         .filter(|n| !self.spent_nullifiers.contains(&n.nullifier))
         .collect()
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

#[cfg(test)]
mod tests {
   use super::*;
   use crate::generate_railgun_keys;
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
      let keys = generate_railgun_keys(seed, 0, None).unwrap();

      let scanner = RailgunScanner::new(&keys, 1).unwrap();

      assert_eq!(scanner.chain_id, 1);
      assert_eq!(scanner.owned_notes.len(), 0);
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
      let keys = generate_railgun_keys(seed, 0, None).unwrap();

      let mut scanner = RailgunScanner::new(&keys, 1).unwrap();

      // Populate some state
      scanner.last_synced_block = 123456;
      scanner.spent_nullifiers.insert(U256::from(999u64));

      let state_path = "/tmp/zeus-railgun-test-scanner.state";

      // Save
      scanner.save_state_to_file(state_path).unwrap();

      // Load into a fresh scanner
      let mut scanner2 = RailgunScanner::new(&keys, 1).unwrap();
      scanner2.load_state_from_file(state_path).unwrap();

      assert_eq!(scanner2.last_synced_block, 123456);
      assert!(scanner2.spent_nullifiers.contains(&U256::from(999u64)));
      assert_eq!(scanner2.owned_notes.len(), 0);

      // cleanup
      let _ = std::fs::remove_file(state_path);
   }
}
