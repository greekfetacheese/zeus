use std::fmt::{Debug, Display};

use alloy_primitives::U256;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
   abi::railgun::{TokenData, TokenDataError},
   account::signer::RailgunSigner,
   caip::{AssetId, TokenRegistry},
   crypto::{
      aes::AesError,
      keys::{
         BlindedKey, KeyError, MasterPublicKey, NullifyingKey, SpendingPublicKey, ViewingPublicKey,
      },
      poseidon_hash,
   },
   indexer::syncer::types::{Shield, Transact},
   merkle_tree::UtxoLeafHash,
   poi::types::BlindedCommitmentType,
};

/// The asset of a transact note, from the only thing its ciphertext kept: `asset.hash()`.
///
/// The hash is one-way, so this *recognizes* the asset among the ones the wallet has seen in the clear
/// ([`TokenRegistry`]) instead of deriving it. When it is not recognized, the old reading is kept — reading
/// a 32-byte value as an ERC-20 address — which is exactly right for a token and silently wrong for
/// anything else: an ERC-721 or ERC-1155 would come back as a phantom token at the hash's low 20 bytes, and
/// the note's commitment would never match the tree again. Persisting the registry, and resyncing once
/// after it exists, is what makes an NFT's note readable.
fn resolve_transact_asset(
   token_hash: &[u8],
   known_tokens: &TokenRegistry,
) -> Result<AssetId, NoteError> {
   let hash = U256::from_be_slice(token_hash);
   if let Some(asset) = known_tokens.get(&hash) {
      return Ok(*asset);
   }

   // Not recognized. `from_hash` cannot fail on the 32 bytes a note always carries, so a failure here
   // means the bundle is not shaped like a note — said so rather than papered over with an invented asset.
   let token_data = TokenData::from_hash(token_hash)?;
   Ok(AssetId::from(token_data))
}

/// Railgun UTXO note
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UtxoNote {
   pub tree_number: u32,
   pub leaf_index: u32,
   pub spending_pubkey: SpendingPublicKey,
   pub viewing_pubkey: ViewingPublicKey,

   pub random: [u8; 16],
   pub value: u128,
   pub asset: AssetId,
   pub memo: String,

   pub hash: UtxoLeafHash,
   pub nullifier: U256,
   pub note_public_key: U256,
   #[allow(private_interfaces)]
   pub nullifying_key: NullifyingKey,
   pub blinded_commitment: U256,
   pub commitment_type: BlindedCommitmentType,
}

#[derive(Debug, Error)]
pub enum NoteError {
   #[error("AES error: {0}")]
   Aes(#[from] AesError),
   #[error("TokenData error: {0}")]
   TokenData(#[from] TokenDataError),
   #[error("Key error: {0}")]
   Key(#[from] KeyError),
}

impl UtxoNote {
   pub fn new(
      tree_number: u32,
      leaf_index: u32,
      signer: &RailgunSigner,
      asset: AssetId,
      value: u128,
      random: [u8; 16],
      memo: &str,
      commitment_type: BlindedCommitmentType,
   ) -> Self {
      let spending_pubkey = signer.keys().spending_public_key.clone();
      let viewing_pubkey = signer.keys().viewing_public_key.clone();
      let nullifying_key = signer.keys().nullifying_key.clone();
      let nullifier = poseidon_hash(&[nullifying_key.to_u256(), U256::from(leaf_index)]).unwrap();
      let npk = note_public_key(spending_pubkey, nullifying_key, &random);
      let hash = note_hash(npk, asset, value);
      let blinded_commitment = blinded_commitment(hash.into(), npk, tree_number, leaf_index);

      UtxoNote {
         tree_number,
         leaf_index,
         spending_pubkey,
         viewing_pubkey,
         asset,
         value,
         random,
         memo: memo.to_string(),
         hash,
         note_public_key: npk,
         nullifying_key,
         nullifier,
         blinded_commitment,
         commitment_type,
      }
   }

   /// Decrypt a transact note into a Note
   pub fn decrypt_transact(
      signer: &RailgunSigner,
      transact: &Transact,
      known_tokens: &TokenRegistry,
   ) -> Result<Self, NoteError> {
      let blinded_sender = BlindedKey::from_bytes(transact.blinded_sender_viewing_key);
      let shared_key =
         signer.keys().viewing_private_key.derive_shared_key_blinded(blinded_sender)?;

      // iv (16) | tag (16)
      // token_hash (32)
      // random (16) | value (16)
      // memo (optional)
      let bundle = shared_key.decrypt_gcm(&transact.ciphertext)?;
      let asset_id = resolve_transact_asset(&bundle[1], known_tokens)?;

      let mut random = [0u8; 16];
      random.copy_from_slice(&bundle[2][..16]);

      let mut value_bytes = [0u8; 16];
      value_bytes.copy_from_slice(&bundle[2][16..]);
      let value = u128::from_be_bytes(value_bytes);

      let memo = if bundle.len() > 3 {
         std::str::from_utf8(&bundle[3]).unwrap_or("")
      } else {
         ""
      };

      Ok(UtxoNote::new(
         transact.tree_number,
         transact.leaf_index,
         signer,
         asset_id,
         value,
         random,
         memo,
         BlindedCommitmentType::Transact,
      ))
   }

   /// Decrypts a shield note into a Note
   pub fn decrypt_shield(signer: &RailgunSigner, shield: &Shield) -> Result<Self, NoteError> {
      let shield_key = ViewingPublicKey::from_bytes(shield.shield_key);
      let shared_key = signer.keys().viewing_private_key.derive_shared_key(shield_key)?;

      let decrypted = shared_key.decrypt_gcm(&shield.ciphertext)?;
      let asset_id = shield.token;
      let value = shield.value;

      let mut random = [0u8; 16];
      random.copy_from_slice(&decrypted[0][..16]);

      Ok(UtxoNote::new(
         shield.tree_number,
         shield.leaf_index,
         signer,
         asset_id,
         value.saturating_to(),
         random,
         "",
         BlindedCommitmentType::Shield,
      ))
   }

   pub fn asset(&self) -> AssetId {
      self.asset
   }

   pub fn value(&self) -> u128 {
      self.value
   }

   pub fn memo(&self) -> String {
      self.memo.clone()
   }

   pub fn random(&self) -> [u8; 16] {
      self.random
   }

   pub fn hash(&self) -> UtxoLeafHash {
      self.hash
   }

   pub fn note_public_key(&self) -> U256 {
      self.note_public_key
   }
}

impl Display for UtxoNote {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      write!(
         f,
         "UtxoNote {{ tree_number: {}, leaf_index: {}, asset: {}, value: {}, memo: {}, type: {:?} }}",
         self.tree_number, self.leaf_index, self.asset, self.value, self.memo, self.commitment_type
      )
   }
}

fn note_hash(note_public_key: U256, asset: AssetId, value: u128) -> UtxoLeafHash {
   poseidon_hash(&[note_public_key, asset.hash(), U256::from(value)])
      .unwrap()
      .into()
}

fn note_public_key(
   spending_pubkey: SpendingPublicKey,
   nullifying_key: NullifyingKey,
   random: &[u8; 16],
) -> U256 {
   let master_key = MasterPublicKey::new(spending_pubkey, nullifying_key);

   poseidon_hash(&[master_key.to_u256(), U256::from_be_slice(random)]).unwrap()
}

pub fn blinded_commitment(hash: U256, npk: U256, tree_number: u32, leaf_index: u32) -> U256 {
   poseidon_hash(&[
      hash,
      npk,
      U256::from((tree_number as u128) * 65536 + (leaf_index as u128)),
   ])
   .unwrap()
}

#[cfg(all(test))]
pub fn test_note() -> UtxoNote {
   use crate::account::signer::RailgunSigner;
   use secure_types::SecureArray;

   let sec_array = SecureArray::from_slice(&[1u8; 64]).unwrap();
   let signer = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

   UtxoNote::new(
      1,
      0,
      &signer,
      AssetId::Erc20(alloy_primitives::address!(
         "0x1234567890123456789012345678901234567890"
      )),
      100u128,
      [3u8; 16],
      "test memo",
      BlindedCommitmentType::Transact,
   )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
   use super::*;

   #[test]
   fn test_note_hash() {
      let note = test_note();
      let _hash: U256 = note.hash().into();
   }

   #[test]
   fn test_note_spending_pubkey() {
      let note = test_note();
      let _pub_key = note.spending_pubkey;
   }

   #[test]
   fn test_note_nullifier() {
      let note = test_note();
      let _nullifier = note.nullifier;
   }

   #[test]
   fn test_note_nullifying_key() {
      let note = test_note();
      let _nullifying_key = note.nullifying_key;
   }

   #[test]
   fn test_note_public_key() {
      let note = test_note();
      let _pub_key = note.note_public_key;
   }

   /// A transact note keeps only `asset.hash()`, so an NFT has to be *recognized* from a registry built out
   /// of the shields the wallet has seen, never derived back. Without it the note decodes as a phantom
   /// ERC-20 **and** no longer hashes to its own leaf — the «Element not found in tree» that no amount of
   /// resyncing gets a wallet out of.
   #[test]
   fn an_nft_transact_note_is_readable_only_while_its_asset_is_known() {
      use alloy_primitives::{B256, address};
      use rand::random;
      use secure_types::SecureArray;

      use crate::note::transfer::TransferNote;

      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let sender = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let recipient = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let asset = AssetId::Erc721(
         address!("0xaf5aa7b670ef209e23d3f7b39a8f42f84bd002ac"),
         U256::from(1071),
      );

      let rng = &mut rand::rng();
      let transfer = TransferNote::new(
         sender.keys().viewing_private_key.clone(),
         recipient.address().clone(),
         asset,
         1u128,
         random(),
         "",
      );
      let ciphertext = transfer.encrypt(rng).unwrap();
      let event = Transact {
         tree_number: 1,
         leaf_index: 0,
         hash: transfer.hash().into(),
         ciphertext: ciphertext.clone().into(),
         blinded_sender_viewing_key: *ciphertext.blindedSenderViewingKey,
         blinded_receiver_viewing_key: *ciphertext.blindedReceiverViewingKey,
         annotation_data: ciphertext.annotationData.to_vec(),
         timestamp: 0,
         tx_hash: B256::ZERO,
      };

      // What a wallet holds once it has seen the collection shielded.
      let mut known = TokenRegistry::new();
      known.insert(asset.hash(), asset);

      let note = UtxoNote::decrypt_transact(&recipient, &event, &known).unwrap();
      assert_eq!(note.asset, asset);
      assert_eq!(note.hash(), transfer.hash().into());

      // Without it there is nothing to recognize the asset by, and a 32-byte hash can only be read as an
      // ERC-20 address. The guess is not the asset the sender used, so the leaf does not match either.
      let guessed = UtxoNote::decrypt_transact(&recipient, &event, &TokenRegistry::new()).unwrap();
      assert_ne!(guessed.asset, asset);
      assert_eq!(guessed.asset.is_erc20(), true);
      assert_ne!(guessed.hash(), transfer.hash().into());
   }
}
