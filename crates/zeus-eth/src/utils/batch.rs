use alloy_contract::private::{Network, Provider};
use alloy_network::TransactionBuilder;
use alloy_primitives::{Address, Bytes, FixedBytes, U64, U256, hex};
use alloy_rpc_types::{BlockId, state::StateOverridesBuilder};
use alloy_sol_types::{SolCall, sol};
use std::sync::LazyLock;

use super::address_book::zeus_stateview_v4;
use crate::{
   abi::{
      erc20::IERC20,
      erc721::{IERC721, IERC721Metadata},
      erc1155::{IERC1155, IERC5216},
      permit::Permit2,
      zeus::ZeusStateViewV3::{self, *},
   },
   utils::address_book,
};
use alloy_provider::CallItem;
use alloy_rpc_client::BatchRequest;

/// Runtime bytecode of `StorageReader`.
///
/// Injected onto target accounts via `eth_call` state override so `SLOAD` runs in that
/// account's storage context. Not meant to be deployed as a permanent on-chain contract.
const STORAGE_READER_BYTECODE: &str = "0x60806040526004361015610011575f80fd5b5f3560e01c80636e374254146100da5763929dfacb1461002f575f80fd5b346100d65761003d3661017c565b61004e6100498261020a565b6101d0565b9181835261005b8261020a565b602084019290601f19013684375f5b8181106100b5578385604051918291602083019060208452518091526040830191905f5b81811061009c575050500390f35b825184528594506020938401939092019160010161008e565b806100c36001928486610222565b35546100cf8288610246565b520161006a565b5f80fd5b346100d6576100e83661017c565b6100f46100498261020a565b918183526101018261020a565b602084019290601f19013684375f5b81811061015b578385604051918291602083019060208452518091526040830191905f5b818110610142575050500390f35b8251845285945060209384019390920191600101610134565b806101696001928486610222565b35546101758288610246565b5201610110565b9060206003198301126100d65760043567ffffffffffffffff81116100d657826023820112156100d65780600401359267ffffffffffffffff84116100d65760248460051b830101116100d6576024019190565b6040519190601f01601f1916820167ffffffffffffffff8111838210176101f657604052565b634e487b7160e01b5f52604160045260245ffd5b67ffffffffffffffff81116101f65760051b60200190565b91908110156102325760051b0190565b634e487b7160e01b5f52603260045260245ffd5b80518210156102325760209160051b01019056fea264697066735822122025b1c3b8ddb70d3ce6a43258bd59f53cfa585331162dabd038b8ec1fea55640864736f6c634300081e0033";

/// High gas ceiling for the reader eth_call so the provider never runs estimateGas
/// against the *real* target bytecode (that produces InvalidJump when overrides are absent
/// from the estimate request).
const STORAGE_READER_CALL_GAS: u64 = 30_000_000;

static STORAGE_READER_CODE: LazyLock<Bytes> = LazyLock::new(|| {
   let hex_str = STORAGE_READER_BYTECODE.strip_prefix("0x").unwrap_or(STORAGE_READER_BYTECODE);
   Bytes::from(hex::decode(hex_str).expect("STORAGE_READER_BYTECODE must be valid hex"))
});

sol! {
   contract StorageReader {
      function readSlotsUint(uint256[] calldata slots) external view returns (uint256[] memory values);
   }
}

/// One account + the slots to read from it.
#[derive(Clone, Debug)]
pub struct AccountSlots {
   pub address: Address,
   pub slots: Vec<U256>,
}

/// Batched storage read result for a single account (`slots[i]` ↔ `values[i]`).
#[derive(Clone, Debug)]
pub struct AccountStorageRead {
   pub address: Address,
   pub slots: Vec<U256>,
   pub values: Vec<U256>,
}

impl AccountStorageRead {
   /// Flatten to `(address, slot, value)` triples for fork-db inserts.
   pub fn into_entries(self) -> impl Iterator<Item = (Address, U256, U256)> {
      let address = self.address;
      self
         .slots
         .into_iter()
         .zip(self.values)
         .map(move |(slot, value)| (address, slot, value))
   }
}

fn storage_reader_code() -> Bytes {
   STORAGE_READER_CODE.clone()
}

/// Batch-read storage slots for `account` in **one** `eth_call`.
///
/// Injects [`STORAGE_READER_BYTECODE`] onto `account` via state override so `SLOAD` runs
/// in that account's storage context (balance / nonce / storage stay real).
///
/// ```ignore
/// let storage = get_account_storage(client, railgun, slots, Some(block_id)).await?;
/// ```
pub async fn get_account_storage<P, N>(
   client: P,
   address: Address,
   slots: Vec<U256>,
   block: Option<BlockId>,
) -> Result<AccountStorageRead, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block_id = block.unwrap_or(BlockId::latest());
   // Raw provider.call — avoids SolCallBuilder edge-cases and forces gas so fillers never
   // estimateGas against the real target bytecode (InvalidJump without overrides).
   let calldata = StorageReader::readSlotsUintCall {
      slots: slots.clone(),
   }
   .abi_encode();

   let tx = N::TransactionRequest::default()
      .with_to(address)
      .with_input(Bytes::from(calldata))
      .with_gas_limit(STORAGE_READER_CALL_GAS);

   let overrides = StateOverridesBuilder::default()
      .with_code(address, storage_reader_code())
      .build();

   let raw = client
      .call(tx)
      .block(block_id)
      .overrides(overrides)
      .await
      .map_err(|e| anyhow::anyhow!("StorageReader eth_call failed for {address}: {e:?}"))?;

   let values = StorageReader::readSlotsUintCall::abi_decode_returns(&raw).map_err(|e| {
      anyhow::anyhow!("failed decoding StorageReader return for {address}: {e:?} (ret={raw})")
   })?;

   if values.len() != slots.len() {
      anyhow::bail!(
         "StorageReader returned {} values for {} slots (account {address})",
         values.len(),
         slots.len()
      );
   }

   Ok(AccountStorageRead {
      address,
      slots,
      values,
   })
}

/// Query the ETH balance for the given addresses
pub async fn get_eth_balances<P, N>(
   client: P,
   chain: u64,
   block: Option<BlockId>,
   addresses: Vec<Address>,
) -> Result<Vec<ETHBalance>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if addresses.is_empty() {
      return Ok(Vec::new());
   }
   let block = block.unwrap_or(BlockId::latest());
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let balance = contract.getETHBalance(addresses).call().block(block).await?;
   Ok(balance)
}

/// `eth_getCode` for many accounts in one JSON-RPC batch.
pub async fn get_account_codes<P, N>(
   client: P,
   accounts: Vec<Address>,
   block: Option<BlockId>,
) -> Result<Vec<Bytes>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if accounts.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());
   let mut batch = BatchRequest::new(client.client());
   let mut waiters = Vec::with_capacity(accounts.len());
   for addr in &accounts {
      let waiter = batch
         .add_call::<_, Bytes>("eth_getCode", &(addr, block))
         .map_err(|e| anyhow::anyhow!("eth_getCode batch serialize: {e:?}"))?;
      waiters.push(waiter);
   }

   batch.send().await.map_err(|e| anyhow::anyhow!("eth_getCode batch: {e:?}"))?;

   let mut out = Vec::with_capacity(waiters.len());

   for waiter in waiters {
      let code = waiter.await.map_err(|e| anyhow::anyhow!("eth_getCode: {e:?}"))?;
      out.push(code);
   }
   Ok(out)
}

/// `eth_getTransactionCount` for many accounts in one JSON-RPC batch.
pub async fn get_account_nonces<P, N>(
   client: P,
   accounts: Vec<Address>,
   block: Option<BlockId>,
) -> Result<Vec<u64>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if accounts.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());
   let mut batch = BatchRequest::new(client.client());
   let mut waiters = Vec::with_capacity(accounts.len());
   for addr in &accounts {
      let waiter = batch
         .add_call::<_, U64>("eth_getTransactionCount", &(addr, block))
         .map_err(|e| anyhow::anyhow!("eth_getTransactionCount batch serialize: {e:?}"))?;
      waiters.push(waiter);
   }

   batch
      .send()
      .await
      .map_err(|e| anyhow::anyhow!("eth_getTransactionCount batch: {e:?}"))?;

   let mut out = Vec::with_capacity(waiters.len());
   for waiter in waiters {
      let nonce = waiter.await.map_err(|e| anyhow::anyhow!("eth_getTransactionCount: {e:?}"))?;
      out.push(nonce.to::<u64>());
   }
   Ok(out)
}

/// Query the balance of multiple ERC20 tokens for the given owner
pub async fn get_erc20_balances<P, N>(
   client: P,
   chain: u64,
   block: Option<BlockId>,
   owner: Address,
   tokens: Vec<Address>,
) -> Result<Vec<ERC20Balance>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let balance = contract.getERC20Balance(tokens, owner).call().block(block).await?;
   Ok(balance)
}

/// Query the ERC20 token info for the given token
pub async fn get_erc20_info<P, N>(
   client: P,
   chain: u64,
   token: Address,
) -> Result<ERC20Info, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let info = contract.getERC20Info(token).call().await?;
   Ok(info)
}

/// Query the ERC20 token info for the given tokens
pub async fn get_erc20_tokens<P, N>(
   client: P,
   chain: u64,
   tokens: Vec<Address>,
) -> Result<Vec<ERC20Info>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let info = contract.getERC20InfoBatch(tokens).call().await?;
   Ok(info)
}

/// Get all possible pools based on the token pairs and fee tiers
pub async fn get_pools<P, N>(
   client: P,
   chain: u64,
   v2_factory: Address,
   v3_factory: Address,
   state_view: Address,
   v4_pools: Vec<FixedBytes<32>>,
   base_tokens: Vec<Address>,
   quote_token: Address,
) -> Result<Pools, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let pools = contract
      .getPools(
         v2_factory,
         v3_factory,
         state_view,
         v4_pools,
         base_tokens,
         quote_token,
      )
      .call()
      .await?;
   Ok(pools)
}

/// Get the pools state for the given pools
pub async fn get_pools_state<P, N>(
   client: P,
   chain: u64,
   v2_pools: Vec<Address>,
   v3_pools: Vec<V3Pool>,
   v4_pools: Vec<V4Pool>,
   state_view: Address,
) -> Result<PoolsState, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let pools_state =
      contract.getPoolsState(v2_pools, v3_pools, v4_pools, state_view).call().await?;
   Ok(pools_state)
}

/// Get all possible V3 pools based on token pair
pub async fn get_v3_pools<P, N>(
   client: P,
   chain: u64,
   factory: Address,
   token_a: Address,
   token_b: Address,
) -> Result<Vec<V3Pool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let pools = contract.getV3Pools(factory, token_a, token_b).call().await?;
   Ok(pools)
}

/// Validate the given V4 pools
pub async fn validate_v4_pools<P, N>(
   client: P,
   chain: u64,
   pools: Vec<FixedBytes<32>>,
) -> Result<Vec<FixedBytes<32>>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let pools = contract.validateV4Pools(stateview, pools).call().await?;
   Ok(pools)
}

/// Query the reserves for the given v2 pools
pub async fn get_v2_reserves<P, N>(
   client: P,
   chain: u64,
   pools: Vec<Address>,
) -> Result<Vec<V2PoolReserves>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let reserves = contract.getV2Reserves(pools).call().await?;
   Ok(reserves)
}

/// Query the state of multiple V3 pools
pub async fn get_v3_state<P, N>(
   client: P,
   chain: u64,
   pools: Vec<V3Pool>,
) -> Result<Vec<V3PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let state = contract.getV3PoolState(pools).call().await?;
   Ok(state)
}

/// Query the state of multiple V4 pools
pub async fn get_v4_pool_state<P, N>(
   client: P,
   chain: u64,
   pools: Vec<V4Pool>,
) -> Result<Vec<V4PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v4(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateViewV3::new(address, client);
   let state = contract.getV4PoolState(pools, stateview).call().await?;
   Ok(state)
}

/// ERC-20 `allowance(owner, spender)` for `(token, spender)` pairs in a **single** Multicall3
/// aggregate.
///
/// Failed calls (non-token, revert) are omitted. Large pair lists must be chunked by the caller
/// so the aggregate eth_call stays under gas limits.
pub async fn get_erc20_allowances<P, N>(
   client: P,
   owner: Address,
   pairs: Vec<(Address, Address)>,
   block: Option<BlockId>,
) -> Result<Vec<(Address, Address, U256)>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if pairs.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());
   let mut builder = client.multicall().dynamic::<IERC20::allowanceCall>().block(block);
   for (token, spender) in &pairs {
      let input = Bytes::from(
         IERC20::allowanceCall {
            owner,
            spender: *spender,
         }
         .abi_encode(),
      );
      let call = CallItem::<IERC20::allowanceCall>::new(*token, input).allow_failure(true);
      builder = builder.add_call_dynamic(call);
   }

   let results = builder.aggregate3().await?;
   let mut out = Vec::with_capacity(results.len());
   for (i, result) in results.into_iter().enumerate() {
      if let Ok(amount) = result {
         let (token, spender) = pairs[i];
         out.push((token, spender, amount));
      }
   }
   Ok(out)
}

/// Permit2 `allowance(user, token, spender)` for `(token, spender)` pairs in a **single** Multicall3
/// aggregate.
///
/// Failed calls are omitted. Large pair lists must be chunked by the caller so the aggregate
/// eth_call stays under gas limits.
pub async fn get_permit2_allowances<P, N>(
   client: P,
   permit2: Address,
   owner: Address,
   pairs: Vec<(Address, Address)>,
   block: Option<BlockId>,
) -> Result<Vec<(Address, Address, U256, u64)>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if pairs.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());
   let mut builder = client.multicall().dynamic::<Permit2::allowanceCall>().block(block);

   for (token, spender) in &pairs {
      let input = Bytes::from(
         Permit2::allowanceCall {
            user: owner,
            token: *token,
            spender: *spender,
         }
         .abi_encode(),
      );

      let call = CallItem::<Permit2::allowanceCall>::new(permit2, input).allow_failure(true);
      builder = builder.add_call_dynamic(call);
   }

   let results = builder.aggregate3().await?;
   let mut out = Vec::with_capacity(results.len());

   for (i, result) in results.into_iter().enumerate() {
      if let Ok(decoded) = result {
         let (token, spender) = pairs[i];
         let expiration = u64::try_from(decoded.expiration).unwrap_or(0);

         out.push((
            token,
            spender,
            U256::from(decoded.amount),
            expiration,
         ));
      }
   }
   Ok(out)
}

/// One NFT to look up: the collection contract and a token id.
pub type NftRef = (Address, U256);

/// Batched ERC-721 lookup result, aligned with the request order.
#[derive(Clone, Debug)]
pub struct Erc721Lookup {
   /// `ownerOf(tokenId)`. `None` when the call failed (nonexistent/burned token, or not a 721).
   pub owner: Option<Address>,
   /// `tokenURI(tokenId)`. `None` when the call failed or returned an empty string.
   pub token_uri: Option<String>,
}

/// How many `ownerOf` / `balanceOf` calls go into one Multicall3 aggregate.
///
/// Sized so an aggregate stays a small fraction of a block: a few hundred sub-calls at a few thousand
/// gas each is a couple of million, comfortably inside the `eth_call` caps nodes advertise, where
/// thousands in one call is where aggregates start reverting whole.
const MULTICALL_CHUNK: usize = 50;

/// Batched ERC-721 `ownerOf(id)` in Multicall3 aggregates, owners only.
///
/// Returns `(collection, id, owner)` aligned with `refs`, where `None` in the owner slot means the
/// call reverted — a burned or never-minted id, which is the contract answering "no owner" rather
/// than a transport failure. Use this instead of [`get_erc721_owners_and_uris`] when only ownership
/// is wanted: the URI leg is a second aggregate and can carry base64 artwork.
///
/// The refs are split across aggregates of [`MULTICALL_CHUNK`]: a portfolio can hold thousands of
/// NFTs, and one `eth_call` carrying all of them can exceed the node's call gas cap and revert
/// *whole* — which reads as nobody owning anything. A failed chunk is still an error for the call
/// (an outage must never be answered as "no owner"), but a chunk that runs out of gas costs only its
/// own rows, and within a chunk a reverted sub-call keeps its own slot.
pub async fn get_erc721_owners<P, N>(
   client: P,
   refs: Vec<NftRef>,
   block: Option<BlockId>,
) -> Result<Vec<(Address, U256, Option<Address>)>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if refs.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());
   let mut out = Vec::with_capacity(refs.len());

   for chunk in refs.chunks(MULTICALL_CHUNK) {
      let mut builder = client.multicall().dynamic::<IERC721::ownerOfCall>().block(block);
      for (collection, token_id) in chunk {
         let input = Bytes::from(IERC721::ownerOfCall { tokenId: *token_id }.abi_encode());
         let call = CallItem::<IERC721::ownerOfCall>::new(*collection, input).allow_failure(true);
         builder = builder.add_call_dynamic(call);
      }
      let owners = builder.aggregate3().await?;

      if owners.len() != chunk.len() {
         anyhow::bail!(
            "multicall returned {} owners for {} refs",
            owners.len(),
            chunk.len()
         );
      }

      out.extend(
         chunk
            .iter()
            .zip(owners)
            .map(|((collection, token_id), owner)| (*collection, *token_id, owner.ok())),
      );
   }

   Ok(out)
}

/// Batched ERC-721 `ownerOf` + `tokenURI`, in **two** Multicall3 aggregates per chunk.
///
/// Two rounds — and two rounds *per chunk*, see [`MULTICALL_CHUNK`] — because the two calls decode to
/// different types and a `MulticallBuilder` decodes an entire aggregate as a single type. Results stay
/// aligned with `refs` — a failed call yields `None` in its own slot instead of being dropped, so a
/// caller can trust the index. A length mismatch is an error rather than silent truncation.
///
/// Chunked because enumeration can legitimately ask about a thousand ids of one collection
/// (`MAX_ENUMERATED_TOKENS`), and a thousand `tokenURI` calls in a single `eth_call` is exactly the
/// aggregate that reverts whole.
///
/// Needs Multicall3 (`0xcA11bde05977b3631167028862bE2a173976CA11`) deployed on the chain. Once the
/// Zeus StateView grows NFT getters (task 2.2) this is the path that gets replaced; until then it
/// is the only one.
pub async fn get_erc721_owners_and_uris<P, N>(
   client: P,
   refs: Vec<NftRef>,
   block: Option<BlockId>,
) -> Result<Vec<Erc721Lookup>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if refs.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());
   let mut lookups = Vec::with_capacity(refs.len());

   for chunk in refs.chunks(MULTICALL_CHUNK) {
      let mut owners_builder = client.multicall().dynamic::<IERC721::ownerOfCall>().block(block);
      for (collection, token_id) in chunk {
         let input = Bytes::from(IERC721::ownerOfCall { tokenId: *token_id }.abi_encode());
         let call = CallItem::<IERC721::ownerOfCall>::new(*collection, input).allow_failure(true);
         owners_builder = owners_builder.add_call_dynamic(call);
      }
      let owners = owners_builder.aggregate3().await?;

      let mut uris_builder =
         client.multicall().dynamic::<IERC721Metadata::tokenURICall>().block(block);
      for (collection, token_id) in chunk {
         let input = Bytes::from(IERC721Metadata::tokenURICall { tokenId: *token_id }.abi_encode());
         let call =
            CallItem::<IERC721Metadata::tokenURICall>::new(*collection, input).allow_failure(true);
         uris_builder = uris_builder.add_call_dynamic(call);
      }
      let uris = uris_builder.aggregate3().await?;

      if owners.len() != chunk.len() || uris.len() != chunk.len() {
         anyhow::bail!(
            "multicall returned {} owners and {} uris for {} refs",
            owners.len(),
            uris.len(),
            chunk.len()
         );
      }

      lookups.extend(
         owners.into_iter().zip(uris).map(|(owner, uri)| Erc721Lookup {
            owner: owner.ok(),
            token_uri: uri.ok().filter(|uri| !uri.trim().is_empty()),
         }),
      );
   }

   Ok(lookups)
}

/// Batched ERC-1155 `balanceOf(owner, id)` in Multicall3 aggregates.
///
/// Returns `(collection, id, balance)` for the calls that succeeded, in request order. An
/// ERC-1155 contract answers even for an id the owner holds none of, so a *failed* call means the
/// address is not ERC-1155 (or the contract rejected the call) and the entry is omitted — the same
/// convention as [`get_erc20_allowances`]. A returned `0` is a real zero balance.
///
/// Chunked by [`MULTICALL_CHUNK`] for the reason spelled out on [`get_erc721_owners`]: one aggregate
/// over a whole portfolio can exceed the call gas cap and revert, losing every balance at once. A
/// length mismatch inside a chunk is an error rather than truncated results, matching
/// [`get_erc721_owners`].
pub async fn get_erc1155_balances<P, N>(
   client: P,
   owner: Address,
   refs: Vec<NftRef>,
   block: Option<BlockId>,
) -> Result<Vec<(Address, U256, U256)>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if refs.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());
   let mut out = Vec::with_capacity(refs.len());

   for chunk in refs.chunks(MULTICALL_CHUNK) {
      let mut builder = client.multicall().dynamic::<IERC1155::balanceOfCall>().block(block);
      for (collection, id) in chunk {
         let input = Bytes::from(
            IERC1155::balanceOfCall {
               account: owner,
               id: *id,
            }
            .abi_encode(),
         );
         let call =
            CallItem::<IERC1155::balanceOfCall>::new(*collection, input).allow_failure(true);
         builder = builder.add_call_dynamic(call);
      }

      let results = builder.aggregate3().await?;

      if results.len() != chunk.len() {
         anyhow::bail!(
            "multicall returned {} balances for {} refs",
            results.len(),
            chunk.len()
         );
      }

      for (i, result) in results.into_iter().enumerate() {
         if let Ok(balance) = result {
            let (collection, id) = chunk[i];
            out.push((collection, id, balance));
         }
      }
   }

   Ok(out)
}

/// Batched ERC-721 `getApproved(id)` in one Multicall3 aggregate.
///
/// Returns `(collection, id, approved)` aligned with `refs`, where `None` is the call reverting —
/// a burned or never-minted id, the contract answering "no such token" rather than a transport
/// failure. Same convention as [`get_erc721_owners`], and for the same reason: a dropped slot would
/// silently shift the neighbours.
pub async fn get_erc721_approved<P, N>(
   client: P,
   refs: Vec<NftRef>,
   block: Option<BlockId>,
) -> Result<Vec<(Address, U256, Option<Address>)>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if refs.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());

   let mut builder = client.multicall().dynamic::<IERC721::getApprovedCall>().block(block);
   for (collection, token_id) in &refs {
      let input = Bytes::from(IERC721::getApprovedCall { tokenId: *token_id }.abi_encode());
      let call = CallItem::<IERC721::getApprovedCall>::new(*collection, input).allow_failure(true);
      builder = builder.add_call_dynamic(call);
   }
   let approved = builder.aggregate3().await?;

   if approved.len() != refs.len() {
      anyhow::bail!(
         "multicall returned {} approvals for {} refs",
         approved.len(),
         refs.len()
      );
   }

   Ok(refs
      .into_iter()
      .zip(approved)
      .map(|((collection, token_id), approved)| (collection, token_id, approved.ok()))
      .collect())
}

/// Batched `isApprovedForAll(owner, operator)` in one Multicall3 aggregate.
///
/// `targets` are `(collection, operator)` pairs — the same call answers it for ERC-721 and
/// ERC-1155 collections alike, so the standard is not part of the request. Aligned with `targets`;
/// `None` in the flag slot is a failed call (most likely not a collection that implements it).
pub async fn get_erc721_is_approved_for_all<P, N>(
   client: P,
   owner: Address,
   targets: Vec<(Address, Address)>,
   block: Option<BlockId>,
) -> Result<Vec<(Address, Address, Option<bool>)>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if targets.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());

   let mut builder = client.multicall().dynamic::<IERC721::isApprovedForAllCall>().block(block);
   for (collection, operator) in &targets {
      let input = Bytes::from(
         IERC721::isApprovedForAllCall {
            owner,
            operator: *operator,
         }
         .abi_encode(),
      );
      let call =
         CallItem::<IERC721::isApprovedForAllCall>::new(*collection, input).allow_failure(true);
      builder = builder.add_call_dynamic(call);
   }
   let approved = builder.aggregate3().await?;

   if approved.len() != targets.len() {
      anyhow::bail!(
         "multicall returned {} flags for {} targets",
         approved.len(),
         targets.len()
      );
   }

   Ok(targets
      .into_iter()
      .zip(approved)
      .map(|((collection, operator), approved)| (collection, operator, approved.ok()))
      .collect())
}

/// Batched ERC-5216 `allowance(owner, operator, id)` in one Multicall3 aggregate.
///
/// `refs` are `(collection, operator, id)` triples. Aligned with `refs`, `None` for a failed call —
/// which is the expected answer from a contract that does not implement ERC-5216 at all.
pub async fn get_erc1155_allowances<P, N>(
   client: P,
   owner: Address,
   refs: Vec<(Address, Address, U256)>,
   block: Option<BlockId>,
) -> Result<Vec<(Address, Address, U256, Option<U256>)>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if refs.is_empty() {
      return Ok(Vec::new());
   }

   let block = block.unwrap_or(BlockId::latest());

   let mut builder = client.multicall().dynamic::<IERC5216::allowanceCall>().block(block);
   for (collection, operator, id) in &refs {
      let input = Bytes::from(
         IERC5216::allowanceCall {
            account: owner,
            operator: *operator,
            id: *id,
         }
         .abi_encode(),
      );
      let call = CallItem::<IERC5216::allowanceCall>::new(*collection, input).allow_failure(true);
      builder = builder.add_call_dynamic(call);
   }
   let allowances = builder.aggregate3().await?;

   if allowances.len() != refs.len() {
      anyhow::bail!(
         "multicall returned {} allowances for {} refs",
         allowances.len(),
         refs.len()
      );
   }

   Ok(refs
      .into_iter()
      .zip(allowances)
      .map(|((collection, operator, id), allowance)| (collection, operator, id, allowance.ok()))
      .collect())
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::address;
   use alloy_provider::ProviderBuilder;
   use alloy_sol_types::SolValue;
   use std::io::{Read, Write};
   use std::net::{TcpListener, TcpStream};
   use std::sync::Arc;
   use std::sync::atomic::{AtomicUsize, Ordering};

   /// Live check of the batched NFT helpers. Ignored by default — see `crate::test_utils`.
   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn batched_nft_lookups_against_mainnet() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());
      let bayc = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");

      // Two real tokens and one nonexistent: a revert must land as `None` in its own slot rather
      // than shifting or dropping the neighbours.
      let refs = vec![
         (bayc, U256::from(1)),
         (bayc, U256::from(9999)),
         (bayc, U256::from(1_000_000_000)),
      ];
      let lookups = get_erc721_owners_and_uris(client.clone(), refs, None).await.unwrap();

      assert_eq!(
         lookups.len(),
         3,
         "results must stay aligned with the request"
      );
      assert_eq!(
         lookups[0].owner,
         Some(address!(
            "46efbaedc92067e6d60e84ed6395099723252496"
         ))
      );
      assert_eq!(
         lookups[1].owner,
         Some(address!(
            "37f11f9d0749a053dfe6243a4c1d294ea293ec12"
         ))
      );
      assert_eq!(
         lookups[2].owner, None,
         "a nonexistent token must be None"
      );
      assert!(
         lookups[0].token_uri.as_deref().is_some_and(|uri| uri.starts_with("ipfs://")),
         "tokenURI(1) should resolve to ipfs"
      );

      // ERC-1155: an owner holding none still gets an entry with a real zero, not an omission.
      let storefront = address!("495f947276749Ce646f68AC8c248420045cb7b5e");
      let vitalik = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
      let balances = get_erc1155_balances(
         client.clone(),
         vitalik,
         vec![(storefront, U256::from(1))],
         None,
      )
      .await
      .unwrap();
      assert_eq!(
         balances,
         vec![(storefront, U256::from(1), U256::ZERO)]
      );

      // ERC-721 `getApproved`: a real token and a nonexistent one, so the revert lands as `None` in
      // its own slot exactly like `ownerOf` does.
      let approved = get_erc721_approved(
         client.clone(),
         vec![(bayc, U256::from(1)), (bayc, U256::from(1_000_000_000))],
         None,
      )
      .await
      .unwrap();

      assert_eq!(approved.len(), 2);
      assert_eq!(approved[0].0, bayc);
      assert_eq!(
         approved[1].2, None,
         "a nonexistent id must be None, not shifted"
      );

      // `isApprovedForAll` on a real operator: a bool either way, and `None` for an address that is
      // not a collection at all.
      let seaport = address!("00000000000000ADc04C56Bf30aC9d3c0aAF14dC");
      let flags = get_erc721_is_approved_for_all(
         client.clone(),
         vitalik,
         vec![(bayc, seaport), (vitalik, seaport)],
         None,
      )
      .await
      .unwrap();

      assert_eq!(flags.len(), 2);
      assert!(
         flags[0].2.is_some(),
         "a real ERC-721 must answer with a flag"
      );

      // ERC-5216 `allowance`: `None` from a collection that does not implement it — the call fails,
      // and the diff reads that as "no ERC-5216 state to compare" rather than as a zero allowance.
      let allowances = get_erc1155_allowances(
         client,
         vitalik,
         vec![(storefront, seaport, U256::from(1))],
         None,
      )
      .await
      .unwrap();

      assert_eq!(allowances.len(), 1);
      assert_eq!(
         allowances[0].3, None,
         "no ERC-5216 on mainnet: the call must fail rather than answer zero"
      );
   }

   /// Requires a local anvil: `anvil --port 8545`
   #[tokio::test]
   #[ignore = "needs a local anvil on 127.0.0.1:8545"]
   async fn storage_reader_override_roundtrip() {
      let url = "http://127.0.0.1:8545";
      let client = ProviderBuilder::new().connect_http(url.parse().unwrap());
      let account = address!("0x00000000000000000000000000000000000000aa");
      let slot = U256::from(0);
      let value = U256::from(123u64);

      let _: bool = client
         .raw_request(
            "anvil_setStorageAt".into(),
            (
               account,
               slot,
               alloy_primitives::B256::from(value.to_be_bytes()),
            ),
         )
         .await
         .expect("anvil_setStorageAt");

      assert_eq!(
         storage_reader_code().len(),
         656,
         "runtime bytecode must be complete"
      );

      let read = get_account_storage(
         client,
         account,
         vec![slot, U256::from(1)],
         Some(BlockId::latest()),
      )
      .await
      .expect("override read");

      assert_eq!(read.values[0], value);
      assert_eq!(read.values[1], U256::ZERO);
   }

   /// A local node that answers every Multicall3 aggregate with the sub-call count the request
   /// carried, and counts the aggregates it was asked for.
   ///
   /// Each sub-call answers a zero address: the batched lookups only need *a* decodable answer, and what
   /// the test is about is how one call is split across aggregates and how the answers come back
   /// together. One HTTP request per aggregate, so the counter *is* the number of `eth_call`s.
   fn counting_node() -> (String, Arc<AtomicUsize>) {
      let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
      let url = format!("http://{}", listener.local_addr().unwrap());
      let requests = Arc::new(AtomicUsize::new(0));

      let counter = Arc::clone(&requests);
      std::thread::spawn(move || {
         for stream in listener.incoming().flatten() {
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || serve(stream, counter));
         }
      });

      (url, requests)
   }

   fn serve(mut stream: TcpStream, counter: Arc<AtomicUsize>) {
      while let Some(body) = read_request(&mut stream) {
         counter.fetch_add(1, Ordering::SeqCst);

         let payload = vec![(true, Bytes::from(Address::ZERO.abi_encode())); entries_in(&body)];
         let result = format!(
            "0x{}",
            alloy_primitives::hex::encode(payload.abi_encode())
         );
         let json = format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":"{}"}}"#,
            jsonrpc_id(&body),
            result
         );
         let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            json.len(),
            json
         );

         if stream.write_all(reply.as_bytes()).is_err() {
            return;
         }
      }
   }

   /// One HTTP request: the headers, then exactly `Content-Length` bytes of body.
   fn read_request(stream: &mut TcpStream) -> Option<String> {
      let mut raw = Vec::new();
      let mut buf = [0u8; 8192];

      let body_at = loop {
         if let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
         }
         let read = stream.read(&mut buf).ok()?;
         if read == 0 {
            return None;
         }
         raw.extend_from_slice(&buf[..read]);
      };

      let headers = String::from_utf8_lossy(&raw[..body_at]).to_lowercase();
      let length: usize = headers
         .split("content-length:")
         .nth(1)
         .and_then(|rest| rest.split("\r\n").next())
         .and_then(|value| value.trim().parse().ok())?;

      while raw.len() < body_at + length {
         let read = stream.read(&mut buf).ok()?;
         if read == 0 {
            break;
         }
         raw.extend_from_slice(&buf[..read]);
      }

      Some(String::from_utf8_lossy(&raw[body_at..]).into_owned())
   }

   /// How many sub-calls the aggregate in `body` carries, read out of its calldata: `aggregate3` takes a
   /// single dynamic array argument, so the array length sits at the head of that argument (`0x24`).
   fn entries_in(body: &str) -> usize {
      let calldata = ["\"input\":\"0x", "\"data\":\"0x"]
         .iter()
         .find_map(|key| body.split(key).nth(1))
         .and_then(|rest| rest.split('"').next())
         .and_then(|hex| alloy_primitives::hex::decode(hex).ok())
         .expect("the call carries calldata");

      U256::from_be_slice(&calldata[36..68]).to::<usize>()
   }

   /// The request's JSON-RPC id, echoed back so the provider matches the answer to its own call.
   fn jsonrpc_id(body: &str) -> String {
      body
         .rsplit("\"id\":")
         .next()
         .and_then(|rest| rest.split([',', '}']).next())
         .unwrap_or("1")
         .trim()
         .to_owned()
   }

   /// A call with more refs than one aggregate can carry is split, and the answers stay aligned.
   ///
   /// The failure this guards against is the one the chunking exists for: a single `eth_call` carrying a
   /// whole portfolio's worth of `ownerOf`s runs out of gas at the node and reverts **whole**, so the
   /// wallet reads "nobody owns anything". Three aggregates — the last one short — must still come back
   /// as every row in request order, with nothing dropped or shifted.
   #[tokio::test]
   async fn a_lookup_larger_than_one_aggregate_is_split_and_stays_aligned() {
      let (url, requests) = counting_node();
      let client = ProviderBuilder::new().connect_http(url.parse().unwrap());

      let collection = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
      let refs: Vec<NftRef> =
         (0..MULTICALL_CHUNK * 2 + 7).map(|id| (collection, U256::from(id))).collect();

      let owners = get_erc721_owners(client, refs.clone(), None).await.expect("owners");

      assert_eq!(
         requests.load(Ordering::SeqCst),
         3,
         "{} refs have to go out as three aggregates of {MULTICALL_CHUNK}",
         refs.len()
      );
      assert_eq!(
         owners,
         refs
            .iter()
            .map(|(collection, id)| (*collection, *id, Some(Address::ZERO)))
            .collect::<Vec<_>>(),
         "every row in request order, none dropped or shifted"
      );
   }
}
