use super::{ExecutionResult, revert_msg};
use alloy_primitives::{Address, TxKind, U256};

use super::Evm2;
use anyhow::anyhow;
use revm::{DatabaseCommit, ExecuteCommitEvm, ExecuteEvm, database::Database};

use crate::abi::{self, uniswap::nft_position::{ encode_decrease_liquidity, INonfungiblePositionManager}};

/// Simulate a swap using [abi::misc::SwapRouter]
///
/// Returns the amount of token we received
pub fn swap<DB>(
   evm: &mut Evm2<DB>,
   params: abi::misc::SwapRouter::Params,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<U256, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::misc::encode_swap(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data.into();
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
         .result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Call Reverted: {}", err));
   }

   let amount = abi::misc::decode_swap(output)?;
   Ok(amount)
}

/// Simulate the balance of function of the ERC20 contract
pub fn erc20_balance<DB>(evm: &mut Evm2<DB>, token: Address, owner: Address) -> Result<U256, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc20::encode_balance_of(owner);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = evm
      .transact(evm.tx.clone())
      .map_err(|e| anyhow!("{:?}", e))?;
   let output = res.result.output().ok_or(anyhow!("Output not found"))?;
   let balance = abi::erc20::decode_balance_of(output)?;
   Ok(balance)
}

/// Simulate the transfer function in the ERC20 contract
pub fn transfer_token<DB>(
   evm: &mut Evm2<DB>,
   token: Address,
   from: Address,
   to: Address,
   amount: U256,
   commit: bool,
) -> Result<ExecutionResult, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::erc20::encode_transfer(to, amount);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = from;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = if commit {
      evm.transact_commit(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
         .result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Failed to transfer token: {}", err));
   }

   Ok(res)
}



/// Simulate the approve function in the ERC20 contract
pub fn approve_token<DB>(
   evm: &mut Evm2<DB>,
   token: Address,
   owner: Address,
   spender: Address,
   amount: U256,
) -> Result<ExecutionResult, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::erc20::encode_approve(spender, amount);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = owner;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = evm
      .transact_commit(evm.tx.clone())
      .map_err(|e| anyhow!("{:?}", e))?;
   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Failed to approve token: {}", err));
   }

   Ok(res)
}

/// Simulate the mint function in the [INonfungiblePositionManager] contract
pub fn mint_position<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::MintParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<(ExecutionResult, INonfungiblePositionManager::mintReturn), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_mint(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
         .result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      eprintln!("Failed to mint position: {} Gas Used: {}", err, res.gas_used());
      return Err(anyhow!("Failed to mint position: {}", err));
   }

   let mint = abi::uniswap::nft_position::decode_mint_call(output)?;
   Ok((res, mint))
}


/// Simulate the increase liquidity function in the [INonfungiblePositionManager] contract
/// 
/// ## Returns
/// 
/// - The execution result
/// - The liquidity that was minted
/// - The amount0 that was minted
/// - The amount1 that was minted
pub fn increase_liquidity<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::IncreaseLiquidityParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<(ExecutionResult, u128, U256, U256), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_increase_liquidity(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
         .result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Call Reverted: {}", err));
   }

   let (liquidity, amount0, amount1) = abi::uniswap::nft_position::decode_increase_liquidity_call(output)?;
   Ok((res, liquidity, amount0, amount1))
}


/// Simulate the decrease liquidity function in the [INonfungiblePositionManager] contract
pub fn decrease_liquidity<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::DecreaseLiquidityParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<(ExecutionResult, U256, U256), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = encode_decrease_liquidity(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
         .result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Call Reverted: {}", err));
   }

   let (amount0, amount1) = abi::uniswap::nft_position::decode_decrease_liquidity_call(output)?;
   Ok((res, amount0, amount1))
}


/// Simulate the collect function in the [INonfungiblePositionManager] contract
///
/// Returns the amount0 and amount1 that were collected
pub fn collect_fees<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::CollectParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<(ExecutionResult, U256, U256), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_collect(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone())
         .map_err(|e| anyhow!("{:?}", e))?
         .result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Failed to collect fees: {}", err));
   }

   let (amount0, amount1) = abi::uniswap::nft_position::decode_collect(output)?;
   Ok((res, amount0, amount1))
}