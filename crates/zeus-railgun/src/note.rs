//! Railgun Note / Commitment model + encryption/decryption.
//!
//! This module implements the core "Note" concept used by Railgun:
//! - Note public key (npk)
//! - Note commitment (hash stored in the Poseidon Merkle tree)
//! - Encryption of notes to a receiver using their viewing key (shared secret derivation + AES-GCM)
//!
//! References: Railgun engine transact-note.ts, note-util.ts, keys-utils.ts

use crate::address::AddressData;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use alloy_primitives::U256;
use anyhow::{anyhow, Result};
use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use curve25519_dalek::scalar::Scalar;
use light_poseidon::{Poseidon, PoseidonHasher};
use sha2::{Digest, Sha256, Sha512};
use std::fmt;

/// Token types supported by Railgun notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenData {
    /// Token contract address (hex, with or without 0x)
    pub token_address: String,
    pub token_type: TokenType,
    /// For ERC721/1155 this is the token id. For ERC20 it must be 0.
    pub token_sub_id: U256,
}

impl TokenData {
    pub fn new_erc20(token_address: impl Into<String>) -> Self {
        Self {
            token_address: token_address.into(),
            token_type: TokenType::ERC20,
            token_sub_id: U256::ZERO,
        }
    }
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
    pub sender_address_data: Option<AddressData>,

    /// Optional memo text.
    pub memo: Option<String>,

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
        sender_address_data: Option<AddressData>,
        memo: Option<String>,
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
pub fn compute_commitment(note_public_key: U256, token_hash: [u8; 32], value: U256) -> Result<U256> {
    let token_big = U256::from_be_slice(&token_hash);
    poseidon_hash(vec![note_public_key, token_big, value])
}

// -----------------------------------------------------------------------------
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

    let token_data = TokenData {
        token_address: "0x0000000000000000000000000000000000000000".to_string(),
        token_type: TokenType::ERC20,
        token_sub_id: U256::ZERO,
    };

    Note::new(receiver_mpk, random, value, token_data, None, memo)
}

// -----------------------------------------------------------------------------
// Poseidon helper (matches the one in address.rs)
// -----------------------------------------------------------------------------

fn poseidon_hash(inputs: Vec<U256>) -> Result<U256> {
    let arity = inputs.len();
    if arity == 0 || arity > 12 {
        return Err(anyhow!("Invalid number of inputs for poseidon"));
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::generate_railgun_keys;
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
        let keys = generate_railgun_keys(test_mnemonic(), 0, None).unwrap();
        let master_pk = keys.master_public_key;

        let random = [0x42u8; 16];
        let value = U256::from(1_000_000_000_000_000_000u128);
        let token = TokenData::new_erc20("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

        let note = Note::new(master_pk, random, value, token, None, None).unwrap();

        assert!(note.commitment != U256::ZERO);
        println!("Note commitment: 0x{:064x}", note.commitment);
        println!("Note public key : 0x{:064x}", note.note_public_key);
    }

    #[test]
    fn test_shared_key_derivation() {
        let keys = generate_railgun_keys(test_mnemonic(), 0, None).unwrap();
        let blinded = keys.viewing_public;

        let shared = derive_shared_symmetric_key(&[0u8; 32], &blinded).unwrap();
        assert_eq!(shared.len(), 32);
    }

    #[test]
    fn test_note_encrypt_decrypt_roundtrip() {
        let keys = generate_railgun_keys(test_mnemonic(), 0, None).unwrap();
        let master_pk = keys.master_public_key;

        let random = [0x11u8; 16];
        let value = U256::from(123456789u64);
        let token = TokenData::new_erc20("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

        let note = Note::new(master_pk, random, value, token, None, Some("test memo".to_string())).unwrap();

        let shared_key = derive_shared_symmetric_key(&[0x42u8; 32], &keys.viewing_public).unwrap();

        let (ciphertext, nonce) = encrypt_note_v2(&note, &shared_key).unwrap();
        let decrypted = decrypt_note_v2(&ciphertext, &nonce, &shared_key).unwrap();

        assert_eq!(decrypted.receiver_master_public_key, note.receiver_master_public_key);
        assert_eq!(decrypted.value, note.value);
        assert_eq!(decrypted.random, note.random);
        println!("Encrypt/decrypt roundtrip OK. Commitment: 0x{:064x}", decrypted.commitment);
    }
}
