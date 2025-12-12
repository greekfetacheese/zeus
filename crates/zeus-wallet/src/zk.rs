use alloy_primitives::U256;
use anyhow::anyhow;
use ark_bn254::Fr;
use ark_ed_on_bn254::{EdwardsAffine, EdwardsProjective, Fq, Fr as BabyJubScalar};
use ark_ff::{BigInteger, One, PrimeField, Zero};
use bech32::{ToBase32, Variant};
use blake2::{Blake2b512, Digest};
use curve25519_dalek::{EdwardsPoint, Scalar, constants::ED25519_BASEPOINT_TABLE};
use hmac::{Hmac, Mac};
use lazy_static::lazy_static;
use light_poseidon::{Poseidon, PoseidonHasher};
use num_bigint::BigUint;
use secure_types::{SecureArray, Zeroize};
use sha2::Sha512;
use std::str::FromStr;
use zeus_bip32::ChainCode;

const GEN_X_HEX: &str = "023343e3445b673d38bcba38f25645adb494b1255b1162bb40f41a59f4d4b45e";
const GEN_Y_HEX: &str = "0c19139cb84c680a6e14116da06056174a0cfa121e6e5c2450f87d64fc000001";

const PREFIX: &str = "0zk";
const _ADDRESS_LENGTH_LIMIT: usize = 127;
const ALL_CHAINS_NETWORK_ID: &str = "ffffffffffffffff";
const RAILGUN_XOR: [u8; 8] = [b'r', b'a', b'i', b'l', b'g', b'u', b'n', 0];
const CURVE_SEED: &[u8] = b"babyjubjub seed";
const ADDRESS_VERSION: u8 = 1;

type Key = SecureArray<u8, 32>;
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

fn poseidon_hash(inputs: Vec<U256>) -> Result<U256, anyhow::Error> {
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
   chain: Option<Chain>,
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
      chain,
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

   let generator = *BABYJUBJUB_GENERATOR;
   eprintln!("GENERATOR: {:?}", generator);
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
      let expected_address = "0zk1qy9r469tey0ptmp7unlph80w5aw3hf8z39une75a2ewd8vlmgvs2hrv7j6fe3z53lugdcpevcmd84mghtk07gd66s4qw452llcuzap2934nyh45jxz4ry55rq67";

      let mnemonic = Mnemonic::parse_in(Language::English, seed_phrase).unwrap();
      let seed = mnemonic.to_seed("");

      let sec_seed = SecureArray::from_slice(&seed).unwrap();
      let address_data = generate_address_data(sec_seed, 0, None).unwrap();

      let encoded_address = encode_address(&address_data).unwrap();
      eprintln!("Encoded Address: {}", encoded_address);
      assert_eq!(encoded_address, expected_address);
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

      let seed = hd_wallet.master_wallet.full_key().unwrap();
      let address_data = generate_address_data(seed, 0, None).unwrap();

      let master_expected = "0zk1qy4rnzwghzj2m88a8pqx0cudc7w7nvlyah6tvhtqfp379t3xav7qdrv7j6fe3z53ll5cu54qjtn5ql34u2m5ztyufcx07dtx6zg9d7gj97wpkmpxc0rrqymd3jc";

      let encoded_address = encode_address(&address_data).unwrap();
      eprintln!("Master Wallet Zk Address: {}", encoded_address);
      // assert_eq!(encoded_address, master_expected);

      for i in 0..10 {
         let name = format!("Child Wallet {}", i);
         hd_wallet.derive_child(name).unwrap();
      }

      let expected_0 = "0zk1qy34hw3cqaafq4ylvvchxzjjt37f8affcavahmxvydglnaa2gapelrv7j6fe3z53lanpv5peal7kwmlll40244zszp74pyetmy8hg899trnf6xmptqvv6n2xwxw";
      let expected_1 = "0zk1qy4f2ycf7g9jthn3fhekek0sye3dlzvzcwagashe4hyhc7trh7xhrrv7j6fe3z53llhj9yqqh6ullwmds86scyw8dcpy5mny2898wnxvvulyww83yy6skhawdw7";
      let expected_2 = "0zk1qyyhz7knm4navzekzdadz0x8el6saf4wtwfcz4hs9yxef6n0msp5frv7j6fe3z53lu0gf4hgty42f34ksq3xsq5vpwdk7qruhpzwd39f86nz5r2ghz3yg56h3ls";
      let expected_3 = "0zk1qyqx3m02q5a55w2qgvt37dpzka5yxpy9ha06jf0xrrevrrcdrkau9rv7j6fe3z53l78f73hn5ny2ch8kweftaxchx76mnyhc69pnrszh9n0ca75ezknawyvq9rn";
      let expected_4 = "0zk1qy46nvy7cqxehelf5su3jcyqaj942um9rjlphpf5qqw8lxdf900ttrv7j6fe3z53lls5vrj4zdlkhus0zr2sgfhv5qk7luxguhdw0pmd5p66ndmucxvrklmda9p";
      let expected_5 = "0zk1qy8w58huge97muh9kl4d6lf0xsavd9ff8wfll5pr5uj6tmqjv5cj3rv7j6fe3z53l7k56p30tnhmkqr7hqtggugqswkaam64wl397aqum9l939un7j9ny3hd95a";
      let expected_6 = "0zk1qyvrncnd5c3pnv2lk6u5jtmsh2zx8ehl29nr8q2uz70uf4mtwc7h3rv7j6fe3z53luszhkqheevavqk4f5amntn4euy9n82kkfvpx3lqzdgzqlqulxufylm2ueg";
      let expected_7 = "0zk1qyf0dh2mp58zhaadl5tmjlpl6jrfdau3lym8l0lwcxcgamvlyk7xmrv7j6fe3z53lapnnefjlf7xmw5gnn8k2apyjz8aqxw84wkv705actxsjzxm59wy5hpsw5k";
      let expected_8 = "0zk1qyrat05cljv7hwps45yfrmndwj57pj3ay32qs7cwe3erm7ejfwdwhrv7j6fe3z53llpqh2w05smu5wt2ltkv6sfkykqqgq2rpa9zs7m2qcfujk09ugxzwhtp2mx";
      let expected_9 = "0zk1qykd5lmlm4h6sayhvlykz9qusrc8pkalkzhexrz8u60l888jsfetnrv7j6fe3z53lu2t9jl0d3m95rpmwmzzfduvzxtlw7k209h0cma3u56qsc0ghcej20pfmeg";

      let expected_vec = vec![
         expected_0, expected_1, expected_2, expected_3, expected_4, expected_5, expected_6,
         expected_7, expected_8, expected_9,
      ];

      for (i, child) in hd_wallet.children.iter().enumerate() {
         let seed = child.full_key().unwrap();
         let expected = expected_vec[i];

         let data = generate_address_data(seed, 0, None).unwrap();
         let encoded_address = encode_address(&data).unwrap();
         eprintln!("{} Zk Address: {}", child.name, encoded_address);
         // assert_eq!(encoded_address, expected);
      }
   }
}
