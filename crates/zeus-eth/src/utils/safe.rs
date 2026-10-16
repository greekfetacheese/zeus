//! Safe{Wallet} singleton (masterCopy) addresses.
//!
//! A Safe account is a proxy whose storage slot 0 holds its singleton; a recipient whose
//! slot 0 points at one of these addresses is a Safe, not a plain contract.
//!
//! Source: https://github.com/safe-global/safe-deployments @
//! 382117f5d97745f611e05f827603aadb493dc620 (assets v1.0.0–v1.5.0; `canonical` + `eip155`
//! deployments; zkSync excluded — Zeus has no zkSync chain). Refresh the tables from that
//! repo when a new Safe version ships.

use alloy_primitives::{Address, address};

/// Singletons deployed on every chain Zeus supports (`canonical` + `eip155`).
const MODERN_SINGLETONS: [Address; 8] = [
   // v1.3.0 Safe
   address!("d9Db270c1B5E3Bd161E8c8503c55cEABeE709552"),
   address!("69f4D1788e39c87893C980c06EdF4b7f686e2938"),
   // v1.3.0 SafeL2
   address!("3E5c63644E683549055b9Be8653de26E0B4CD36E"),
   address!("fb1bffC9d739B8D520DaF37dF666da4C687191EA"),
   // v1.4.1 Safe, SafeL2
   address!("41675C099F32341bf84BFc5382aF534df5C7461a"),
   address!("29fcB43b46531BcA003ddC8FCB67FFE91900C762"),
   // v1.5.0 Safe, SafeL2
   address!("Ff51A5898e281Db6DfC7855790607438dF2ca44b"),
   address!("Edd160fEBBD92E350D4D398fb636302fccd67C7e"),
];

/// Older singletons only ever deployed on Ethereum mainnet.
const LEGACY_MAINNET_SINGLETONS: [Address; 3] = [
   // v1.0.0
   address!("b6029EA3B2c51D09a50B53CA8012FeEB05bDa35A"),
   // v1.1.1
   address!("34CfAC646f301356fAa8B21e94227e3583Fe3F5F"),
   // v1.2.0
   address!("6851D6fDFAfD08c0295C392436245E5bc78B0185"),
];

/// Whether `address` is a known Safe singleton on `chain_id`.
pub fn is_safe_singleton(chain_id: u64, address: Address) -> bool {
   MODERN_SINGLETONS.contains(&address)
      || (chain_id == 1 && LEGACY_MAINNET_SINGLETONS.contains(&address))
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The modern singletons are deployed on every chain Zeus supports, so each is known
   /// on each of those chains.
   #[test]
   fn modern_singletons_are_known_on_every_supported_chain() {
      const SUPPORTED: [u64; 7] = [1, 11155111, 10, 56, 8453, 42161, 4663];

      for chain in SUPPORTED {
         for singleton in MODERN_SINGLETONS {
            assert!(
               is_safe_singleton(chain, singleton),
               "{singleton} should be known on chain {chain}"
            );
         }
      }
   }

   /// The legacy singletons only ever deployed on mainnet: known there, unknown elsewhere.
   #[test]
   fn legacy_singletons_are_mainnet_only() {
      for singleton in LEGACY_MAINNET_SINGLETONS {
         assert!(
            is_safe_singleton(1, singleton),
            "{singleton} should be known on mainnet"
         );
         assert!(
            !is_safe_singleton(10, singleton),
            "{singleton} should not be known on Optimism"
         );
      }
   }

   /// An address that is not in either table is never a singleton.
   #[test]
   fn an_unknown_address_is_never_a_singleton() {
      let unknown = address!("fcb19e6a322b27c06842a71e8c725399f049ae3a");

      assert!(!is_safe_singleton(1, unknown));
      assert!(!is_safe_singleton(4663, unknown));
   }
}
