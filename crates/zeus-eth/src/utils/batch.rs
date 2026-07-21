use alloy_contract::private::{Network, Provider};
use alloy_network::TransactionBuilder;
use alloy_primitives::{Address, Bytes, FixedBytes, U256, hex};
use alloy_rpc_types::{BlockId, state::StateOverridesBuilder};
use alloy_sol_types::{SolCall, sol};
use std::sync::LazyLock;

use super::address_book::zeus_stateview_v2;
use crate::{
   abi::zeus::ZeusStateViewV2::{self, *},
   utils::address_book,
};

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
   let hex_str = STORAGE_READER_BYTECODE
      .strip_prefix("0x")
      .unwrap_or(STORAGE_READER_BYTECODE);
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

   let values = StorageReader::readSlotsUintCall::abi_decode_returns(&raw)
      .map_err(|e| anyhow::anyhow!("failed decoding StorageReader return for {address}: {e:?} (ret={raw})"))?;

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
   let block = block.unwrap_or(BlockId::latest());
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let balance = contract
      .getETHBalance(addresses)
      .call()
      .block(block)
      .await?;
   Ok(balance)
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
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let balance = contract
      .getERC20Balance(tokens, owner)
      .call()
      .block(block)
      .await?;
   Ok(balance)
}

/// Query the ERC20 token info for the given token
pub async fn get_erc20_info<P, N>(client: P, chain: u64, token: Address) -> Result<ERC20Info, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
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
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
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
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
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
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let pools_state = contract
      .getPoolsState(v2_pools, v3_pools, v4_pools, state_view)
      .call()
      .await?;
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
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let pools = contract
      .getV3Pools(factory, token_a, token_b)
      .call()
      .await?;
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
   let address = zeus_stateview_v2(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
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
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let reserves = contract.getV2Reserves(pools).call().await?;
   Ok(reserves)
}

/// Query the state of multiple V3 pools
pub async fn get_v3_state<P, N>(client: P, chain: u64, pools: Vec<V3Pool>) -> Result<Vec<V3PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
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
   let address = zeus_stateview_v2(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let state = contract.getV4PoolState(pools, stateview).call().await?;
   Ok(state)
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::address;
   use alloy_provider::ProviderBuilder;

   /// Requires a local anvil: `anvil --port 8545`
   #[tokio::test]
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
}
