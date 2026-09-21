use crate::core::{ZeusClient, ZeusCtx};
use crate::utils::RT;

use alloy_eips::eip7702::SignedAuthorization;
use either::Either;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_primitives::{Address, Bytes, KECCAK256_EMPTY, Log, TxKind, U256, address, keccak256},
   alloy_rpc_types::{Block, BlockId},
   amm::uniswap::{AnyUniswapPool, UniswapPool},
   revm_utils::{
      Database, DatabaseCommit, Evm2, ExecuteCommitEvm, ExecutionResult, ForkDB, ForkFactory, Host,
      new_evm, revert_msg,
      revm::state::{AccountInfo, Bytecode},
   },
   types::ChainId,
   utils::{address_book, batch, client::RpcClient},
};

use anyhow::anyhow;
use std::collections::HashMap;
use std::str::FromStr;
use std::{sync::Arc, time::Instant};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::info;

/// Max slots per StorageReader eth_call
pub const STORAGE_FETCH_CHUNK_SIZE: usize = 50;

/// Max addresses per StateView / `eth_getCode` batch.
const ACCOUNT_INFO_BATCH: usize = 20;

/// Concurrent RPC batches (balances, codes, and nonce fetches).
const CONCURRENCY: usize = 1;

/// EIP-7702 designated code is `0xef0100 || implementation`.
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
   let client = ctx.get_zeus_client();

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
   let client = ctx.get_zeus_client();

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

/// Fork-sim result used to build a first-party [`crate::core::TransactionAnalysis`].
pub struct SimulatedCall {
   pub logs: Vec<Log>,
   pub gas_used: u64,
   pub balance_before: U256,
   pub balance_after: U256,
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
pub fn simulate_on_fork_db<R>(
   chain: ChainId,
   block: &Block,
   fork_db: ForkDB,
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

   let mut evm = new_evm(chain, Some(block), fork_db);

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

   let factory = prepare_fork(ctx, chain, &block, accounts, storage).await?;

   simulate_on_fork_db(
      chain,
      &block,
      factory.new_sandbox_fork(),
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
   let client = ctx.get_zeus_client();

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
   })
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

pub fn railgun_common_accounts(chain: u64) -> Vec<Address> {
   let mut accounts = Vec::new();

   if let Ok(addr) = address_book::railgun_implementation(chain) {
      accounts.push(addr);
   }

   if chain == 1 {
      accounts.push(address!(
         "0x7D9ef64f35B6Afda8d258d1d2548a9aC997e35A1"
      ));
      accounts.push(address!(
         "0xd0198Dde1187b12aF01a743d9e9f2B4B84e8f59b"
      ));
   }

   accounts
}

pub fn railgun_smart_wallet_known_slots() -> Vec<U256> {
   vec![
      U256::from(100),
      U256::from(101),
      U256::from(102),
      U256::from(103),
      U256::from(104),
      U256::from(105),
      U256::from(106),
      U256::from(250),
      U256::from(249),
      U256::from(111),
      U256::from(122),
      U256::from(123),
      U256::from(124),
      U256::from(125),
      U256::from(110),
      U256::from(126),
      U256::from(127),
      U256::from(112),
      U256::from(128),
      U256::from(129),
      U256::from(114),
      U256::from(130),
      U256::from(115),
      U256::from(131),
      U256::from(132),
      U256::from(133),
      U256::from(134),
      U256::from(119),
      U256::from(135),
      U256::from(120),
      U256::from(136),
      U256::from(121),
      U256::from(137),
      U256::from(254),
      U256::from(108),
      U256::from(109),
      U256::from(107),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578059",
      )
      .unwrap(),
      U256::from_str(
         "34151261456300087439997391738331726178288962906741376914590545081241414870078",
      )
      .unwrap(),
      U256::from_str(
         "34151261456300087439997391738331726178288962906741376914590545081241414870079",
      )
      .unwrap(),
      U256::from_str(
         "41686179514459682887445184874087805914735208064873070197648607631960135268241",
      )
      .unwrap(),
      U256::from_str(
         "70317207819681945256554025353136292375664589604508357446255978928579956073267",
      )
      .unwrap(),
      U256::from_str(
         "94399812825888861499486677605933707837548266014517085953451337810630634584187",
      )
      .unwrap(),
      U256::from_str(
         "18296122654818958850168284695448851410897147423951460005733279896325587213801",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277359",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277358",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277357",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277356",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277355",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277354",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277353",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277352",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277351",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277350",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277349",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277348",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277347",
      )
      .unwrap(),
      U256::from_str(
         "31167265274857606537906508571182340861878936415094759065010277415513360277346",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578060",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578061",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578062",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578063",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578064",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578065",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578066",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578067",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578068",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578069",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578070",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578071",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578072",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578073",
      )
      .unwrap(),
      U256::from_str(
         "106975538549889489890283625107144024636981818160187513719796593493211775578074",
      )
      .unwrap(),
   ]
}


#[derive(Clone, Copy, Debug)]
pub struct AccountPrefetch {
   pub address: Address,
   pub is_eoa: bool,
}

impl AccountPrefetch {
   pub fn eoa(address: Address) -> Self {
      Self {
         address,
         is_eoa: true,
      }
   }

   pub fn contract(address: Address) -> Self {
      Self {
         address,
         is_eoa: false,
      }
   }
}

fn dedupe_accounts(accounts: Vec<AccountPrefetch>) -> Vec<AccountPrefetch> {
   let mut out: Vec<AccountPrefetch> = Vec::new();
   for acc in accounts {
      if acc.address.is_zero() {
         continue;
      }
      if let Some(existing) = out.iter_mut().find(|a| a.address == acc.address) {
         existing.is_eoa |= acc.is_eoa;
         continue;
      }
      out.push(acc);
   }
   out
}

pub async fn fetch_accounts_info(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   accounts: Vec<AccountPrefetch>,
) -> Vec<AccountInfo2> {
   let accounts = dedupe_accounts(accounts);
   if accounts.is_empty() {
      return Vec::new();
   }

   let time = Instant::now();

   let client = ctx.get_zeus_client();
   let addresses: Vec<Address> = accounts.iter().map(|a| a.address).collect();
   let eoas: Vec<Address> = accounts.iter().filter(|a| a.is_eoa).map(|a| a.address).collect();

   let balances_fut =
      fetch_eth_balances_batched(client.clone(), chain, block_id, addresses.clone());
   let codes_fut = fetch_account_codes_batched(client.clone(), chain, block_id, addresses.clone());
   let nonces_fut = fetch_eoa_nonces(client.clone(), chain, block_id, eoas);

   let (balances, codes, nonces) = tokio::join!(balances_fut, codes_fut, nonces_fut);

   let mut out = Vec::with_capacity(accounts.len());

   for acc in accounts {
      let balance = balances.get(&acc.address).copied().unwrap_or(U256::ZERO);
      let code = codes.get(&acc.address).cloned().unwrap_or_default();
      let nonce = if acc.is_eoa {
         nonces.get(&acc.address).copied().unwrap_or(0)
      } else {
         0
      };

      let (code, code_hash) = if !code.is_empty() {
         (Some(code.clone()), keccak256(&code))
      } else {
         (Some(Bytes::default()), KECCAK256_EMPTY)
      };

      out.push(AccountInfo2 {
         address: acc.address,
         info: AccountInfo {
            nonce,
            balance,
            code: code.map(|bytes| Bytecode::new_raw(bytes)),
            account_id: None,
            code_hash,
         },
      });
   }

   info!(
      "Fetched accounts info in {} ms",
      time.elapsed().as_millis()
   );

   out
}

async fn fetch_eth_balances_batched(
   client: ZeusClient,
   chain: u64,
   block_id: BlockId,
   addresses: Vec<Address>,
) -> HashMap<Address, U256> {
   #[cfg(feature = "dev")]
   let time = Instant::now();

   let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
   let mut tasks = Vec::new();

   for chunk in addresses.chunks(ACCOUNT_INFO_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = semaphore.clone();
      tasks.push(RT.spawn(async move {
         let _permit = semaphore.acquire().await.unwrap();
         client
            .request(chain, |client| {
               let chunk = chunk.clone();
               async move { batch::get_eth_balances(client, chain, Some(block_id), chunk).await }
            })
            .await
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok(rows)) => {
            for row in rows {
               out.insert(row.owner, row.balance);
            }
         }
         Ok(Err(e)) => tracing::error!("ETH balance batch failed: {:?}", e),
         Err(e) => tracing::error!("ETH balance batch join error: {:?}", e),
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Fetched ETH balances in {} ms",
      time.elapsed().as_millis()
   );

   out
}

async fn fetch_account_codes_batched(
   client: ZeusClient,
   chain: u64,
   block_id: BlockId,
   addresses: Vec<Address>,
) -> HashMap<Address, Bytes> {
   #[cfg(feature = "dev")]
   let time = Instant::now();

   let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
   let mut tasks = Vec::new();

   for chunk in addresses.chunks(ACCOUNT_INFO_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = semaphore.clone();
      tasks.push(RT.spawn(async move {
         let _permit = semaphore.acquire().await.unwrap();
         client
            .request(chain, |client| {
               let chunk = chunk.clone();
               async move { batch::get_account_codes(client, chunk, Some(block_id)).await }
            })
            .await
            .map(|codes| (chunk, codes))
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok((chunk, codes))) => {
            for (addr, code) in chunk.into_iter().zip(codes) {
               out.insert(addr, code);
            }
         }
         Ok(Err(e)) => tracing::error!("account code batch failed: {:?}", e),
         Err(e) => tracing::error!("account code batch join error: {:?}", e),
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Fetched account codes in {} ms",
      time.elapsed().as_millis()
   );

   out
}

async fn fetch_eoa_nonces(
   client: ZeusClient,
   chain: u64,
   block_id: BlockId,
   eoas: Vec<Address>,
) -> HashMap<Address, u64> {
   if eoas.is_empty() {
      return HashMap::new();
   }

   #[cfg(feature = "dev")]
   let time = Instant::now();

   let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
   let mut tasks = Vec::new();
   for chunk in eoas.chunks(ACCOUNT_INFO_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = semaphore.clone();
      tasks.push(RT.spawn(async move {
         let _permit = semaphore.acquire().await.unwrap();
         client
            .request(chain, |client| {
               let chunk = chunk.clone();
               async move { batch::get_account_nonces(client, chunk, Some(block_id)).await }
            })
            .await
            .map(|nonces| (chunk, nonces))
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok((chunk, nonces))) => {
            for (addr, nonce) in chunk.into_iter().zip(nonces) {
               out.insert(addr, nonce);
            }
         }
         Ok(Err(e)) => tracing::error!("EOA nonce batch failed: {:?}", e),
         Err(e) => tracing::error!("EOA nonce batch join error: {:?}", e),
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Fetched EOA nonces in {} ms",
      time.elapsed().as_millis()
   );

   out
}

pub async fn fetch_storage_for_railgun(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   railgun_address: Address,
) -> Vec<AccountStorage> {
   let account = AccountSlots {
      address: railgun_address,
      slots: railgun_smart_wallet_known_slots(),
   };

   let account_storage = fetch_storage(ctx.clone(), chain, block_id, account).await;

   account_storage
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

   let semaphore = Arc::new(Semaphore::new(CONCURRENCY));

   for acc in account_slots {
      let ctx = ctx.clone();
      let accounts = accounts.clone();
      let semaphore = semaphore.clone();

      let task = RT.spawn(async move {
         let _permit = semaphore.acquire().await.unwrap();
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
   let address = account.address;

   let chunks: Vec<Vec<U256>> =
      account.slots.chunks(STORAGE_FETCH_CHUNK_SIZE).map(|c| c.to_vec()).collect();

   if chunks.is_empty() {
      return Vec::new();
   }

   let mut out = Vec::new();
   let time = Instant::now();

   for chunk in chunks {
      let client = client.clone();

      let read_res = client
         .request(chain, |client| {
            let chunk = chunk.clone();
            async move { batch::get_account_storage(client, address, chunk, Some(block_id)).await }
         })
         .await;

      let read = match read_res {
         Ok(read) => read,
         Err(e) => {
            tracing::error!("Failed to read storage for account {}: {:?}", address, e);
            continue;
         }
      };

      let storage = read
         .slots
         .into_iter()
         .zip(read.values)
         .map(|(slot, value)| AccountStorage {
            address: read.address,
            slot,
            value,
         })
         .collect::<Vec<_>>();

      out.extend(storage);
   }

   info!(
      "Fetched storage in {} ms",
      time.elapsed().as_millis()
   );

   out
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn railgun_slots() {
      let slots = railgun_smart_wallet_known_slots();
      println!("{}", slots.len());
   }

   /// The send path is spawned, so callers hold a `ForkSim` across awaits. If this
   /// ever stops compiling, the EVM has leaked back out of `simulate_on_fork_with`.
   #[test]
   fn fork_sim_is_send() {
      fn assert_send<T: Send>() {}
      assert_send::<ForkSim>();
   }
}
