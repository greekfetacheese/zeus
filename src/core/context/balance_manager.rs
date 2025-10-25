use crate::core::{ZeusCtx, context::data_dir, serde_hashmap, utils::RT};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::{sync::Semaphore, time::sleep};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   types::SUPPORTED_CHAINS,
   utils::{NumericValue, batch},
};

use serde::{Deserialize, Serialize};
use std::time::Duration;

const BALANCE_DATA_FILE: &str = "balances.json";

#[derive(Clone)]
pub struct BalanceManagerHandle(Arc<RwLock<BalanceManager>>);

impl Default for BalanceManagerHandle {
   fn default() -> Self {
      Self(Arc::new(RwLock::new(BalanceManager::default())))
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

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(BALANCE_DATA_FILE);
      let data = std::fs::read(dir)?;
      let manager = serde_json::from_slice(&data)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = self.read(|db| serde_json::to_string(db))?;
      let dir = data_dir()?.join(BALANCE_DATA_FILE);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn set_concurrency(&self, concurrency: usize) {
      self.write(|manager| manager.concurrency = concurrency);
   }

   pub fn set_max_retries(&self, max_retries: usize) {
      self.write(|manager| manager.max_retries = max_retries);
   }

   pub fn set_retry_delay(&self, retry_delay: u64) {
      self.write(|manager| manager.retry_delay = retry_delay);
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

   pub fn max_retries(&self) -> usize {
      self.read(|manager| manager.max_retries)
   }

   pub fn retry_delay(&self) -> u64 {
      self.read(|manager| manager.retry_delay)
   }

   pub async fn update_eth_balance_across_wallets_and_chains(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();
      let batch_size = self.batch_size();

      for chain in SUPPORTED_CHAINS {
         let wallets: Vec<Address> = ctx.get_all_wallets_info().iter().map(|w| w.address).collect();
         let manager = self.clone();
         let ctx = ctx.clone();


         let task = RT.spawn(async move {
            for chunk in wallets.chunks(batch_size) {
               match manager.update_eth_balance(ctx.clone(), chain, chunk.to_vec(), false).await {
                  Ok(_) => {}
                  Err(e) => {
                     tracing::error!(
                        "Error updating eth ChainId {} balance: {:?}",
                        chain,
                        e
                     );
                  }
               }
            }
         });
         tasks.push(task);
      }

      for task in tasks {
         let _r = task.await;
      }
   }

   pub async fn update_tokens_balance_across_wallets_and_chains(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
         let manager = self.clone();
         let ctx = ctx.clone();

         let task = RT.spawn(async move {
            for portfolio in &portfolios {
               let tokens = portfolio.tokens.clone();

               match manager
                  .update_tokens_balance(ctx.clone(), chain, portfolio.owner, tokens, false)
                  .await
               {
                  Ok(_) => {}
                  Err(e) => {
                     tracing::error!(
                        "Error updating tokens balance ChainId {} balance: {:?}",
                        chain,
                        e
                     );
                  }
               }
            }
         });
         tasks.push(task);
      }

      for task in tasks {
         let _r = task.await;
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
      let client = ctx.get_zeus_client();

      let max_retries = self.max_retries();
      let retry_delay = self.retry_delay();
      let mut attempt = 0;

      let mut old_balances = HashMap::new();

      let owners_clone = owners.clone();
      for owner in owners_clone {
         let balance = self.get_eth_balance(chain, owner);
         old_balances.insert(owner, balance);
      }

      loop {
         if attempt > max_retries {
            tracing::error!("Max retries reached");
            break;
         }

         let balances = client
            .request(chain, |client| {
               let owners_clone = owners.clone();
               async move { batch::get_eth_balances(client, chain, None, owners_clone).await }
            })
            .await?;

         let mut should_break = false;

         if !retry_if_unchanged {
            should_break = true;

            for balance in balances {
               let owner = balance.owner;
               let eth_balance = balance.balance;
               let native = NativeCurrency::from(chain);
               self.insert_eth_balance(chain, owner, eth_balance, &native);
            }
         } else {
            for balance in balances {
               let owner = balance.owner;
               let eth_balance = balance.balance;
               let native = NativeCurrency::from(chain);
               let old_balance = old_balances.get(&owner).unwrap();
               if eth_balance == old_balance.wei() {
                  tracing::warn!(
                     "Balances for owner {} are the same New {} Old {}, retrying",
                     owner,
                     eth_balance,
                     old_balance.wei()
                  );
                  should_break = false;
                  break;
               } else {
                  should_break = true;
                  self.insert_eth_balance(chain, owner, eth_balance, &native);
               }
            }
         }

         match should_break {
            true => break,
            false => {
               attempt += 1;
               sleep(Duration::from_millis(retry_delay)).await;
            }
         }
      }

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
      let client = ctx.get_zeus_client();
      let semaphore = Arc::new(Semaphore::new(self.concurrency()));
      let tokens_addr = tokens.iter().map(|t| t.address).collect::<Vec<_>>();
      let token_map: HashMap<Address, ERC20Token> =
         tokens.iter().map(|token| (token.address, token.clone())).collect();

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

         let mut old_balances = HashMap::new();

         let tokens = tokens_addr.clone();
         for token in tokens {
            let balance = self.get_token_balance(chain, owner, token);
            old_balances.insert(token, balance);
         }

         let task = RT.spawn(async move {
            let mut attempt = 0;

            loop {
               if attempt > max_retries {
                  tracing::error!("Max retries reached");
                  break;
               }

                let _permit = semaphore.acquire().await.unwrap();
                let balances = client
                    .request(chain, |client| {
                        let tokens_addr_clone = tokens_addr.clone();
                        async move {
                            batch::get_erc20_balances(client, chain, None, owner, tokens_addr_clone).await
                        }
                    })
                    .await;

                let balances = match balances {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!(
                            "Failed to get erc20 balances for Owner: {owner:?} ChainId: {chain:?} Error: {e:?}"
                        );
                        tokio::time::sleep(Duration::from_millis(retry_delay)).await;
                        attempt += 1;
                        continue;
                    }
                };

                let mut should_break = false;

                // Just update the balances
                if !retry_if_unchanged {
                   should_break = true;

                for balance in &balances {
                     let token =  token_map.get(&balance.token).unwrap();
                     manager.insert_token_balance(chain, owner, balance.balance, token);
               }
               tracing::info!("Updated balances for {} tokens", balances.len());
            } else {
                // In case the picked RPC is not synced with the latest block,
                // it will return the same balances, in that case we need to retry

                for balance in balances {
                     let token = token_map.get(&balance.token).unwrap();
                        let old_balance = old_balances.get(&balance.token).unwrap();
                        if balance.balance == old_balance.wei() {
                           tracing::warn!("Balances for token {} are the same New {} Old {}, retrying", token.symbol, balance.balance, old_balance.wei());
                           should_break = false;
                           break;
                        } else {
                           should_break = true;
                           manager.insert_token_balance(chain, owner, balance.balance, token);
                        }
                }
            }

               match should_break {
                  true => break,
                  false => {
                     attempt += 1;
                     sleep(Duration::from_millis(retry_delay)).await;
                  }
               }
            }
        });
         tasks.push(task);
      }

      for task in tasks {
         match task.await {
            Ok(()) => (),
            Err(e) => tracing::error!("Error updating token balance: {:?}", e),
         }
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

   pub fn insert_currency_balance(
      &self,
      owner: Address,
      balance: NumericValue,
      currency: &Currency,
   ) {
      if currency.is_native() {
         let native = currency.native_opt().unwrap();
         let balance = balance.wei();
         self.insert_eth_balance(native.chain_id, owner, balance, native);
      } else {
         let token = currency.erc20_opt().unwrap();
         let balance = balance.wei();
         self.insert_token_balance(token.chain_id, owner, balance, token);
      }
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
}

fn default_concurrency() -> usize {
   2
}

fn default_max_retries() -> usize {
   10
}

fn default_retry_delay() -> u64 {
   500
}

fn default_batch_size() -> usize {
   10
}

#[derive(Default, Serialize, Deserialize)]
pub struct BalanceManager {
   /// Eth Balances (or any native currency for evm compatable chains)
   #[serde(with = "serde_hashmap")]
   pub eth_balances: HashMap<(u64, Address), NumericValue>,

   /// Token Balances
   #[serde(with = "serde_hashmap")]
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
