//! Registered ERC-4337 smart-account implementations and clone targets.
//!
//! A funds recipient whose ERC-1967 implementation slot holds one of these published
//! smart-account implementations — or whose EIP-1167 minimal clone targets one of these
//! addresses — is a smart account, not a plain contract, and may receive funds. Addresses
//! were verified present with `eth_getCode` on all Zeus chains (1, 10, 56, 8453, 42161,
//! 11155111, 4663); one exception: the legacy Alchemy LightAccount v1.0.1/v1.0.2 proxies
//! were only deployed on Ethereum, Sepolia, Optimism, Base and Arbitrum One — they are
//! absent on BSC (56) and RobinHood (4663); allowing them elsewhere would let a lookalike
//! contract at those addresses pass the screen.
//!
//! Sources:
//! - Coinbase Smart Wallet: `github.com/coinbase/smart-wallet` (v1 = `0x000100ab…`,
//!   v1.1 = `0x00000110…`).
//! - Alchemy smart accounts:
//!   https://www.alchemy.com/docs/wallets/smart-contracts/deployed-addresses
//! - Biconomy Nexus: https://docs.biconomy.io/contracts-and-audits (Nexus Implementation
//!   v1.2.0, "same across all chains").
//! - eth-infinitism: factories `0x91E60e0613810449d098b0b5Ec8b51A0FE8c8985` (v0.7) and
//!   `0x9406Cc6185a346906296840746125a0E44976454` (v0.6); impl addresses read live from
//!   their `accountImplementation()` getters, both factory sources use
//!   `new ERC1967Proxy{salt}(...)`.
//! - Ambire: `github.com/AmbireTech/ambire-common` — `src/consts/deploy.ts`
//!   (`PROXY_AMBIRE_ACCOUNT`) + `src/libs/proxyDeploy/deploy.ts` (EIP-1167 codegen).
//!
//! Refresh recipe: re-read the sources above and re-check every address with `eth_getCode`
//! on each supported chain (1, 10, 56, 8453, 42161, 11155111, 4663).

use alloy_primitives::{Address, address, hex};

/// ERC-1967 implementation storage slot: the word read for an ERC-1967 proxy account.
pub const ERC1967_IMPLEMENTATION_SLOT: [u8; 32] =
   hex!("360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc");

/// ERC-1967 implementations deployed on every chain Zeus supports.
const PROXY_IMPLEMENTATIONS: [Address; 12] = [
   // Coinbase Smart Wallet v1.1
   address!("00000110dCdEdC9581cb5eCB8467282f2926534d"),
   // Coinbase Smart Wallet v1
   address!("000100abaad02f1cfC8Bbe32bD5a564817339E72"),
   // Alchemy LightAccount v1.1.0
   address!("ae8c656ad28F2B59a196AB61815C16A0AE1c3cba"),
   // Alchemy LightAccount v2.0.0
   address!("8E8e658E22B12ada97B402fF0b044D6A325013C7"),
   // Alchemy MultiOwnerLightAccount v2.0.0
   address!("d2c27F9eE8E4355f71915ffD5568cB3433b6823D"),
   // Alchemy ModularAccount v2.0.0
   address!("00000000000002377B26b1EdA7b0BC371C60DD4f"),
   // Alchemy SemiModularAccountBytecode v1.0.0
   address!("000000000000c5A9089039570Dd36455b5C07383"),
   // Alchemy SemiModularAccountStorageOnly v1.0.0
   address!("0000000000006E2f9d80CaEc0Da6500f005EB25A"),
   // Alchemy UpgradeableModularAccount v1.0.0
   address!("0046000000000151008789797b54fdb500E2a61e"),
   // eth-infinitism SimpleAccount v0.7 impl
   address!("68641DE71cfEa5a5d0D29712449Ee254bb1400C2"),
   // eth-infinitism SimpleAccount v0.6 impl
   address!("8ABB13360b87Be5EEb1B98647A016adD927a136c"),
   // Biconomy Nexus v1.2.0
   address!("000000004F43C49e93C970E84001853a70923B03"),
];

/// Deployed only on Ethereum, Sepolia, Optimism, Base and Arbitrum One — absent on
/// BSC (56) and RobinHood (4663).
const LIMITED_CHAIN_IMPLEMENTATIONS: [Address; 2] = [
   // Alchemy LightAccount v1.0.2
   address!("5467b1947F47d0646704EB801E075e72aeAe8113"),
   // Alchemy LightAccount v1.0.1
   address!("c1b2fc4197c9187853243e6e4eb5a4af8879a1c0"),
];

/// Chains where the `LIMITED_CHAIN_IMPLEMENTATIONS` proxies were actually deployed.
const LIMITED_IMPLEMENTATION_CHAINS: [u64; 5] = [1, 11155111, 10, 8453, 42161];

/// EIP-1167 clone targets deployed on every chain Zeus supports.
const CLONE_TARGETS: [Address; 1] = [
   // Ambire `AmbireAccount` v2 master (ambire-common `PROXY_AMBIRE_ACCOUNT`)
   address!("0F2AA7bcda3d9D210dF69a394b6965CB2566c828"),
];

/// Whether `implementation` is a known smart-account implementation (ERC-1967) on `chain_id`.
pub fn is_known_erc1967_implementation(chain_id: u64, implementation: Address) -> bool {
   PROXY_IMPLEMENTATIONS.contains(&implementation)
      || (LIMITED_IMPLEMENTATION_CHAINS.contains(&chain_id)
         && LIMITED_CHAIN_IMPLEMENTATIONS.contains(&implementation))
}

/// Whether `target` is a known smart-account clone target (EIP-1167). Present on every
/// supported chain, hence no chain parameter.
pub fn is_known_eip1167_target(target: Address) -> bool {
   CLONE_TARGETS.contains(&target)
}

/// Strictly parse the canonical EIP-1167 runtime (exact 45 bytes; prefix
/// `363d3d373d3d3d363d73`; suffix `5af43d82803e903d91602b57fd5bf3`) and return the
/// embedded target. Anything else is None.
pub fn eip1167_clone_target(code: &[u8]) -> Option<Address> {
   const PREFIX: [u8; 10] = [0x36, 0x3d, 0x3d, 0x37, 0x3d, 0x3d, 0x3d, 0x36, 0x3d, 0x73];
   const SUFFIX: [u8; 15] = [
      0x5a, 0xf4, 0x3d, 0x82, 0x80, 0x3e, 0x90, 0x3d, 0x91, 0x60, 0x2b, 0x57, 0xfd, 0x5b, 0xf3,
   ];

   if code.len() == 45 && code.starts_with(&PREFIX) && code.ends_with(&SUFFIX) {
      Some(Address::from_slice(&code[10..30]))
   } else {
      None
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The canonical Ambire account runtime parses to its embedded master address.
   #[test]
   fn the_ambire_runtime_parses_to_its_target() {
      let code = hex::decode(
         "363d3d373d3d3d363d730f2aa7bcda3d9d210df69a394b6965cb2566c8285af43d82803e903d91602b57fd5bf3",
      )
      .unwrap();

      assert_eq!(
         eip1167_clone_target(&code),
         Some(address!(
            "0F2AA7bcda3d9D210dF69a394b6965CB2566c828"
         ))
      );
   }

   /// Only the exact 45-byte canonical runtime parses; every deviation is None.
   #[test]
   fn only_the_exact_clone_runtime_parses() {
      let canonical = hex::decode(
         "363d3d373d3d3d363d730f2aa7bcda3d9d210df69a394b6965cb2566c8285af43d82803e903d91602b57fd5bf3",
      )
      .unwrap();

      let mut extra = canonical.clone();
      extra.push(0x00);
      assert_eq!(
         eip1167_clone_target(&extra),
         None,
         "one trailing byte"
      );

      assert_eq!(
         eip1167_clone_target(&canonical[..canonical.len() - 1]),
         None,
         "truncated by one byte"
      );

      let mut first_changed = canonical.clone();
      first_changed[0] = 0x37;
      assert_eq!(
         eip1167_clone_target(&first_changed),
         None,
         "first byte changed"
      );

      let mut suffix_changed = canonical.clone();
      suffix_changed[35] = 0x00;
      assert_eq!(
         eip1167_clone_target(&suffix_changed),
         None,
         "a suffix byte changed"
      );

      assert_eq!(eip1167_clone_target(&[]), None, "empty");
   }

   /// All-chain implementations are known on every chain; the legacy LightAccount pair is
   /// known only on its deployed chains; the Ambire master is a known clone target; USDC is
   /// neither.
   #[test]
   fn the_registry_knows_the_published_implementations() {
      assert!(is_known_erc1967_implementation(
         1,
         address!("00000110dCdEdC9581cb5eCB8467282f2926534d")
      ));
      assert!(is_known_erc1967_implementation(
         8453,
         address!("8E8e658E22B12ada97B402fF0b044D6A325013C7")
      ));
      assert!(is_known_erc1967_implementation(
         4663,
         address!("000000004F43C49e93C970E84001853a70923B03")
      ));

      let legacy = address!("5467b1947F47d0646704EB801E075e72aeAe8113");
      assert!(is_known_erc1967_implementation(1, legacy));
      assert!(!is_known_erc1967_implementation(56, legacy));
      assert!(!is_known_erc1967_implementation(4663, legacy));

      assert!(is_known_eip1167_target(address!(
         "0F2AA7bcda3d9D210dF69a394b6965CB2566c828"
      )));

      let usdc = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
      assert!(!is_known_erc1967_implementation(1, usdc));
      assert!(!is_known_eip1167_target(usdc));
   }
}
