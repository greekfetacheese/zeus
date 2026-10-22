//! Simulation entry points and the fork machinery.

use crate::ZeusCtx;

use super::prefetch::{
   AccountPrefetch, AccountStorage, fetch_accounts_info, fetch_storage_for_pools,
   fetch_storage_for_railgun,
};
use alloy_eips::eip7702::SignedAuthorization;
use either::Either;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_primitives::{Address, Bytes, Log, TxKind, U256},
   alloy_rpc_types::{Block, BlockId},
   amm::uniswap::AnyUniswapPool,
   revm_utils::{
      Database, DatabaseCommit, Evm2, ExecuteCommitEvm, ExecutionResult, ForkDB, ForkFactory, Host,
      new_evm, revert_msg, revm::primitives::hardfork::SpecId,
   },
   types::ChainId,
   utils::client::RpcClient,
};

use anyhow::anyhow;
use std::time::Instant;

pub fn eip7702_implementation(code: &[u8]) -> Option<Address> {
   if code.len() == 23 && code[0] == 0xef && code[1] == 0x01 && code[2] == 0x00 {
      Some(Address::from_slice(&code[3..]))
   } else {
      None
   }
}

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

   let time = Instant::now();

   let sim_res = evm
      .transact_commit(evm.tx.clone())
      .map_err(|e| anyhow!("Simulation failed: {:?}", e))?;
   let output = sim_res.output().unwrap_or_default();
   let gas_used = sim_res.tx_gas_used();

   if !sim_res.is_success() {
      let err = revert_msg(output);
      tracing::error!(
         "Simulation failed: {} \n Gas Used {}",
         err,
         gas_used
      );
      return Err(anyhow!("Failed to simulate transaction: {}", err));
   }

   tracing::info!(
      "Simulate Transaction took {} ms",
      time.elapsed().as_millis()
   );

   Ok(sim_res)
}

/// Fork head fetched from `source` (usually [`BlockId::latest`]) plus the pinned
/// [`BlockId`] callers must use for their "before" reads.
///
/// Pinning every before/after read to the returned id keeps them from straddling
/// a new head while the transaction is being built and simulated. Use
/// `BlockId::number(last_synced_block)` as `source` for the Railgun flows.
pub async fn pinned_head(
   ctx: ZeusCtx,
   chain: ChainId,
   source: BlockId,
) -> Result<(Block, BlockId), anyhow::Error> {
   let client = ctx.get_client_manager();

   let block = client
      .request(chain.id(), |client| async move {
         client.get_block(source).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let block = block.ok_or_else(|| anyhow!("No block found, this is usually a provider issue"))?;

   let block_id = BlockId::number(block.header.number);

   Ok((block, block_id))
}

/// Native balance of `owner` at `block_id`.
pub async fn native_balance_at(
   ctx: ZeusCtx,
   chain: ChainId,
   owner: Address,
   block_id: BlockId,
) -> Result<U256, anyhow::Error> {
   let client = ctx.get_client_manager();

   client
      .request(chain.id(), |client| async move {
         client
            .get_balance(owner)
            .block_id(block_id)
            .await
            .map_err(|e| anyhow!("{:?}", e))
      })
      .await
}

/// Fork-sim result used to build a first-party [`crate::TransactionAnalysis`].
pub struct SimulatedCall {
   pub logs: Vec<Log>,
   pub gas_used: u64,
   pub balance_before: U256,
   pub balance_after: U256,
   /// The block the fork was pinned to (see [`ForkSim::block_number`]).
   pub block_number: u64,
}

/// Contract storage to load into the fork before simulating.
///
/// Pool state and the Railgun smart wallet are read straight from storage during
/// the simulation, so they must be prefetched or the fork replays RPC calls
/// mid-transact.
pub enum StoragePrefetch {
   None,
   Pools(Vec<AnyUniswapPool>),
   Railgun(Address),
}

impl StoragePrefetch {
   async fn fetch(self, ctx: ZeusCtx, chain: ChainId, block_id: BlockId) -> Vec<AccountStorage> {
      match self {
         StoragePrefetch::None => Vec::new(),
         StoragePrefetch::Pools(pools) => {
            fetch_storage_for_pools(ctx, chain.id(), block_id, pools).await
         }
         StoragePrefetch::Railgun(address) => {
            fetch_storage_for_railgun(ctx, chain.id(), block_id, address).await
         }
      }
   }
}

/// What to load into the fork before simulating.
pub struct ForkPrefetch {
   /// Pinned head; its block id is used for every prefetch and for the fork itself.
   pub block: Block,
   /// Caller decides the set — it is protocol knowledge, not boilerplate.
   pub accounts: Vec<AccountPrefetch>,
   pub storage: StoragePrefetch,
}

impl ForkPrefetch {
   /// Prefetch at `block` with no storage beyond the accounts themselves.
   pub fn new(block: Block, accounts: Vec<AccountPrefetch>, storage: StoragePrefetch) -> Self {
      Self {
         block,
         accounts,
         storage,
      }
   }
}

/// The call to simulate on a forked state.
pub struct ForkSimRequest {
   pub from: Address,
   pub interact_to: Address,
   pub call_data: Bytes,
   pub value: U256,
   /// Railgun forces 30M because proving makes simulated gas meaningless.
   pub gas_limit: Option<u64>,
   pub authorization_list: Vec<SignedAuthorization>,
}

/// Result of a fork simulation.
///
/// Deliberately `Send` (guarded by a test): the send path is spawned, so callers
/// hold this across awaits. The EVM stays inside the simulate functions.
pub struct ForkSim {
   pub sim_res: ExecutionResult,
   pub logs: Vec<Log>,
   pub balance_before: U256,
   pub balance_after: U256,
   /// The block the fork was pinned to. Callers that show what they simulated
   /// ("simulated at block N") carry it onto the analysis.
   pub block_number: u64,
}

/// Prefetch `prefetch`'s accounts and storage and return a factory holding them.
///
/// [`simulate_on_fork_with`] takes the common case; this is for callers that must
/// commit state before simulating — `swap_via_ur` commits the Permit2 approval
/// and simulates against that fork.
pub async fn prepare_fork(
   ctx: ZeusCtx,
   chain: ChainId,
   block: &Block,
   accounts: Vec<AccountPrefetch>,
   storage: StoragePrefetch,
) -> Result<ForkFactory<RpcClient>, anyhow::Error> {
   let block_id = BlockId::number(block.header.number);

   let accounts_info_fut = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts);
   let storage_info_fut = storage.fetch(ctx.clone(), chain, block_id);

   let time = Instant::now();

   let accounts_info = accounts_info_fut.await;
   let storage_info = storage_info_fut.await;

   tracing::info!(
      "Fetched accounts & storage info in {} ms",
      time.elapsed().as_millis()
   );

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   for info in storage_info {
      match factory.insert_account_storage(info.address, info.slot, info.value) {
         Ok(_) => {}
         Err(e) => tracing::error!("Failed to insert account storage: {:?}", e),
      }
   }

   Ok(factory)
}

/// Simulate `req` on a fork that is already prepared, handing the post-simulation
/// EVM to `after` before dropping it.
///
/// `after` sees the state the tx actually produced, so it can read the balances
/// and allowances the simulation created (`sim_diff` does). Keeping the EVM in
/// here is what lets [`ForkSim`] stay `Send`.
///
/// `spec` is the chain's hardfork as its node reports it ([`new_evm`]); the caller resolves it,
/// since asking the node is async and this is not.
pub fn simulate_on_fork_db<R>(
   chain: ChainId,
   block: &Block,
   fork_db: ForkDB,
   spec: Option<SpecId>,
   req: ForkSimRequest,
   after: impl FnOnce(&mut Evm2<ForkDB>, &ForkSim) -> R,
) -> Result<(ForkSim, R), anyhow::Error> {
   let ForkSimRequest {
      from,
      interact_to,
      call_data,
      value,
      gas_limit,
      authorization_list,
   } = req;

   let mut evm = new_evm(chain, Some(block), fork_db, spec);

   if let Some(gas_limit) = gas_limit {
      evm.tx.gas_limit = gas_limit;
   }

   let balance_before = evm.balance(from).map(|state| state.data).unwrap_or(U256::ZERO);

   let sim_res = simulate_transaction(
      &mut evm,
      from,
      interact_to,
      call_data,
      value,
      authorization_list,
   )?;

   let balance_after = evm.balance(from).map(|state| state.data).unwrap_or(U256::ZERO);
   let logs = sim_res.clone().into_logs();

   let sim = ForkSim {
      sim_res,
      logs,
      balance_before,
      balance_after,
      block_number: block.header.number,
   };

   let after_result = after(&mut evm, &sim);

   Ok((sim, after_result))
}

/// Prefetch, fork, and simulate the call.
pub async fn simulate_on_fork(
   ctx: ZeusCtx,
   chain: ChainId,
   prefetch: ForkPrefetch,
   req: ForkSimRequest,
) -> Result<ForkSim, anyhow::Error> {
   simulate_on_fork_with(ctx, chain, prefetch, req, |_, _| ())
      .await
      .map(|(sim, _)| sim)
}

/// Prefetch, fork, and simulate the call, handing the post-simulation EVM to
/// `after` before dropping it.
pub async fn simulate_on_fork_with<R>(
   ctx: ZeusCtx,
   chain: ChainId,
   prefetch: ForkPrefetch,
   req: ForkSimRequest,
   after: impl FnOnce(&mut Evm2<ForkDB>, &ForkSim) -> R,
) -> Result<(ForkSim, R), anyhow::Error> {
   let ForkPrefetch {
      block,
      accounts,
      storage,
   } = prefetch;

   // What the chain's node says it runs, so the simulation is priced by the fork the chain
   // actually has; `None` (no answer) leaves the chain's own spec.
   let spec = ctx.node_spec(chain).await;

   let factory = prepare_fork(ctx, chain, &block, accounts, storage).await?;

   simulate_on_fork_db(
      chain,
      &block,
      factory.new_sandbox_fork(),
      spec,
      req,
      after,
   )
}

/// Prefetch, fork, and simulate a call. Native before/after are the same EVM snapshot.
pub async fn simulate_for_analysis(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
   extra_prefetch: Vec<AccountPrefetch>,
) -> Result<SimulatedCall, anyhow::Error> {
   let client = ctx.get_client_manager();

   let (block, _) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let bytecode = client
      .request(chain.id(), |client| async move {
         client.get_code_at(interact_to).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let interact_prefetch = if bytecode.is_empty() {
      AccountPrefetch::eoa(interact_to)
   } else {
      AccountPrefetch::contract(interact_to)
   };

   let mut accounts = vec![
      AccountPrefetch::eoa(from),
      interact_prefetch,
      AccountPrefetch::eoa(block.header.beneficiary),
   ];
   accounts.extend(extra_prefetch);

   let sim = simulate_on_fork(
      ctx,
      chain,
      ForkPrefetch {
         block,
         accounts,
         storage: StoragePrefetch::None,
      },
      ForkSimRequest {
         from,
         interact_to,
         call_data,
         value,
         gas_limit: None,
         authorization_list: Vec::new(),
      },
   )
   .await?;

   Ok(SimulatedCall {
      logs: sim.logs,
      gas_used: sim.sim_res.tx_gas_used(),
      balance_before: sim.balance_before,
      balance_after: sim.balance_after,
      block_number: sim.block_number,
   })
}
