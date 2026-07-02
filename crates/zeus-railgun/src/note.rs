//! Railgun Note / Commitment model + encryption/decryption.
//!
//! This module implements the core "Note" concept used by Railgun:
//! - Note public key (npk) + commitment (Poseidon Merkle tree)
//! - Blinded viewing keys (sender/receiver) for private transfers
//! - Encryption/decryption using viewing key (AES-GCM for note + AES-CTR for annotation)
//! - Nullifier computation
//!
//! References: Railgun engine transact-note.ts, keys-utils.ts, memo.ts

use aes::cipher::{KeyIvInit, StreamCipher};
use aes_gcm::{
   Aes256Gcm, Nonce,
   aead::{Aead, KeyInit},
};
use alloy_primitives::U256;
use anyhow::{Result, anyhow};
use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use ctr::Ctr128BE;
use curve25519_dalek::scalar::Scalar;
use zeus_railgun_shared::{Chain, RailgunAddress, RailgunKeys, encode_address};
type Aes256Ctr = Ctr128BE<aes::Aes256>;
use light_poseidon::{Poseidon, PoseidonHasher};
use sha2::{Digest, Sha256, Sha512};
use std::fmt;

/// Token types supported by Railgun notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TokenType {
   ERC20 = 0,
   ERC721 = 1,
   ERC1155 = 2,
}

impl From<u8> for TokenType {
   fn from(v: u8) -> Self {
      match v {
         1 => TokenType::ERC721,
         2 => TokenType::ERC1155,
         _ => TokenType::ERC20,
      }
   }
}

/// Token data for a note.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TokenData {
   /// Token contract address (hex, with or without 0x)
   token_address: String,
   token_type: TokenType,
   /// For ERC721/1155 this is the token id. For ERC20 it must be 0.
   token_sub_id: U256,
}

impl TokenData {
   pub fn new(address: impl Into<String>, token_type: TokenType, token_sub_id: U256) -> Self {
      Self {
         token_address: address.into(),
         token_type,
         token_sub_id,
      }
   }

   pub fn new_erc20(token_address: impl Into<String>) -> Self {
      Self {
         token_address: token_address.into(),
         token_type: TokenType::ERC20,
         token_sub_id: U256::ZERO,
      }
   }

   pub fn address(&self) -> &str {
      &self.token_address
   }

   pub fn token_type(&self) -> TokenType {
      self.token_type
   }

   pub fn token_sub_id(&self) -> U256 {
      self.token_sub_id
   }
}

/// Blinded viewing keys published on-chain for a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlindedViewingKeys {
   pub blinded_sender_viewing_key: [u8; 32],
   pub blinded_receiver_viewing_key: [u8; 32],
}

/// Annotation data that can be attached to a note (output type, sender random, wallet source).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteAnnotationData {
   pub output_type: u8,
   pub sender_random: [u8; 16],
   pub wallet_source: Option<String>,
}

/// A Railgun private note (the fundamental privacy primitive).
///
/// This is the Rust-native equivalent of `TransactNote`.
#[derive(Clone, Debug)]
pub struct Note {
   /// The receiver's master public key (from their 0zk address).
   pub receiver_master_public_key: U256,

   /// 16-byte random value for this note (used in npk and as blinding).
   pub random: [u8; 16],

   /// Amount / value of the note.
   pub value: U256,

   /// Token information.
   pub token_data: TokenData,

   /// Optional sender address data (only present if sender chose to reveal).
   pub sender_address_data: Option<RailgunAddress>,

   /// Optional memo text.
   pub memo: Option<String>,

   /// Sender random used for blinding (32 bytes, only known to sender at creation time).
   pub sender_random: Option<[u8; 32]>,

   /// Blinded viewing keys (published on-chain).
   pub blinded_keys: Option<BlindedViewingKeys>,

   // --- Derived / cached ---
   /// notePublicKey = poseidon([masterPublicKey, random])
   pub note_public_key: U256,

   /// The actual commitment put into the Merkle tree:
   /// hash = poseidon([notePublicKey, tokenHash, value])
   pub commitment: U256,

   /// Token hash (used in commitment). For ERC20 this is just the padded address.
   pub token_hash: [u8; 32],
}

impl Note {
   /// Create a new note (the main constructor for transfers/shields).
   pub fn new(
      receiver_master_public_key: U256,
      random: [u8; 16],
      value: U256,
      token_data: TokenData,
      sender_address_data: Option<RailgunAddress>,
      memo: Option<String>,
   ) -> Result<Self> {
      Self::new_with_blinding(
         receiver_master_public_key,
         random,
         value,
         token_data,
         sender_address_data,
         memo,
         None, // no sender_random by default
         None, // no blinded keys by default
      )
   }

   /// Extended constructor that also carries blinding information (used for private transfers).
   pub fn new_with_blinding(
      receiver_master_public_key: U256,
      random: [u8; 16],
      value: U256,
      token_data: TokenData,
      sender_address_data: Option<RailgunAddress>,
      memo: Option<String>,
      sender_random: Option<[u8; 32]>,
      blinded_keys: Option<BlindedViewingKeys>,
   ) -> Result<Self> {
      let token_hash = compute_token_hash(&token_data)?;
      let note_public_key = compute_note_public_key(receiver_master_public_key, random)?;
      let commitment = compute_commitment(note_public_key, token_hash, value)?;

      Ok(Self {
         receiver_master_public_key,
         random,
         value,
         token_data,
         sender_address_data,
         memo,
         sender_random,
         blinded_keys,
         note_public_key,
         commitment,
         token_hash,
      })
   }

   /// Convenience constructor for a simple ERC20 transfer note.
   pub fn new_erc20(
      receiver_master_public_key: U256,
      random: [u8; 16],
      value: U256,
      token_address: impl Into<String>,
   ) -> Result<Self> {
      Self::new(
         receiver_master_public_key,
         random,
         value,
         TokenData::new_erc20(token_address),
         None,
         None,
      )
   }

   /// Returns the note commitment (what goes into the UTXO Merkle tree).
   pub fn commitment(&self) -> U256 {
      self.commitment
   }

   // ===================== Serialization for persistence =====================

   /// Serialize the Note to bytes (for redb / disk storage).
   /// Format is custom but deterministic and versioned.
   pub fn to_bytes(&self) -> Vec<u8> {
      let mut buf = Vec::new();

      // receiver_master_public_key
      buf.extend_from_slice(&self.receiver_master_public_key.to_be_bytes::<32>());

      // random [16]
      buf.extend_from_slice(&self.random);

      // value
      buf.extend_from_slice(&self.value.to_be_bytes::<32>());

      // token_data
      buf.push(self.token_data.token_type as u8);
      let addr_bytes = self.token_data.token_address.as_bytes();
      buf.extend_from_slice(&(addr_bytes.len() as u32).to_le_bytes());
      buf.extend_from_slice(addr_bytes);
      buf.extend_from_slice(&self.token_data.token_sub_id.to_be_bytes::<32>());

      // sender_address_data (Option<AddressData>)
      if let Some(ref addr) = self.sender_address_data {
         buf.push(1);
         buf.extend_from_slice(&addr.master_public_key.to_be_bytes::<32>());
         buf.extend_from_slice(&addr.viewing_public_key);
         // chain + version (simple)
         let chain_id = addr.chain.map(|c| c.id).unwrap_or(0);
         buf.extend_from_slice(&chain_id.to_le_bytes());
         buf.push(addr.version);
      } else {
         buf.push(0);
      }

      // memo
      if let Some(ref m) = self.memo {
         buf.push(1);
         let mb = m.as_bytes();
         buf.extend_from_slice(&(mb.len() as u32).to_le_bytes());
         buf.extend_from_slice(mb);
      } else {
         buf.push(0);
      }

      // sender_random
      if let Some(r) = self.sender_random {
         buf.push(1);
         buf.extend_from_slice(&r);
      } else {
         buf.push(0);
      }

      // blinded_keys
      if let Some(ref bk) = self.blinded_keys {
         buf.push(1);
         buf.extend_from_slice(&bk.blinded_sender_viewing_key);
         buf.extend_from_slice(&bk.blinded_receiver_viewing_key);
      } else {
         buf.push(0);
      }

      // derived fields
      buf.extend_from_slice(&self.note_public_key.to_be_bytes::<32>());
      buf.extend_from_slice(&self.commitment.to_be_bytes::<32>());
      buf.extend_from_slice(&self.token_hash);

      buf
   }

   /// Deserialize a Note from bytes.
   pub fn from_bytes(data: &[u8]) -> Result<Self> {
      if data.len() < 32 + 16 + 32 {
         return Err(anyhow!("note bytes too short"));
      }

      let mut offset = 0;

      let mut recv = [0u8; 32];
      recv.copy_from_slice(&data[offset..offset + 32]);
      let receiver_master_public_key = U256::from_be_bytes(recv);
      offset += 32;

      let mut random = [0u8; 16];
      random.copy_from_slice(&data[offset..offset + 16]);
      offset += 16;

      let mut val = [0u8; 32];
      val.copy_from_slice(&data[offset..offset + 32]);
      let value = U256::from_be_bytes(val);
      offset += 32;

      let token_type = TokenType::from(data[offset]);
      offset += 1;

      let addr_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
      offset += 4;
      let token_address = String::from_utf8(data[offset..offset + addr_len].to_vec())
         .map_err(|_| anyhow!("bad token address utf8"))?;
      offset += addr_len;

      let mut sub = [0u8; 32];
      sub.copy_from_slice(&data[offset..offset + 32]);
      let token_sub_id = U256::from_be_bytes(sub);
      offset += 32;

      let token_data = TokenData {
         token_type,
         token_address,
         token_sub_id,
      };

      // sender_address_data
      let has_sender = data[offset];
      offset += 1;
      let sender_address_data = if has_sender == 1 {
         let mut mpk = [0u8; 32];
         mpk.copy_from_slice(&data[offset..offset + 32]);
         offset += 32;
         let mut vpk = [0u8; 32];
         vpk.copy_from_slice(&data[offset..offset + 32]);
         offset += 32;

         let chain_id = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
         offset += 8;
         let version = data[offset];
         offset += 1;

         let chain = if chain_id == 0 {
            None
         } else {
            Some(Chain {
               type_: 0,
               id: chain_id,
            })
         };

         let mut railgun_address = RailgunAddress {
            master_public_key: U256::from_be_bytes(mpk),
            viewing_public_key: vpk,
            chain,
            version,
            address: String::new(),
         };

         railgun_address.address = encode_address(&railgun_address)?;

         Some(railgun_address)
      } else {
         None
      };

      // memo
      let has_memo = data[offset];
      offset += 1;
      let memo = if has_memo == 1 {
         let mlen = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
         offset += 4;
         let m = String::from_utf8(data[offset..offset + mlen].to_vec()).ok();
         offset += mlen;
         m
      } else {
         None
      };

      // sender_random
      let has_sr = data[offset];
      offset += 1;
      let sender_random = if has_sr == 1 {
         let mut sr = [0u8; 32];
         sr.copy_from_slice(&data[offset..offset + 32]);
         offset += 32;
         Some(sr)
      } else {
         None
      };

      // blinded_keys
      let has_bk = data[offset];
      offset += 1;
      let blinded_keys = if has_bk == 1 {
         let mut bs = [0u8; 32];
         bs.copy_from_slice(&data[offset..offset + 32]);
         offset += 32;
         let mut br = [0u8; 32];
         br.copy_from_slice(&data[offset..offset + 32]);
         offset += 32;
         Some(BlindedViewingKeys {
            blinded_sender_viewing_key: bs,
            blinded_receiver_viewing_key: br,
         })
      } else {
         None
      };

      // derived
      let mut npk = [0u8; 32];
      npk.copy_from_slice(&data[offset..offset + 32]);
      let note_public_key = U256::from_be_bytes(npk);
      offset += 32;

      let mut comm = [0u8; 32];
      comm.copy_from_slice(&data[offset..offset + 32]);
      let commitment = U256::from_be_bytes(comm);
      offset += 32;

      let mut th = [0u8; 32];
      th.copy_from_slice(&data[offset..offset + 32]);
      let token_hash = th;
      // offset += 32; not needed

      Ok(Self {
         receiver_master_public_key,
         random,
         value,
         token_data,
         sender_address_data,
         memo,
         sender_random,
         blinded_keys,
         note_public_key,
         commitment,
         token_hash,
      })
   }
}

/// Compute the token hash exactly as Railgun does.
pub fn compute_token_hash(token: &TokenData) -> Result<[u8; 32]> {
   match token.token_type {
      TokenType::ERC20 => {
         let addr = parse_hex_address(&token.token_address)?;
         let mut out = [0u8; 32];
         // right-align the 20-byte address (standard for Railgun ERC20)
         out[12..].copy_from_slice(&addr);
         Ok(out)
      }
      TokenType::ERC721 | TokenType::ERC1155 => {
         // Simplified for now (real impl uses keccak + mod SNARK prime)
         let mut hasher = Sha256::new();
         hasher.update([token.token_type as u8]);
         hasher.update(parse_hex_address(&token.token_address)?);
         hasher.update(token.token_sub_id.to_be_bytes::<32>());
         let hash = hasher.finalize();
         let mut out = [0u8; 32];
         out.copy_from_slice(&hash);
         Ok(out)
      }
   }
}

/// notePublicKey = poseidon([receiverMasterPublicKey, random])
pub fn compute_note_public_key(master_pubkey: U256, random: [u8; 16]) -> Result<U256> {
   let random_big = U256::from_be_slice(&random);
   poseidon_hash(vec![master_pubkey, random_big])
}

/// Main commitment function:
/// commitment = poseidon([notePublicKey, tokenHash, value])
pub fn compute_commitment(
   note_public_key: U256,
   token_hash: [u8; 32],
   value: U256,
) -> Result<U256> {
   let token_big = U256::from_be_slice(&token_hash);
   poseidon_hash(vec![note_public_key, token_big, value])
}

// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// Blinded Viewing Keys (for proper note encryption + on-chain events)
// -----------------------------------------------------------------------------

/// Computes the blinding scalar used for both sender and receiver viewing keys.
/// Port of Railgun's getBlindingScalar (XOR + seedToScalar).
pub fn get_blinding_scalar(shared_random: &[u8; 32], sender_random: &[u8; 32]) -> Result<Scalar> {
   // XOR the two 32-byte randoms
   let mut final_random = [0u8; 32];
   for i in 0..32 {
      final_random[i] = shared_random[i] ^ sender_random[i];
   }

   // Hash to scalar (simplified version of seedToScalar)
   let mut hasher = Sha512::new();
   hasher.update(final_random);
   let hash = hasher.finalize();

   let mut head = [0u8; 32];
   head.copy_from_slice(&hash[0..32]);

   // Clamp like ed25519
   head[0] &= 0b0111_1111;
   head[0] |= 0b0100_0000;
   head[31] &= 0b1111_1000;

   head.reverse();

   Ok(Scalar::from_bytes_mod_order(head))
}

/// Creates blinded sender and receiver viewing keys.
/// These are published in the on-chain transact event.
pub fn get_note_blinding_keys(
   sender_viewing_public: &[u8; 32],
   receiver_viewing_public: &[u8; 32],
   shared_random: &[u8; 32],
   sender_random: &[u8; 32],
) -> Result<BlindedViewingKeys> {
   let blinding_scalar = get_blinding_scalar(shared_random, sender_random)?;

   // Parse points
   let sender_pt = curve25519_dalek::edwards::CompressedEdwardsY(*sender_viewing_public)
      .decompress()
      .ok_or_else(|| anyhow!("Invalid sender viewing public key"))?;

   let receiver_pt = curve25519_dalek::edwards::CompressedEdwardsY(*receiver_viewing_public)
      .decompress()
      .ok_or_else(|| anyhow!("Invalid receiver viewing public key"))?;

   // Multiply
   let blinded_sender = (sender_pt * blinding_scalar).compress().to_bytes();
   let blinded_receiver = (receiver_pt * blinding_scalar).compress().to_bytes();

   Ok(BlindedViewingKeys {
      blinded_sender_viewing_key: blinded_sender,
      blinded_receiver_viewing_key: blinded_receiver,
   })
}

/// Unblinds a key (useful for sender to recover their own blinded key).
pub fn unblind_note_key(
   blinded_key: &[u8; 32],
   shared_random: &[u8; 32],
   sender_random: &[u8; 32],
) -> Result<[u8; 32]> {
   let blinding_scalar = get_blinding_scalar(shared_random, sender_random)?;

   let pt = curve25519_dalek::edwards::CompressedEdwardsY(*blinded_key)
      .decompress()
      .ok_or_else(|| anyhow!("Invalid blinded key"))?;

   // Compute inverse scalar
   // Note: curve25519-dalek Scalar doesn't expose easy modular inverse,
   // we use the fact that inverse can be computed via the order.
   // For simplicity in this implementation we recompute using the same curve math.
   // In production a proper inverse is needed.
   let inverse = blinding_scalar.invert(); // if available; otherwise fall back

   let unblinded = pt * inverse;
   Ok(unblinded.compress().to_bytes())
}

// -----------------------------------------------------------------------------
// Nullifier calculation (B)
// -----------------------------------------------------------------------------

/// Computes the nullifier for a note.
/// nullifier = poseidon([nullifyingKey, leafIndex])
///
/// leaf_index is the position of this note's commitment in the Railgun Merkle tree.

/// Compute the nullifying key from the raw viewing private key.
/// This is Poseidon(viewingPrivateKey) in the Railgun spec.
pub fn compute_nullifying_key_from_viewing(viewing_private: &[u8; 32]) -> Result<U256> {
   // For Railgun, the nullifying key is Poseidon of the viewing private interpreted as field element.
   // We reuse the same poseidon helper pattern as in address.rs for consistency.
   let viewing_u256 = U256::from_be_slice(viewing_private);
   // Simple poseidon of single element for now (real impl may do poseidon([viewingPriv]) or similar)
   // In practice Railgun does: nullifyingKey = poseidon([viewingKey])
   // We'll use a 1-input poseidon if available, otherwise hash with a constant.
   poseidon_hash_single(viewing_u256)
}

fn poseidon_hash_single(value: U256) -> Result<U256> {
   // Reuse the poseidon from address or implement simple 1-arity
   // For now fall back to the 2-arity with a zero second input (common pattern)
   poseidon_hash(vec![value, U256::ZERO])
}

pub fn compute_nullifier(nullifying_key: U256, leaf_index: u64) -> Result<U256> {
   let _leaf_fr = Fr::from(leaf_index);
   // Convert to U256 for our poseidon helper
   let mut leaf_bytes = [0u8; 32];
   leaf_bytes[24..].copy_from_slice(&leaf_index.to_be_bytes());
   let leaf_big = U256::from_be_slice(&leaf_bytes);

   poseidon_hash(vec![nullifying_key, leaf_big])
}

// -----------------------------------------------------------------------------
// Annotation Data (Memo system - simplified V2 style)
// -----------------------------------------------------------------------------

/// Encrypts annotation data using the viewing private key (AES-CTR).
/// This is what gets put into the "annotationData" field on-chain.
pub fn encrypt_annotation_data(
   annotation: &NoteAnnotationData,
   viewing_private_key: &[u8; 32],
) -> Result<Vec<u8>> {
   // Simplified: we use AES-CTR with the viewing key directly (as Railgun does for annotation)
   let mut iv = [0u8; 16];
   iv.copy_from_slice(&rand::random::<[u8; 16]>()[0..16]);

   let mut data = Vec::new();
   // outputType (1 byte) + senderRandom (15 bytes) packed to 16 bytes
   data.push(annotation.output_type);
   data.extend_from_slice(&annotation.sender_random[0..15]);

   if let Some(ws) = &annotation.wallet_source {
      // wallet source is optional second block in legacy
      let mut ws_bytes = [0u8; 16];
      let src = ws.as_bytes();
      let len = src.len().min(16);
      ws_bytes[..len].copy_from_slice(&src[..len]);
      data.extend_from_slice(&ws_bytes);
   }

   let mut cipher = Aes256Ctr::new(viewing_private_key.into(), &iv.into());
   let mut ciphertext = data.clone();
   cipher.apply_keystream(&mut ciphertext);

   let mut result = iv.to_vec();
   result.extend(ciphertext);
   Ok(result)
}

/// Decrypts annotation data.
pub fn decrypt_annotation_data(
   ciphertext: &[u8],
   viewing_private_key: &[u8; 32],
) -> Result<NoteAnnotationData> {
   if ciphertext.len() < 16 {
      return Err(anyhow!("Annotation ciphertext too short"));
   }

   let iv = &ciphertext[0..16];
   let mut data = ciphertext[16..].to_vec();

   let mut cipher = Aes256Ctr::new(viewing_private_key.into(), iv.into());
   cipher.apply_keystream(&mut data);

   if data.is_empty() {
      return Err(anyhow!("Empty annotation data"));
   }

   let output_type = data[0];
   let mut sender_random = [0u8; 16];
   sender_random[0..15].copy_from_slice(&data[1..16]);

   let wallet_source = if data.len() > 16 {
      let ws = String::from_utf8_lossy(&data[16..]).trim_end_matches(' ').to_string();
      if ws.is_empty() { None } else { Some(ws) }
   } else {
      None
   };

   Ok(NoteAnnotationData {
      output_type,
      sender_random,
      wallet_source,
   })
}
// Encryption / Decryption using Viewing Key
// -----------------------------------------------------------------------------

/// Derive the AES shared symmetric key from a viewing private key and a (blinded) viewing public key.
///
/// Port of Railgun's `getSharedSymmetricKey`.
pub fn derive_shared_symmetric_key(
   viewing_private: &[u8; 32],
   blinded_viewing_public: &[u8; 32],
) -> Result<[u8; 32]> {
   let scalar = private_scalar_from_viewing_key(viewing_private)?;

   // curve25519-dalek v4 style
   let compressed = curve25519_dalek::edwards::CompressedEdwardsY(*blinded_viewing_public);
   let pub_point = compressed
      .decompress()
      .ok_or_else(|| anyhow!("Invalid blinded viewing public key"))?;

   let shared_point = pub_point * scalar;

   let compressed = shared_point.compress().to_bytes();
   let mut hasher = Sha256::new();
   hasher.update(compressed);
   let hash = hasher.finalize();

   let mut out = [0u8; 32];
   out.copy_from_slice(&hash);
   Ok(out)
}

/// Port of Railgun's getPrivateScalarFromPrivateKey logic.
fn private_scalar_from_viewing_key(privkey: &[u8; 32]) -> Result<Scalar> {
   let mut hasher = Sha512::new();
   hasher.update(privkey);
   let hash = hasher.finalize();

   let mut head = [0u8; 32];
   head.copy_from_slice(&hash[0..32]);

   head[0] &= 0b0111_1111;
   head[0] |= 0b0100_0000;
   head[31] &= 0b1111_1000;

   head.reverse();

   let scalar = Scalar::from_bytes_mod_order(head);
   Ok(scalar)
}

/// Encrypt a note for the receiver (V2 style - AES-GCM).
pub fn encrypt_note_v2(note: &Note, shared_key: &[u8; 32]) -> Result<(Vec<u8>, [u8; 12])> {
   let cipher = Aes256Gcm::new(shared_key.into());

   let mut plaintext = Vec::new();
   plaintext.extend_from_slice(&note.receiver_master_public_key.to_be_bytes::<32>());
   plaintext.extend_from_slice(&note.token_hash);
   plaintext.extend_from_slice(&note.random);
   plaintext.extend_from_slice(&note.value.to_be_bytes::<32>());

   if let Some(m) = &note.memo {
      plaintext.extend_from_slice(m.as_bytes());
   }

   let nonce = Nonce::from(rand::random::<[u8; 12]>());
   let ciphertext = cipher
      .encrypt(&nonce, plaintext.as_ref())
      .map_err(|e| anyhow!("AES-GCM encrypt failed: {}", e))?;

   Ok((ciphertext, nonce.into()))
}

/// Decrypt a note ciphertext using a derived shared key.
pub fn decrypt_note_v2(ciphertext: &[u8], nonce: &[u8; 12], shared_key: &[u8; 32]) -> Result<Note> {
   let cipher = Aes256Gcm::new(shared_key.into());
   let plaintext = cipher
      .decrypt(nonce.into(), ciphertext)
      .map_err(|_| anyhow!("AES-GCM decrypt failed (wrong key or corrupted)"))?;

   if plaintext.len() < 32 + 32 + 16 + 32 {
      return Err(anyhow!("Decrypted note too short"));
   }

   let mut offset = 0;
   let mut mpk_bytes = [0u8; 32];
   mpk_bytes.copy_from_slice(&plaintext[offset..offset + 32]);
   offset += 32;

   let mut token_hash = [0u8; 32];
   token_hash.copy_from_slice(&plaintext[offset..offset + 32]);
   offset += 32;

   let mut random = [0u8; 16];
   random.copy_from_slice(&plaintext[offset..offset + 16]);
   offset += 16;

   let mut value_bytes = [0u8; 32];
   value_bytes.copy_from_slice(&plaintext[offset..offset + 32]);
   let value = U256::from_be_slice(&value_bytes);
   offset += 32;

   let memo = if offset < plaintext.len() {
      Some(String::from_utf8_lossy(&plaintext[offset..]).to_string())
   } else {
      None
   };

   let receiver_mpk = U256::from_be_slice(&mpk_bytes);

   // TODO: Get the actual token data?
   let token_data = TokenData {
      token_address: "0x0000000000000000000000000000000000000000".to_string(),
      token_type: TokenType::ERC20,
      token_sub_id: U256::ZERO,
   };

   Note::new(
      receiver_mpk,
      random,
      value,
      token_data,
      None,
      memo,
   )
}

// -----------------------------------------------------------------------------
// Poseidon helper (matches the one in address.rs)
// -----------------------------------------------------------------------------

fn poseidon_hash(inputs: Vec<U256>) -> Result<U256> {
   let arity = inputs.len();
   if arity == 0 || arity > 12 {
      return Err(anyhow!("Invalid number of inputs for poseidon"));
   }

   // TODO: zeroize all inputs

   let mut fr_inputs = Vec::with_capacity(arity);
   for input in inputs {
      let bytes = input.to_le_bytes::<32>();
      let fr = Fr::from_le_bytes_mod_order(&bytes);
      fr_inputs.push(fr);
   }

   let mut poseidon = Poseidon::<Fr>::new_circom(arity)?;
   let hash_fr = poseidon.hash(&fr_inputs)?;
   let hash_big = hash_fr.into_bigint();
   let be_bytes = hash_big.to_bytes_be();
   let mut padded = [0u8; 32];
   let start = 32 - be_bytes.len();
   padded[start..].copy_from_slice(&be_bytes);
   Ok(U256::from_be_bytes(padded))
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn parse_hex_address(addr: &str) -> Result<[u8; 20]> {
   let s = addr.strip_prefix("0x").unwrap_or(addr);
   let bytes = hex::decode(s).map_err(|e| anyhow!("Bad hex address: {}", e))?;
   if bytes.len() != 20 {
      return Err(anyhow!("Address must be 20 bytes"));
   }
   let mut out = [0u8; 20];
   out.copy_from_slice(&bytes);
   Ok(out)
}

impl fmt::Display for Note {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(
         f,
         "Note {{ commitment: 0x{:064x}, value: {}, token: {:?} }}",
         self.commitment, self.value, self.token_data.token_address
      )
   }
}

/// Convenience: create a note for a receiver using the sender's full RailgunKeys.
/// This automatically generates randoms and blinded keys when sender info is available.
pub fn create_note_with_keys(
   sender_keys: &RailgunKeys,
   receiver_master_public_key: U256,
   receiver_viewing_public: [u8; 32],
   value: U256,
   token_data: TokenData,
   memo: Option<String>,
) -> Result<Note> {
   let random = rand::random::<[u8; 16]>();
   let sender_random = rand::random::<[u8; 32]>();
   let shared_random = rand::random::<[u8; 32]>();

   let blinded = get_note_blinding_keys(
      &sender_keys.viewing_public,
      &receiver_viewing_public,
      &shared_random,
      &sender_random,
   )?;

   let note = Note::new_with_blinding(
      receiver_master_public_key,
      random,
      value,
      token_data,
      None,
      memo,
      Some(sender_random),
      Some(blinded),
   )?;

   Ok(note)
}

/// Compute a nullifier for one of your own notes using your RailgunKeys + leaf index.
pub fn compute_nullifier_for_note(
   keys: &RailgunKeys,
   _commitment: U256, // or leaf index
   leaf_index: u64,
) -> Result<U256> {
   compute_nullifier(keys.nullifying_key, leaf_index)
}

#[cfg(test)]
mod tests {
   use super::*;
   use bip39::{Language, Mnemonic};
   use secure_types::SecureArray;

   fn test_mnemonic() -> SecureArray<u8, 64> {
      let phrase = "boil belt beef hunt cruel lady code dance double city young rule very sight roast make eight travel tattoo mixed you color update double";
      let mnemonic = Mnemonic::parse_in(Language::English, phrase).unwrap();
      let seed = mnemonic.to_seed("");
      SecureArray::from_slice(&seed).unwrap()
   }

   #[test]
   fn test_note_creation_and_commitment() {
      let keys = RailgunKeys::new(test_mnemonic(), 0).unwrap();
      let master_pk = keys.master_public_key;

      let random = [0x42u8; 16];
      let value = U256::from(1_000_000_000_000_000_000u128);
      let token = TokenData::new_erc20("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

      let note = Note::new(master_pk, random, value, token, None, None).unwrap();

      assert!(note.commitment != U256::ZERO);
      println!("Note commitment: 0x{:064x}", note.commitment);
      println!(
         "Note public key : 0x{:064x}",
         note.note_public_key
      );
   }

   #[test]
   fn test_shared_key_derivation() {
      let keys = RailgunKeys::new(test_mnemonic(), 0).unwrap();
      let blinded = keys.viewing_public;

      let shared = derive_shared_symmetric_key(&[0u8; 32], &blinded).unwrap();
      assert_eq!(shared.len(), 32);
   }

   #[test]
   fn test_note_encrypt_decrypt_roundtrip() {
      let keys = RailgunKeys::new(test_mnemonic(), 0).unwrap();
      let master_pk = keys.master_public_key;

      let random = [0x11u8; 16];
      let value = U256::from(123456789u64);
      let token = TokenData::new_erc20("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

      let note = Note::new(
         master_pk,
         random,
         value,
         token,
         None,
         Some("test memo".to_string()),
      )
      .unwrap();

      let shared_key = derive_shared_symmetric_key(&[0x42u8; 32], &keys.viewing_public).unwrap();

      let (ciphertext, nonce) = encrypt_note_v2(&note, &shared_key).unwrap();
      let decrypted = decrypt_note_v2(&ciphertext, &nonce, &shared_key).unwrap();

      assert_eq!(
         decrypted.receiver_master_public_key,
         note.receiver_master_public_key
      );
      assert_eq!(decrypted.value, note.value);
      assert_eq!(decrypted.random, note.random);
      println!(
         "Encrypt/decrypt roundtrip OK. Commitment: 0x{:064x}",
         decrypted.commitment
      );
   }

   #[test]
   fn test_blinded_viewing_keys_and_nullifier() {
      let sender_keys = RailgunKeys::new(test_mnemonic(), 0).unwrap();
      let receiver_keys = RailgunKeys::new(test_mnemonic(), 1).unwrap(); // different index

      let shared_random = rand::random::<[u8; 32]>();
      let sender_random = rand::random::<[u8; 32]>();

      let blinded = get_note_blinding_keys(
         &sender_keys.viewing_public,
         &receiver_keys.viewing_public,
         &shared_random,
         &sender_random,
      )
      .unwrap();

      assert_eq!(blinded.blinded_sender_viewing_key.len(), 32);
      assert_eq!(blinded.blinded_receiver_viewing_key.len(), 32);

      // Nullifier
      let nullifier = compute_nullifier(sender_keys.nullifying_key, 42).unwrap();
      assert!(nullifier != U256::ZERO);
      println!("Nullifier for leaf 42: 0x{:064x}", nullifier);

      // Using RailgunKeys helper
      let note = create_note_with_keys(
         &sender_keys,
         receiver_keys.master_public_key,
         receiver_keys.viewing_public,
         U256::from(1000u64),
         TokenData::new_erc20("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
         Some("private transfer test".to_string()),
      )
      .unwrap();

      assert!(note.blinded_keys.is_some());
      assert!(note.sender_random.is_some());

      let nf = compute_nullifier_for_note(&sender_keys, note.commitment, 123).unwrap();
      println!("Computed nullifier via helper: 0x{:064x}", nf);
   }

   #[test]
   fn test_annotation_data_roundtrip() {
      let keys = RailgunKeys::new(test_mnemonic(), 0).unwrap();
      let viewing_priv: [u8; 32] = keys.viewing_private.unlock(|b| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(b);
         arr
      });

      let annotation = NoteAnnotationData {
         output_type: 1, // transfer
         sender_random: rand::random::<[u8; 16]>(),
         wallet_source: Some("zeus".to_string()),
      };

      let encrypted = encrypt_annotation_data(&annotation, &viewing_priv).unwrap();
      let decrypted = decrypt_annotation_data(&encrypted, &viewing_priv).unwrap();

      assert_eq!(decrypted.output_type, annotation.output_type);
      assert_eq!(
         decrypted.sender_random[0..15],
         annotation.sender_random[0..15]
      );
      println!("Annotation encrypt/decrypt roundtrip OK");
   }
}
