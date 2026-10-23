use alloy_primitives::{Address, ChainId, address};
use serde::{Deserialize, Serialize};

use crate::poi::types::ListKey;

/// Chain Configurations
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainConfig {
   /// EIP-155 Chain ID
   pub id: ChainId,
   /// Railgun Smart Wallet Address on this chain
   ///
   /// Sourced from
   /// <https://docs.railgun.org/wiki/learn/helpful-links>
   pub railgun_smart_wallet: Address,
   /// Unshield fee in basis points (bps).
   pub unshield_fee_bps: u16,
   /// RelayAdapt contract for native base-token shielding (wrap + shield via `multicall`)
   ///
   /// Sourced from
   /// <https://github.com/Railgun-Community/shared-models/blob/main/src/models/network-config.ts>
   pub relay_adapt_contract: Address,
   /// Wrapped base token (e.g. WETH on Ethereum) used in shield note preimages when shielding
   /// native ETH
   pub wrapped_base_token: Address,
   /// Block number the railgun smart wallet was deployed at
   pub deployment_block: u64,
   /// Block number when POI was launched for this chain
   ///
   /// Sourced from
   /// <https://github.com/Railgun-Community/shared-models/blob/dc3af7873305938f9f0771a24ad91f807f1b88e0/src/models/network-config.ts#L340>
   pub poi_start_block: u64,
   /// Subsquid GraphQL Endpoint for fast syncing
   ///
   /// Sourced from
   /// <https://github.com/Railgun-Community/wallet/blob/3ee3364648d416aa055bb1d5f5a2c4961be00ed6/src/services/railgun/railgun-txids/graphql/index.ts#L3187>
   pub subsquid_endpoint: String,

   /// Optional POI endpoint for this chain.
   ///
   /// Sourced from signal chat with the railgun team. Not publicly documented anywhere as far as
   /// I can tell, but the POI endpoint is required for any POI-related functionality.
   /// ¯\_(ツ)_/¯
   pub poi_endpoint: String,
   /// Optional list keys for POI
   pub list_keys: Vec<ListKey>,

   /// Privacy Paymaster address for this chain. This is used as the paymaster for all
   /// UserOperations.
   pub privacy_paymaster: Option<Address>,

   /// Railgun Fee Adapter address for this chain.
   pub railgun_fee_adapter: Option<Address>,
}

/// Chain configurations are values, not constructor calls: every field is public and every deployment
/// is written out with named fields so a transposed address is a compile error to spot, not a silent
/// wrong contract. Adding a field is a deliberate minor break; the crate fixes its own call sites.
impl ChainConfig {
   pub fn from_chain_id(chain_id: ChainId) -> Option<Self> {
      match chain_id {
         c if c == Self::mainnet().id => Some(Self::mainnet()),
         c if c == Self::sepolia().id => Some(Self::sepolia()),
         _ => None,
      }
   }

   pub fn mainnet() -> Self {
      Self {
         id: 1,
         railgun_smart_wallet: address!("0xFA7093CDD9EE6932B4eb2c9e1cde7CE00B1FA4b9"),
         unshield_fee_bps: 25,
         relay_adapt_contract: address!("0xAc9f360Ae85469B27aEDdEaFC579Ef2d052aD405"),
         wrapped_base_token: address!("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
         deployment_block: 14693013,
         poi_start_block: 18514200,
         subsquid_endpoint: "https://rail-squid.squids.live/squid-railgun-ethereum-v2/v/v1/graphql"
            .to_string(),
         poi_endpoint: "https://ppoi.fdi.network/".to_string(),
         list_keys: vec![ListKey::from(
            "efc6ddb59c098a13fb2b618fdae94c1c3a807abc8fb1837c93620c9143ee9e88",
         )],
         privacy_paymaster: Some(address!(
            "0x5FCd478c286528aE735e8CDAEF707956469ED208"
         )),
         railgun_fee_adapter: Some(address!(
            "0x0Aff8d142E3655714B716bBE1d66165a060D4155"
         )),
      }
   }

   pub fn sepolia() -> Self {
      Self {
         id: 11155111,
         railgun_smart_wallet: address!("0xeCFCf3b4eC647c4Ca6D49108b311b7a7C9543fea"),
         unshield_fee_bps: 25,
         relay_adapt_contract: address!("0x7e3d929EbD5bDC84d02Bd3205c777578f33A214D"),
         wrapped_base_token: address!("0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14"),
         deployment_block: 5784774,
         poi_start_block: 5944700,
         subsquid_endpoint:
            "https://rail-squid.squids.live/squid-railgun-eth-sepolia-v2/v/v1/graphql".to_string(),
         poi_endpoint: "https://ppoi.fdi.network/".to_string(),
         list_keys: vec![ListKey::from(
            "efc6ddb59c098a13fb2b618fdae94c1c3a807abc8fb1837c93620c9143ee9e88",
         )],
         privacy_paymaster: Some(address!(
            "0xBb9D6507B5dE027dEb0196c83A7DC6Eef325bEe4"
         )),
         railgun_fee_adapter: Some(address!(
            "0xeBabF510f824a349a9Be7F40cad3486B7249b1e0"
         )),
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The anti-transposition pin: every deployment is a struct literal of same-typed `Address`/`u64`
   /// neighbours, so this test states the values once more in a form the compiler cannot cross-check.
   #[test]
   fn chain_configurations_pin_their_deployment_addresses() {
      let mainnet = ChainConfig::mainnet();
      assert_eq!(
         mainnet.railgun_smart_wallet,
         address!("0xFA7093CDD9EE6932B4eb2c9e1cde7CE00B1FA4b9")
      );
      assert_eq!(
         mainnet.wrapped_base_token,
         address!("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")
      );
      assert_eq!(
         mainnet.relay_adapt_contract,
         address!("0xAc9f360Ae85469B27aEDdEaFC579Ef2d052aD405")
      );
      assert_eq!(
         mainnet.privacy_paymaster,
         Some(address!(
            "0x5FCd478c286528aE735e8CDAEF707956469ED208"
         ))
      );
      assert_eq!(
         mainnet.railgun_fee_adapter,
         Some(address!(
            "0x0Aff8d142E3655714B716bBE1d66165a060D4155"
         ))
      );
      assert_eq!(mainnet.deployment_block, 14693013);
      assert_eq!(mainnet.poi_start_block, 18514200);
      assert_eq!(mainnet.list_keys.len(), 1);

      let sepolia = ChainConfig::sepolia();
      assert_eq!(
         sepolia.railgun_smart_wallet,
         address!("0xeCFCf3b4eC647c4Ca6D49108b311b7a7C9543fea")
      );
      assert_eq!(
         sepolia.wrapped_base_token,
         address!("0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14")
      );
      assert_eq!(
         sepolia.relay_adapt_contract,
         address!("0x7e3d929EbD5bDC84d02Bd3205c777578f33A214D")
      );
      assert_eq!(sepolia.deployment_block, 5784774);
      assert_eq!(sepolia.list_keys.len(), 1);
   }
}
