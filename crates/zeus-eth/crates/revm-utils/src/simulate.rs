use crate::revert_msg;
use alloy_primitives::{Address, Bytes, TxKind, U256};

use super::Evm2;
use anyhow::anyhow;
use revm::{DatabaseCommit, ExecuteCommitEvm, ExecuteEvm, context::result::ExecutionResult, database::Database};

use abi::uniswap::nft_position::{INonfungiblePositionManager, MintReturn};

/// Simulate the depositV3 of the SpokePool contract
pub fn across_deposit_v3<DB>(
   evm: &mut Evm2<DB>,
   call_data: Bytes,
   value: U256,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<ExecutionResult, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   evm.tx.caller = caller;
   evm.tx.data = call_data;
   evm.tx.value = value;
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
      let err = revert_msg(&output);
      return Err(anyhow!("Failed to deposit: {}", err));
   }

   Ok(res)
}

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
      let err = revert_msg(&output);
      return Err(anyhow!("Failed to swap: {}", err));
   }

   let amount = abi::misc::decode_swap(&output)?;
   Ok(amount)
}

/// Simulate the balance of function of the ERC20 contract
pub fn erc20_balance<DB>(evm: &mut Evm2<DB>, token: Address, owner: Address) -> Result<U256, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc20::encode_balance_of(owner);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = evm
      .transact(evm.tx.clone())
      .map_err(|e| anyhow!("{:?}", e))?;
   let output = res.result.output().ok_or(anyhow!("Output not found"))?;
   let balance = abi::erc20::decode_balance_of(&output)?;
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
) -> Result<(), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::erc20::encode_transfer(to, amount);
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

   println!("Execution result: {:?}", res);

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(&output);
      return Err(anyhow!("Failed to transfer token: {}", err));
   }

   Ok(())
}

/// Simulate the approve function in the ERC20 contract
pub fn approve_token<DB>(
   evm: &mut Evm2<DB>,
   token: Address,
   owner: Address,
   spender: Address,
   amount: U256,
) -> Result<(), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::erc20::encode_approve(spender, amount);
   evm.tx.caller = owner;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = evm
      .transact_commit(evm.tx.clone())
      .map_err(|e| anyhow!("{:?}", e))?;
   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(&output);
      return Err(anyhow!("Failed to approve token: {}", err));
   }

   Ok(())
}

/// Simulate the mint function in the [INonfungiblePositionManager] contract
pub fn mint_position<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::MintParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<MintReturn, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_mint(params);
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
      let err = revert_msg(&output);
      return Err(anyhow!("Failed to mint position: {}", err));
   }

   let mint = abi::uniswap::nft_position::decode_mint(&output)?;
   Ok(mint)
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
) -> Result<(U256, U256), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_collect(params);
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
      let err = revert_msg(&output);
      return Err(anyhow!("Failed to collect fees: {}", err));
   }

   let (amount0, amount1) = abi::uniswap::nft_position::decode_collect(&output)?;
   Ok((amount0, amount1))
}
