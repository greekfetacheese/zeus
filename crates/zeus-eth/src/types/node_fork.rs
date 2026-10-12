//! What a chain's own node says it is configured for — EIP-7910 `eth_config`.

use std::collections::BTreeMap;

use revm::primitives::hardfork::SpecId;
use serde::Deserialize;

use super::ChainId;

/// System contracts ePBS (EIP-7732) adds, which is what says a fork is Amsterdam or later: the
/// builder deposit and exit contracts arrive with it. Verified against two live nodes — Sepolia
/// lists both, mainnet lists neither.
const AMSTERDAM_SYSTEM_CONTRACTS: [&str; 2] = [
   "BUILDER_DEPOSIT_CONTRACT_ADDRESS",
   "BUILDER_EXIT_CONTRACT_ADDRESS",
];

/// One fork's configuration, as `eth_config` reports it (`current`, `next` or `last`).
///
/// The method reports configuration rather than naming forks, so an activation time identifies a
/// fork only to someone who already knows the chain's schedule. What can be read from this is
/// therefore: whether the node is still on the fork Zeus was built with
/// ([`ChainId::spec_activation_time`]), and whether it carries the system contracts that arrived
/// with the next one.
///
/// [`ChainId::spec_activation_time`]: crate::types::ChainId::spec_activation_time
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkConfig {
   /// Unix seconds at which this configuration activated, as the node reports it. `0` means
   /// genesis; absent means the node does not report it.
   #[serde(default)]
   pub activation_time: Option<u64>,

   /// The system contracts this fork knows, by the names EIP-7910 defines. A map rather than a
   /// list, and the values are addresses.
   #[serde(default)]
   pub system_contracts: BTreeMap<String, String>,
}

impl ForkConfig {
   /// Whether the node runs Amsterdam or later, judged by the system contracts it lists.
   ///
   /// This is a floor rather than an identification: a later fork keeps these contracts, so it says
   /// "at least Amsterdam", which is what a simulation needs to stop running pre-Amsterdam rules.
   pub fn ran_amsterdam_or_later(&self) -> bool {
      AMSTERDAM_SYSTEM_CONTRACTS
         .iter()
         .any(|contract| self.system_contracts.contains_key(*contract))
   }
}

impl ChainId {
   /// The activation time of the fork [`ChainId::spec`] returns, as the chain's node reports it
   /// (EIP-7910 `eth_config`).
   ///
   /// This is what lets a simulation defer to the chain's own node: a node reporting the time Zeus
   /// expects is on the fork Zeus was built for, and one reporting a different time is on a fork
   /// Zeus cannot name — see [`ChainId::spec_from_node`]. Only the chains whose fork schedule Zeus
   /// tracks by hand have one here; an L2 runs its own schedule and is left to [`ChainId::spec`].
   pub fn spec_activation_time(&self) -> Option<u64> {
      match self {
         // Osaka: 2026-01-07 01:01 UTC.
         ChainId::Ethereum => Some(1_767_747_671),
         // Amsterdam: 2026-10-06 13:53 UTC.
         ChainId::EthereumSepolia => Some(1_791_294_816),
         _ => None,
      }
   }

   /// The spec to simulate this chain with, given what its own node says about the fork it runs.
   ///
   /// `None` when the node says nothing Zeus can act on — the method is not implemented, the chain
   /// is one Zeus does not track, or the fork is not one Zeus can place — which leaves the caller on
   /// [`ChainId::spec`]. The hardcoded answer is the fallback, never a guess about the node.
   pub fn spec_from_node(&self, node: &ForkConfig) -> Option<SpecId> {
      // Only for the chains Zeus tracks: an L2 can list an L1's system contracts without running
      // the L1's rules, so nothing it reports is evidence about its hardfork.
      let known = self.spec_activation_time()?;

      if node.activation_time == Some(known) {
         return Some(self.spec());
      }

      // A fork Zeus has no activation time for. ePBS arriving says the chain has moved past what
      // Zeus was built with, because it is part of the same upgrade set as the repricings — so
      // simulate on the newest spec Zeus has rather than on the stale one. This is what keeps a
      // chain correct on the day it forks, without waiting for a release.
      if node.ran_amsterdam_or_later() {
         return Some(SpecId::AMSTERDAM);
      }

      None
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// A trimmed capture of `eth_config` on Sepolia (2026-10-12), Amsterdam live: the members are
   /// the ones the method returned, the precompile list shortened to the two that matter.
   const SEPOLIA: &str = r#"{
      "activationTime": 1791294816,
      "blobSchedule": { "baseFeeUpdateFraction": 11684671, "max": 21, "target": 14 },
      "chainId": "0xaa36a7",
      "forkId": "0x6c1d9423",
      "precompiles": {
         "ECREC": "0x0000000000000000000000000000000000000001",
         "KZG_POINT_EVALUATION": "0x000000000000000000000000000000000000000a"
      },
      "systemContracts": {
         "BEACON_ROOTS_ADDRESS": "0x000f3df6d732807ef1319fb7b8bb8522d0beac02",
         "BUILDER_DEPOSIT_CONTRACT_ADDRESS": "0x0000bff46984e3725691fa540a8c7589300d8282",
         "BUILDER_EXIT_CONTRACT_ADDRESS": "0x000064d678505ad48f8ccb093bc65613800e8282",
         "CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS": "0x0000bbddc7ce488642fb579f8b00f3a590007251",
         "DEPOSIT_CONTRACT_ADDRESS": "0x7f02c3e3c98b133055b8b348b2ac625669ed295d"
      }
   }"#;

   /// The same capture on mainnet, still on Osaka.
   const MAINNET: &str = r#"{
      "activationTime": 1767747671,
      "blobSchedule": { "baseFeeUpdateFraction": 11684671, "max": 21, "target": 14 },
      "chainId": "0x1",
      "forkId": "0x07c9462e",
      "precompiles": { "ECREC": "0x0000000000000000000000000000000000000001" },
      "systemContracts": {
         "BEACON_ROOTS_ADDRESS": "0x000f3df6d732807ef1319fb7b8bb8522d0beac02",
         "CONSOLIDATION_REQUEST_PREDEPLOY_ADDRESS": "0x0000bbddc7ce488642fb579f8b00f3a590007251",
         "DEPOSIT_CONTRACT_ADDRESS": "0x00000000219ab540356cbb839cbe05303d7705fa",
         "WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS": "0x00000961ef480eb55e80d19ad83579a64c007002"
      }
   }"#;

   /// The shape is a live node's, not this crate's idea of one: the camelCase keys and the system
   /// contract names are what the two captures above carry, and the addresses parse.
   #[test]
   fn parses_what_a_live_node_returns() {
      let sepolia: ForkConfig = serde_json::from_str(SEPOLIA).unwrap();
      assert_eq!(sepolia.activation_time, Some(1_791_294_816));
      assert!(sepolia.ran_amsterdam_or_later());
      assert_eq!(sepolia.system_contracts.len(), 5);

      let mainnet: ForkConfig = serde_json::from_str(MAINNET).unwrap();
      assert_eq!(mainnet.activation_time, Some(1_767_747_671));
      assert!(!mainnet.ran_amsterdam_or_later());
   }

   /// A configuration object from a client that does not report everything still parses: the
   /// members are optional, and an absent `systemContracts` means no Amsterdam rather than a
   /// parse failure that would take the simulation down with it.
   #[test]
   fn tolerates_a_sparse_config() {
      let sparse: ForkConfig = serde_json::from_str(r#"{ "activationTime": 0 }"#).unwrap();
      assert_eq!(sparse.activation_time, Some(0));
      assert!(sparse.system_contracts.is_empty());
      assert!(!sparse.ran_amsterdam_or_later());

      let empty: ForkConfig = serde_json::from_str("{}").unwrap();
      assert_eq!(empty.activation_time, None);
      assert!(!empty.ran_amsterdam_or_later());
   }

   /// The node decides the spec when it speaks, and Zeus's own answer stands when it does not. The
   /// activation times are the ones two live nodes report, and the builder contracts are the ones
   /// Sepolia lists and mainnet does not.
   #[test]
   fn a_node_decides_the_spec_when_it_speaks() {
      let amsterdam_contracts = || ForkConfig {
         activation_time: None,
         system_contracts: [
            (
               "BUILDER_DEPOSIT_CONTRACT_ADDRESS".to_string(),
               "0x0000bff46984e3725691fa540a8c7589300d8282".to_string(),
            ),
            (
               "BUILDER_EXIT_CONTRACT_ADDRESS".to_string(),
               "0x000064d678505ad48f8ccb093bc65613800e8282".to_string(),
            ),
         ]
         .into_iter()
         .collect(),
      };

      // Each chain's own activation time, as its node reports it: an answer Zeus can place.
      let sepolia = ForkConfig {
         activation_time: Some(1_791_294_816),
         ..amsterdam_contracts()
      };
      assert_eq!(
         ChainId::EthereumSepolia.spec_from_node(&sepolia),
         Some(SpecId::AMSTERDAM)
      );

      let mainnet = ForkConfig {
         activation_time: Some(1_767_747_671),
         ..ForkConfig::default()
      };
      assert_eq!(
         ChainId::Ethereum.spec_from_node(&mainnet),
         Some(SpecId::OSAKA)
      );

      // Mainnet on the day it forks, simulated by a build that predates the release: an activation
      // time Zeus has no entry for, with the ePBS contracts on top of it. Simulating on the spec
      // Zeus ships would price the transaction by rules the chain no longer has — the Sepolia
      // estimate that came out at 21,000 against the chain's 204,600, one chain over.
      let forked = ForkConfig {
         activation_time: Some(1_800_000_000),
         ..amsterdam_contracts()
      };
      assert_eq!(
         ChainId::Ethereum.spec_from_node(&forked),
         Some(SpecId::AMSTERDAM)
      );

      // A fork Zeus can neither place nor recognise stays on the spec Zeus ships: the answer to
      // "I do not know" is the hardcoded one, not a guess about somebody else's node.
      let unknown = ForkConfig {
         activation_time: Some(1_800_000_000),
         ..ForkConfig::default()
      };
      assert_eq!(ChainId::Ethereum.spec_from_node(&unknown), None);

      // An L2 reports its own schedule, and nothing in it is evidence about the L1's rules.
      assert_eq!(ChainId::Optimism.spec_from_node(&forked), None);
   }
}
