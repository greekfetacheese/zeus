use anyhow::anyhow;
use bech32::{FromBase32, ToBase32, Variant};
use secure_types::SecureArray;

use crate::types::Chain;
use crate::{
   account::{
      compute_public_spending_key,
      keys::{spending_key_path, viewing_key_path},
   },
   crypto::keys::{MasterPublicKey, SpendingKey, SpendingPublicKey, ViewingKey, ViewingPublicKey},
};

const PREFIX: &str = "0zk";
const ALL_CHAINS_NETWORK_ID: &str = "ffffffffffffffff";
const RAILGUN_XOR: [u8; 8] = [b'r', b'a', b'i', b'l', b'g', b'u', b'n', 0];
const ADDRESS_VERSION: u8 = 1;

#[derive(Clone, PartialEq, Eq)]
pub struct RailgunAddress {
   pub address: String,
   pub master_public_key: MasterPublicKey,
   pub viewing_public_key: ViewingPublicKey,
   pub chain: Option<Chain>,
   pub version: u8,
}

impl std::fmt::Debug for RailgunAddress {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      write!(
         f,
         "RailgunAddress {{ address: {:?}, master_public_key: {:?}, viewing_public_key: {:?}, chain: {:?}, version: {:?} }}",
         self.address, self.master_public_key, self.viewing_public_key, self.chain, self.version
      )
   }
}

impl RailgunAddress {
   pub fn from_private_keys(
      spending_key: SpendingKey,
      viewing_key: ViewingKey,
      chain: Option<Chain>,
   ) -> Self {
      let master_pubkey = MasterPublicKey::new(
         spending_key.public_key(),
         viewing_key.nullifying_key(),
      );

      let address = encode_address(
         ADDRESS_VERSION,
         master_pubkey,
         viewing_key.public_key(),
         chain,
      )
      .unwrap();

      RailgunAddress {
         address,
         master_public_key: master_pubkey,
         viewing_public_key: viewing_key.public_key(),
         chain,
         version: ADDRESS_VERSION,
      }
   }

   pub fn new(
      seed: &SecureArray<u8, 64>,
      index: u32,
      _chain: Option<Chain>,
   ) -> Result<Self, anyhow::Error> {
      let spending_path = spending_key_path(index);
      let viewing_path = viewing_key_path(index);

      let (spend_x, spend_y) =
         compute_public_spending_key(&seed, &spending_path).expect("Spending key");

      let viewing_priv = seed.unlock(|seed_bytes| ViewingKey::derive(seed_bytes, &viewing_path))?;
      let nullifying_key = viewing_priv.nullifying_key();
      let viewing_public_key = viewing_priv.public_key();

      let x = spend_x.to_be_bytes();
      let y = spend_y.to_be_bytes();
      let spending_public_key = SpendingPublicKey::new(x, y);

      let master_public_key = MasterPublicKey::new(spending_public_key, nullifying_key);

      let mut data = RailgunAddress {
         master_public_key,
         viewing_public_key,
         chain: _chain,
         version: ADDRESS_VERSION,
         address: String::new(),
      };

      data.address = encode_address(
         data.version,
         data.master_public_key,
         data.viewing_public_key,
         data.chain,
      )?;

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

      let master_public_key = MasterPublicKey(master_bytes);
      let viewing_public_key = ViewingPublicKey(viewing_bytes);

      Ok(Self {
         address: address.to_string(),
         version,
         master_public_key,
         viewing_public_key,
         chain: None,
      })
   }

   pub fn viewing_pubkey(&self) -> ViewingPublicKey {
      self.viewing_public_key
   }

   pub fn master_pubkey(&self) -> MasterPublicKey {
      self.master_public_key
   }
}

pub fn encode_address(
   version: u8,
   master_public_key: MasterPublicKey,
   viewing_public_key: ViewingPublicKey,
   chain: Option<Chain>,
) -> Result<String, anyhow::Error> {
   let version_hex = format!("{:02x}", version);
   let master_hex = master_public_key.to_hex();
   let network_hex = xor_network_id(&chain_to_network_id(chain))?;
   let viewing_hex = viewing_public_key.to_hex();

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

#[cfg(test)]
mod test {
   use crate::account::keys::RailgunKeys;

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
      let address_data = RailgunAddress::new(&sec_seed, 0, None).unwrap();

      assert_eq!(address_data.address, railway_address);
   }

   #[test]
   fn test_zeus_wallet() {
      let wallet = gen_wallet();

      let full_key = wallet.master_wallet.full_key().unwrap();
      let address = RailgunAddress::new(&full_key, 0, None).unwrap();
      let encoded_address = encode_address(
         address.version,
         address.master_public_key,
         address.viewing_public_key,
         None,
      )
      .unwrap();
      println!("Address: {}", encoded_address);
   }

   #[test]
   fn test_decode_specific_address() {
      let wallet = gen_wallet();
      let full_key = wallet.master_wallet.full_key().unwrap();
      let address_data = RailgunAddress::new(&full_key, 0, None).unwrap();

      let decoded = RailgunAddress::from_zk_address(&address_data.address).unwrap();

      assert_eq!(address_data, decoded);
   }

   #[test]
   fn test_railgun_address_produces_same_as_railgun_keys() {
      let wallet = gen_wallet();
      let seed = wallet.master_wallet.seed().unwrap();
      let railgun_address = RailgunAddress::new(&seed, 0, None).unwrap();

      let keys = RailgunKeys::new(&seed, 0).unwrap();
      let address = encode_address(
         1,
         keys.master_public_key,
         keys.viewing_public_key,
         None,
      )
      .unwrap();

      assert_eq!(railgun_address.address, address);
      println!("Address from RailgunAddress::new: {}", address);
      println!(
         "Address from RailgunKeys::new: {}",
         railgun_address.address
      );
   }
}
