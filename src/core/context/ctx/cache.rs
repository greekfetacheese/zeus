use super::super::ZeusCtx;
use crate::core::types::*;
use crate::utils::TimeStamp;
use anyhow::anyhow;
use std::collections::HashMap;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, FixedBytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{
      Block as RpcBlock, BlockId, Transaction, TransactionReceipt, TransactionRequest,
   },
};

/// Max entries in the wallet-connector RPC caches (`eth_call`, gas, code, …).
pub(super) const CONNECTOR_CACHE_CAP: usize = 20;

fn cache_elapsed_fresh(now: u64, old: u64, ttl_ms: u64) -> bool {
   let elapsed = if now > old {
      now - old
   } else {
      tracing::warn!("System time is behind block timestamp");
      u64::MAX
   };
   elapsed < ttl_ms
}

fn cache_insert_capped<K, V>(map: &mut HashMap<K, V>, key: K, value: V)
where
   K: Eq + std::hash::Hash + Clone,
{
   map.insert(key, value);
   if map.len() >= CONNECTOR_CACHE_CAP {
      if let Some(oldest) = map.keys().next().cloned() {
         map.remove(&oldest);
      }
   }
}

impl ZeusCtx {
   /// Get the receipt for the given transaction hash
   ///
   /// If the receipt is not found in cache, it will be retrieved from the blockchain
   pub async fn get_receipt_by_hash(
      &self,
      hash: FixedBytes<32>,
   ) -> Result<Option<TransactionReceipt>, anyhow::Error> {
      let chain = self.chain().id();

      let receipt = self.read(|ctx| ctx.receipts.get(&(chain, hash)).cloned());

      if let Some(receipt) = receipt {
         return Ok(Some(receipt));
      }

      let client = self.get_client_manager();
      let receipt = client
         .request(chain, |client| async move {
            client.get_transaction_receipt(hash).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(receipt) = &receipt {
         self.write(|ctx| {
            cache_insert_capped(&mut ctx.receipts, (chain, hash), receipt.clone());
         });
      }

      Ok(receipt)
   }

   /// Get the transaction for the given transaction hash
   ///
   /// If the transaction is not found in cache, it will be retrieved from the blockchain
   pub async fn get_tx_by_hash(
      &self,
      hash: FixedBytes<32>,
   ) -> Result<Option<Transaction>, anyhow::Error> {
      let chain = self.chain().id();

      let transaction = self.read(|ctx| ctx.transactions.get(&(chain, hash)).cloned());

      if let Some(transaction) = transaction {
         return Ok(Some(transaction));
      }

      let client = self.get_client_manager();
      let transaction = client
         .request(chain, |client| async move {
            client.get_transaction_by_hash(hash).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(transaction) = &transaction {
         self.write(|ctx| {
            cache_insert_capped(
               &mut ctx.transactions,
               (chain, hash),
               transaction.clone(),
            );
         });
      }

      Ok(transaction)
   }

   /// Get the storage value for the given address and slot
   ///
   /// If the storage value is not found in cache, it will be retrieved from the blockchain
   pub async fn get_storage(
      &self,
      block_id: BlockId,
      address: Address,
      slot: U256,
   ) -> Result<U256, anyhow::Error> {
      let chain = self.chain().id();

      let block = if let Some(block) = block_id.as_u64() {
         block
      } else {
         self.get_latest_block().await?.number
      };

      let storage = self.read(|ctx| ctx.storage.get(&(chain, block, address, slot)).cloned());

      if let Some(storage) = storage {
         return Ok(storage);
      }

      let client = self.get_client_manager();
      let storage = client
         .request(chain, |client| async move {
            client
               .get_storage_at(address, slot)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.storage,
            (chain, block, address, slot),
            storage.clone(),
         );
      });

      Ok(storage)
   }

   /// Get the code for the given address
   ///
   /// If the code is not found in cache, it will be retrieved from the blockchain
   pub async fn get_code(
      &self,
      block_id: BlockId,
      address: Address,
   ) -> Result<Bytes, anyhow::Error> {
      let chain = self.chain().id();

      let block = if let Some(block) = block_id.as_u64() {
         block
      } else {
         self.get_latest_block().await?.number
      };

      let code = self.read(|ctx| ctx.codes.get(&(chain, block, address)).cloned());

      if let Some(code) = code {
         return Ok(code);
      }

      let client = self.get_client_manager();
      let code = client
         .request(chain, |client| async move {
            client
               .get_code_at(address)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.codes,
            (chain, block, address),
            code.clone(),
         );
      });

      Ok(code)
   }

   /// Estimate the gas for the given transaction
   ///
   /// If the gas is not found in cache, it will be estimated from the blockchain
   pub async fn estimate_gas(&self, tx: TransactionRequest) -> Result<u64, anyhow::Error> {
      let chain = self.chain();
      let res = self.read(|ctx| ctx.estimate_gas.get(&(chain.id(), tx.clone())).cloned());
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();

      if let Some(res) = res {
         if cache_elapsed_fresh(now, res.timestamp, block_time) {
            return Ok(res.gas);
         }
      }

      let client = self.get_client_manager();
      let gas = client
         .request_with(chain.id(), tx.clone(), |client, tx| async move {
            client.estimate_gas(tx).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let now = TimeStamp::now_as_millis()?.timestamp();

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.estimate_gas,
            (chain.id(), tx),
            EstimateGas {
               timestamp: now,
               gas,
            },
         );
      });

      Ok(gas)
   }

   /// Get the eth_call for the given transaction
   ///
   /// If the eth_call is not found in cache, it will be retrieved from the blockchain
   pub async fn get_eth_call(&self, tx: TransactionRequest) -> Result<EthCall, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let eth_call = self.read(|ctx| ctx.eth_calls.get(&(chain.id(), tx.clone())).cloned());

      if let Some(eth_call) = eth_call {
         if cache_elapsed_fresh(now, eth_call.timestamp, block_time) {
            return Ok(eth_call);
         }
      }

      let z_client = self.get_client_manager();
      let result = z_client
         .request_with(chain.id(), tx.clone(), |client, tx| async move {
            client.call(tx).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let now = TimeStamp::now_as_millis()?.timestamp();

      let eth_call = EthCall {
         timestamp: now,
         result,
      };

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.eth_calls,
            (chain.id(), tx),
            eth_call.clone(),
         );
      });

      Ok(eth_call)
   }

   /// Get the latest block
   ///
   /// If the block is not found in cache, it will be retrieved from the blockchain
   pub async fn get_latest_block(&self) -> Result<Block, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let block = self.read(|ctx| ctx.latest_block.get(&chain.id()).cloned());

      if let Some(block) = block {
         let block_timestamp_ms = block.timestamp * 1000u64;
         if cache_elapsed_fresh(now, block_timestamp_ms, block_time) {
            return Ok(block);
         }
      }

      let z_client = self.get_client_manager();
      let block = z_client
         .request(chain.id(), |client| async move {
            client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(block) = block {
         let block = Block::new(block.header.number, block.header.timestamp);
         self.write(|ctx| {
            ctx.latest_block.insert(chain.id(), block.clone());
         });
         return Ok(block);
      }

      Err(anyhow!("No block found"))
   }

   /// Get a block by number or tag
   ///
   /// If the block is not found in cache, it will be retrieved from the blockchain.
   /// `hydrated` selects full transactions (`true`) or hashes only (`false`).
   ///
   /// Only requests with a stable numeric key are cached: an explicit number,
   /// `latest` (resolved via the cached tip) and `earliest` (genesis). Tags that
   /// can move independently of the tip (`safe` / `finalized` / `pending`) bypass
   /// the cache so a stale entry is never served for them.
   pub async fn get_block_by_number(
      &self,
      block_id: BlockId,
      hydrated: bool,
   ) -> Result<Option<RpcBlock>, anyhow::Error> {
      let chain = self.chain();

      let cache_number = if let Some(number) = block_id.as_u64() {
         Some(number)
      } else if block_id.is_latest() {
         Some(self.get_latest_block().await?.number)
      } else if block_id.is_earliest() {
         Some(0)
      } else {
         None
      };

      if let Some(number) = cache_number {
         let cached = self.read(|ctx| ctx.blocks_by_number.get(&(chain.id(), number)).cloned());
         if let Some(cached) = cached {
            let now = TimeStamp::now_as_millis()?.timestamp();
            if cached.hydrated == hydrated
               && cache_elapsed_fresh(now, cached.timestamp, chain.block_time_millis())
            {
               return Ok(Some(cached.block));
            }
         }
      }

      let z_client = self.get_client_manager();
      let block = z_client
         .request(chain.id(), move |client| async move {
            let req = client.get_block(block_id);
            if hydrated {
               req.full().await.map_err(|e| anyhow!("{:?}", e))
            } else {
               req.await.map_err(|e| anyhow!("{:?}", e))
            }
         })
         .await?;

      let Some(block) = block else {
         return Ok(None);
      };

      if let Some(number) = cache_number {
         let now = TimeStamp::now_as_millis()?.timestamp();
         self.write(|ctx| {
            cache_insert_capped(
               &mut ctx.blocks_by_number,
               (chain.id(), number),
               CachedBlock {
                  timestamp: now,
                  hydrated,
                  block: block.clone(),
               },
            );
         });
      }

      Ok(Some(block))
   }

   /// Get a block by its hash
   ///
   /// If the block is not found in cache, it will be retrieved from the blockchain.
   /// `hydrated` selects full transactions (`true`) or hashes only (`false`); the
   /// cache stores one variant per hash and refetches when the requested one differs.
   pub async fn get_block_by_hash(
      &self,
      hash: FixedBytes<32>,
      hydrated: bool,
   ) -> Result<Option<RpcBlock>, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let cached = self.read(|ctx| ctx.blocks_by_hash.get(&(chain.id(), hash)).cloned());

      if let Some(cached) = cached {
         if cached.hydrated == hydrated && cache_elapsed_fresh(now, cached.timestamp, block_time) {
            return Ok(Some(cached.block));
         }
      }

      let z_client = self.get_client_manager();
      let block = z_client
         .request(chain.id(), move |client| async move {
            let req = client.get_block_by_hash(hash);
            if hydrated {
               req.full().await.map_err(|e| anyhow!("{:?}", e))
            } else {
               req.await.map_err(|e| anyhow!("{:?}", e))
            }
         })
         .await?;

      let Some(block) = block else {
         return Ok(None);
      };

      let now = TimeStamp::now_as_millis()?.timestamp();

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.blocks_by_hash,
            (chain.id(), hash),
            CachedBlock {
               timestamp: now,
               hydrated,
               block: block.clone(),
            },
         );
      });

      Ok(Some(block))
   }

   /// Get the transaction count (nonce) for the given address
   ///
   /// If the count is not found in cache, it will be retrieved from the blockchain
   pub async fn get_transaction_count(&self, address: Address) -> Result<u64, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let cached = self.read(|ctx| ctx.tx_counts.get(&(chain.id(), address)).cloned());

      if let Some(cached) = cached {
         if cache_elapsed_fresh(now, cached.timestamp, block_time) {
            return Ok(cached.count);
         }
      }

      let z_client = self.get_client_manager();
      let count = z_client
         .request(chain.id(), move |client| async move {
            client.get_transaction_count(address).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let now = TimeStamp::now_as_millis()?.timestamp();

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.tx_counts,
            (chain.id(), address),
            TransactionCount {
               timestamp: now,
               count,
            },
         );
      });

      Ok(count)
   }
}
