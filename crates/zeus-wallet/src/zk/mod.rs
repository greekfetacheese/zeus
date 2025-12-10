use alloy_primitives::U256;
use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use bech32::{ToBase32, Variant};
use hmac::{Hmac, Mac};
use jubjub::{AffinePoint, ExtendedPoint, Fr as JubJubScalar, SubgroupPoint};
use k256::elliptic_curve::Group;
use light_poseidon::{Poseidon, PoseidonHasher};
use secure_types::{SecureArray, Zeroize};
use sha2::{Digest, Sha512};
use std::error::Error;
use zeus_bip32::ChainCode;

const PREFIX: &str = "0zk";
const _ADDRESS_LENGTH_LIMIT: usize = 127;
const ALL_CHAINS_NETWORK_ID: &str = "ffffffffffffffff";
const RAILGUN_XOR: [u8; 8] = [b'r', b'a', b'i', b'l', b'g', b'u', b'n', 0];
const CURVE_SEED: &[u8] = b"ed25519 seed";
const ADDRESS_VERSION: u8 = 1;

type Key = SecureArray<u8, 32>;

#[derive(Clone, Copy)]
pub struct Chain {
   pub type_: u8,
   pub id: u64,
}

impl From<u64> for Chain {
   fn from(id: u64) -> Self {
      Self { type_: 0, id }
   }
}

pub struct AddressData {
   pub master_public_key: U256,
   pub viewing_public_key: [u8; 32],
   pub chain: Option<Chain>,
   pub version: u8,
}

fn hmac_sha512(key: &[u8], data: &[u8]) -> (Key, ChainCode) {
   let mut mac = Hmac::<Sha512>::new_from_slice(key).unwrap();
   mac.update(data);

   let mut result = mac.finalize().into_bytes();

   let mut left = [0u8; 32];
   let mut right = [0u8; 32];
   left.copy_from_slice(&result[..32]);
   right.copy_from_slice(&result[32..]);

   result.zeroize();

   let key = Key::from_slice_mut(&mut left).unwrap();
   let chain_code = ChainCode::from_slice_mut(&mut right).unwrap();

   (key, chain_code)
}

fn derive_private_key(seed: &[u8], path: &str) -> Result<Key, Box<dyn Error>> {
   // Master node
   let (mut current_key, mut current_chain_code) = hmac_sha512(CURVE_SEED, seed);

   // Parse and derive for each segment in path (skip 'm')
   let segments: Vec<&str> = path.split('/').skip(1).collect();
   for segment in segments {
      let is_hardened = segment.ends_with("'");
      let index_str = segment.trim_end_matches("'");
      let mut index: u32 = index_str.parse()?;

      if is_hardened {
         index += 0x80000000; // Hardened index (MSB set)
      }

      let index_bytes = index.to_be_bytes();

      // Data = 0x00 + current_key + index_bytes
      let mut data = vec![0u8];
      current_key.unlock(|slice| data.extend_from_slice(slice));
      data.extend_from_slice(&index_bytes);

      let (key, chain_code) = current_chain_code.data.unlock(|code| {
         let (new_key, new_code) = hmac_sha512(code, &data);
         (new_key, new_code)
      });

      data.zeroize();

      current_key = key;
      current_chain_code = chain_code;
   }

   Ok(current_key)
}

// Compute Poseidon hash on a vec of bigints (reduced to BN254 Fr)
fn poseidon_hash(inputs: Vec<U256>) -> Result<U256, Box<dyn Error>> {
   let arity = inputs.len();
   if arity == 0 || arity > 12 {
      return Err("Invalid number of inputs for Poseidon".into());
   }
   let mut fr_inputs = Vec::with_capacity(arity);
   for input in inputs {
      let bytes = input.to_be_bytes::<32>();
      let fr = Fr::from_be_bytes_mod_order(&bytes);
      fr_inputs.push(fr);
   }
   let mut poseidon = Poseidon::<Fr>::new_circom(arity)?;
   let hash_fr = poseidon.hash(&fr_inputs)?;
   let hash_big = hash_fr.into_bigint();
   let be_bytes = hash_big.to_bytes_be();
   let mut padded_bytes = [0u8; 32];
   let start = 32 - be_bytes.len();
   padded_bytes[start..].copy_from_slice(&be_bytes);
   let result = U256::from_be_bytes(padded_bytes);
   Ok(result)
}

// Generate AddressData from mnemonic (index defaults to 0 for first wallet)
pub fn generate_address_data(
   seed: SecureArray<u8, 64>,
   index: u32,
   chain: Option<Chain>,
) -> Result<AddressData, Box<dyn Error>> {
   // Derive paths with index
   let spending_path = format!("m/44'/1984'/0'/0'/{}'", index);
   let viewing_path = format!("m/420'/1984'/0'/0'/{}'", index);

   let (spending_priv, viewing_priv) = seed.unlock(|seed_bytes| {
      let spending_priv =
         derive_private_key(seed_bytes, &spending_path).expect("Derive spending key");
      let viewing_priv = derive_private_key(seed_bytes, &viewing_path).expect("Derive viewing key");
      (spending_priv, viewing_priv)
   });

   // Compute spending public key on BabyJubJub (EdDSA-style: hash, clamp, mul)
   let mut h = spending_priv.unlock(|priv_bytes| Sha512::digest(priv_bytes));
   let mut scalar_bytes = [0u8; 32];
   scalar_bytes.copy_from_slice(&h[0..32]);

   h.zeroize();
   scalar_bytes[0] &= 248; // Clamp low bits
   scalar_bytes[31] &= 127; // Clamp high bits
   scalar_bytes[31] |= 64;

   let mut wide = [0u8; 64];
   wide[0..32].copy_from_slice(&scalar_bytes);
   let scalar = JubJubScalar::from_bytes_wide(&wide);

   scalar_bytes.zeroize();
   wide.zeroize();

   let point = SubgroupPoint::generator() * scalar;
   let extended = ExtendedPoint::from(point);
   let affine = AffinePoint::from(extended);

   let x = affine.get_u();
   let y = affine.get_v();

   let x_bytes = x.to_bytes(); // LE bytes
   let y_bytes = y.to_bytes(); // LE bytes

   let spend_x = U256::from_le_bytes(x_bytes);
   let spend_y = U256::from_le_bytes(y_bytes);

   // Compute viewing public key on BabyJubJub (same as spending)
   let mut h_view = viewing_priv.unlock(|priv_bytes| Sha512::digest(priv_bytes));
   let mut scalar_bytes_view = [0u8; 32];
   scalar_bytes_view.copy_from_slice(&h_view[0..32]);

   h_view.zeroize();
   scalar_bytes_view[0] &= 248; // Clamp low bits
   scalar_bytes_view[31] &= 127; // Clamp high bits
   scalar_bytes_view[31] |= 64;

   let mut wide_view = [0u8; 64];
   wide_view[0..32].copy_from_slice(&scalar_bytes_view);

   let scalar_view = JubJubScalar::from_bytes_wide(&wide_view);
   scalar_bytes_view.zeroize();
   wide_view.zeroize();

   let point_view = SubgroupPoint::generator() * scalar_view;
   let extended_view = ExtendedPoint::from(point_view);
   let affine_view = AffinePoint::from(extended_view);

   // let viewing_x = affine_view.get_u();
   // let viewing_y = affine_view.get_v();

   // let viewing_x_bytes = viewing_x.to_bytes();
   // let viewing_y_bytes = viewing_y.to_bytes();
   // let viewing_pub_x = U256::from_le_bytes(viewing_x_bytes);
   // let viewing_pub_y = U256::from_le_bytes(viewing_y_bytes);

   let mut viewing_public_key = [0u8; 32];
   viewing_public_key.copy_from_slice(&affine_view.to_bytes()); // Compressed public key

   // Compute nullifying key: Poseidon([viewing_priv as U256])
   let mut view_priv_slice = viewing_priv.unlock(|bytes| {
      let mut slice = [0u8; 32];
      slice.copy_from_slice(bytes);
      slice
   });

   let view_priv_u256 = U256::from_le_bytes(view_priv_slice);
   view_priv_slice.zeroize();

   let nullifying_key = poseidon_hash(vec![view_priv_u256]).expect("Poseidon hash");

   // Compute master public key: Poseidon([spend_x, spend_y, nullifying_key])
   let master_public_key =
      poseidon_hash(vec![spend_x, spend_y, nullifying_key]).expect("Poseidon hash");

   Ok(AddressData {
      master_public_key,
      viewing_public_key,
      chain,
      version: ADDRESS_VERSION,
   })
}
fn chain_to_network_id(chain: Option<Chain>) -> String {
   match chain {
      Some(c) => format!("{:02x}{:014x}", c.type_, c.id),
      None => ALL_CHAINS_NETWORK_ID.to_string(),
   }
}

fn xor_network_id(network_id: &str) -> Result<String, Box<dyn Error>> {
   let mut chain_buf = hex::decode(network_id)?;
   if chain_buf.len() != 8 {
      return Err("Invalid network ID length".into());
   }
   for i in 0..8 {
      chain_buf[i] ^= RAILGUN_XOR[i];
   }
   Ok(hex::encode(chain_buf))
}

pub fn encode_address(data: &AddressData) -> Result<String, Box<dyn Error>> {
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
      return Err("Invalid address bytes length".into());
   }

   let base32_data = address_bytes.to_base32();
   let address = bech32::encode(PREFIX, base32_data, Variant::Bech32m)?;

   Ok(address)
}

#[cfg(test)]
mod test {
   use super::super::wallet::*;
   use super::*;
   use bip39::{Language, Mnemonic};
   use secure_types::SecureString;

   const TEST_M_COST: u32 = 16_000;
   const TEST_T_COST: u32 = 5;
   const TEST_P_COST: u32 = 4;

   #[test]
   fn test_with_mnemonic() {
      let seed_phrase = "boil belt beef hunt cruel lady code dance double city young rule very sight roast make eight travel tattoo mixed you color update double";
      let _expected_address = "0zk1qy9r469tey0ptmp7unlph80w5aw3hf8z39une75a2ewd8vlmgvs2hrv7j6fe3z53lugdcpevcmd84mghtk07gd66s4qw452llcuzap2934nyh45jxz4ry55rq67";

      let mnemonic = Mnemonic::parse_in(Language::English, seed_phrase).unwrap();
      let seed = mnemonic.to_seed("");

      let sec_seed = SecureArray::from_slice(&seed).unwrap();
      let address_data = generate_address_data(sec_seed, 0, None).unwrap();

      let encoded_address = encode_address(&address_data).unwrap();
      eprintln!("Encoded Address: {}", encoded_address);
   }

   #[test]
   fn test_with_derived_seed() {
      let username = SecureString::from("username");
      let password = SecureString::from("password");

      let seed = derive_seed(
         &username,
         &password,
         TEST_M_COST,
         TEST_T_COST,
         TEST_P_COST,
      )
      .unwrap();

      let mut hd_wallet = SecureHDWallet::new_from_seed(None, seed.clone());
      eprintln!(
         "Master Wallet Address: {}",
         hd_wallet.master_wallet.address()
      );

      let chain = Chain::from(1);
      let seed = hd_wallet.master_wallet.full_key().unwrap();
      let address_data = generate_address_data(seed, 0, Some(chain)).unwrap();

      let encoded_address = encode_address(&address_data).unwrap();
      eprintln!("Master Wallet Zk Address: {}", encoded_address);

      for i in 0..10 {
         let name = format!("Child Wallet {}", i);
         hd_wallet.derive_child(name).unwrap();
      }

      for child in &hd_wallet.children {
         let seed = child.full_key().unwrap();

         let data = generate_address_data(seed, 0, Some(chain)).unwrap();
         let encoded_address = encode_address(&data).unwrap();
         eprintln!("{} Zk Address: {}", child.name, encoded_address);
      }
   }
}
