use super::super::{ClientKind, RpcFunctionality, ZeusContext, ZeusCtx};
use crate::{client::Rpc, types::*};
use anyhow::anyhow;
use zeus_eth::{
   alloy_primitives::{Address, FixedBytes, U256},
   amm::uniswap::{
      AnyUniswapPool, DexKind, FeeAmount, State, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      UniswapV4Pool,
   },
   currency::{Currency, NativeCurrency, erc20::ERC20Token},
   nft::{NftCollection, NftToken},
   revm::primitives::hardfork::SpecId,
   types::ChainId,
   utils::{NumericValue, client::RpcClient},
};

impl ZeusCtx {
   /// Check if any RPC client is available for the given chain
   pub fn client_available(&self, chain: u64) -> bool {
      let z_client = self.get_client_manager();
      z_client.rpc_available(chain)
   }

   /// The hardfork to simulate the chain on: what the chain's own node reports when it has an
   /// answer, Zeus's per-chain spec otherwise.
   ///
   /// See [`ClientManager::node_spec`] — the node is asked because a spec baked into a release is
   /// a guess about somebody else's chain, and `None` here means that guess is all Zeus has.
   pub async fn node_spec(&self, chain: ChainId) -> Option<SpecId> {
      self.get_client_manager().node_spec(chain.id()).await
   }

   pub async fn get_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.get_client(chain).await
   }

   pub async fn connect_to_rpc(&self, rpc: &Rpc) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.connect_to(rpc).await
   }

   /// Get an archive client for the given chain.
   ///
   /// If `http` is true, it will use an http endpoint.
   pub async fn get_archive_client(
      &self,
      chain: u64,
      http: bool,
   ) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.get_archive_client(chain, http).await
   }

   pub async fn get_mev_protect_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.get_mev_protect_client(chain).await
   }

   pub fn get_base_fee(&self, chain: u64) -> Option<BaseFee> {
      self.read(|ctx| ctx.base_fee.get(&chain).cloned())
   }

   pub fn get_priority_fee(&self, chain: u64) -> Option<NumericValue> {
      self.read(|ctx| ctx.priority_fee.get(chain).cloned())
   }

   pub fn update_base_fee(&self, chain: u64, base_fee: u64, next_base_fee: u64) {
      self.write(|ctx| {
         ctx.base_fee.insert(chain, BaseFee::new(base_fee, next_base_fee));
      });
   }

   pub fn update_priority_fee(&self, chain: u64, fee: NumericValue) {
      self.write(|ctx| {
         ctx.priority_fee.fee.insert(chain, fee);
      });
   }

   /// Get the V2 pool for the given address
   ///
   /// If the pool is not found in cache, it will be retrieved from the blockchain
   pub async fn get_v2_pool(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let z_client = self.get_client_manager();
      let cached = self.read(|ctx| ctx.pool_manager.get_v2_pool_from_address(chain, address));

      if let Some(pool) = cached {
         return Ok(pool);
      } else {
         let pool = z_client
            .request(chain, |client| async move {
               UniswapV2Pool::from_address(client, chain, address).await
            })
            .await?;
         let pool = AnyUniswapPool::from_pool(pool);
         self.write(|ctx| {
            ctx.pool_manager.add_pool(pool.clone());
            ctx.currency_db.insert_currency(chain, pool.currency0().clone());
            ctx.currency_db.insert_currency(chain, pool.currency1().clone());
         });

         return Ok(pool);
      };
   }

   /// Get the V3 pool for the given address
   ///
   /// If the pool is not found in cache, it will be retrieved from the blockchain
   pub async fn get_v3_pool(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let z_client = self.get_client_manager();
      let cached = self.read(|ctx| ctx.pool_manager.get_v3_pool_from_address(chain, address));

      if let Some(pool) = cached {
         return Ok(pool);
      } else {
         let pool = z_client
            .request(chain, |client| async move {
               UniswapV3Pool::from_address(client, chain, address).await
            })
            .await?;
         let pool = AnyUniswapPool::from_pool(pool);
         self.write(|ctx| {
            ctx.pool_manager.add_pool(pool.clone());
            ctx.currency_db.insert_currency(chain, pool.currency0().clone());
            ctx.currency_db.insert_currency(chain, pool.currency1().clone());
         });

         return Ok(pool);
      };
   }

   /// Get the V4 pool for the given pool id
   ///
   /// If the pool is not found in cache, it does a best-effort search over known
   /// tokens and common fee / tick-spacing combos.
   ///
   /// `fee` is typically the Swap-event fee (charged LP + protocol). That is **not**
   /// the PoolKey fee — e.g. Base ETH/VIRTUAL is PoolKey.fee=500 but Swap.fee=625.
   pub async fn get_v4_pool(
      &self,
      chain: u64,
      fee: u32,
      expected_id: FixedBytes<32>,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let cached = self.read(|ctx| ctx.pool_manager.get_v4_pool_from_id(chain, expected_id));

      if let Some(pool) = cached {
         return Ok(pool);
      }

      let mut fees = vec![100u32, 500, 3000, 10000, fee];
      fees.sort_unstable();
      fees.dedup();

      let spacings_for = |f: u32| -> Vec<i32> {
         let mut spacings = vec![1, 10, 60, 200, FeeAmount::new(f).tick_spacing_i32()];
         spacings.sort_unstable();
         spacings.dedup();
         spacings
      };

      let mut base_tokens = ERC20Token::base_tokens(chain);

      // remove WETH since in V4 is not used
      let weth = ERC20Token::wrapped_native_token(chain);
      base_tokens.retain(|t| t.address != weth.address);

      let mut base_currencies: Vec<Currency> =
         base_tokens.iter().map(|t| Currency::from(t.clone())).collect();

      // Add ETH native
      let currency = Currency::from(NativeCurrency::from(chain));
      base_currencies.push(currency);

      let quote_currencies = self.get_currencies(chain);

      // Best effort pool finding from all known tokens
      for quote_currency in quote_currencies.iter() {
         for base_currency in &base_currencies {
            if quote_currency.address() == base_currency.address() {
               continue;
            }

            for &cand_fee in &fees {
               for spacing in spacings_for(cand_fee) {
                  let pool = UniswapV4Pool::new_with_spacing(
                     chain,
                     FeeAmount::new(cand_fee),
                     spacing,
                     DexKind::UniswapV4,
                     base_currency.clone(),
                     quote_currency.clone(),
                     State::none(),
                     Address::ZERO,
                  );

                  if pool.id() == expected_id {
                     let pool_manager = self.pool_manager();
                     pool_manager.add_pool(pool.clone());
                     return Ok(pool.into());
                  }
               }
            }
         }
      }

      Err(anyhow!("V4 Pool not found"))
   }

   /// Get the ERC20 token for the given address
   ///
   /// If the token is not found in cache, it will be retrieved from the blockchain
   pub async fn get_token(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<ERC20Token, anyhow::Error> {
      let cached = self.read(|ctx| ctx.currency_db.get_erc20_token(chain, address));

      if let Some(token) = cached {
         return Ok(token);
      } else {
         let z_client = self.get_client_manager();
         let rpc = z_client.get_best_rpc(chain).ok_or(anyhow!("No available RPC found"))?;
         let client = z_client.client_for(&rpc, ClientKind::Short).await?;

         let token = ERC20Token::new(client, address, chain).await?;

         self.write(|ctx| ctx.currency_db.insert_currency(chain, Currency::from(token.clone())));

         return Ok(token);
      };
   }

   /// Get an NFT
   ///
   /// If it is not cached, the collection is resolved first — the standard costs an ERC-165 sweep and
   /// cannot differ per token — and then the token itself. Both are stored in `nft_db`.
   pub async fn get_nft(
      &self,
      chain: u64,
      collection: Address,
      token_id: U256,
   ) -> Result<NftToken, anyhow::Error> {
      if let Some(nft) = self.read(|ctx| ctx.nft_db.get_nft(chain, collection, token_id)) {
         return Ok(nft);
      }

      let z_client = self.get_client_manager();
      let rpc = z_client.get_best_rpc(chain).ok_or(anyhow!("No available RPC found"))?;
      let client = z_client.client_for(&rpc, ClientKind::Short).await?;

      let collection = NftCollection::fetch(client.clone(), chain, collection).await?;
      let nft = NftToken::fetch(client, &collection, token_id).await?;

      self.write(|ctx| {
         ctx.nft_db.insert_collection(chain, collection);
         ctx.nft_db.insert_nft(nft.clone());
      });

      Ok(nft)
   }
}

impl ZeusContext {
   pub fn get_base_fee(&self, chain: u64) -> Option<BaseFee> {
      self.base_fee.get(&chain).cloned()
   }

   /// Check if we have any enabled and working RPCs for the given chain
   ///
   /// Returns true if we have at least one enabled and working RPC
   pub fn check_for_available_rpcs(&mut self, now_millis: u64, chain: u64, threshold: u64) -> bool {
      let last_checked = self.last_checked_for_available_rpcs.get(&chain).cloned().unwrap_or(0);

      let should_check = now_millis.saturating_sub(last_checked) > threshold;

      if should_check {
         let ok = self.client_manager.rpc_available(chain);
         self.last_checked_for_available_rpcs.insert(chain, now_millis);
         self.available_rpcs.insert(chain, ok);

         return ok;
      }

      self.available_rpcs.get(&chain).cloned().unwrap_or(false)
   }

   /// Check if any enabled, already-tested RPC is not fully functional.
   ///
   /// Returns true at most once per `toast_duration_ms` while the problem lasts, so the
   /// caller can enqueue a toast without stacking a new one every frame.
   pub fn check_for_malfunction(
      &mut self,
      toast_duration_ms: u64,
      now_millis: u64,
      chain: u64,
      threshold: u64,
   ) -> bool {
      if self.state_sync.get(&chain).copied().unwrap_or(false) {
         return false;
      }

      let last_checked = self.last_checked_for_malfunction.get(&chain).copied().unwrap_or(0);
      if now_millis.saturating_sub(last_checked) <= threshold {
         return false;
      }

      self.last_checked_for_malfunction.insert(chain, now_millis);

      if self.client_manager.rpcs_fully_functional(chain) {
         return false;
      }

      let last_detected = self.last_detected_malfunction.get(&chain).copied().unwrap_or(0);
      if now_millis.saturating_sub(last_detected) <= toast_duration_ms {
         return false;
      }

      self.last_detected_malfunction.insert(chain, now_millis);
      true
   }

   /// The [`RpcFunctionality`]s no enabled, already-checked RPC for `chain` can answer.
   ///
   /// Read beside [`Self::check_for_malfunction`] when its warning fires, so the toast can
   /// name what is missing instead of saying "some features may not work".
   pub fn unavailable_rpc_functionalities(&self, chain: u64) -> Vec<RpcFunctionality> {
      self.client_manager.unavailable_functionalities(chain)
   }
}
