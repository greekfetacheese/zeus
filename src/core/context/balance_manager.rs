use crate::core::{
   ZeusCtx, serde_hashmap,
   utils::{RT, data_dir},
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::sync::Semaphore;
use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   currency::{Currency, ERC20Token, NativeCurrency},
   types::SUPPORTED_CHAINS,
   utils::{NumericValue, batch},
};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "balances.json";

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
      let dir = data_dir()?.join(FILE_NAME);
      let data = std::fs::read(dir)?;
      let manager = serde_json::from_slice(&data)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = self.read(|db| serde_json::to_string(db))?;
      let dir = data_dir()?.join(FILE_NAME);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn set_concurrency(&self, concurrency: usize) {
      self.write(|manager| manager.concurrency = concurrency);
   }

   pub fn set_batch_size(&self, batch_size: usize) {
      self.write(|manager| manager.batch_size = batch_size);
   }

   pub fn concurrency(&self) -> usize {
      self.read(|manager| manager.concurrency)
   }

   pub fn batch_size(&self) -> usize {
      self.read(|manager| manager.batch_size)
   }

   pub async fn update_eth_balance_across_wallets_and_chains(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
         let manager = self.clone();
         let ctx = ctx.clone();

         let task = RT.spawn(async move {
            for portfolio in &portfolios {
               match manager
                  .update_eth_balance(ctx.clone(), chain, portfolio.owner)
                  .await
               {
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
         task.await.unwrap();
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
               let tokens = portfolio.get_tokens();

               match manager
                  .update_tokens_balance(ctx.clone(), chain, portfolio.owner, tokens)
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
         task.await.unwrap();
      }
   }

   pub async fn update_eth_balance(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      owner: Address,
   ) -> Result<(), anyhow::Error> {
      let client = ctx.get_client(chain).await?;
      let balance = client.get_balance(owner).await?;
      let native = NativeCurrency::from(chain);
      self.insert_eth_balance(chain, owner, balance, &native);
      Ok(())
   }

   pub async fn update_tokens_balance(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      owner: Address,
      tokens: Vec<ERC20Token>,
   ) -> Result<(), anyhow::Error> {
      let client = ctx.get_client(chain).await?;
      let semaphore = Arc::new(Semaphore::new(self.concurrency()));
      let tokens_addr = tokens.iter().map(|t| t.address).collect::<Vec<_>>();
      let token_map: HashMap<Address, ERC20Token> = tokens
         .iter()
         .map(|token| (token.address, token.clone()))
         .collect();

      let mut tasks = Vec::new();
      let batch_size = self.batch_size();
      for chunk in tokens_addr.chunks(batch_size) {
         let client = client.clone();
         let semaphore = semaphore.clone();
         let manager = self.clone();
         let token_map = token_map.clone();
         let tokens_addr = chunk.to_vec();

         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let balances =
               batch::get_erc20_balances(client.clone(), None, owner, tokens_addr).await;

            match balances {
               Ok(balances) => {
                for balance in balances {
                  if let Some(token) = token_map.get(&balance.token) {
                  manager.insert_token_balance(chain, owner, balance.balance, token);
                  }
               }
               }
               Err(e) => {
                tracing::error!(
                  "Failed to get erc20 balances for Owner: {owner:?} ChainId: {chain:?} Error: {e:?}"
               );
               }
            }

         });
         tasks.push(task);
      }

      for task in tasks {
         task.await.unwrap();
      }

      Ok(())
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.read(|manager| {
         manager
            .eth_balances
            .get(&(chain, owner))
            .cloned()
            .unwrap_or_default()
      })
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self.read(|manager| {
         manager
            .token_balances
            .get(&(chain, owner, token))
            .cloned()
            .unwrap_or_default()
      })
   }

   pub fn insert_currency_balance(
      &self,
      owner: Address,
      balance: NumericValue,
      currency: &Currency,
   ) {
      if currency.is_native() {
         let native = currency.native().unwrap();
         let balance = balance.wei();
         self.insert_eth_balance(native.chain_id, owner, balance, native);
      } else {
         let token = currency.erc20().unwrap();
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
         manager
            .token_balances
            .insert((chain, owner, token.address), balance);
      });
   }
}

fn default_concurrency() -> usize {
   1
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

   #[serde(default = "default_batch_size")]
   pub batch_size: usize,
}
