use alloy_primitives::U256;
use anyhow::anyhow;
use ark_ed_on_bn254::Fr as BabyJubScalar;
use ark_ff::{BigInteger, PrimeField};
use bech32::{FromBase32, ToBase32, Variant};
use blake2::{Blake2b512, Digest};
use curve25519_dalek::{EdwardsPoint, Scalar, constants::ED25519_BASEPOINT_TABLE};
use num_bigint::BigUint;
use secure_types::{SecureArray, Zeroize};

use crate::crypto::*;
use crate::{Chain, Key32, Key64};

const PREFIX: &str = "0zk";
const _ADDRESS_LENGTH_LIMIT: usize = 127;
const ALL_CHAINS_NETWORK_ID: &str = "ffffffffffffffff";
const RAILGUN_XOR: [u8; 8] = [b'r', b'a', b'i', b'l', b'g', b'u', b'n', 0];
const CURVE_SEED: &[u8] = b"babyjubjub seed";
const ADDRESS_VERSION: u8 = 1;

type SpendX = U256;
type SpendY = U256;
type NullifyingKey = U256;
type ViewPublicKey = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailgunAddress {
   pub address: String,
   pub master_public_key: U256,
   pub viewing_public_key: [u8; 32],
   pub chain: Option<Chain>,
   pub version: u8,
}

impl RailgunAddress {
   pub fn new(
      seed: SecureArray<u8, 64>,
      index: u32,
      _chain: Option<Chain>,
   ) -> Result<Self, anyhow::Error> {
      let (spend_x, spend_y) = compute_spending_key(&seed, index).expect("Spending key");
      let (viewing_public_key, nullifying_key) =
         compute_viewing_key(&seed, index).expect("Viewing key");

      // Compute master public key
      let master_public_key = poseidon_hash(vec![spend_x, spend_y, nullifying_key])
         .map_err(|e| anyhow!("Poseidon hash {}", e))?;

      let mut data = RailgunAddress {
         master_public_key,
         viewing_public_key,
         chain: _chain,
         version: ADDRESS_VERSION,
         address: String::new(),
      };

      data.address = encode_address(&data)?;

      Ok(data)
   }

   pub fn from_zk_address(address: &str) -> Result<Self, anyhow::Error> {
      // Decode the FULL address string (e.g. "0zk1qys..."), do NOT strip the "0zk" prefix.
      // bech32::decode expects the complete bech32m string including HRP + "1" + data.
      let (hrp, data, variant) = bech32::decode(address)?;

      if hrp.to_lowercase() != "0zk" {
         return Err(anyhow!("Invalid HRP for Railgun address"));
      }
      if variant != Variant::Bech32m {
         return Err(anyhow!("Expected Bech32m address"));
      }

      // Use the crate's FromBase32 (symmetric to the to_base32 used in encode_address)
      let bytes: Vec<u8> = Vec::<u8>::from_base32(&data)
         .map_err(|e| anyhow!("bech32 from_base32 conversion failed: {:?}", e))?;

      if bytes.len() != 73 {
         return Err(anyhow!(
            "Invalid decoded address length, expected 73 bytes, got {}",
            bytes.len()
         ));
      }

      let version = bytes[0];
      let master_bytes: [u8; 32] = bytes[1..33].try_into().map_err(|_| anyhow!("bad master"))?;
      let _network_bytes: [u8; 8] = bytes[33..41].try_into().map_err(|_| anyhow!("bad network"))?;
      let viewing_bytes: [u8; 32] = bytes[41..73].try_into().map_err(|_| anyhow!("bad viewing"))?;

      let master_public_key = U256::from_be_bytes(master_bytes);

      Ok(Self {
         address: address.to_string(),
         version,
         master_public_key,
         viewing_public_key: viewing_bytes,
         chain: None,
      })
   }
}

pub fn encode_address(data: &RailgunAddress) -> Result<String, anyhow::Error> {
   let version_hex = format!("{:02x}", data.version);
   let master_hex = hex::encode(data.master_public_key.to_be_bytes::<32>());
   let network_hex = xor_network_id(&chain_to_network_id(data.chain))?;
   let viewing_hex = hex::encode(data.viewing_public_key);

   let address_string = format!(
      "{}{}{}{}",
      version_hex, master_hex, network_hex, viewing_hex
   );

   let address_bytes = hex::decode(&address_string)?;

   if address_bytes.len() != 73 {
      return Err(anyhow!(
         "Invalid address bytes length, expected 73 got {}",
         address_bytes.len()
      ));
   }

   let base32_data = address_bytes.to_base32();
   let address = bech32::encode(PREFIX, base32_data, Variant::Bech32m)?;

   Ok(address)
}

fn derive_private_key(seed: &[u8], path: &str) -> Result<Key32, anyhow::Error> {
   let (mut current_key, mut current_chain_code) = hmac_sha512(CURVE_SEED, seed);

   // Parse and derive for each segment in path (skip 'm')
   let segments: Vec<&str> = path.split('/').skip(1).collect();
   for segment in segments {
      let index_str = segment.trim_end_matches("'");
      let mut index: u32 = index_str.parse()?;

      index += 0x80000000;

      let index_bytes = index.to_be_bytes();

      // Data = 0x00 + current_key + index_bytes
      let mut data = vec![0u8];
      current_key.unlock(|slice| data.extend_from_slice(slice));
      data.extend_from_slice(&index_bytes);

      let (key, chain_code) = current_chain_code.unlock(|code| {
         let (new_key, new_code) = hmac_sha512(code, &data);
         (new_key, new_code)
      });

      data.zeroize();

      current_key = key;
      current_chain_code = chain_code;
   }

   Ok(current_key)
}

pub fn compute_viewing_key(
   seed: &Key64,
   index: u32,
) -> Result<(ViewPublicKey, NullifyingKey), anyhow::Error> {
   let viewing_path = format!("m/420'/1984'/0'/0'/{}'", index);
   let viewing_priv_res = seed.unlock(|seed_bytes| derive_private_key(seed_bytes, &viewing_path));

   let viewing_priv = viewing_priv_res.map_err(|e| anyhow!("Derive viewing key {}", e))?;
   let mut view_priv_bytes = viewing_priv.unlock(|bytes| {
      let mut slice = [0u8; 32];
      slice.copy_from_slice(bytes);
      slice
   });

   // Compute scalar per Ed25519 spec (SHA-512 hash, clamp lower 32 bytes)
   let h_view = sha2::Sha512::digest(&view_priv_bytes);
   let mut scalar_bytes: [u8; 32] = [0; 32];
   scalar_bytes.copy_from_slice(&h_view[0..32]);
   scalar_bytes[0] &= 248;
   scalar_bytes[31] &= 127;
   scalar_bytes[31] |= 64;

   let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
   let point: EdwardsPoint = ED25519_BASEPOINT_TABLE * &scalar;

   let viewing_public_key = point.compress().to_bytes();
   let view_priv_u256 = U256::from_be_bytes(view_priv_bytes);
   view_priv_bytes.zeroize();

   let nullifying_key = poseidon_hash(vec![view_priv_u256])?;

   Ok((viewing_public_key, nullifying_key))
}

pub fn compute_spending_key(seed: &Key64, index: u32) -> Result<(SpendX, SpendY), anyhow::Error> {
   let spending_path = format!("m/44'/1984'/0'/0'/{}'", index);

   let spending_priv_res = seed.unlock(|seed_bytes| derive_private_key(seed_bytes, &spending_path));
   let spending_priv = spending_priv_res.map_err(|e| anyhow!("Derive spending key {}", e))?;

   let mut h = spending_priv.unlock(|priv_bytes| {
      let mut hasher = Blake2b512::new();
      hasher.update(priv_bytes);
      hasher.finalize()
   });

   let mut scalar_bytes = [0u8; 32];
   scalar_bytes.copy_from_slice(&h[0..32]);
   h.zeroize();

   scalar_bytes[0] &= 248; // Clamp low bits
   scalar_bytes[31] &= 127; // Clamp high bits
   scalar_bytes[31] |= 64;

   // LE bigint
   let scalar_big = BigUint::from_bytes_le(&scalar_bytes);
   let scalar_shifted: BigUint = scalar_big >> 3;

   // Convert back to Fr
   let scalar_bytes_shifted = scalar_shifted.to_bytes_le();
   let mut padded = [0u8; 32];
   padded[0..scalar_bytes_shifted.len()].copy_from_slice(&scalar_bytes_shifted);
   let scalar = BabyJubScalar::from_le_bytes_mod_order(&padded);

   scalar_bytes.zeroize();
   padded.zeroize();

   let point = mul_point_escalar(*BASE8, scalar.into_bigint().into());
   let affine = point;

   let mut x_bytes = affine.x.into_bigint().to_bytes_le();
   let mut y_bytes = affine.y.into_bigint().to_bytes_le();

   let mut spend_x_bytes = [0u8; 32];
   let mut spend_y_bytes = [0u8; 32];

   spend_x_bytes[0..x_bytes.len()].copy_from_slice(&x_bytes);
   spend_y_bytes[0..y_bytes.len()].copy_from_slice(&y_bytes);

   let spend_x = U256::from_le_bytes(spend_x_bytes);
   let spend_y = U256::from_le_bytes(spend_y_bytes);

   x_bytes.zeroize();
   y_bytes.zeroize();
   spend_x_bytes.zeroize();
   spend_y_bytes.zeroize();

   Ok((spend_x, spend_y))
}

fn chain_to_network_id(chain: Option<Chain>) -> String {
   match chain {
      Some(c) => format!("{:02x}{:014x}", c.type_, c.id),
      None => ALL_CHAINS_NETWORK_ID.to_string(),
   }
}

fn xor_network_id(network_id: &str) -> Result<String, anyhow::Error> {
   let mut chain_buf = hex::decode(network_id)?;

   if chain_buf.len() != 8 {
      return Err(anyhow!("Invalid network ID length"));
   }

   for i in 0..8 {
      chain_buf[i] ^= RAILGUN_XOR[i];
   }

   Ok(hex::encode(chain_buf))
}

/// Derive the raw spending private key (before clamping).
pub fn derive_spending_private_key(seed: &Key64, index: u32) -> Result<Key32, anyhow::Error> {
   let spending_path = format!("m/44'/1984'/0'/0'/{}'", index);
   seed
      .unlock(|seed_bytes| derive_private_key(seed_bytes, &spending_path))
      .map_err(|e| anyhow!("Derive spending private key: {}", e))
}

/// Derive the raw viewing private key.
pub fn derive_viewing_private_key(seed: &Key64, index: u32) -> Result<Key32, anyhow::Error> {
   let viewing_path = format!("m/420'/1984'/0'/0'/{}'", index);
   seed
      .unlock(|seed_bytes| derive_private_key(seed_bytes, &viewing_path))
      .map_err(|e| anyhow!("Derive viewing private key: {}", e))
}

#[cfg(test)]
mod test {
   use super::*;
   use crate::keys::RailgunKeys;
   use bip39::{Language, Mnemonic};
   use secure_types::SecureString;
   use zeus_wallet::*;

   fn gen_wallet() -> SecureHDWallet {
      let username = "dev";
      let password = "dev";

      let username = SecureString::from(username);
      let password = SecureString::from(password);

      let m_cost = 2048;
      let t_cost = 1;
      let p_cost = 4;

      let seed = derive_seed(&username, &password, m_cost, t_cost, p_cost).unwrap();
      let wallet = SecureHDWallet::new_from_seed(None, seed);
      wallet
   }

   #[test]
   fn test_against_railway() {
      // Generated from Railway wallet
      let seed_phrase = "boil belt beef hunt cruel lady code dance double city young rule very sight roast make eight travel tattoo mixed you color update double";
      let railway_address = "0zk1qy9r469tey0ptmp7unlph80w5aw3hf8z39une75a2ewd8vlmgvs2hrv7j6fe3z53lugdcpevcmd84mghtk07gd66s4qw452llcuzap2934nyh45jxz4ry55rq67";

      let mnemonic = Mnemonic::parse_in(Language::English, seed_phrase).unwrap();
      let seed = mnemonic.to_seed("");

      let sec_seed = SecureArray::from_slice(&seed).unwrap();
      let address_data = RailgunAddress::new(sec_seed.clone(), 0, None).unwrap();

      let encoded_address = encode_address(&address_data).unwrap();
      eprintln!("Encoded Address: {}", encoded_address);
      println!("Railway Address: {}", railway_address);

      // Test new full key derivation
      let keys = RailgunKeys::new(sec_seed, 0).expect("generate_railgun_keys failed");
      assert_eq!(
         keys.viewing_public,
         address_data.viewing_public_key
      );
      assert_eq!(
         keys.master_public_key,
         address_data.master_public_key
      );
      // private keys should be 32 bytes when unlocked
      let _ = keys.spending_private.unlock(|b| assert_eq!(b.len(), 32));
      let _ = keys.viewing_private.unlock(|b| assert_eq!(b.len(), 32));
   }

   #[test]
   fn test_zeus_wallet() {
      let wallet = gen_wallet();

      let full_key = wallet.master_wallet.full_key().unwrap();
      let address = RailgunAddress::new(full_key, 0, None).unwrap();
      let encoded_address = encode_address(&address).unwrap();
      println!("Address: {}", encoded_address);
   }

   #[test]
   fn test_decode_specific_address() {
      let wallet = gen_wallet();
      let full_key = wallet.master_wallet.full_key().unwrap();
      let address_data = RailgunAddress::new(full_key, 0, None).unwrap();

      let decoded = RailgunAddress::from_zk_address(&address_data.address).unwrap();

      assert_eq!(address_data, decoded);
   }
}
