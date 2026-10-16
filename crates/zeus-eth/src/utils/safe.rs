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

/// Legacy singletons and the chains each one is actually deployed on. The upstream
/// safe-deployments tables list these only on Ethereum, but live `eth_getCode` reads show
/// genuine instances also on the chains below (verified 2026-10-16; each byte-identical to
/// mainnet except v1.1.1 on RobinHood, where only the trailing solc metadata hash differs).
/// A singleton is allowed only where an instance exists — elsewhere a lookalike at the
/// same address must stay refused.
const LEGACY_SINGLETONS: [(Address, &[u64]); 3] = [
   // v1.0.0 — Optimism
   (
      address!("b6029EA3B2c51D09a50B53CA8012FeEB05bDa35A"),
      &[1, 10],
   ),
   // v1.1.1 — Optimism, Base, Arbitrum One, Sepolia, RobinHood
   (
      address!("34CfAC646f301356fAa8B21e94227e3583Fe3F5F"),
      &[1, 10, 8453, 42161, 11155111, 4663],
   ),
   // v1.2.0 — Base, Sepolia
   (
      address!("6851D6fDFAfD08c0295C392436245E5bc78B0185"),
      &[1, 8453, 11155111],
   ),
];

/// Whether `address` is a known Safe singleton on `chain_id`.
pub fn is_safe_singleton(chain_id: u64, address: Address) -> bool {
   MODERN_SINGLETONS.contains(&address)
      || LEGACY_SINGLETONS
         .iter()
         .any(|(singleton, chains)| *singleton == address && chains.contains(&chain_id))
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

   /// Each legacy singleton is known only on the chains where an instance is actually
   /// deployed (verified on-chain, see the table comment): known on its chains, refused
   /// everywhere else.
   #[test]
   fn legacy_singletons_are_known_only_where_deployed() {
      let v1_0_0 = address!("b6029EA3B2c51D09a50B53CA8012FeEB05bDa35A");
      let v1_1_1 = address!("34CfAC646f301356fAa8B21e94227e3583Fe3F5F");
      let v1_2_0 = address!("6851D6fDFAfD08c0295C392436245E5bc78B0185");

      let cases: [(Address, &[u64], &[u64]); 3] = [
         (
            v1_0_0,
            &[1, 10],
            &[56, 8453, 42161, 11155111, 4663],
         ),
         (
            v1_1_1,
            &[1, 10, 8453, 42161, 11155111, 4663],
            &[56],
         ),
         (
            v1_2_0,
            &[1, 8453, 11155111],
            &[10, 56, 42161, 4663],
         ),
      ];

      for (singleton, known, unknown) in cases {
         for &chain in known {
            assert!(
               is_safe_singleton(chain, singleton),
               "{singleton} should be known on chain {chain}"
            );
         }
         for &chain in unknown {
            assert!(
               !is_safe_singleton(chain, singleton),
               "{singleton} should not be known on chain {chain}"
            );
         }
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
