//! Account and storage prefetch for forked simulations.

use crate::{ClientManager, ZeusCtx};

use zeus_eth::{
   alloy_primitives::{Address, Bytes, KECCAK256_EMPTY, U256, address, keccak256},
   alloy_rpc_types::BlockId,
   amm::uniswap::UniswapPool,
   revm_utils::revm::state::{AccountInfo, Bytecode},
   utils::{address_book, batch},
};

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::info;

/// Max slots per StorageReader eth_call
pub const STORAGE_FETCH_CHUNK_SIZE: usize = 50;

/// Max addresses per StateView / `eth_getCode` batch.
const ACCOUNT_INFO_BATCH: usize = 20;

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

static RAILGUN_SMART_WALLET_KNOWN_SLOTS: LazyLock<Vec<U256>> = LazyLock::new(|| {
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
});

pub fn railgun_smart_wallet_known_slots() -> Vec<U256> {
   RAILGUN_SMART_WALLET_KNOWN_SLOTS.clone()
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

pub(super) fn dedupe_accounts(accounts: Vec<AccountPrefetch>) -> Vec<AccountPrefetch> {
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

/// Fetch balances, codes and EOA nonces for `accounts`.
///
/// An account whose reads failed is left out entirely, so the fork reads it from the node on first
/// touch. A zeroed [`AccountInfo`] must not stand in for a failed read: `insert_account_info` puts
/// it in the fork's cache, every later read is answered from there, and the node is never asked.
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

   let client = ctx.get_client_manager();
   let addresses: Vec<Address> = accounts.iter().map(|a| a.address).collect();
   let eoas: Vec<Address> = accounts.iter().filter(|a| a.is_eoa).map(|a| a.address).collect();

   let balances_fut = fetch_eth_balances_batched(
      ctx.rt(),
      client.clone(),
      chain,
      block_id,
      addresses.clone(),
   );
   let codes_fut = fetch_account_codes_batched(
      ctx.rt(),
      client.clone(),
      chain,
      block_id,
      addresses.clone(),
   );
   let nonces_fut = fetch_eoa_nonces(ctx.rt(), client.clone(), chain, block_id, eoas);

   let (balances, codes, nonces) = tokio::join!(balances_fut, codes_fut, nonces_fut);

   let total = accounts.len();
   let mut out = Vec::with_capacity(total);

   for acc in accounts {
      if let Some(info) = account_info_from_prefetch(acc, &balances, &codes, &nonces) {
         out.push(info);
      }
   }

   if out.len() < total {
      tracing::warn!(
         "Account prefetch failed for {} of {} accounts on chain {}, the fork will read them from the node",
         total - out.len(),
         total,
         chain
      );
   }

   info!(
      "Fetched accounts info in {} ms",
      time.elapsed().as_millis()
   );

   out
}

/// The fork state for one prefetched account, or `None` when a read for it failed.
///
/// A missing map entry is the failure signal, and only that: a present zero balance or an empty
/// code is the node's answer for an empty account and is still prefetched. A contract's nonce is
/// never fetched, so only an EOA needs a nonce entry.
fn account_info_from_prefetch(
   acc: AccountPrefetch,
   balances: &HashMap<Address, U256>,
   codes: &HashMap<Address, Bytes>,
   nonces: &HashMap<Address, u64>,
) -> Option<AccountInfo2> {
   let balance = *balances.get(&acc.address)?;
   let code = codes.get(&acc.address)?;
   let nonce = if acc.is_eoa {
      *nonces.get(&acc.address)?
   } else {
      0
   };

   let (code, code_hash) = if !code.is_empty() {
      (Some(code.clone()), keccak256(code))
   } else {
      (Some(Bytes::default()), KECCAK256_EMPTY)
   };

   Some(AccountInfo2 {
      address: acc.address,
      info: AccountInfo {
         nonce,
         balance,
         code: code.map(Bytecode::new_raw),
         account_id: None,
         code_hash,
      },
   })
}

pub(super) async fn fetch_eth_balances_batched(
   rt: tokio::runtime::Handle,
   client: ClientManager,
   chain: u64,
   block_id: BlockId,
   addresses: Vec<Address>,
) -> HashMap<Address, U256> {
   #[cfg(feature = "dev")]
   let time = Instant::now();

   let semaphore = Arc::new(Semaphore::new(client.concurrency()));
   let mut tasks = Vec::new();

   for chunk in addresses.chunks(ACCOUNT_INFO_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = semaphore.clone();
      tasks.push(rt.spawn(async move {
         let _permit = semaphore.acquire().await.unwrap();
         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_eth_balances(client, chain, Some(block_id), chunk).await
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

pub(super) async fn fetch_account_codes_batched(
   rt: tokio::runtime::Handle,
   client: ClientManager,
   chain: u64,
   block_id: BlockId,
   addresses: Vec<Address>,
) -> HashMap<Address, Bytes> {
   #[cfg(feature = "dev")]
   let time = Instant::now();

   let semaphore = Arc::new(Semaphore::new(client.concurrency()));
   let mut tasks = Vec::new();

   for chunk in addresses.chunks(ACCOUNT_INFO_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = semaphore.clone();
      tasks.push(rt.spawn(async move {
         let _permit = semaphore.acquire().await.unwrap();
         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_account_codes(client, chunk, Some(block_id)).await
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

pub(super) async fn fetch_eoa_nonces(
   rt: tokio::runtime::Handle,
   client: ClientManager,
   chain: u64,
   block_id: BlockId,
   eoas: Vec<Address>,
) -> HashMap<Address, u64> {
   if eoas.is_empty() {
      return HashMap::new();
   }

   #[cfg(feature = "dev")]
   let time = Instant::now();

   let semaphore = Arc::new(Semaphore::new(client.concurrency()));
   let mut tasks = Vec::new();
   for chunk in eoas.chunks(ACCOUNT_INFO_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = semaphore.clone();
      tasks.push(rt.spawn(async move {
         let _permit = semaphore.acquire().await.unwrap();
         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_account_nonces(client, chunk, Some(block_id)).await
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

   fetch_storage(ctx.clone(), chain, block_id, account).await
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

   let concurrency = ctx.get_client_manager().concurrency();
   let semaphore = Arc::new(Semaphore::new(concurrency));

   for acc in account_slots {
      let ctx = ctx.clone();
      let accounts = accounts.clone();
      let semaphore = semaphore.clone();

      let task = ctx.rt().spawn(async move {
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

   Arc::try_unwrap(accounts).unwrap().into_inner()
}

pub async fn fetch_storage(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   account: AccountSlots,
) -> Vec<AccountStorage> {
   let client = ctx.get_client_manager();
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
         .request_with(chain, chunk.clone(), |client, chunk| async move {
            batch::get_account_storage(client, address, chunk, Some(block_id)).await
         })
         .await;

      let read = match read_res {
         Ok(read) => read,
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!(
               "Failed to read storage for account {}: {:?}",
               address,
               _e
            );
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

   fn addr(b: u8) -> Address {
      Address::repeat_byte(b)
   }

   /// A missing entry is a failed read, and the account stays out of the fork so the backend asks
   /// the node. Filling in a default here would cache a wrong answer instead: the backend serves
   /// every later read from its cache and never re-asks.
   #[test]
   fn a_failed_read_leaves_the_account_out_of_the_fork() {
      let who = addr(0x11);
      let eoa = AccountPrefetch::eoa(who);

      let balances = HashMap::from([(who, U256::from(5))]);
      let codes = HashMap::from([(who, Bytes::from_static(&[0x60, 0x00]))]);
      let nonces = HashMap::from([(who, 3u64)]);

      assert!(
         account_info_from_prefetch(eoa, &balances, &codes, &nonces).is_some(),
         "every read answered, so the account is prefetched"
      );

      assert!(
         account_info_from_prefetch(eoa, &HashMap::new(), &codes, &nonces).is_none(),
         "a failed balance read is not a zero balance"
      );
      assert!(
         account_info_from_prefetch(eoa, &balances, &HashMap::new(), &nonces).is_none(),
         "a failed code read is not an empty code"
      );
      assert!(
         account_info_from_prefetch(eoa, &balances, &codes, &HashMap::new()).is_none(),
         "a failed nonce read is not nonce zero"
      );
   }

   /// Zero is the node's answer for an empty account, not a missing entry, so it is still
   /// prefetched. A contract carries no nonce read at all.
   #[test]
   fn an_empty_answer_is_still_prefetched() {
      let eoa = addr(0x22);
      let token = addr(0x33);

      let balances = HashMap::from([(eoa, U256::ZERO), (token, U256::ZERO)]);
      let codes = HashMap::from([
         (eoa, Bytes::default()),
         (token, Bytes::from_static(&[0x60, 0x00])),
      ]);
      let nonces = HashMap::from([(eoa, 0u64)]);

      let info = account_info_from_prefetch(
         AccountPrefetch::eoa(eoa),
         &balances,
         &codes,
         &nonces,
      )
      .expect("a zero balance is an answer, not a failure");

      assert_eq!(info.info.balance, U256::ZERO);
      assert_eq!(info.info.nonce, 0);
      assert_eq!(info.info.code_hash, KECCAK256_EMPTY);

      let info = account_info_from_prefetch(
         AccountPrefetch::contract(token),
         &balances,
         &codes,
         &nonces,
      )
      .expect("a contract has no nonce read to fail");

      assert_eq!(info.info.nonce, 0);
      assert_eq!(info.info.code_hash, keccak256([0x60, 0x00]));
   }
}
