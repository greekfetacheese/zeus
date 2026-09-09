use crate::core::ZeusCtx;
use crate::core::serde_hashmap;
use crate::utils::RT;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use tokio::{sync::Semaphore, time::sleep};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{ERC20Token, NativeCurrency},
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

      let client = ctx.get_zeus_client();
      let max_retries = self.max_retries();
      let retry_delay = self.retry_delay();
      let native = NativeCurrency::from(chain);

      let old_balances: HashMap<Address, NumericValue> = if retry_if_unchanged {
         owners
            .iter()
            .map(|&owner| (owner, self.get_eth_balance(chain, owner)))
            .collect()
      } else {
         HashMap::new()
      };

      for attempt in 0..=max_retries {
         let balances = client
            .request(chain, |client| {
               let owners_clone = owners.clone();
               async move { batch::get_eth_balances(client, chain, None, owners_clone).await }
            })
            .await?;

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

         self.write(|manager| {
            manager.eth_balances.shrink_to_fit();
         });
         return Ok(());
      }

      Err(anyhow!("Max retries reached"))
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

      let client = ctx.get_zeus_client();
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

   /// Drop balance entries whose owner is not in `wallets`.
   ///
   /// Returns `(eth_removed, token_removed)`.
   pub fn retain_wallets(&self, wallets: &HashSet<Address>) -> (usize, usize) {
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

         (eth_removed, token_removed)
      })
   }

   /// Remove all entries that have a 0 balance
   ///
   /// This will save up space and the manager will still return 0 balance for the removed entries
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
}
