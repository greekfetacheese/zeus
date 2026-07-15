use super::address::RailgunAddress;
use super::keys::RailgunKeys;
use crate::crypto::keys::SpendingSignature;
use crate::types::Chain;

use alloy_primitives::U256;
use secure_types::SecureArray;

/// A railgun signer which can sign transactions and provide the associated 0xzk address.
#[derive(Clone)]
pub struct RailgunSigner {
   keys: RailgunKeys,
   address: RailgunAddress,
   chain: Chain,
}

impl std::fmt::Debug for RailgunSigner {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      write!(f, "RailgunSigner {{ address: {:?}, chain: {:?} }}", self.address, self.chain)
   }
}

impl RailgunSigner {
   /// Create a new RailgunSigner from a RailgunKeys and RailgunAddress.
   pub fn new(keys: RailgunKeys, address: RailgunAddress, chain: Chain) -> Self {
      Self {
         keys,
         address,
         chain,
      }
   }

   /// Create a new RailgunSigner from a 64-byte seed, index and chain.
   pub fn from_seed(
      seed: &SecureArray<u8, 64>,
      index: u32,
      chain_id: u64,
   ) -> Result<Self, anyhow::Error> {
      let keys = RailgunKeys::new(&seed, index)?;
      let address = RailgunAddress::new(&seed, index, None)?;
      Ok(Self {
         keys,
         address,
         chain: Chain::from(chain_id),
      })
   }

   /// Return the RailgunKeys associated with this signer.
   pub fn keys(&self) -> &RailgunKeys {
      &self.keys
   }

   /// Return the RailgunAddress associated with this signer.
   pub fn address(&self) -> &RailgunAddress {
      &self.address
   }

   /// Return the chain associated with this signer.
   pub fn chain(&self) -> &Chain {
      &self.chain
   }

   pub fn sign(&self, inputs: U256) -> Result<SpendingSignature, anyhow::Error> {
      self.keys.spending_private_key.sign(inputs)
   }

   /// BIP-32 derivation paths for railgun spending keys.
   ///
   /// <https://github.com/Railgun-Community/engine/blob/e2913b39e13f82f43556d23705fa20d2ece2e8ab/src/key-derivation/wallet-node.ts#L17>
   pub fn spending_key_path(index: u32) -> String {
      format!("m/44'/1984'/0'/0'/{}'", index)
   }

   /// BIP-32 derivation paths for railgun viewing keys.
   ///
   ///  <https://github.com/Railgun-Community/engine/blob/e2913b39e13f82f43556d23705fa20d2ece2e8ab/src/key-derivation/wallet-node.ts#L17>
   pub fn viewing_key_path(index: u32) -> String {
      format!("m/420'/1984'/0'/0'/{}'", index)
   }
}
