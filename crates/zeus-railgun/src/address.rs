use alloy_primitives::U256;
use anyhow::anyhow;
use ark_bn254::Fr;
use ark_ed_on_bn254::{EdwardsAffine, EdwardsProjective, Fq, Fr as BabyJubScalar};
use ark_ff::{BigInteger, One, PrimeField, Zero};
use bech32::{FromBase32, ToBase32, Variant};
use blake2::{Blake2b512, Digest};
use curve25519_dalek::{EdwardsPoint, Scalar, constants::ED25519_BASEPOINT_TABLE};
use hmac::{Hmac, Mac};
use lazy_static::lazy_static;
use light_poseidon::{Poseidon, PoseidonHasher};
use num_bigint::BigUint;
use secure_types::{SecureArray, Zeroize};
use sha2::Sha512;
use std::str::FromStr;

#[allow(dead_code)]
const GEN_X_HEX: &str = "023343e3445b673d38bcba38f25645adb494b1255b1162bb40f41a59f4d4b45e";

#[allow(dead_code)]
const GEN_Y_HEX: &str = "0c19139cb84c680a6e14116da06056174a0cfa121e6e5c2450f87d64fc000001";

const PREFIX: &str = "0zk";
const _ADDRESS_LENGTH_LIMIT: usize = 127;
const ALL_CHAINS_NETWORK_ID: &str = "ffffffffffffffff";
const RAILGUN_XOR: [u8; 8] = [b'r', b'a', b'i', b'l', b'g', b'u', b'n', 0];
const CURVE_SEED: &[u8] = b"babyjubjub seed";
const ADDRESS_VERSION: u8 = 1;

type Key = SecureArray<u8, 32>;
type ChainCode = SecureArray<u8, 32>;
type Key64 = SecureArray<u8, 64>;

#[derive(Clone, Copy)]
struct BabyJubPoint {
   x: Fr,
   y: Fr,
}

const A: u64 = 168700;
const D: u64 = 168696;

lazy_static! {
   static ref BABYJUB_A: Fr = Fr::from(A);
   static ref BABYJUB_D: Fr = Fr::from(D);
   static ref BASE8_X: Fr =
      Fr::from_str("5299619240641551281634865583518297030282874472190772894086521144482721001553")
         .expect("Invalid base8 x");
   static ref BASE8_Y: Fr =
      Fr::from_str("16950150798460657717958625567821834550301663161624707787222815936182638968203")
         .expect("Invalid base8 y");
   static ref BASE8: BabyJubPoint = BabyJubPoint {
      x: *BASE8_X,
      y: *BASE8_Y
   };
}

fn add_point(a: BabyJubPoint, b: BabyJubPoint) -> BabyJubPoint {
   let beta = a.x * b.y;
   let gamma = a.y * b.x;
   let delta = (a.y - (*BABYJUB_A * a.x)) * (b.x + b.y);
   let tau = beta * gamma;
   let dtau = *BABYJUB_D * tau;

   let x = (beta + gamma) / (Fr::one() + dtau);
   let y = (delta + (*BABYJUB_A * beta - gamma)) / (Fr::one() - dtau);

   BabyJubPoint { x, y }
}

fn mul_point_escalar(base: BabyJubPoint, mut e: num_bigint::BigUint) -> BabyJubPoint {
   let mut res = BabyJubPoint {
      x: Fr::zero(),
      y: Fr::one(),
   };

   let mut exp = base;

   while !e.is_zero() {
      if (&e & BigUint::one()) == BigUint::one() {
         res = add_point(res, exp);
      }
      exp = add_point(exp, exp);
      e >>= 1;
   }
   res
}

lazy_static! {
   static ref BABYJUBJUB_GENERATOR: EdwardsProjective = {
      let x_bytes = hex::decode(GEN_X_HEX).expect("Invalid hex for gen_x");
      let y_bytes = hex::decode(GEN_Y_HEX).expect("Invalid hex for gen_y");

      let gen_x = Fq::from_be_bytes_mod_order(&x_bytes);
      let gen_y = Fq::from_be_bytes_mod_order(&y_bytes);

      let affine = EdwardsAffine { x: gen_x, y: gen_y };
      EdwardsProjective::from(affine)
   };
}

#[derive(Debug, Clone, Copy)]
pub struct Chain {
   pub type_: u8,
   pub id: u64,
}

impl From<u64> for Chain {
   fn from(id: u64) -> Self {
      Self { type_: 0, id }
   }
}

#[derive(Debug, Clone)]
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

fn derive_private_key(seed: &[u8], path: &str) -> Result<Key, anyhow::Error> {
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

pub fn poseidon_hash(inputs: Vec<U256>) -> Result<U256, anyhow::Error> {
   let arity = inputs.len();

   if arity == 0 || arity > 12 {
      return Err(anyhow!("Invalid number of inputs"));
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
   let mut padded_bytes = [0u8; 32];
   let start = 32 - be_bytes.len();
   padded_bytes[start..].copy_from_slice(&be_bytes);
   let result = U256::from_be_bytes(padded_bytes);
   Ok(result)
}

pub fn generate_address_data(
   seed: SecureArray<u8, 64>,
   index: u32,
   _chain: Option<Chain>,
) -> Result<AddressData, anyhow::Error> {
   let (spend_x, spend_y) = compute_spending_key(&seed, index).expect("Spending key");
   let (viewing_public_key, nullifying_key) =
      compute_viewing_key(&seed, index).expect("Viewing key");

   // Compute master public key
   let master_public_key = poseidon_hash(vec![spend_x, spend_y, nullifying_key])
      .map_err(|e| anyhow!("Poseidon hash {}", e))?;

   Ok(AddressData {
      master_public_key,
      viewing_public_key,
      chain: _chain,
      version: ADDRESS_VERSION,
   })
}

type SpendX = U256;
type SpendY = U256;
type NullifyingKey = U256;
type ViewPublicKey = [u8; 32];

fn compute_viewing_key(
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

fn compute_spending_key(seed: &Key64, index: u32) -> Result<(SpendX, SpendY), anyhow::Error> {
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

   // let scalar = BabyJubScalar::from_le_bytes_mod_order(&scalar_bytes);
   // let scalar_shifted = scalar.into_bigint() >> 3;
   // let scalar = BabyJubScalar::from_bigint(scalar_shifted).unwrap();

   // scalar_bytes.zeroize();

   // let generator = *BABYJUBJUB_GENERATOR;
   // eprintln!("GENERATOR: {:?}", generator);
   let point = mul_point_escalar(*BASE8, scalar.into_bigint().into());
   // let point = generator.mul(scalar);
   // let affine = EdwardsAffine::from(point);
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

pub fn encode_address(data: &AddressData) -> Result<String, anyhow::Error> {
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

pub fn decode_address(address: &str) -> Result<AddressData, anyhow::Error> {
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

   Ok(AddressData {
      version,
      master_public_key,
      viewing_public_key: viewing_bytes,
      chain: None,
   })
}

pub fn get_broadcaster_viewing_key(railgun_address: &str) -> Result<[u8; 32], anyhow::Error> {
   let data = decode_address(railgun_address)?;
   Ok(data.viewing_public_key)
}

/// Full set of Railgun keys derived from a seed + index.
/// This is what the engine will use for note decryption, nullifier creation, etc.
#[derive(Clone)]
pub struct RailgunKeys {
   /// Raw spending private key (before final BabyJub clamping for scalar).
   pub spending_private: Key,
   /// Spending public key point (x, y) on BabyJubJub.
   pub spending_public: (U256, U256),
   /// Raw viewing private key.
   pub viewing_private: Key,
   /// Viewing public key (compressed Ed25519 point, 32 bytes) - used in 0zk address.
   pub viewing_public: [u8; 32],
   /// Nullifying key = Poseidon(viewing private).
   pub nullifying_key: U256,
   /// Master public key = Poseidon(spend_x, spend_y, nullifying_key).
   pub master_public_key: U256,
}

/// Derive the raw spending private key (before clamping).
pub fn derive_spending_private_key(seed: &Key64, index: u32) -> Result<Key, anyhow::Error> {
   let spending_path = format!("m/44'/1984'/0'/0'/{}'", index);
   seed
      .unlock(|seed_bytes| derive_private_key(seed_bytes, &spending_path))
      .map_err(|e| anyhow!("Derive spending private key: {}", e))
}

/// Derive the raw viewing private key.
pub fn derive_viewing_private_key(seed: &Key64, index: u32) -> Result<Key, anyhow::Error> {
   let viewing_path = format!("m/420'/1984'/0'/0'/{}'", index);
   seed
      .unlock(|seed_bytes| derive_private_key(seed_bytes, &viewing_path))
      .map_err(|e| anyhow!("Derive viewing private key: {}", e))
}

/// Generate the complete set of Railgun keys (private + public) for a given seed and index.
/// This is the recommended entry point for the engine.
pub fn generate_railgun_keys(
   seed: SecureArray<u8, 64>,
   index: u32,
   _chain: Option<Chain>,
) -> Result<RailgunKeys, anyhow::Error> {
   let spending_priv = derive_spending_private_key(&seed, index)?;
   let viewing_priv = derive_viewing_private_key(&seed, index)?;

   let (spend_x, spend_y) = compute_spending_key(&seed, index)?;
   let (viewing_public, nullifying_key) = compute_viewing_key(&seed, index)?;

   let master_public_key = poseidon_hash(vec![spend_x, spend_y, nullifying_key])?;

   Ok(RailgunKeys {
      spending_private: spending_priv,
      spending_public: (spend_x, spend_y),
      viewing_private: viewing_priv,
      viewing_public,
      nullifying_key,
      master_public_key,
   })
}

pub fn babyjub_shared_secret(
   random_priv: &[u8; 32],
   broadcaster_viewing_pub: &[u8; 32],
) -> Result<([u8; 32], [u8; 32]), anyhow::Error> {
   // Re-implementation of the ECDH using the crate's BabyJub primitives (clamped priv, mul)
   let mut priv_clamped = [0u8; 32];
   priv_clamped.copy_from_slice(random_priv);
   priv_clamped[0] &= 248;
   priv_clamped[31] &= 127;
   priv_clamped[31] |= 64;

   // Treat the viewing pub as point coords for mul (simplified from prior working version)
   let pub_x = Fq::from_be_bytes_mod_order(broadcaster_viewing_pub);
   let pub_y = Fq::from_be_bytes_mod_order(broadcaster_viewing_pub);
   let pub_point = BabyJubPoint { x: pub_x, y: pub_y };

   let priv_big = num_bigint::BigUint::from_bytes_be(&priv_clamped);
   let shared_point = mul_point_escalar(pub_point, priv_big.clone());

   let mut shared = [0u8; 32];
   let sx = shared_point.x.into_bigint().to_bytes_be();
   if sx.len() >= 32 {
      shared.copy_from_slice(&sx[sx.len() - 32..]);
   }

   // random pub for the ECDH pair
   let base = *BASE8;
   let rpub_point = mul_point_escalar(base, priv_big);
   let mut rpub = [0u8; 32];
   let rx = rpub_point.x.into_bigint().to_bytes_be();
   if rx.len() >= 32 {
      rpub.copy_from_slice(&rx[rx.len() - 32..]);
   }

   Ok((rpub, shared))
}

#[cfg(test)]
mod test {
   use super::*;
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
      let address_data = generate_address_data(sec_seed.clone(), 0, None).unwrap();

      let encoded_address = encode_address(&address_data).unwrap();
      eprintln!("Encoded Address: {}", encoded_address);
      println!("Railway Address: {}", railway_address);

      // Test new full key derivation
      let keys = generate_railgun_keys(sec_seed, 0, None).expect("generate_railgun_keys failed");
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
      let address = generate_address_data(full_key, 0, None).unwrap();
      let encoded_address = encode_address(&address).unwrap();
      println!("Address: {}", encoded_address);
   }

   #[test]
   fn test_decode_specific_address() {
      let wallet = gen_wallet();
      let full_key = wallet.master_wallet.full_key().unwrap();
      let address_data = generate_address_data(full_key, 0, None).unwrap();
      let address = encode_address(&address_data).unwrap();

      let decoded = decode_address(&address).expect("decode_address failed");

      let viewing_key =
         get_broadcaster_viewing_key(&address).expect("get_broadcaster_viewing_key failed");
      println!(
         "Viewing public key (hex): {}",
         hex::encode(viewing_key)
      );

      assert_eq!(decoded.version, address_data.version);
      assert_eq!(
         viewing_key.len(),
         address_data.viewing_public_key.len()
      );
      assert_eq!(
         decoded.viewing_public_key,
         address_data.viewing_public_key
      );
   }
}
