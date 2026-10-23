use std::fmt::{Debug, Display};

use alloy_primitives::U256;
use serde::{Deserialize, Serialize};

use crate::{
   abi::railgun::{TokenData, TokenDataError},
   account::signer::RailgunSigner,
   caip::{AssetId, TokenRegistry},
   crypto::{
      aes::AesError,
      keys::{
         BlindedKey, KeyError, MasterPublicKey, NullifyingKey, SpendingPublicKey, ViewingPublicKey,
      },
      poseidon2, poseidon3,
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
   // A note's token-hash block is 32 bytes, and the registry is keyed by that `U256`. A longer slice is
   // a bundle that is not shaped like a note, so it reads as an error (the caller skips the note)
   // rather than the panic `from_be_slice` raises on a value that does not fit.
   let hash = U256::try_from_be_slice(token_hash).ok_or(NoteError::TokenData(
      TokenDataError::InvalidHashLength,
   ))?;
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
#[non_exhaustive]
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

#[derive(Debug)]
#[non_exhaustive]
pub enum NoteError {
   Aes(AesError),
   TokenData(TokenDataError),
   Key(KeyError),
}

impl std::fmt::Display for NoteError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::Aes(err) => write!(f, "AES error: {err}"),
         Self::TokenData(err) => write!(f, "TokenData error: {err}"),
         Self::Key(err) => write!(f, "Key error: {err}"),
      }
   }
}

impl std::error::Error for NoteError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::Aes(err) => Some(err),
         Self::TokenData(err) => Some(err),
         Self::Key(err) => Some(err),
      }
   }
}

impl From<AesError> for NoteError {
   fn from(err: AesError) -> Self {
      Self::Aes(err)
   }
}

impl From<TokenDataError> for NoteError {
   fn from(err: TokenDataError) -> Self {
      Self::TokenData(err)
   }
}

impl From<KeyError> for NoteError {
   fn from(err: KeyError) -> Self {
      Self::Key(err)
   }
}

/// Everything a note needs apart from the key material it is built for.
#[non_exhaustive]
pub struct UtxoNoteParams {
   pub tree_number: u32,
   pub leaf_index: u32,
   pub asset: AssetId,
   pub value: u128,
   pub random: [u8; 16],
   pub memo: String,
   pub commitment_type: BlindedCommitmentType,
}

impl UtxoNote {
   pub fn new(signer: &RailgunSigner, params: UtxoNoteParams) -> Self {
      let UtxoNoteParams {
         tree_number,
         leaf_index,
         asset,
         value,
         random,
         memo,
         commitment_type,
      } = params;

      let spending_pubkey = signer.keys().spending_public_key;
      let viewing_pubkey = signer.keys().viewing_public_key;
      let nullifying_key = signer.keys().nullifying_key;
      let nullifier = poseidon2(nullifying_key.to_u256(), U256::from(leaf_index));
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
         memo,
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

      // The bundle's blocks keep the lengths they had in the ciphertext, which arrives from a remote
      // source: a bundle that is not shaped like a note — a missing block, or a block of the wrong
      // size — has to read as an error the caller skips, never as an index panic.
      let token_hash = bundle.get(1).ok_or(NoteError::TokenData(
         TokenDataError::InvalidHashLength,
      ))?;
      let asset_id = resolve_transact_asset(token_hash, known_tokens)?;

      let Some(block) = bundle.get(2).filter(|block| block.len() == 32) else {
         return Err(NoteError::TokenData(
            TokenDataError::InvalidHashLength,
         ));
      };
      let (random_block, value_block) = block.split_at(16);

      let mut random = [0u8; 16];
      random.copy_from_slice(random_block);

      let mut value_bytes = [0u8; 16];
      value_bytes.copy_from_slice(value_block);
      let value = u128::from_be_bytes(value_bytes);

      let memo = if bundle.len() > 3 {
         std::str::from_utf8(&bundle[3]).unwrap_or("")
      } else {
         ""
      };

      Ok(UtxoNote::new(
         signer,
         UtxoNoteParams {
            tree_number: transact.tree_number,
            leaf_index: transact.leaf_index,
            asset: asset_id,
            value,
            random,
            memo: memo.to_string(),
            commitment_type: BlindedCommitmentType::Transact,
         },
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
         signer,
         UtxoNoteParams {
            tree_number: shield.tree_number,
            leaf_index: shield.leaf_index,
            asset: asset_id,
            value: value.saturating_to(),
            random,
            memo: String::new(),
            commitment_type: BlindedCommitmentType::Shield,
         },
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
   poseidon3(note_public_key, asset.hash(), U256::from(value)).into()
}

fn note_public_key(
   spending_pubkey: SpendingPublicKey,
   nullifying_key: NullifyingKey,
   random: &[u8; 16],
) -> U256 {
   let master_key = MasterPublicKey::new(spending_pubkey, nullifying_key);

   poseidon2(master_key.to_u256(), U256::from_be_slice(random))
}

pub fn blinded_commitment(hash: U256, npk: U256, tree_number: u32, leaf_index: u32) -> U256 {
   poseidon3(
      hash,
      npk,
      U256::from((tree_number as u128) * 65536 + (leaf_index as u128)),
   )
}

#[cfg(test)]
pub fn test_note() -> UtxoNote {
   use crate::account::signer::RailgunSigner;
   use secure_types::SecureArray;

   let sec_array = SecureArray::from_slice(&[1u8; 64]).unwrap();
   let signer = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

   UtxoNote::new(
      &signer,
      UtxoNoteParams {
         tree_number: 1,
         leaf_index: 0,
         asset: AssetId::Erc20(alloy_primitives::address!(
            "0x1234567890123456789012345678901234567890"
         )),
         value: 100u128,
         random: [3u8; 16],
         memo: "test memo".to_string(),
         commitment_type: BlindedCommitmentType::Transact,
      },
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
      assert_eq!(note.hash(), transfer.hash());

      // Without it there is nothing to recognize the asset by, and a 32-byte hash can only be read as an
      // ERC-20 address. The guess is not the asset the sender used, so the leaf does not match either.
      let guessed = UtxoNote::decrypt_transact(&recipient, &event, &TokenRegistry::new()).unwrap();
      assert_ne!(guessed.asset, asset);
      assert!(guessed.asset.is_erc20());
      assert_ne!(guessed.hash(), transfer.hash());
   }

   fn random_signer() -> RailgunSigner {
      use rand::random;
      use secure_types::SecureArray;

      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      RailgunSigner::from_seed(&sec_array, 0, 1).unwrap()
   }

   /// A `Transact` whose ciphertext decrypts to exactly `blocks` under the shared key the recipient
   /// derives from it. The blinded key is only ever used to derive that shared key, so the
   /// recipient's own public key stands in for it and the test controls the bundle's block lengths —
   /// which is the point: those come from a remote source.
   fn transact_with_blocks(recipient: &RailgunSigner, blocks: &[&[u8]]) -> Transact {
      use alloy_primitives::B256;

      let blinded_sender = recipient.keys().viewing_public_key;
      let shared_key = recipient
         .keys()
         .viewing_private_key
         .derive_shared_key_blinded(BlindedKey::from_bytes(*blinded_sender.as_bytes()))
         .unwrap();
      let ciphertext = shared_key.encrypt_gcm(blocks, &[9u8; 16]).unwrap();

      Transact {
         tree_number: 1,
         leaf_index: 0,
         hash: U256::ZERO,
         ciphertext,
         blinded_sender_viewing_key: *blinded_sender.as_bytes(),
         blinded_receiver_viewing_key: [0u8; 32],
         annotation_data: Vec::new(),
         timestamp: 0,
         tx_hash: B256::ZERO,
      }
   }

   /// The subgraph source appends the memo to the note's `data` blocks, so a response carrying a
   /// single `data` entry makes the memo the second block. Read as the token hash, an over-long block
   /// has to come back as an error — the old read panicked inside `U256::from_be_slice`.
   #[test]
   fn a_transact_bundle_whose_second_block_is_a_memo_is_rejected() {
      let recipient = random_signer();
      let data_block: &[u8] = &[0u8; 32];
      let memo: &[u8] = &[0xAB; 40];

      let event = transact_with_blocks(&recipient, &[data_block, memo]);
      let note = UtxoNote::decrypt_transact(&recipient, &event, &TokenRegistry::new());

      assert!(matches!(
         note,
         Err(NoteError::TokenData(
            TokenDataError::InvalidHashLength
         ))
      ));
   }

   /// The bundle keeps the ciphertext's block lengths, so it can arrive with fewer blocks than the
   /// note layout has. That is a skip, not an index panic.
   #[test]
   fn a_transact_bundle_with_too_few_blocks_is_rejected() {
      let recipient = random_signer();
      let hash: &[u8] = &[0u8; 32];
      let value: &[u8] = &[0u8; 32];

      for blocks in [vec![], vec![hash], vec![hash, value]] {
         let event = transact_with_blocks(&recipient, &blocks);
         let note = UtxoNote::decrypt_transact(&recipient, &event, &TokenRegistry::new());

         assert!(
            matches!(
               note,
               Err(NoteError::TokenData(
                  TokenDataError::InvalidHashLength
               ))
            ),
            "a bundle of {} blocks must be rejected",
            blocks.len()
         );
      }
   }

   /// The random/value block is two 16-byte halves, so it is exactly 32 bytes. A shorter block has no
   /// second half and used to be a `copy_from_slice` panic.
   #[test]
   fn a_transact_bundle_with_a_truncated_random_value_block_is_rejected() {
      let recipient = random_signer();
      let hash: &[u8] = &[0u8; 32];
      let truncated: &[u8] = &[0u8; 16];

      let event = transact_with_blocks(&recipient, &[hash, hash, truncated]);
      let note = UtxoNote::decrypt_transact(&recipient, &event, &TokenRegistry::new());

      assert!(matches!(
         note,
         Err(NoteError::TokenData(
            TokenDataError::InvalidHashLength
         ))
      ));
   }
}

#[cfg(test)]
mod error_tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      let inner = AesError::DataTooShort;
      let expected = format!("AES error: {inner}");
      assert_eq!(NoteError::Aes(inner).to_string(), expected);

      let inner = TokenDataError::InvalidHashLength;
      let expected = format!("TokenData error: {inner}");
      assert_eq!(NoteError::TokenData(inner).to_string(), expected);

      let inner = KeyError::DecompressionFailed;
      let expected = format!("Key error: {inner}");
      assert_eq!(NoteError::Key(inner).to_string(), expected);
   }

   #[test]
   fn the_from_variants_delegate_their_source() {
      let inner = AesError::DataTooShort;
      let rendered = inner.to_string();
      assert_eq!(
         NoteError::Aes(inner).source().map(ToString::to_string),
         Some(rendered)
      );

      let inner = TokenDataError::InvalidHashLength;
      let rendered = inner.to_string();
      assert_eq!(
         NoteError::TokenData(inner).source().map(ToString::to_string),
         Some(rendered)
      );

      let inner = KeyError::DecompressionFailed;
      let rendered = inner.to_string();
      assert_eq!(
         NoteError::Key(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }
}
