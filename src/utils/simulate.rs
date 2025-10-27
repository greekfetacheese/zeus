use crate::core::ZeusCtx;
use crate::utils::RT;

use zeus_eth::{
   alloy_primitives::{Address, TxKind, Bytes, KECCAK256_EMPTY, U256, keccak256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   amm::uniswap::UniswapPool,
   revm_utils::{
      Database, DatabaseCommit, revert_msg, Evm2, ExecuteCommitEvm, ExecutionResult,
      revm::state::{AccountInfo, Bytecode},
   },
};
use alloy_eips::eip7702::SignedAuthorization;
use either::Either;

use anyhow::anyhow;
use std::sync::Arc;
use tokio::{sync::Mutex, task::JoinHandle};

pub fn simulate_transaction<DB>(
   evm: &mut Evm2<DB>,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
   authorization_list: Vec<SignedAuthorization>,
) -> Result<ExecutionResult, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = from;
   evm.tx.kind = TxKind::Call(interact_to);
   evm.tx.data = call_data.clone();
   evm.tx.value = value;

   if authorization_list.len() > 0 {
      evm.tx.authorization_list = authorization_list.into_iter().map(Either::Left).collect();
      evm.tx.tx_type = 4;
   }

   let sim_res = evm
      .transact_commit(evm.tx.clone())
      .map_err(|e| anyhow!("Simulation failed: {:?}", e))?;
   let output = sim_res.output().unwrap_or_default();
   let gas_used = sim_res.gas_used();

   if !sim_res.is_success() {
      let err = revert_msg(output);
      tracing::error!(
         "Simulation failed: {} \n Gas Used {}",
         err,
         gas_used
      );
      return Err(anyhow!("Failed to simulate transaction: {}", err));
   }

   Ok(sim_res)
}

#[derive(Clone, Debug)]
pub struct AccountInfo2 {
   pub address: Address,
   pub info: AccountInfo,
}

#[derive(Clone, Debug)]
pub struct AccountStorage {
   pub address: Address,
   pub slot: U256,
   pub value: U256,
}

#[derive(Clone, Debug)]
pub struct AccountSlots {
   pub address: Address,
   pub slots: Vec<U256>,
}

pub fn v2_pool_standard_slots() -> Vec<U256> {
   vec![
      U256::from(6),
      U256::from(7),
      U256::from(8),
      U256::from(9),
      U256::from(10),
      U256::from(12),
   ]
}

pub fn v3_pool_standard_slots() -> Vec<U256> {
   vec![U256::from(0), U256::from(1), U256::from(4)]
}

pub async fn fetch_accounts_info(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   addr: Vec<Address>,
) -> Vec<AccountInfo2> {
   let client = ctx.get_zeus_client();

   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let accounts = Arc::new(Mutex::new(Vec::new()));

   for addr in addr {
      let client = client.clone();
      let accounts = accounts.clone();

      let task = RT.spawn(async move {
         let balance = client.request(chain, |client| async move {
            client
               .get_balance(addr)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         });

         let nonce = client.request(chain, |client| async move {
            client
               .get_transaction_count(addr)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         });

         let code = client.request(chain, |client| async move {
            client
               .get_code_at(addr)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         });

         let (balance, nonce, code) = tokio::try_join!(balance, nonce, code)?;

         let (code, code_hash) = if !code.is_empty() {
            (Some(code.clone()), keccak256(&code))
         } else {
            (Some(Bytes::default()), KECCAK256_EMPTY)
         };

         let info = AccountInfo {
            nonce,
            balance,
            code: code.map(|bytes| Bytecode::new_raw(bytes)),
            code_hash,
         };

         let acc_info = AccountInfo2 {
            address: addr,
            info,
         };

         accounts.lock().await.push(acc_info);
         Ok(())
      });

      tasks.push(task);
   }

   for task in tasks {
      match task.await {
         Ok(Ok(())) => {}
         Ok(Err(e)) => tracing::error!("Fetch failed for address: {:?}", e),
         Err(e) => tracing::error!("Join error: {:?}", e),
      }
   }

   let accounts = Arc::try_unwrap(accounts).unwrap().into_inner();
   accounts
}

pub async fn fetch_storage_for_pools(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   pools: Vec<impl UniswapPool>,
) -> Vec<AccountStorage> {
   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let accounts = Arc::new(Mutex::new(Vec::new()));
   let mut account_slots = Vec::new();

   for pool in pools {
      if pool.dex_kind().is_v2() {
         let acc = AccountSlots {
            address: pool.address(),
            slots: v2_pool_standard_slots(),
         };
         account_slots.push(acc);
         continue;
      } else if pool.dex_kind().is_v3() {
         let acc = AccountSlots {
            address: pool.address(),
            slots: v3_pool_standard_slots(),
         };
         account_slots.push(acc);
         continue;
      } else {
         continue;
      };
   }

   for acc in account_slots {
      let ctx = ctx.clone();
      let accounts = accounts.clone();

      let task = RT.spawn(async move {
         let acc_info = fetch_storage(ctx.clone(), chain, block_id, acc).await;

         accounts.lock().await.extend(acc_info);
         Ok(())
      });

      tasks.push(task);
   }

   for task in tasks {
      match task.await {
         Ok(Ok(())) => {}
         Ok(Err(e)) => tracing::error!("Fetch failed for address: {:?}", e),
         Err(e) => tracing::error!("Join error: {:?}", e),
      }
   }

   let accounts = Arc::try_unwrap(accounts).unwrap().into_inner();
   accounts
}

pub async fn fetch_storage(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   account: AccountSlots,
) -> Vec<AccountStorage> {
   let client = ctx.get_zeus_client();

   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let accounts = Arc::new(Mutex::new(Vec::new()));

   for slot in account.slots {
      let client = client.clone();
      let accounts = accounts.clone();

      let task = RT.spawn(async move {
         let value = client
            .request(chain, |client| async move {
               client
                  .get_storage_at(account.address, slot)
                  .block_id(block_id)
                  .await
                  .map_err(|e| anyhow!("{:?}", e))
            })
            .await?;

         let acc = AccountStorage {
            address: account.address,
            slot,
            value,
         };

         accounts.lock().await.push(acc);
         Ok(())
      });

      tasks.push(task);
   }

   for task in tasks {
      match task.await {
         Ok(Ok(())) => {}
         Ok(Err(e)) => tracing::error!("Fetch failed for address: {:?}", e),
         Err(e) => tracing::error!("Join error: {:?}", e),
      }
   }

   let accounts = Arc::try_unwrap(accounts).unwrap().into_inner();
   accounts
}
