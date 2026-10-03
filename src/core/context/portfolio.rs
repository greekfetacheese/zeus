use crate::core::ZeusCtx;
use crate::core::serde_hashmap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::{debug, error, warn};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   nft::{NftStandard, NftToken},
   utils::NumericValue,
};
use zeus_railgun::{RailgunSigner, caip::AssetId};

type Balance = NumericValue;
type Value = NumericValue;
type Price = NumericValue;

type TokenList = Vec<(ERC20Token, Balance, Value, Price)>;

/// Helper struct that represents the total public & private value of a wallet
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WalletValue {
   pub public: NumericValue,
   pub private: NumericValue,
}

impl WalletValue {
   /// Public value, or private (Railgun) when privacy mode is on.
   pub fn for_mode(&self, privacy_mode: bool) -> &NumericValue {
      if privacy_mode {
         &self.private
      } else {
         &self.public
      }
   }
}

/// Portfolio DB persisted inside the encrypted vault.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortfolioDB {
   #[serde(default, with = "serde_hashmap")]
   pub portfolios: HashMap<(u64, Address), WalletPortfolio>,
}

impl PortfolioDB {
   pub fn new() -> Self {
      Self {
         portfolios: HashMap::new(),
      }
   }

   /// Get the wallet portfolio for the given chain and owner
   pub fn get(&self, chain_id: u64, owner: Address) -> WalletPortfolio {
      let key = (chain_id, owner);
      self
         .portfolios
         .get(&key)
         .cloned()
         .unwrap_or(WalletPortfolio::new(owner, chain_id))
   }

   /// Get all portfolios for the given chain
   pub fn get_all(&self, chain_id: u64) -> Vec<WalletPortfolio> {
      let mut portfolios = self.portfolios.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>();
      portfolios.retain(|p| p.chain_id == chain_id);
      portfolios
   }

   pub fn insert_portfolio(&mut self, chain_id: u64, owner: Address, portfolio: WalletPortfolio) {
      let key = (chain_id, owner);
      self.portfolios.insert(key, portfolio);
   }

   /// Get all tokens for the given chain and owner
   pub fn get_tokens(&self, chain_id: u64, owner: Address) -> Vec<ERC20Token> {
      let portfolio = self.get(chain_id, owner);
      portfolio.tokens.clone()
   }

   /// Get all NFTs for the given chain and owner
   pub fn get_nfts(&self, chain_id: u64, owner: Address) -> Vec<NftToken> {
      let portfolio = self.get(chain_id, owner);
      portfolio.nfts
   }

   /// Drop portfolios whose owner is not in `wallets`. Returns how many entries were removed.
   pub fn retain_wallets(&mut self, wallets: &HashSet<Address>) -> usize {
      let before = self.portfolios.len();
      self.portfolios.retain(|(_chain, owner), _| wallets.contains(owner));
      self.portfolios.shrink_to_fit();
      before.saturating_sub(self.portfolios.len())
   }
}

/// Wallet portfolio persisted inside the encrypted vault.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletPortfolio {
   /// All the tokens in the wallet
   #[serde(default)]
   tokens: Vec<ERC20Token>,
   /// NFTs (ERC-721 / ERC-1155) tracked for this wallet.
   ///
   /// Kept beside `tokens` rather than inside a `TokenList`: a `TokenList` entry is
   /// `(ERC20Token, balance, value, price)`, and an NFT has none of those — no `decimals` to format
   /// an amount with, and no pool price to value it by.
   #[serde(default)]
   nfts: Vec<NftToken>,
   /// The subset of `nfts` that the last private balance scan found in Railgun custody.
   ///
   /// Replaced wholesale by every scan, like `private_tokens` and unlike `nfts`: the private side is the
   /// one place where a scan **is** the whole truth. This is what the privacy-mode picker lists, because
   /// only a shielded NFT can be unshielded or privately transferred.
   #[serde(default)]
   private_nfts: Vec<NftToken>,
   /// Chain ID
   #[serde(default)]
   chain_id: u64,
   /// Wallet owner
   #[serde(default)]
   owner: Address,
   /// Estimated USD of the public value of the portfolio
   #[serde(default)]
   public_value: NumericValue,
   /// Estimated USD of the private value of the portfolio
   #[serde(default)]
   private_value: NumericValue,
   /// Cached and sorted list of public tokens by value
   #[serde(default)]
   public_tokens: TokenList,
   /// Cached and sorted list of private tokens by value
   #[serde(default)]
   private_tokens: TokenList,
}

impl WalletPortfolio {
   pub fn new(owner: Address, chain_id: u64) -> Self {
      Self {
         tokens: Vec::new(),
         nfts: Vec::new(),
         private_nfts: Vec::new(),
         chain_id,
         owner,
         public_value: NumericValue::default(),
         private_value: NumericValue::default(),
         public_tokens: Vec::new(),
         private_tokens: Vec::new(),
      }
   }

   pub fn tokens(&self) -> &Vec<ERC20Token> {
      &self.tokens
   }

   pub fn nfts(&self) -> &Vec<NftToken> {
      &self.nfts
   }

   /// NFTs held in Railgun custody, per the last private balance scan.
   pub fn private_nfts(&self) -> &Vec<NftToken> {
      &self.private_nfts
   }

   pub fn public_tokens(&self) -> &TokenList {
      &self.public_tokens
   }

   pub fn private_tokens(&self) -> &TokenList {
      &self.private_tokens
   }

   pub fn chain_id(&self) -> u64 {
      self.chain_id
   }

   pub fn owner(&self) -> Address {
      self.owner
   }

   /// Returns the total value of the portfolio (public + private)
   pub fn total_value(&self) -> WalletValue {
      WalletValue {
         public: self.public_value.clone(),
         private: self.private_value.clone(),
      }
   }

   pub fn public_value(&self) -> NumericValue {
      self.public_value.clone()
   }

   pub fn private_value(&self) -> NumericValue {
      self.private_value.clone()
   }

   pub fn set_public_value(&mut self, value: NumericValue) {
      self.public_value = value;
   }

   pub fn set_private_value(&mut self, value: NumericValue) {
      self.private_value = value;
   }

   pub fn add_token(&mut self, token: ERC20Token) {
      if self.tokens.contains(&token) {
         return;
      }
      self.tokens.push(token);
   }

   pub fn has_token(&self, token: &ERC20Token) -> bool {
      self.tokens.contains(token)
   }

   pub fn has_private_tokens(&self) -> bool {
      self.private_tokens().len() > 0
   }

   pub fn remove_token(&mut self, token: &ERC20Token) {
      self.tokens.retain(|t| t != token);
   }

   /// Track an NFT for this wallet.
   ///
   /// Identity is `(chain, collection, token id)`, so re-adding a token we already track is not a
   /// duplicate — but it does refresh the cached metadata URI, so a token added before its URI was
   /// known picks it up instead of making us read `tokenURI` from the chain again.
   pub fn add_nft(&mut self, nft: NftToken) {
      match self.nfts.iter().position(|tracked| tracked == &nft) {
         Some(index) => {
            if nft.metadata_uri.is_some() {
               self.nfts[index].metadata_uri = nft.metadata_uri;
            }
         }
         None => self.nfts.push(nft),
      }
   }

   pub fn has_nft(&self, nft: &NftToken) -> bool {
      self.nfts.contains(nft)
   }

   pub fn remove_nft(&mut self, nft: &NftToken) {
      self.nfts.retain(|tracked| tracked != nft);
   }

   /// Update the public data for the portfolio
   ///
   /// What it does:
   ///
   /// - Calculates the public token list and sorts it by value
   /// - Updates the portfolio public value based on the latest price data
   pub fn update_public_data(&mut self, ctx: ZeusCtx) {
      let chain_id = self.chain_id;
      let owner = self.owner;
      let tokens = &self.tokens;
      let mut value = 0.0;

      let public_tokens = process_public_tokens(ctx.clone(), chain_id, owner, tokens);

      for (_token, _balance, token_value, _price) in &public_tokens {
         value += token_value.f64();
      }

      let eth = Currency::native(chain_id);
      let eth_price = ctx.get_currency_price(&eth);
      let balance = ctx.get_eth_balance(chain_id, owner);
      let eth_value = eth_price.f64() * balance.f64();
      value += eth_value;

      let new_value = NumericValue::from_f64(value);

      self.set_public_value(new_value);
      self.public_tokens = public_tokens;
   }

   /// Update the private data for the portfolio
   ///
   /// What it does:
   ///
   /// - Indexes the private tokens and sorts them by value
   /// - Updates the portfolio private value based on the latest price data
   pub async fn update_private_data(&mut self, ctx: ZeusCtx) {
      let chain_id = self.chain_id;
      let owner = self.owner;

      let mut private_tokens = self.private_tokens.clone();

      let updated_holdings = match process_private_tokens(ctx.clone(), chain_id, owner).await {
         Ok(holdings) => holdings,
         Err(e) => {
            // Leave both lists alone: a transport failure is not evidence of an empty wallet.
            error!("Error calculating private tokens: {:?}", e);
            PrivateHoldings {
               tokens: private_tokens,
               nfts: self.private_nfts.clone(),
            }
         }
      };

      private_tokens = updated_holdings.tokens;

      // Private NFTs join the portfolio's list. A **union, not a replacement**: that list also holds what
      // the wallet owns publicly, and a scan cannot tell "no longer held" apart from "not mine to see".
      // The private list is the opposite — replaced, because this scan *is* the whole truth for it.
      for nft in &updated_holdings.nfts {
         self.add_nft(nft.clone());
      }

      self.private_nfts = updated_holdings.nfts;

      let mut value = 0.0;

      for (_token, _balance, token_value, _price) in &private_tokens {
         value += token_value.f64();
      }

      let new_value = NumericValue::from_f64(value);

      self.set_private_value(new_value);
      self.private_tokens = private_tokens;
   }
}

fn process_public_tokens(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
   tokens: &Vec<ERC20Token>,
) -> TokenList {
   let mut token_list: TokenList = tokens
      .iter()
      .map(|token| {
         let price = ctx.get_token_price(token);
         let balance = ctx.get_token_balance(chain_id, owner, token.address);
         let value = ctx.get_token_value_for_owner(chain_id, owner, token);
         (token.clone(), balance, value, price)
      })
      .collect();

   token_list
      .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));

   token_list
}

/// What one private balance scan yields.
///
/// The two sides are asymmetric on purpose: a private ERC-20 has a balance and a price, so it is valued
/// and sorted, while an NFT has neither — it is either held or it is not — so it comes back as a plain
/// list.
struct PrivateHoldings {
   tokens: TokenList,
   nfts: Vec<NftToken>,
}

/// The NFT an `AssetId` names, with no lookup at all.
///
/// Everything but the display name and the art is already known from the balance scan: the collection
/// and the id are the asset, and the standard is the variant itself.
fn private_nft(chain_id: u64, asset: &AssetId) -> Option<NftToken> {
   match asset {
      AssetId::Erc721(collection, token_id) => Some(NftToken {
         chain_id,
         collection: *collection,
         token_id: *token_id,
         standard: NftStandard::Erc721,
         metadata_uri: None,
      }),
      // An ERC-1155 balance is a quantity of an id, so it stays out of scope for now (D6).
      _ => None,
   }
}

async fn process_private_tokens(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
) -> Result<PrivateHoldings, anyhow::Error> {
   let mut token_list: TokenList = Vec::new();
   let mut nft_list: Vec<NftToken> = Vec::new();

   if !ctx.railgun_is_supported(chain_id.into()) || !ctx.is_railgun_enabled(chain_id) {
      return Ok(PrivateHoldings {
         tokens: token_list,
         nfts: nft_list,
      });
   }

   let mut provider = ctx.get_railgun_provider(chain_id, false).await?;

   let Some(wallet) = ctx.get_wallet(owner) else {
      #[cfg(feature = "dev")]
      error!("Wallet not found for address {}", owner);
      return Ok(PrivateHoldings {
         tokens: token_list,
         nfts: nft_list,
      });
   };

   if !wallet.can_derive_zk_address() {
      debug!(
         "Wallet {} cannot derive a zkAddress",
         wallet.name_with_id()
      );
      return Ok(PrivateHoldings {
         tokens: token_list,
         nfts: nft_list,
      });
   }

   let seed = wallet.seed()?;
   let raligun_signer = RailgunSigner::from_seed(&seed, 0, chain_id)?;
   let railgun_address = raligun_signer.address().clone();

   let private_balances = provider.balance(railgun_address).await;

   #[cfg(feature = "dev")]
   debug!(
      "Found {} private balances",
      private_balances.len()
   );

   for entry in private_balances {
      match &entry.asset {
         AssetId::Erc20(address) => {
            let erc20 = ctx.get_token(chain_id, *address).await?;
            let balance = NumericValue::format_wei(U256::from(entry.amount), erc20.decimals);
            let price = ctx.get_token_price(&erc20);
            let value = NumericValue::value(balance.f64(), price.f64());
            token_list.push((erc20.clone(), balance, value, price));
         }
         asset => {
            let Some(fallback) = private_nft(chain_id, asset) else {
               continue;
            };

            // A privately held NFT is real whatever the metadata call says, so a failed lookup costs the
            // name and the art and nothing else — the placeholder stands in until something resolves it.
            let token = match ctx.get_nft(chain_id, fallback.collection, fallback.token_id).await {
               Ok(token) => token,
               Err(e) => {
                  warn!(
                     "Could not resolve privately held NFT {} #{}: {}",
                     fallback.collection, fallback.token_id, e
                  );
                  fallback
               }
            };

            if !nft_list.contains(&token) {
               nft_list.push(token);
            }
         }
      }
   }

   token_list
      .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));

   Ok(PrivateHoldings {
      tokens: token_list,
      nfts: nft_list,
   })
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::nft::NftStandard;

   fn owner() -> Address {
      Address::from([0x11; 20])
   }

   fn nft(token_id: u64) -> NftToken {
      NftToken {
         chain_id: 1,
         collection: Address::from([0xbc; 20]),
         token_id: U256::from(token_id),
         standard: NftStandard::Erc721,
         metadata_uri: None,
      }
   }

   /// The private list is persisted with the portfolio. An older payload has no key at all and must keep
   /// loading, and one that carries a private NFT must keep it — this list is what privacy mode lists.
   #[test]
   fn private_nfts_are_persisted_and_optional() {
      let portfolio = WalletPortfolio::new(owner(), 1);

      let mut stored = serde_json::to_value(&portfolio).unwrap();
      stored.as_object_mut().unwrap().remove("private_nfts");

      let restored: WalletPortfolio = serde_json::from_value(stored).unwrap();
      assert!(restored.private_nfts().is_empty());

      let mut stored = serde_json::to_value(&portfolio).unwrap();
      stored.as_object_mut().unwrap().insert(
         "private_nfts".to_string(),
         serde_json::to_value(vec![nft(1)]).unwrap(),
      );

      let restored: WalletPortfolio = serde_json::from_value(stored).unwrap();
      assert_eq!(restored.private_nfts(), &vec![nft(1)]);
   }

   /// Tracking an NFT is not the same as holding it privately. `nfts` is everything the wallet is known
   /// to hold, `private_nfts` is what a Railgun balance scan found, and only privacy mode reads the
   /// latter — a token in the wrong one of those two lists is a token offered for the wrong action.
   #[test]
   fn tracking_an_nft_does_not_make_it_private() {
      let mut portfolio = WalletPortfolio::new(owner(), 1);
      portfolio.add_nft(nft(1));

      assert!(portfolio.has_nft(&nft(1)));
      assert!(portfolio.private_nfts().is_empty());
   }

   /// The balance scan already knows the collection and the id — the asset *is* the pair — so an NFT it
   /// reports has to arrive complete enough to store and show, with no metadata call at all.
   #[test]
   fn a_private_erc721_balance_names_its_token_without_a_lookup() {
      let nft = private_nft(
         1,
         &AssetId::Erc721(Address::from([0xbc; 20]), U256::from(7)),
      )
      .unwrap();

      assert_eq!(nft.chain_id, 1);
      assert_eq!(nft.collection, Address::from([0xbc; 20]));
      assert_eq!(nft.token_id, U256::from(7));
      assert_eq!(nft.standard, NftStandard::Erc721);
      assert_eq!(
         nft.metadata_uri, None,
         "the art is a later, optional step"
      );
   }

   /// ERC-1155 stays out of scope (D6): a private 1155 balance is a *quantity* of an id, and this list has
   /// nowhere to record an amount — the NFT list treats a token as held or not.
   #[test]
   fn private_erc20_and_erc1155_balances_are_not_nfts() {
      let erc1155 = AssetId::Erc1155(Address::from([0x11; 20]), U256::from(1));
      let erc20 = AssetId::Erc20(Address::from([0x22; 20]));

      assert!(private_nft(1, &erc1155).is_none());
      assert!(private_nft(1, &erc20).is_none());
   }

   /// The private scan feeds the portfolio by union. It cannot know what the wallet holds publicly, so an
   /// NFT it does not mention must survive the update — losing it would make a token vanish from the UI
   /// because an unrelated balance fetch said nothing about it.
   #[test]
   fn the_private_scan_is_additive_for_nfts() {
      let mut portfolio = WalletPortfolio::new(owner(), 1);
      portfolio.add_nft(nft(1));
      portfolio.add_nft(nft(2));

      // One scan finding one of them again, the way `update_private_data` applies it.
      portfolio.add_nft(nft(2));

      assert_eq!(portfolio.nfts().len(), 2);
      assert!(portfolio.has_nft(&nft(1)));
      assert!(portfolio.has_nft(&nft(2)));
   }

   #[test]
   fn nfts_are_added_looked_up_and_removed_by_identity() {
      let mut portfolio = WalletPortfolio::new(owner(), 1);

      portfolio.add_nft(nft(1));
      portfolio.add_nft(nft(2));

      assert_eq!(portfolio.nfts().len(), 2);
      assert!(portfolio.has_nft(&nft(1)));
      assert!(!portfolio.has_nft(&nft(3)));

      portfolio.remove_nft(&nft(1));

      assert!(!portfolio.has_nft(&nft(1)));
      assert!(
         portfolio.has_nft(&nft(2)),
         "only the named token goes"
      );
      assert_eq!(portfolio.nfts().len(), 1);
   }

   /// Sibling tokens in one collection are separate entries, and token id 0 is a real token rather
   /// than an "empty" value that gets skipped.
   #[test]
   fn sibling_tokens_and_token_id_zero_are_distinct_entries() {
      let mut portfolio = WalletPortfolio::new(owner(), 1);

      portfolio.add_nft(nft(0));
      portfolio.add_nft(nft(1));

      assert_eq!(portfolio.nfts().len(), 2);
      assert!(portfolio.has_nft(&nft(0)));
   }

   /// Identity carries the chain, so removing a token on one chain leaves the same collection and
   /// token id on another chain alone.
   #[test]
   fn identity_includes_the_chain() {
      let mut other_chain = nft(1);
      other_chain.chain_id = 137;

      let mut portfolio = WalletPortfolio::new(owner(), 1);
      portfolio.add_nft(nft(1));
      portfolio.add_nft(other_chain.clone());

      assert_eq!(
         portfolio.nfts().len(),
         2,
         "the same collection and token id on another chain is a different NFT"
      );

      portfolio.remove_nft(&other_chain);

      assert!(
         portfolio.has_nft(&nft(1)),
         "the chain-1 entry survives"
      );
      assert!(!portfolio.has_nft(&other_chain));
   }

   /// Metadata is not part of identity: a refresh must not create a second entry, it must actually
   /// take (so the icon pipeline need not re-read `tokenURI`), and it must not be wiped by a later
   /// add that carries no URI.
   #[test]
   fn re_adding_a_token_refreshes_its_metadata_instead_of_duplicating() {
      let mut portfolio = WalletPortfolio::new(owner(), 1);
      portfolio.add_nft(nft(1));

      let mut refreshed = nft(1);
      refreshed.metadata_uri = Some("ipfs://QmExample/1".to_string());
      portfolio.add_nft(refreshed);

      assert_eq!(
         portfolio.nfts().len(),
         1,
         "identity is (chain, collection, token id)"
      );
      assert_eq!(
         portfolio.nfts()[0].metadata_uri.as_deref(),
         Some("ipfs://QmExample/1"),
         "a newly known URI is kept"
      );

      portfolio.add_nft(nft(1));

      assert_eq!(
         portfolio.nfts()[0].metadata_uri.as_deref(),
         Some("ipfs://QmExample/1"),
         "a later add without a URI must not clear the one we have"
      );
   }

   #[test]
   fn nfts_are_scoped_to_the_chain_and_owner() {
      let mut db = PortfolioDB::new();

      let mut chain_one = WalletPortfolio::new(owner(), 1);
      chain_one.add_nft(nft(1));
      db.insert_portfolio(1, owner(), chain_one);

      let mut chain_ten = WalletPortfolio::new(owner(), 10);
      chain_ten.add_nft(nft(2));
      db.insert_portfolio(10, owner(), chain_ten);

      assert_eq!(db.get_nfts(1, owner())[0].token_id, U256::from(1));
      assert_eq!(
         db.get_nfts(10, owner())[0].token_id,
         U256::from(2)
      );
      assert!(
         db.get_nfts(1, Address::from([0x22; 20])).is_empty(),
         "another owner is an empty portfolio, not a leak from this one"
      );
   }

   /// Dropping a wallet drops its NFTs with it — no orphaned entries left behind.
   #[test]
   fn removing_a_wallet_drops_its_nfts() {
      let mut db = PortfolioDB::new();

      let mut portfolio = WalletPortfolio::new(owner(), 1);
      portfolio.add_nft(nft(1));
      db.insert_portfolio(1, owner(), portfolio);

      assert_eq!(db.retain_wallets(&HashSet::new()), 1);
      assert!(db.get_nfts(1, owner()).is_empty());
   }

   /// A vault written before NFTs existed has no `nfts` key at all, and must still open — the field
   /// is `#[serde(default)]`.
   #[test]
   fn a_portfolio_saved_before_nfts_existed_still_loads() {
      let mut value = serde_json::to_value(WalletPortfolio::new(owner(), 1)).unwrap();

      assert!(
         value.as_object_mut().unwrap().remove("nfts").is_some(),
         "the field is serialized, so removing it really does simulate an older payload"
      );

      let loaded: WalletPortfolio = serde_json::from_value(value).unwrap();

      assert!(loaded.nfts().is_empty());
   }

   #[test]
   fn nfts_survive_a_round_trip() {
      let mut portfolio = WalletPortfolio::new(owner(), 1);
      portfolio.add_nft(nft(1));
      portfolio.add_nft(nft(2));

      let json = serde_json::to_vec(&portfolio).unwrap();
      let loaded: WalletPortfolio = serde_json::from_slice(&json).unwrap();

      assert_eq!(loaded.nfts(), portfolio.nfts());
   }
}
