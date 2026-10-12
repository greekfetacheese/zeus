use alloy_primitives::{Address, U256};
use anyhow::bail;
use revm::primitives::eip8037::{CPSB_GLAMSTERDAM, NEW_ACCOUNT_BYTES, SSTORE_SET_BYTES};
use revm::primitives::hardfork::SpecId;

pub const ETH: u64 = 1;
pub const ETH_SEPOLIA: u64 = 11155111;
pub const OPTIMISM: u64 = 10;
pub const BSC: u64 = 56;
pub const BASE: u64 = 8453;
pub const ARBITRUM: u64 = 42161;
pub const ROBIN_HOOD: u64 = 4663;

pub const SUPPORTED_CHAINS: [u64; 7] =
   [ETH, ETH_SEPOLIA, OPTIMISM, BSC, BASE, ARBITRUM, ROBIN_HOOD];

const ERR_MSG: &str = "Supported chains are: Ethereum(1), Ethereum Sepolia(11155111), Optimism(10), Binance Smart Chain(56), Base(8453), Arbitrum(42161), RobinHood(4663)";

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u64)]
pub enum ChainId {
   Ethereum = 1,
   EthereumSepolia = 11155111,
   Optimism = 10,
   BinanceSmartChain = 56,
   Base = 8453,
   Arbitrum = 42161,
   RobinHood = 4663,
}

impl Default for ChainId {
   fn default() -> Self {
      ChainId::Ethereum
   }
}

impl From<u64> for ChainId {
   fn from(id: u64) -> Self {
      ChainId::new(id).unwrap()
   }
}

impl ChainId {
   pub fn new(id: u64) -> Result<Self, anyhow::Error> {
      let chain = match id {
         1 => ChainId::Ethereum,
         11155111 => ChainId::EthereumSepolia,
         10 => ChainId::Optimism,
         56 => ChainId::BinanceSmartChain,
         8453 => ChainId::Base,
         42161 => ChainId::Arbitrum,
         4663 => ChainId::RobinHood,
         _ => bail!(format!(
            "Unsupported chain id: {}\n{}",
            id, ERR_MSG
         )),
      };
      Ok(chain)
   }

   pub fn eth() -> Self {
      ChainId::Ethereum
   }

   pub fn eth_sepolia() -> Self {
      ChainId::EthereumSepolia
   }

   pub fn optimism() -> Self {
      ChainId::Optimism
   }

   pub fn bsc() -> Self {
      ChainId::BinanceSmartChain
   }

   pub fn base() -> Self {
      ChainId::Base
   }

   pub fn arbitrum() -> Self {
      ChainId::Arbitrum
   }

   pub fn robinhood() -> Self {
      ChainId::RobinHood
   }

   pub fn is_ethereum(&self) -> bool {
      matches!(self, ChainId::Ethereum)
   }

   pub fn is_eth_sepolia(&self) -> bool {
      matches!(self, ChainId::EthereumSepolia)
   }

   pub fn is_optimism(&self) -> bool {
      matches!(self, ChainId::Optimism)
   }

   pub fn is_base(&self) -> bool {
      matches!(self, ChainId::Base)
   }

   pub fn is_arbitrum(&self) -> bool {
      matches!(self, ChainId::Arbitrum)
   }

   pub fn is_bsc(&self) -> bool {
      matches!(self, ChainId::BinanceSmartChain)
   }

   pub fn is_robinhood(&self) -> bool {
      matches!(self, ChainId::RobinHood)
   }

   /// Return all supported chains
   pub fn supported_chains() -> Vec<ChainId> {
      SUPPORTED_CHAINS.iter().map(|id| ChainId::new(*id).unwrap()).collect()
   }

   pub fn is_supported(chain_id: u64) -> bool {
      SUPPORTED_CHAINS.contains(&chain_id)
   }

   pub fn coin_symbol(&self) -> &str {
      match self {
         ChainId::BinanceSmartChain => "BNB",
         _ => "ETH",
      }
   }

   pub fn id(&self) -> u64 {
      match self {
         ChainId::Ethereum => 1,
         ChainId::EthereumSepolia => 11155111,
         ChainId::Optimism => 10,
         ChainId::BinanceSmartChain => 56,
         ChainId::Base => 8453,
         ChainId::Arbitrum => 42161,
         ChainId::RobinHood => 4663,
      }
   }

   pub fn id_as_hex(&self) -> String {
      format!("0x{:x}", self.id())
   }

   pub fn name(&self) -> &str {
      match self {
         ChainId::Ethereum => "Ethereum",
         ChainId::EthereumSepolia => "Sepolia Testnet",
         ChainId::Optimism => "Optimism",
         ChainId::BinanceSmartChain => "Binance Smart Chain",
         ChainId::Base => "Base",
         ChainId::Arbitrum => "Arbitrum",
         ChainId::RobinHood => "Robinhood",
      }
   }

   /// Block time in milliseconds
   pub fn block_time_millis(&self) -> u64 {
      match self {
         ChainId::Ethereum => 12000,
         ChainId::EthereumSepolia => 12000,
         ChainId::Optimism => 2000,
         ChainId::BinanceSmartChain => 3000,
         ChainId::Base => 2000,
         // Arbitrum doesnt have a fixed block time but lets assume on average its 250ms (based on arbscan)
         ChainId::Arbitrum => 250,
         ChainId::RobinHood => 100,
      }
   }

   /// Block time in seconds
   pub fn block_time_secs(&self) -> f32 {
      self.block_time_millis() as f32 / 1000.0
   }

   /// Block gas limit
   pub fn block_gas_limit(&self) -> u64 {
      match self {
         ChainId::Ethereum => 60_000_000,
         ChainId::EthereumSepolia => 60_000_000,
         ChainId::Optimism => 60_000_000,
         ChainId::BinanceSmartChain => 140_000_000,
         ChainId::Base => 264_000_000,
         ChainId::Arbitrum => 32_000_000,
         ChainId::RobinHood => 32_000_000,
      }
   }

   /// Block Explorer URL
   pub fn block_explorer(&self) -> &str {
      match self {
         ChainId::Ethereum => "https://etherscan.io",
         ChainId::EthereumSepolia => "https://sepolia.etherscan.io",
         ChainId::Optimism => "https://optimistic.etherscan.io",
         ChainId::BinanceSmartChain => "https://bscscan.com",
         ChainId::Base => "https://basescan.org",
         ChainId::Arbitrum => "https://arbiscan.io",
         ChainId::RobinHood => "https://robin.etherscan.io/",
      }
   }

   /// Block explorer URL for one NFT.
   ///
   /// Every supported chain is Etherscan-family, where an NFT lives at `/nft/{contract}/{tokenId}`
   /// (the older `/token/{contract}?a={tokenId}` is the same page on Etherscan). `block_explorer`
   /// carries a trailing slash for RobinHood, so it is trimmed rather than emitting `//nft/…`.
   pub fn nft_url(&self, collection: Address, token_id: U256) -> String {
      format!(
         "{}/nft/{collection}/{token_id}",
         self.block_explorer().trim_end_matches('/')
      )
   }

   /// The hardfork the local simulation runs this chain on.
   ///
   /// The single source of truth: `revm_utils::new_evm` simulates with it and the gas constants
   /// below are derived from it, so a chain moving to a new fork is one edit here. Simulating a
   /// chain on an older spec is silent — the transaction is priced by rules the chain no longer
   /// has — which is how a Sepolia transfer to a fresh account came to estimate 21,000 against the
   /// 204,600 the chain charges.
   pub fn spec(&self) -> SpecId {
      match self {
         ChainId::Ethereum => SpecId::OSAKA,
         ChainId::EthereumSepolia => SpecId::AMSTERDAM,
         ChainId::Optimism
         | ChainId::BinanceSmartChain
         | ChainId::Base
         | ChainId::Arbitrum
         | ChainId::RobinHood => SpecId::PRAGUE,
      }
   }

   /// State gas a new account is charged (EIP-8037): its state bytes at the cost per state byte.
   ///
   /// Both figures come from revm rather than being written out, because they are *parameters* —
   /// EIP-8037 was still in Last Call while Sepolia was already charging for it, and a revision that
   /// moves the cost per state byte would leave this reserve quoting the old price, silently. That
   /// is the same underprice that put the spec in the simulation's hands in the first place.
   ///
   /// Verified against a Sepolia node: 183,600.
   const NEW_ACCOUNT_STATE_GAS: u64 = NEW_ACCOUNT_BYTES * CPSB_GLAMSTERDAM;

   /// State gas a new storage slot is charged: 64 state bytes at the same rate (verified: 97,920).
   const NEW_SLOT_STATE_GAS: u64 = SSTORE_SET_BYTES * CPSB_GLAMSTERDAM;

   /// Execution gas a Railgun shield costs, before the state it creates.
   const SHIELD_EXECUTION_GAS: u64 = 500_000;

   /// Storage slots a shield writes: the commitment tree leaf and the token's balance entry.
   const SHIELD_NEW_SLOTS: u64 = 2;

   /// Minimum gas usage for a transaction — the intrinsic floor, and the base a transfer starts
   /// from.
   pub fn min_gas(&self) -> u64 {
      match self {
         ChainId::Arbitrum | ChainId::RobinHood => 97_818,
         _ => 21_000,
      }
   }

   /// Gas needed for a transfer.
   ///
   /// The send screen keeps this back from the balance to size the **Max** amount, so it covers the
   /// transfer that has to create the recipient's account rather than the common one: on a chain
   /// that meters state creation that transfer pays [`Self::NEW_ACCOUNT_STATE_GAS`] on top, which a
   /// 21,000 reserve cannot cover. A transfer to an account that already exists still costs 21,000.
   pub fn transfer_gas(&self) -> u64 {
      self.min_gas() + self.state_gas(Self::NEW_ACCOUNT_STATE_GAS)
   }

   /// Gas needed for an ERC20 Transfer
   ///
   /// This is an estimate since the actual gas cost may vary depending on the token. It carries the
   /// storage slot [`Self::NEW_SLOT_STATE_GAS`] that a transfer to a target the token has never
   /// seen creates, for the same reason as [`Self::transfer_gas`].
   pub fn erc20_transfer_gas(&self) -> u64 {
      let base = match self {
         ChainId::Arbitrum | ChainId::RobinHood => 97_818,
         _ => 50_000,
      };

      base + self.state_gas(Self::NEW_SLOT_STATE_GAS)
   }

   /// Gas needed for a private send — shielding through Railgun.
   ///
   /// The same contract as the other two: it sizes the **Max** amount, so it covers the shield that
   /// writes the most state rather than the cheapest one. A shield writes the commitment tree leaf
   /// and the token's balance entry, which a chain that meters state creation charges on top of the
   /// execution — the figure `500_000` used to stand in for both.
   pub fn privacy_transfer_gas(&self) -> u64 {
      Self::SHIELD_EXECUTION_GAS + Self::SHIELD_NEW_SLOTS * self.state_gas(Self::NEW_SLOT_STATE_GAS)
   }

   /// `state_gas` where the chain meters state creation separately, zero where it does not
   /// (EIP-8037 arrives with Amsterdam).
   fn state_gas(&self, state_gas: u64) -> u64 {
      if self.spec().is_enabled_in(SpecId::AMSTERDAM) {
         state_gas
      } else {
         0
      }
   }

   pub fn supports_type_2_tx(&self) -> bool {
      match self {
         ChainId::Ethereum => true,
         ChainId::EthereumSepolia => true,
         ChainId::Optimism => true,
         ChainId::BinanceSmartChain => false,
         ChainId::Base => true,
         ChainId::Arbitrum => true,
         ChainId::RobinHood => true,
      }
   }

   /// EIP-7702 (type-4) authorizations. Same set as type-2 chains today.
   pub fn supports_eip7702(&self) -> bool {
      self.supports_type_2_tx()
   }

   pub fn is_testnet(&self) -> bool {
      match self {
         ChainId::Ethereum => false,
         ChainId::EthereumSepolia => true,
         ChainId::Optimism => false,
         ChainId::BinanceSmartChain => false,
         ChainId::Base => false,
         ChainId::Arbitrum => false,
         ChainId::RobinHood => false,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// An unsupported chain id must not resolve. `ChainId::new` keeps its own id → variant match and
   /// `supported_chains()` unwraps it, so a missing arm is a startup panic rather than an error —
   /// this guards that contract.
   #[test]
   fn chain_new_err() {
      assert!(
         ChainId::new(1000).is_err(),
         "an unsupported chain id must not resolve to a ChainId"
      );
   }

   /// Every supported chain is Etherscan-family, where one NFT lives at `/nft/{contract}/{tokenId}`.
   /// Checked for **all** of them because `block_explorer` is hand-written per chain: RobinHood's
   /// base URL carries a trailing slash, which must not turn into `//nft`.
   #[test]
   fn nft_url_points_at_the_explorer_nft_page() {
      let collection = Address::from([0xbc; 20]);

      for id in SUPPORTED_CHAINS {
         let url = ChainId::new(id).unwrap().nft_url(collection, U256::from(1));

         assert!(url.starts_with("https://"), "{url}");
         assert!(!url.contains("//nft"), "doubled slash in {url}");
         assert!(url.ends_with("/1"), "{url}");
         assert!(url.contains(&collection.to_string()), "{url}");
      }

      assert_eq!(
         ChainId::Ethereum.nft_url(collection, U256::from(7)),
         format!("https://etherscan.io/nft/{collection}/7")
      );
   }

   /// The spec is what the reserves are derived from, so a chain that meters state creation
   /// reserves for it and one that does not keeps the legacy numbers. The Sepolia figures are what
   /// the chain charges, measured against a node.
   #[test]
   fn gas_reserves_follow_the_chain_spec() {
      assert_eq!(ChainId::EthereumSepolia.spec(), SpecId::AMSTERDAM);
      assert_eq!(
         ChainId::EthereumSepolia.transfer_gas(),
         21_000 + 183_600
      );
      assert_eq!(
         ChainId::EthereumSepolia.erc20_transfer_gas(),
         50_000 + 97_920
      );
      assert_eq!(
         ChainId::EthereumSepolia.privacy_transfer_gas(),
         500_000 + 2 * 97_920,
         "a shield writes two slots"
      );

      for id in SUPPORTED_CHAINS {
         let chain = ChainId::new(id).unwrap();
         let base = if chain.is_arbitrum() || chain.is_robinhood() {
            97_818
         } else {
            21_000
         };
         let state_gas = if chain.is_eth_sepolia() { 183_600 } else { 0 };

         assert_eq!(
            chain.transfer_gas(),
            base + state_gas,
            "{chain:?}"
         );
         assert_eq!(chain.min_gas(), base, "{chain:?}");

         let slot_gas = if chain.is_eth_sepolia() {
            2 * 97_920
         } else {
            0
         };
         assert_eq!(
            chain.privacy_transfer_gas(),
            500_000 + slot_gas,
            "{chain:?}"
         );
      }
   }

   /// Mainnet is the simulation's default spec, which is what let a Sepolia simulation run as an
   /// Osaka one without anything looking wrong.
   #[test]
   fn a_chain_names_the_fork_it_runs_on() {
      assert_eq!(ChainId::Ethereum.spec(), SpecId::OSAKA);
      assert_eq!(ChainId::Base.spec(), SpecId::PRAGUE);
      assert_eq!(ChainId::Optimism.spec(), SpecId::PRAGUE);

      for id in SUPPORTED_CHAINS {
         let chain = ChainId::new(id).unwrap();
         assert!(chain.spec() <= SpecId::NEXT, "{chain:?}");
      }
   }
}
