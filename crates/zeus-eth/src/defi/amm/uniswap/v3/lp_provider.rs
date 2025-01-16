use alloy_primitives::{
    utils::{format_units, parse_units},
    Signed, Uint, U256,
};

use alloy_rpc_types::BlockId;
use alloy_sol_types::SolEvent;

use alloy_network::Ethereum;
use alloy_provider::Provider;
use alloy_transport::Transport;

use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;

use crate::{
    defi::currency::erc20::ERC20Token,
    revm_utils::{dummy_account::*, fork_db::fork_factory::ForkFactory, simulate::*, utils::*},
};
use revm::db::{CacheDB, EmptyDB};

use super::{fee_math::*, UniswapV3Pool};
use crate::{
    abi::{
        swap_router::*,
        uniswap::{nft_position::*, pool::v3::*},
    },
    utils::{logs::query::get_logs_for, BlockTime},
};

use anyhow::Context;
use tracing::trace;

#[derive(Debug, Clone)]
pub struct PositionArgs {
    /// Lower price range (token0 in terms of token1)
    pub lower_range: f64,

    /// Upper price range (token0 in terms of token1)
    pub upper_range: f64,

    /// Where the price you believe will move the most (token0 in terms of token1)
    pub price_assumption: f64,

    /// The total deposit amount in USD value
    pub deposit_amount: f64,

    /// The Uniswap V3 pool
    pub pool: UniswapV3Pool,
}

impl PositionArgs {
    pub fn new(
        lower_range: f64,
        upper_range: f64,
        price_assumption: f64,
        deposit_amount: f64,
        pool: UniswapV3Pool,
    ) -> Self {
        Self {
            lower_range,
            upper_range,
            price_assumption,
            deposit_amount,
            pool,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PositionResult {
    pub token0: ERC20Token,
    pub token1: ERC20Token,
    pub deposit: DepositAmounts,

    /// Token0 USD Price at fork block
    pub past_token0_usd: f64,

    /// Token1 USD Price at fork block
    pub past_token1_usd: f64,

    /// Latest Token0 USD Price
    pub token0_usd: f64,

    /// Latest Token1 USD Price
    pub token1_usd: f64,

    /// Amount of Token0 earned
    pub earned0: f64,

    /// Amount of Token1 earned
    pub earned1: f64,

    /// Amount of Token0 earned in USD
    pub earned0_usd: f64,

    /// Amount of Token1 earned in USD
    pub earned1_usd: f64,

    /// The total buy volume in USD that occured in the pool
    pub buy_volume_usd: f64,

    /// The total sell volume in USD that occured in the pool
    pub sell_volume_usd: f64,

    /// The total fees that the pool has collected in token0
    pub total_fee0: f64,

    /// The total fees that the pool has collected in token1
    pub total_fee1: f64,

    /// The total number of failed swaps (for debugging purposes)
    pub failed_swaps: u64,

    /// The total number of times that our position was out of the range
    pub out_of_range: usize,

    /// The total number of times that our position was in the range
    pub in_range: usize,

    pub apr: f64,
}

impl PositionResult {
    /// Create a pretty string representation of the result
    pub fn pretty(&self) -> String {
        format!(
            "\nPast Price of {}: ${:.2}
             Past Price of {}: ${:.2}
             Latest Price of {}: ${:.2}
             Latest Price of {}: ${:.2}
             Earned0: {:.2} {} (${:.2})
             Earned1: {:.2} {} (${:.2})
             Total Earned: ${:.2}
             APR: {:.2}%
             Buy Volume USD: {:.2}
             Sell Volume USD: {:.2}
             Total Fee0: {:.2}
             Total Fee1: {:.2}
             Failed Swaps: {}
             Out of Range: {}
             In Range: {}",
            self.token0.symbol,
            self.past_token0_usd,
            self.token1.symbol,
            self.past_token1_usd,
            self.token0.symbol,
            self.token0_usd,
            self.token1.symbol,
            self.token1_usd,
            self.earned0,
            self.token0.symbol,
            self.earned0_usd,
            self.earned1,
            self.token1.symbol,
            self.earned1_usd,
            self.earned0_usd + self.earned1_usd,
            self.apr,
            self.buy_volume_usd,
            self.sell_volume_usd,
            self.total_fee0,
            self.total_fee1,
            self.failed_swaps,
            self.out_of_range,
            self.in_range
        )
    }
}

/// Keep track in which block the price is in the range or not
#[derive(Debug, Clone)]
pub struct PriceRange {
    pub is_in_range: bool,
    pub block: u64,
}

impl PriceRange {
    pub fn new(is_in_range: bool, block: u64) -> Self {
        Self { is_in_range, block }
    }
}

/// Simulate a position on a Uniswap V3 pool
///
/// It works by quering and forking the historically required chain state and simulate all the swaps that occured in the past
/// Because of that it may be slow and not suitable for some usecases
///
/// ## Arguments
///
/// * `client` - The provided client
/// * `block_time` - Simulate the position based on the past time (x days or x hours ago)
/// * `args` - See [PositionArgs]
pub async fn simulate_position<T, P>(
    client: P,
    block_time: BlockTime,
    args: PositionArgs,
) -> Result<PositionResult, anyhow::Error>
where
    T: Transport + Clone + Unpin,
    P: Provider<T, Ethereum> + Clone + 'static + Unpin,
{
    let full_block = client
        .get_block(BlockId::latest(), false.into())
        .await?
        .unwrap();
    let chain_id = client.get_chain_id().await?;

    let latest_block = full_block.clone().header.number.clone();
    let fork_block = block_time.go_back(chain_id, latest_block)?;
    let fork_block = BlockId::number(fork_block);

    let mut pool = args.pool.clone();

    let price_assumption = args.price_assumption;

    let events = vec![IUniswapV3Pool::Swap::SIGNATURE];
    let logs = get_logs_for(
        client.clone(),
        chain_id,
        vec![args.pool.address],
        events,
        block_time.clone(),
    )
    .await?;

    let volume = args.pool.get_volume_from_logs(logs)?;

    let state =
        UniswapV3Pool::fetch_state(args.pool.address, client.clone(), Some(fork_block.clone()))
            .await?;
    pool.update_state(state);

    // get token0 and token1 prices in USD at the fork block
    let (past_token0_usd, past_token1_usd) = pool
        .tokens_usd(client.clone(), Some(fork_block.clone()))
        .await?;

    let deposit = get_tokens_deposit_amount(
        price_assumption,
        args.lower_range,
        args.upper_range,
        past_token0_usd,
        past_token1_usd,
        args.deposit_amount,
    );

    let amount0 =
        parse_units(&deposit.amount0.to_string(), args.pool.token0.decimals)?.get_absolute();
    let amount1 =
        parse_units(&deposit.amount1.to_string(), args.pool.token1.decimals)?.get_absolute();

    let lower_tick = get_tick_from_price(args.lower_range);
    let upper_tick = get_tick_from_price(args.upper_range);

    // prepare the fork enviroment
    let db = CacheDB::new(EmptyDB::default());
    let mut fork_factory = ForkFactory::new_sandbox_factory(client.clone(), db, Some(fork_block));

    // a simple router to simulate uniswap swaps
    let swap_router = DummyAccount::new(AccountType::Contract(swap_router_bytecode()?), U256::ZERO);

    // a dummy account that act as the swapper
    let swapper = DummyAccount::new(AccountType::EOA, U256::ZERO);

    // a dummy account that act as the lp provider
    let lp_provider = DummyAccount::new(AccountType::EOA, U256::ZERO);

    let amount_to_fund_0 = args.pool.token0.total_supply;
    let amount_to_fund_1 = args.pool.token1.total_supply;

    swap_router.insert(&mut fork_factory, args.pool.token0.clone(), U256::from(1))?;
    swapper.insert(
        &mut fork_factory,
        args.pool.token0.clone(),
        amount_to_fund_0,
    )?;
    swapper.insert(
        &mut fork_factory,
        args.pool.token1.clone(),
        amount_to_fund_1,
    )?;

    // we give the lp provider just as much to create the position
    lp_provider.insert(&mut fork_factory, args.pool.token0.clone(), amount0)?;
    lp_provider.insert(&mut fork_factory, args.pool.token1.clone(), amount1)?;

    let fork_db = fork_factory.new_sandbox_fork();
    let mut evm = new_evm(fork_db, Some(full_block.clone()));

    let fee: Uint<24, 1> = args
        .pool
        .fee
        .to_string()
        .parse()
        .context("Failed to parse fee")?;
    let lower_tick: Signed<24, 1> = lower_tick
        .to_string()
        .parse()
        .context("Failed to parse tick")?;
    let upper_tick: Signed<24, 1> = upper_tick
        .to_string()
        .parse()
        .context("Failed to parse tick")?;

    let mint_params = INonfungiblePositionManager::MintParams {
        token0: args.pool.token0.address,
        token1: args.pool.token1.address,
        fee,
        tickLower: lower_tick,
        tickUpper: upper_tick,
        amount0Desired: amount0,
        amount1Desired: amount1,
        amount0Min: U256::ZERO,
        amount1Min: U256::ZERO,
        recipient: lp_provider.address,
        deadline: U256::from(full_block.header.timestamp),
    };

    // aprove the nft and swapper contract to spent the tokens
    let tokens = vec![args.pool.token0.clone(), args.pool.token1.clone()];
    for token in tokens {
        approve_token(
            &mut evm,
            token.clone(),
            lp_provider.address,
            NFT_POSITION_CONTRACT,
            U256::MAX,
        )?;
        approve_token(
            &mut evm,
            token.clone(),
            swapper.address,
            swap_router.address,
            U256::MAX,
        )?;
    }

    // create the position
    let mint_res = mint_position(
        &mut evm,
        mint_params,
        lp_provider.address,
        NFT_POSITION_CONTRACT,
        true,
    )?;
    let token_id = mint_res.0;

    let mut price_ranges = Vec::new();

    // keep track of the amounts we have collected
    let mut collected0 = U256::ZERO;
    let mut collected1 = U256::ZERO;

    // keep track how many times we failed to swap
    let mut failed_swaps = 0;

    // simulate all the swaps that occured
    trace!("Simulating {} swaps", volume.swaps.len());
    for pool_swap in &volume.swaps {
        let swap_params = SwapRouter::Params {
            input_token: pool_swap.token_in.address,
            output_token: pool_swap.token_out.address,
            amount_in: pool_swap.amount_in,
            pool: args.pool.address,
            pool_variant: U256::from(1),
            fee,
            minimum_received: U256::ZERO,
        };

        if let Err(e) = swap(
            &mut evm,
            swap_params,
            swapper.address,
            swap_router.address,
            true,
        ) {
            failed_swaps += 1;
            trace!("Failed to swap: {:?}", e);
            continue;
        }

        // collect the fees
        let collect_params = INonfungiblePositionManager::CollectParams {
            tokenId: token_id,
            recipient: lp_provider.address,
            amount0Max: u128::MAX,
            amount1Max: u128::MAX,
        };

        let (amount0, amount1) = collect_fees(
            &mut evm,
            collect_params,
            lp_provider.address,
            NFT_POSITION_CONTRACT,
            false,
        )?;

        // compare the amount0 and amount1 with the collected amounts
        let is_in_range = if amount0 > collected0 || amount1 > collected1 {
            collected0 = amount0;
            collected1 = amount1;
            true
        } else {
            false
        };

        // TODO: store big swaps in a separate struct

        price_ranges.push(PriceRange::new(is_in_range, pool_swap.block));
    }

    // Collect all the fees earned
    let collect_params = INonfungiblePositionManager::CollectParams {
        tokenId: token_id,
        recipient: swapper.address,
        amount0Max: u128::MAX,
        amount1Max: u128::MAX,
    };

    let (amount0, amount1) = collect_fees(
        &mut evm,
        collect_params,
        lp_provider.address,
        NFT_POSITION_CONTRACT,
        true,
    )?;

    let earned0 = format_units(amount0, args.pool.token0.decimals)?.parse::<f64>()?;
    let earned1 = format_units(amount1, args.pool.token1.decimals)?.parse::<f64>()?;

    // get the current usd price of token0 and token1
    let state = UniswapV3Pool::fetch_state(args.pool.address, client.clone(), None).await?;
    pool.update_state(state);

    let (latest_token0_usd, latest_token1_usd) = pool.tokens_usd(client.clone(), None).await?;

    let earned0_usd = latest_token0_usd * earned0;
    let earned1_usd = latest_token1_usd * earned1;

    // not sure what's most correct but calculate the volume based on the latest prices
    let buy_volume_usd = volume.buy_volume_usd(latest_token0_usd, pool.token0.decimals)?;
    let sell_volume_usd = volume.sell_volume_usd(latest_token1_usd, pool.token1.decimals)?;

    let total_fee0 = divide_by_fee(args.pool.fee, buy_volume_usd);
    let total_fee1 = divide_by_fee(args.pool.fee, sell_volume_usd);

    // calculate how many times we were out of the range
    let out_of_range = price_ranges.iter().filter(|r| !r.is_in_range).count();
    let in_range = price_ranges.iter().filter(|r| r.is_in_range).count();

    // calculate the APR of the position
    let total_earned = earned0_usd + earned1_usd;
    let mut apr = 0.0;

    match block_time {
        BlockTime::Days(days) => {
            apr = (total_earned / args.deposit_amount) * (365.0 / days as f64) * 100.0;
        }
        BlockTime::Hours(hours) => {
            apr = (total_earned / args.deposit_amount) * (8760.0 / hours as f64) * 100.0;
        }
        BlockTime::Block(_) => {
            // TODO
        }
    }

    let result = PositionResult {
        token0: args.pool.token0.clone(),
        token1: args.pool.token1.clone(),
        deposit: deposit.clone(),
        past_token0_usd,
        past_token1_usd,
        token0_usd: latest_token0_usd,
        token1_usd: latest_token1_usd,
        earned0,
        earned1,
        earned0_usd,
        earned1_usd,
        buy_volume_usd,
        sell_volume_usd,
        total_fee0,
        total_fee1,
        failed_swaps,
        out_of_range,
        in_range,
        apr,
    };

    Ok(result)
}

pub fn divide_by_fee(fee: u32, amount: f64) -> f64 {
    let fee_percent = match fee {
        fee if fee == 100 => 0.01 / 100.0,
        fee if fee == 500 => 0.05 / 100.0,
        fee if fee == 3000 => 0.3 / 100.0,
        fee if fee == 10000 => 1.0 / 100.0,
        _ => panic!("Invalid fee tier"),
    };

    amount * fee_percent
}

pub struct AvgPrice {
    pub min: f64,
    pub median: f64,
    pub max: f64,
}

impl AvgPrice {
    pub fn new(prices: Vec<f64>) -> Self {
        let min = prices
            .iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let median = prices.iter().sum::<f64>() / prices.len() as f64;
        let max = prices
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();

        Self {
            min: *min,
            median,
            max: *max,
        }
    }
}

/// Get the average price of a Uniswap V3 pool (token0 in terms of token1)
#[allow(dead_code)]
pub async fn get_average_price<T, P>(
    client: P,
    chain_id: u64,
    latest_block: u64,
    block_time: BlockTime,
    step: usize,
    pool: UniswapV3Pool,
) -> Result<AvgPrice, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + Clone + 'static,
{
    let pool_address = pool.address.clone();
    let prices = Arc::new(Mutex::new(Vec::new()));
    let pool = Arc::new(Mutex::new(pool));
    let semaphore = Arc::new(Semaphore::new(10));
    let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

    let from_block = block_time.go_back(chain_id, latest_block)?;

    for block in (from_block..latest_block).step_by(step) {
        let client = client.clone();
        let prices = prices.clone();
        let pool = pool.clone();
        let semaphore = semaphore.clone();

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire_owned().await.unwrap();
            let block_id = BlockId::number(block);
            let state = UniswapV3Pool::fetch_state(pool_address, client, Some(block_id)).await?;

            let mut pool = pool.lock().await;
            pool.update_state(state);
            let price = pool.calculate_price(pool.token0.address)?;
            prices.lock().await.push(price);
            Ok(())
        });
        tasks.push(task);
    }

    for task in tasks {
        match task.await {
            Ok(_) => (),
            Err(e) => {
                trace!("Error while getting average price: {:?}", e);
            }
        }
    }

    let prices = prices.lock().await;

    let average_price = AvgPrice::new(prices.clone());

    Ok(average_price)
}
