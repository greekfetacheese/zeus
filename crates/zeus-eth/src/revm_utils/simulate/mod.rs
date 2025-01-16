// ! Shortcuts for simulating commonly used interactions with contracts

use crate::abi::{swap_router::*, uniswap::nft_position::{*, INonfungiblePositionManager}};
use crate::defi::currency::erc20::ERC20Token;
use alloy_primitives::{Address, U256};
use revm::{Evm, primitives::TransactTo, db::{Database, DatabaseCommit}};
use super::utils::revert_msg;


/// Simulate a swap using [SwapRouter]
pub fn swap<DB>(
    evm: &mut Evm<'static, (), DB>,
    params: SwapRouter::Params,
    caller: Address,
    contract: Address,
    commit: bool,
) -> Result<U256, anyhow::Error>
where
    DB: Database + DatabaseCommit,
{
    let call_data = encode_swap(params);
    evm.tx_mut().caller = caller;
    evm.tx_mut().data = call_data.into();
    evm.tx_mut().value = U256::ZERO;
    evm.tx_mut().transact_to = TransactTo::Call(contract);

    let res = if commit {
        evm.transact_commit().ok().unwrap()
    } else {
        evm.transact().ok().unwrap().result
    };

    let output = res.output().unwrap();

    if !res.is_success() {
        let err = revert_msg(output);
        return Err(anyhow::anyhow!("Failed to swap: {}", err));
    }

    let amount = decode_swap(output)?;
    Ok(amount)
}

/// Simulate the collect function in the [INonfungiblePositionManager] contract
pub fn collect_fees<DB>(
    evm: &mut Evm<'static, (), DB>,
    params: INonfungiblePositionManager::CollectParams,
    caller: Address,
    contract: Address,
    commit: bool
) -> Result<(U256, U256), anyhow::Error>
where
    DB: Database + DatabaseCommit,
{
    let call_data = encode_collect(params);
    evm.tx_mut().caller = caller;
    evm.tx_mut().data = call_data.into();
    evm.tx_mut().value = U256::ZERO;
    evm.tx_mut().transact_to = TransactTo::Call(contract);

    let res = if commit {
        evm.transact_commit().ok().unwrap()
    } else {
        evm.transact().ok().unwrap().result
    };

    let output = res.output().unwrap();

    if !res.is_success() {
        let err = revert_msg(&output);
        return Err(anyhow::anyhow!("Failed to collect: {}", err));
    }

    let (amount0, amount1) = decode_collect(output)?;
    Ok((amount0, amount1))
}

/// Simulate the mint function in the [INonfungiblePositionManager] contract
pub fn mint_position<DB>(
    evm: &mut Evm<'static, (), DB>,
    params: INonfungiblePositionManager::MintParams,
    caller: Address,
    contract: Address,
    commit: bool
) -> Result<(U256, u128, U256, U256), anyhow::Error>
where
    DB: Database + DatabaseCommit,
{
    let call_data = encode_mint(params);
    evm.tx_mut().caller = caller;
    evm.tx_mut().data = call_data.into();
    evm.tx_mut().value = U256::ZERO;
    evm.tx_mut().transact_to = TransactTo::Call(contract);

    let res = if commit {
        evm.transact_commit().ok().unwrap()
    } else {
        evm.transact().ok().unwrap().result
    };

    let output = res.output().unwrap();

    if !res.is_success() {
        let err = revert_msg(&output);
        return Err(anyhow::anyhow!("Failed to collect: {}", err));
    }

    let (token_id, liquidity, amount0, amount1) = decode_mint(output)?;
    Ok((token_id, liquidity, amount0, amount1))
}


pub fn erc20_balance<DB>(
    evm: &mut Evm<'static, (), DB>,
    token: ERC20Token,
    owner: Address,
) -> Result<U256, anyhow::Error>
where
    DB: Database,
{
    let call_data = token.encode_balance_of(owner);
    evm.tx_mut().data = call_data.into();
    evm.tx_mut().value = U256::ZERO;
    evm.tx_mut().transact_to = TransactTo::Call(token.address);

    let res = evm.transact().ok().unwrap();
    let output = res.result.output().unwrap();

    let balance = token.decode_balance_of(output)?;

    Ok(balance)
}

/// Simulate the approve function in the [ERC20Token] contract
pub fn approve_token<DB>(
    evm: &mut Evm<'static, (), DB>,
    token: ERC20Token,
    owner: Address,
    spender: Address,
    amount: U256,
) -> Result<(), anyhow::Error>
where
    DB: Database + DatabaseCommit,
{
    let call_data = token.encode_approve(spender, amount);
    evm.tx_mut().caller = owner;
    evm.tx_mut().data = call_data.into();
    evm.tx_mut().value = U256::ZERO;
    evm.tx_mut().transact_to = TransactTo::Call(token.address);

    let res = evm.transact_commit().ok().unwrap();
    let output = res.output().unwrap();

    if !res.is_success() {
        let err = revert_msg(&output);
        return Err(anyhow::anyhow!("Failed to approve token: {}", err));
    }

    Ok(())
}


pub fn can_tranfer_erc20<DB>(
    evm: &mut Evm<'static, (), DB>,
    token: ERC20Token,
    from: Address,
    to: Address,
    amount: U256,
) -> Result<(bool, String), anyhow::Error>
where
    DB: Database,
{
    let call_data = token.encode_transfer(to, amount);
    evm.tx_mut().caller = from;
    evm.tx_mut().data = call_data.into();
    evm.tx_mut().value = U256::ZERO;
    evm.tx_mut().transact_to = TransactTo::Call(token.address);

    let res = evm.transact().ok().unwrap().result;
    let output = res.output().unwrap();

    if !res.is_success() {
        let reason = revert_msg(&output);
        return Ok((false, reason));
    }

    Ok((true, "".to_string()))
}