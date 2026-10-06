use crate::core::ZeusCtx;
use crate::core::serde_hashmap;
use crate::utils::RT;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use tokio::{sync::Semaphore, time::sleep};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{ERC20Token, NativeCurrency},
   nft::{NftToken, verify_ownership_batch},
   utils::{NumericValue, batch},
};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct BalanceManagerHandle(Arc<RwLock<BalanceManager>>);

impl Default for BalanceManagerHandle {
   fn default() -> Self {
      Self(Arc::new(RwLock::new(BalanceManager::default())))
   }
}

impl Serialize for BalanceManagerHandle {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      self.read(|m| m.serialize(serializer))
   }
}

impl<'de> Deserialize<'de> for BalanceManagerHandle {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let manager = BalanceManager::deserialize(deserializer)?;
      Ok(Self::new(manager))
   }
}

impl BalanceManagerHandle {
   pub fn new(balance_manager: BalanceManager) -> Self {
      Self(Arc::new(RwLock::new(balance_manager)))
   }

   pub fn read<R>(&self, reader: impl FnOnce(&BalanceManager) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut BalanceManager) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   pub fn reset_default_settings(&self) {
      self.write(|manager| {
         manager.concurrency = default_concurrency();
         manager.batch_size = default_batch_size();
         manager.max_retries = default_max_retries();
         manager.retry_delay = default_retry_delay();
      });
   }

   pub fn set_concurrency(&self, concurrency: usize) {
      self.write(|manager| manager.concurrency = concurrency);
   }

   pub fn set_batch_size(&self, batch_size: usize) {
      self.write(|manager| manager.batch_size = batch_size);
   }

   pub fn concurrency(&self) -> usize {
      let concurrency = self.read(|manager| manager.concurrency);
      if concurrency == 0 { 1 } else { concurrency }
   }

   pub fn batch_size(&self) -> usize {
      let size = self.read(|manager| manager.batch_size);
      if size == 0 {
         default_batch_size()
      } else {
         size
      }
   }

   fn max_retries(&self) -> usize {
      let retries = self.read(|manager| manager.max_retries);
      if retries == 0 {
         default_max_retries()
      } else {
         retries
      }
   }

   fn retry_delay(&self) -> u64 {
      let delay = self.read(|manager| manager.retry_delay);
      if delay == 0 {
         default_retry_delay()
      } else {
         delay
      }
   }

   /// `retry_if_unchanged` true if we expect the balance to change,
   /// for example after a tx
   pub async fn update_eth_balance(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      owners: Vec<Address>,
      retry_if_unchanged: bool,
   ) -> Result<(), anyhow::Error> {
      if owners.is_empty() {
         return Ok(());
      }

      let client = ctx.get_client_manager();
      let batch_size = self.batch_size();
      let max_retries = self.max_retries();
      let retry_delay = self.retry_delay();
      let native = NativeCurrency::from(chain);

      for chunk in owners.chunks(batch_size) {
         let chunk = chunk.to_vec();
         let old_balances: HashMap<Address, NumericValue> = if retry_if_unchanged {
            chunk.iter().map(|&owner| (owner, self.get_eth_balance(chain, owner))).collect()
         } else {
            HashMap::new()
         };

         for attempt in 0..=max_retries {
            let balances = match client
               .request(chain, |client| {
                  let chunk = chunk.clone();
                  async move { batch::get_eth_balances(client, chain, None, chunk).await }
               })
               .await
            {
               Ok(b) => b,
               Err(_e) => {
                  #[cfg(feature = "dev")]
                  tracing::error!(
                     "Failed to get ETH balances for ChainId: {chain:?} Error: {_e:?}"
                  );
                  if attempt == max_retries {
                     return Err(anyhow!("Max retries reached"));
                  }
                  sleep(Duration::from_millis(retry_delay)).await;
                  continue;
               }
            };

            let unchanged = retry_if_unchanged
               && balances.iter().any(|balance| {
                  old_balances.get(&balance.owner).is_some_and(|old| balance.balance == old.wei())
               });

            if unchanged {
               #[cfg(feature = "dev")]
               tracing::debug!(
                  "ETH balances unchanged for chain {}, retrying",
                  chain
               );

               if attempt == max_retries {
                  return Err(anyhow!("Max retries reached"));
               }
               sleep(Duration::from_millis(retry_delay)).await;
               continue;
            }

            for balance in balances {
               self.insert_eth_balance(chain, balance.owner, balance.balance, &native);
            }
            break;
         }
      }

      self.write(|manager| {
         manager.eth_balances.shrink_to_fit();
      });
      Ok(())
   }

   /// `retry_if_unchanged` true if we expect the balance to change,
   /// for example after a swap involving the tokens
   pub async fn update_tokens_balance(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      owner: Address,
      tokens: Vec<ERC20Token>,
      retry_if_unchanged: bool,
   ) -> Result<(), anyhow::Error> {
      if tokens.is_empty() {
         return Ok(());
      }

      let client = ctx.get_client_manager();
      let semaphore = Arc::new(Semaphore::new(self.concurrency()));
      let token_map: Arc<HashMap<Address, ERC20Token>> =
         Arc::new(tokens.iter().map(|token| (token.address, token.clone())).collect());
      let tokens_addr = tokens.iter().map(|t| t.address).collect::<Vec<_>>();

      let mut tasks = Vec::new();
      let batch_size = self.batch_size();
      let max_retries = self.max_retries();
      let retry_delay = self.retry_delay();

      for chunk in tokens_addr.chunks(batch_size) {
         let client = client.clone();
         let semaphore = semaphore.clone();
         let manager = self.clone();
         let token_map = token_map.clone();
         let tokens_addr = chunk.to_vec();

         let old_balances: HashMap<Address, NumericValue> = if retry_if_unchanged {
            tokens_addr
               .iter()
               .map(|&token| (token, self.get_token_balance(chain, owner, token)))
               .collect()
         } else {
            HashMap::new()
         };

         let task = RT.spawn(async move {
            for attempt in 0..=max_retries {
               let balances = {
                  let _permit = semaphore.acquire().await?;
                  client
                     .request(chain, |client| {
                        let tokens_addr = tokens_addr.clone();
                        async move {
                           batch::get_erc20_balances(client, chain, None, owner, tokens_addr).await
                        }
                     })
                     .await
               };

               let balances = match balances {
                  Ok(b) => b,
                  Err(_e) => {
                     #[cfg(feature = "dev")]
                     tracing::error!(
                        "Failed to get erc20 balances for Owner: {owner:?} ChainId: {chain:?} Error: {_e:?}"
                     );
                     if attempt == max_retries {
                        tracing::error!("Max retries reached");
                        break;
                     }
                     sleep(Duration::from_millis(retry_delay)).await;
                     continue;
                  }
               };

               let unchanged = retry_if_unchanged
                  && balances.iter().any(|balance| {
                     old_balances
                        .get(&balance.token)
                        .is_some_and(|old| balance.balance == old.wei())
                  });

               if unchanged {
                  #[cfg(feature = "dev")]
                  tracing::warn!(
                     "Token balances unchanged for owner {} on chain {}, retrying",
                     owner,
                     chain
                  );

                  if attempt == max_retries {
                     tracing::error!("Max retries reached");
                     break;
                  }
                  sleep(Duration::from_millis(retry_delay)).await;
                  continue;
               }

               for balance in &balances {
                  let Some(token) = token_map.get(&balance.token) else {
                     #[cfg(feature = "dev")]
                     tracing::error!("Token not found: {}", balance.token);
                     continue;
                  };
                  manager.insert_token_balance(chain, owner, balance.balance, token);
               }

               #[cfg(feature = "dev")]
               tracing::debug!("Updated balances for {} tokens", balances.len());
               break;
            }

            Ok::<(), anyhow::Error>(())
         });
         tasks.push(task);
      }

      for task in tasks {
         match task.await {
            Ok(Ok(())) => (),
            Ok(Err(e)) => tracing::error!("Error updating token balance: {:?}", e),
            Err(e) => tracing::error!("Error updating token balance: {:?}", e),
         }
      }

      self.write(|manager| {
         manager.token_balances.shrink_to_fit();
      });

      Ok(())
   }

   /// `retry_if_unchanged` true if we expect ownership to change, for example after a tx.
   ///
   /// One Multicall3 aggregate per standard covers the whole list, so unlike the token path there is
   /// nothing to chunk: the batch is the round trip. Answers are written per token id, `0` included —
   /// it is a real answer (the chain says the wallet has none) and not a missing one.
   pub async fn update_nft_balances(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      owner: Address,
      nfts: Vec<NftToken>,
      retry_if_unchanged: bool,
   ) -> Result<(), anyhow::Error> {
      if nfts.is_empty() {
         return Ok(());
      }

      // Only ids that have already been answered: an absent entry is not an expectation, and retrying on
      // it would spin on the first token we never asked about.
      let before: HashMap<batch::NftRef, u64> = if retry_if_unchanged {
         nfts
            .iter()
            .filter_map(|nft| {
               self
                  .get_nft_balance(chain, owner, nft.collection, nft.token_id)
                  .map(|amount| ((nft.collection, nft.token_id), amount))
            })
            .collect()
      } else {
         HashMap::new()
      };

      let max_retries = self.max_retries();
      let retry_delay = self.retry_delay();

      for attempt in 0..=max_retries {
         let client = match ctx.get_client(chain).await {
            Ok(client) => client,
            Err(e) => {
               if attempt == max_retries {
                  return Err(anyhow!(
                     "Failed to get client for chain {chain}: {e:?}"
                  ));
               }
               sleep(Duration::from_millis(retry_delay)).await;
               continue;
            }
         };

         let Some(holdings) = verify_ownership_batch(client, owner, &nfts).await else {
            if attempt == max_retries {
               return Err(anyhow!("Max retries reached"));
            }
            sleep(Duration::from_millis(retry_delay)).await;
            continue;
         };

         // Ownership only ever moves *after* the tx that moves it, so the round is worth waiting on —
         // but only while nothing has moved. One moved id is enough to write the whole round.
         if awaiting_change(retry_if_unchanged, &before, &holdings) {
            if attempt == max_retries {
               return Err(anyhow!("Max retries reached"));
            }
            sleep(Duration::from_millis(retry_delay)).await;
            continue;
         }

         for nft in &nfts {
            let amount = holdings.get(&(nft.collection, nft.token_id)).copied().unwrap_or(0);
            self.insert_nft_balance(chain, owner, nft.collection, nft.token_id, amount);
         }

         self.write(|manager| {
            manager.nft_balances.shrink_to_fit();
         });

         return Ok(());
      }

      Ok(())
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.read(|manager| manager.eth_balances.get(&(chain, owner)).cloned().unwrap_or_default())
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self.read(|manager| {
         manager.token_balances.get(&(chain, owner, token)).cloned().unwrap_or_default()
      })
   }

   pub fn insert_eth_balance(
      &self,
      chain: u64,
      owner: Address,
      balance: U256,
      currency: &NativeCurrency,
   ) {
      let balance = NumericValue::currency_balance(balance, currency.decimals);
      self.write(|manager| {
         manager.eth_balances.insert((chain, owner), balance);
      });
   }

   pub fn insert_token_balance(
      &self,
      chain: u64,
      owner: Address,
      balance: U256,
      token: &ERC20Token,
   ) {
      let balance = NumericValue::currency_balance(balance, token.decimals);
      self.write(|manager| {
         manager.token_balances.insert((chain, owner, token.address), balance);
      });
   }

   /// How many of this NFT the wallet holds, or `None` when it has never been asked.
   ///
   /// The `Option` is the point. `Some(0)` — the chain says the wallet has none — is a row's
   /// `NOT OWNED`; `None`, nobody has asked, is a row showing no ownership claim at all.
   pub fn get_nft_balance(
      &self,
      chain: u64,
      owner: Address,
      collection: Address,
      token_id: U256,
   ) -> Option<u64> {
      self.read(|manager| manager.nft_balances.get(&(chain, owner, collection, token_id)).copied())
   }

   pub fn insert_nft_balance(
      &self,
      chain: u64,
      owner: Address,
      collection: Address,
      token_id: U256,
      amount: u64,
   ) {
      self.write(|manager| {
         manager.nft_balances.insert((chain, owner, collection, token_id), amount);
      });
   }

   /// Drop balance entries whose owner is not in `wallets`.
   ///
   /// Returns `(eth_removed, token_removed, nft_removed)`.
   pub fn retain_wallets(&self, wallets: &HashSet<Address>) -> (usize, usize, usize) {
      self.write(|manager| {
         let eth_before = manager.eth_balances.len();
         manager.eth_balances.retain(|(_chain, owner), _| wallets.contains(owner));
         manager.eth_balances.shrink_to_fit();
         let eth_removed = eth_before.saturating_sub(manager.eth_balances.len());

         let token_before = manager.token_balances.len();
         manager
            .token_balances
            .retain(|(_chain, owner, _token), _| wallets.contains(owner));
         manager.token_balances.shrink_to_fit();
         let token_removed = token_before.saturating_sub(manager.token_balances.len());

         let nft_before = manager.nft_balances.len();
         manager
            .nft_balances
            .retain(|(_chain, owner, _collection, _id), _| wallets.contains(owner));
         manager.nft_balances.shrink_to_fit();
         let nft_removed = nft_before.saturating_sub(manager.nft_balances.len());

         (eth_removed, token_removed, nft_removed)
      })
   }

   /// Remove all entries that have a 0 balance
   ///
   /// This will save up space and the manager will still return 0 balance for the removed entries
   ///
   /// NFTs are left alone: a token's `0` and its absence read the same, but an NFT's `0` is the
   /// `NOT OWNED` answer and its absence is «never asked» — dropping one would silently take a badge
   /// away. The map only holds tokens the wallet was asked about, so there is little to reclaim.
   ///
   /// # Returns
   ///
   /// The number of removed entries (eth, tokens)
   pub fn remove_zero_balances(&self) -> (usize, usize) {
      self.write(|manager| {
         let eth_before = manager.eth_balances.len();
         let token_before = manager.token_balances.len();

         manager.eth_balances.retain(|_, balance| !balance.is_zero());
         manager.eth_balances.shrink_to_fit();
         manager.token_balances.retain(|_, balance| !balance.is_zero());
         manager.token_balances.shrink_to_fit();

         let eth_removed = eth_before.saturating_sub(manager.eth_balances.len());
         let token_removed = token_before.saturating_sub(manager.token_balances.len());

         (eth_removed, token_removed)
      })
   }
}

/// Whether a balance round should be retried rather than written.
///
/// `retry_if_unchanged` is the caller saying it just sent a tx, so the balances should be about to move.
/// A round counts as *unchanged* only when **none** of the ids that already had an answer has moved: one
/// moved id means the chain has caught up, and the round is then written for every id in it.
///
/// The `all` is the whole point, and `any` is the trap. An NFT update answers about a whole portfolio at
/// once, so `any` retries on whichever tokens were untouched, exhausts the retries and returns *without
/// writing anything* — which is how a shield left every badge stale and logged a «Max retries reached».
/// With nothing answered yet there is nothing to wait for either, so the round is written straight away.
fn awaiting_change(
   retry_if_unchanged: bool,
   before: &HashMap<batch::NftRef, u64>,
   after: &HashMap<batch::NftRef, u64>,
) -> bool {
   retry_if_unchanged
      && !before.is_empty()
      && before.iter().all(|(key, old)| after.get(key).copied().unwrap_or(0) == *old)
}

fn default_concurrency() -> usize {
   1
}

fn default_max_retries() -> usize {
   10
}

fn default_retry_delay() -> u64 {
   500
}

fn default_batch_size() -> usize {
   20
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BalanceManager {
   /// Eth Balances (or any native currency for evm compatable chains)
   #[serde(default, with = "serde_hashmap")]
   pub eth_balances: HashMap<(u64, Address), NumericValue>,

   /// Token Balances key: (chain, owner, token)
   #[serde(default, with = "serde_hashmap")]
   pub token_balances: HashMap<(u64, Address, Address), NumericValue>,

   /// NFT holdings: how many of a token id the wallet owns. Key: (chain, owner, collection, token id).
   ///
   /// An ERC-721 is only ever `0` or `1`; an ERC-1155 carries its real amount, so one map serves both —
   /// the row needs a number and the standard is already on the `NftToken`.
   ///
   /// `0` is a real answer: the chain says the wallet has none. An **absent** entry means nobody has
   /// asked yet, and the two must read differently — `NOT OWNED` against no badge at all — which is why
   /// `get_nft_balance` returns an `Option` where the token getters can fall back to zero.
   #[serde(default, with = "serde_hashmap")]
   pub nft_balances: HashMap<(u64, Address, Address, U256), u64>,

   #[serde(default = "default_concurrency")]
   pub concurrency: usize,
   #[serde(default = "default_max_retries")]
   pub max_retries: usize,
   #[serde(default = "default_retry_delay")]
   pub retry_delay: u64,
   #[serde(default = "default_batch_size")]
   pub batch_size: usize,
}

impl Default for BalanceManager {
   fn default() -> Self {
      Self {
         eth_balances: HashMap::new(),
         token_balances: HashMap::new(),
         nft_balances: HashMap::new(),
         concurrency: default_concurrency(),
         max_retries: default_max_retries(),
         retry_delay: default_retry_delay(),
         batch_size: default_batch_size(),
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[tokio::test]
   async fn test_update_tokens_balance() {
      let ctx = ZeusCtx::new();
      let chain = 1;

      let manager = ctx.balance_manager();
      let owner = Address::ZERO;
      let tokens = vec![ERC20Token::weth()];

      manager
         .update_tokens_balance(ctx.clone(), chain, owner, tokens, false)
         .await
         .unwrap();
   }

   /// The badge contract. `Some(0)` — the chain says the wallet has none — must read differently from
   /// `None`, which is «nobody asked»: the first is a row's `NOT OWNED`, the second is no claim at all.
   #[test]
   fn an_unanswered_nft_is_not_a_zero() {
      let manager = BalanceManagerHandle::default();
      let wallet = Address::from([0x11; 20]);
      let other_wallet = Address::from([0x22; 20]);
      let collection = Address::from([0xcc; 20]);
      let other_collection = Address::from([0xdd; 20]);
      let token_id = U256::from(7);

      assert_eq!(
         manager.get_nft_balance(1, wallet, collection, token_id),
         None
      );

      manager.insert_nft_balance(1, wallet, collection, token_id, 0);
      assert_eq!(
         manager.get_nft_balance(1, wallet, collection, token_id),
         Some(0)
      );

      manager.insert_nft_balance(1, wallet, collection, token_id, 3);
      assert_eq!(
         manager.get_nft_balance(1, wallet, collection, token_id),
         Some(3)
      );

      // An answer is only ever about the one key it was made for: same wallet and chain, other ids,
      // and the same id elsewhere, all stay unanswered.
      assert_eq!(
         manager.get_nft_balance(1, wallet, collection, U256::from(8)),
         None
      );
      assert_eq!(
         manager.get_nft_balance(1, wallet, other_collection, token_id),
         None
      );
      assert_eq!(
         manager.get_nft_balance(10, wallet, collection, token_id),
         None
      );
      assert_eq!(
         manager.get_nft_balance(1, other_wallet, collection, token_id),
         None
      );
   }

   /// Dropping a wallet's balances drops its NFT holdings with them.
   #[test]
   fn retaining_wallets_drops_nft_holdings() {
      let manager = BalanceManagerHandle::default();
      let kept = Address::from([0x11; 20]);
      let dropped = Address::from([0x22; 20]);
      let collection = Address::from([0xcc; 20]);

      manager.insert_nft_balance(1, kept, collection, U256::from(7), 1);
      manager.insert_nft_balance(1, dropped, collection, U256::from(7), 1);
      manager.insert_nft_balance(1, dropped, collection, U256::from(8), 2);

      let (eth_removed, token_removed, nft_removed) =
         manager.retain_wallets(&HashSet::from([kept]));

      assert_eq!((eth_removed, token_removed), (0, 0));
      assert_eq!(nft_removed, 2);
      assert_eq!(
         manager.get_nft_balance(1, kept, collection, U256::from(7)),
         Some(1)
      );
      assert_eq!(
         manager.get_nft_balance(1, dropped, collection, U256::from(7)),
         None
      );
   }

   /// `remove_zero_balances` sweeps zero *token* balances, where nothing distinguishes zero from absent.
   /// An NFT's zero is the `NOT OWNED` answer, so it stays — and that asymmetry is deliberate.
   #[test]
   fn removing_zero_balances_keeps_nft_answers() {
      let manager = BalanceManagerHandle::default();
      let wallet = Address::from([0x11; 20]);
      let collection = Address::from([0xcc; 20]);

      manager.insert_nft_balance(1, wallet, collection, U256::from(7), 0);
      manager.remove_zero_balances();

      assert_eq!(
         manager.get_nft_balance(1, wallet, collection, U256::from(7)),
         Some(0)
      );
   }

   /// A round is retried only while **nothing** has moved. `any` would retry on every portfolio with more
   /// than one already-answered token — the untouched ones — exhaust the retries and return without
   /// writing, which is how a shield left every badge stale while logging «Max retries reached».
   #[test]
   fn a_round_is_retried_only_while_nothing_has_moved() {
      let a: batch::NftRef = (Address::from([0xaa; 20]), U256::from(1));
      let b: batch::NftRef = (Address::from([0xbb; 20]), U256::from(2));
      let before = HashMap::from([(a, 1u64), (b, 1u64)]);

      // Nothing moved yet: wait for the chain.
      assert!(awaiting_change(
         true,
         &before,
         &HashMap::from([(a, 1), (b, 1)])
      ));

      // One of them moved: write the round, including the one that did not move.
      assert!(!awaiting_change(
         true,
         &before,
         &HashMap::from([(a, 0), (b, 1)])
      ));
      assert!(!awaiting_change(
         true,
         &before,
         &HashMap::from([(a, 1), (b, 4)])
      ));

      // Nothing was ever answered, so there is nothing to wait for.
      assert!(!awaiting_change(
         true,
         &HashMap::new(),
         &HashMap::from([(a, 0)])
      ));

      // Not expecting a move at all.
      assert!(!awaiting_change(
         false,
         &before,
         &HashMap::from([(a, 1), (b, 1)])
      ));

      // An id that lost its answer counts as moved, not as unchanged.
      assert!(!awaiting_change(
         true,
         &before,
         &HashMap::from([(b, 1)])
      ));
   }
}
