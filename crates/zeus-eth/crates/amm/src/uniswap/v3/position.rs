use alloy_primitives::{
   Signed, U256,
   utils::{format_units, parse_units},
};

use alloy_contract::private::{Ethereum, Provider};
use alloy_rpc_types::{Log, BlockId};
use alloy_sol_types::SolEvent;

use std::sync::Arc;
use std::str::FromStr;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;

use serde::{Deserialize, Serialize};
use super::fee_math::*;
use super::{UniswapPool, pool::UniswapV3Pool};
use abi::uniswap::{nft_position::INonfungiblePositionManager, v3::{self, pool::IUniswapV3Pool}};
use currency::ERC20Token;
use types::BlockTime;
use utils::{get_logs_for, address::uniswap_nft_position_manager};


use revm_utils::{
   AccountType, DummyAccount, ForkFactory, new_evm, simulate,
   revm::state::Bytecode,
};

use anyhow::{anyhow, Context};
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
   pub fn result_str(&self) -> String {
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
pub async fn simulate_position<P>(
   client: P,
   block_time: BlockTime,
   args: PositionArgs,
) -> Result<PositionResult, anyhow::Error>
where
   P: Provider<Ethereum> + Clone + 'static + Unpin,
{
   let full_block = client.get_block(BlockId::latest()).await?.unwrap();
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
      1,
   )
   .await?;

   let volume = get_volume_from_logs(&args.pool, logs)?;

   let state = UniswapV3Pool::fetch_state(client.clone(), args.pool.clone(), Some(fork_block.clone())).await?;
   pool.set_state(state);

   // get token0 and token1 prices in USD at the fork block
   let (base_usd, quote_usd) = pool
      .tokens_price(client.clone(), Some(fork_block.clone()))
      .await?;

    // make sure we set the prices in the correct order
    let (past_token0_usd, past_token1_usd) = if pool.is_token0(pool.base_token().address) {
        (base_usd, quote_usd)
    } else {
        (quote_usd, base_usd)
    };

   let deposit = get_tokens_deposit_amount(
      price_assumption,
      args.lower_range,
      args.upper_range,
      past_token0_usd,
      past_token1_usd,
      args.deposit_amount,
   );

   let amount0 = parse_units(&deposit.amount0.to_string(), args.pool.token0().decimals)?.get_absolute();
   let amount1 = parse_units(&deposit.amount1.to_string(), args.pool.token1().decimals)?.get_absolute();

   let lower_tick = get_tick_from_price(args.lower_range);
   let upper_tick = get_tick_from_price(args.upper_range);

   // prepare the fork enviroment
   let mut fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, Some(fork_block));

   // a simple router to simulate uniswap swaps
   let bytecode = Bytecode::new_raw(abi::misc::SWAP_ROUTER_BYTECODE.parse()?);
   let swap_router = DummyAccount::new(AccountType::Contract(bytecode), U256::ZERO);

   // a dummy account that act as the swapper
   let swapper = DummyAccount::new(AccountType::EOA, U256::ZERO);

   // a dummy account that act as the lp provider
   let lp_provider = DummyAccount::new(AccountType::EOA, U256::ZERO);

   // insert the accounts into the fork factory
   fork_factory.insert_dummy_account(swap_router.clone());
   fork_factory.insert_dummy_account(swapper.clone());
   fork_factory.insert_dummy_account(lp_provider.clone());

   let amount_to_fund_0 = args.pool.token0().total_supply;
   let amount_to_fund_1 = args.pool.token1().total_supply;


   // Fund the accounts
   fork_factory.give_token(swapper.address, args.pool.token0().address, amount_to_fund_0)?;
   fork_factory.give_token(swapper.address, args.pool.token1().address, amount_to_fund_1)?;

   // we give the lp provider just as much to create the position
   fork_factory.give_token(lp_provider.address, args.pool.token0().address, amount0)?;
   fork_factory.give_token(lp_provider.address, args.pool.token1().address, amount1)?;

   let fork_db = fork_factory.new_sandbox_fork();
   let mut evm = new_evm(chain_id, Some(full_block.clone()), fork_db);

   let fee = args.pool.fee.fee_u24();
   let lower_tick: Signed<24, 1> = lower_tick
      .to_string()
      .parse()
      .context("Failed to parse tick")?;
   let upper_tick: Signed<24, 1> = upper_tick
      .to_string()
      .parse()
      .context("Failed to parse tick")?;

   let mint_params = INonfungiblePositionManager::MintParams {
      token0: args.pool.token0().address,
      token1: args.pool.token1().address,
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

   let nft_contract = uniswap_nft_position_manager(chain_id)?;

   // aprove the nft and swapper contract to spent the tokens
   let tokens = vec![args.pool.token0().clone(), args.pool.token1().clone()];
   for token in tokens {
      // approve the nft and swapper contract to spent the tokens
      simulate::approve_token(
         &mut evm,
         token.address,
         lp_provider.address,
         nft_contract,
         U256::MAX,
      )?;

      // approve the swapper contract to spent the tokens
      simulate::approve_token(
         &mut evm,
         token.address,
         swapper.address,
         swap_router.address,
         U256::MAX,
      )?;
   }

   // create the position
   let mint_res = simulate::mint_position(
      &mut evm,
      mint_params,
      lp_provider.address,
      nft_contract,
      true,
   )?;
   let token_id = mint_res.token_id;

   let mut price_ranges = Vec::new();

   // keep track of the amounts we have collected
   let mut collected0 = U256::ZERO;
   let mut collected1 = U256::ZERO;

   // keep track how many times we failed to swap
   let mut failed_swaps = 0;

   // simulate all the swaps that occured
   trace!(target: "zeus_eth::amm::uniswap::v3::position", "Simulating {} swaps", volume.swaps.len());
   for pool_swap in &volume.swaps {
      let swap_params = abi::misc::SwapRouter::Params {
         input_token: pool_swap.token_in.address,
         output_token: pool_swap.token_out.address,
         amount_in: pool_swap.amount_in,
         pool: args.pool.address,
         pool_variant: U256::from(1),
         fee,
         minimum_received: U256::ZERO,
      };

      if let Err(e) = simulate::swap(
         &mut evm,
         swap_params,
         swapper.address,
         swap_router.address,
         true,
      ) {
         failed_swaps += 1;
         trace!(target: "zeus_eth::amm::uniswap::v3::position", "Failed to swap: {:?}", e);
         continue;
      }

      // collect the fees
      let collect_params = INonfungiblePositionManager::CollectParams {
         tokenId: token_id,
         recipient: lp_provider.address,
         amount0Max: u128::MAX,
         amount1Max: u128::MAX,
      };

      let (amount0, amount1) = simulate::collect_fees(
         &mut evm,
         collect_params,
         lp_provider.address,
         nft_contract,
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

   let (amount0, amount1) = simulate::collect_fees(
      &mut evm,
      collect_params,
      lp_provider.address,
      nft_contract,
      true,
   )?;

   let earned0 = format_units(amount0, args.pool.token0().decimals)?.parse::<f64>()?;
   let earned1 = format_units(amount1, args.pool.token1().decimals)?.parse::<f64>()?;

   // get the current usd price of token0 and token1
   let state = UniswapV3Pool::fetch_state(client.clone(), args.pool.clone(), None).await?;
   pool.set_state(state);

   let (latest_token0_usd, latest_token1_usd) = pool.tokens_price(client.clone(), None).await?;

   // make sure we set the prices in the correct order
   let (latest_token0_usd, latest_token1_usd) = if pool.is_token0(pool.base_token().address) {
      (latest_token0_usd, latest_token1_usd)
   } else {
      (latest_token1_usd, latest_token0_usd)
   };

   let earned0_usd = latest_token0_usd * earned0;
   let earned1_usd = latest_token1_usd * earned1;

   // not sure what's most correct but calculate the volume based on the latest prices
   let buy_volume_usd = volume.buy_volume_usd(latest_token0_usd, pool.token0().decimals)?;
   let sell_volume_usd = volume.sell_volume_usd(latest_token1_usd, pool.token1().decimals)?;

   let total_fee0 = divide_by_fee(args.pool.fee.fee(), buy_volume_usd);
   let total_fee1 = divide_by_fee(args.pool.fee.fee(), sell_volume_usd);

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
      BlockTime::Minutes(_) => {
         // TODO
      }
   }

   let result = PositionResult {
      token0: args.pool.token0().into_owned(),
      token1: args.pool.token1().into_owned(),
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
pub async fn get_average_price<P>(
   client: P,
   chain_id: u64,
   latest_block: u64,
   block_time: BlockTime,
   step: usize,
   pool: UniswapV3Pool,
) -> Result<AvgPrice, anyhow::Error>
where
   P: Provider<Ethereum> + Clone + 'static + Unpin,
{
   let prices = Arc::new(Mutex::new(Vec::new()));
   let shared_pool = Arc::new(Mutex::new(pool.clone()));
   let semaphore = Arc::new(Semaphore::new(10));
   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

   let from_block = block_time.go_back(chain_id, latest_block)?;

   for block in (from_block..latest_block).step_by(step) {
      let client = client.clone();
      let prices = prices.clone();
      let pool_clone = pool.clone();
      let shared_pool = shared_pool.clone();
      let semaphore = semaphore.clone();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire_owned().await.unwrap();
         let block_id = BlockId::number(block);
         let state = UniswapV3Pool::fetch_state(client, pool_clone, Some(block_id)).await?;

         let mut pool = shared_pool.lock().await;
         pool.set_state(state);
         let price = pool.calculate_price(pool.token0().address)?;
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


/// Represents the volume of a pool that occured at some point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolVolume {
    pub buy_volume: U256,
    pub sell_volume: U256,
    pub swaps: Vec<SwapData>,
}

impl PoolVolume {
   /// Return the total buy volume in USD based on the token0 usd value
    pub fn buy_volume_usd(&self, usd_value: f64, decimals: u8) -> Result<f64, anyhow::Error> {
        let formatted = format_units(self.buy_volume, decimals)?.parse::<f64>()?;
        Ok(formatted * usd_value)
    }

    /// Return the total sell volume in USD based on the token1 usd value
    pub fn sell_volume_usd(&self, usd_value: f64, decimals: u8) -> Result<f64, anyhow::Error> {
        let formatted = format_units(self.sell_volume, decimals)?.parse::<f64>()?;
        Ok(formatted * usd_value)
}
}

/// A swap that took place on a DEX (Uniswap)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapData {
   pub token_in: ERC20Token,
   pub token_out: ERC20Token,
   pub amount_in: U256,
   pub amount_out: U256,
   pub block: u64,
   pub tx_hash: String,
}

impl SwapData {
   pub fn new(
      token_in: ERC20Token,
      token_out: ERC20Token,
      amount_in: U256,
      amount_out: U256,
      block: u64,
      tx_hash: String,
   ) -> Self {
      Self {
         token_in,
         token_out,
         amount_in,
         amount_out,
         block,
         tx_hash,
      }
   }

   /// Return a formatted string to print in the console
   pub fn pretty(&self) -> Result<String, anyhow::Error> {
      let s = format!(
         "Swap: {} -> {} | Amount: {} -> {} | Block: {} | Tx: {}",
         self.token_in.symbol,
         self.token_out.symbol,
         format_units(self.amount_in, self.token_in.decimals)?,
         format_units(self.amount_out, self.token_out.decimals)?,
         self.block,
         self.tx_hash,
      );
      Ok(s)
   }
}


   /// Get the volume of the pool
   fn get_volume_from_logs(pool: &UniswapV3Pool, logs: Vec<Log>) -> Result<PoolVolume, anyhow::Error> {
      let mut buy_volume = U256::ZERO;
      let mut sell_volume = U256::ZERO;
      let mut swaps = Vec::new();

      for log in &logs {
         let swap_data = decode_swap(pool, log)?;
         if swap_data.token_in.address == pool.token1().address {
            buy_volume += swap_data.amount_in;
         }

         if swap_data.token_out.address == pool.token0().address {
            sell_volume += swap_data.amount_out;
         }
         swaps.push(swap_data);
      }

      // sort swaps by the newest block to oldest
      swaps.sort_by(|a, b| a.block.cmp(&b.block));

      Ok(PoolVolume {
         buy_volume,
         sell_volume,
         swaps,
      })
   }

   /// Decode a swap log against this pool
   fn decode_swap(pool: &UniswapV3Pool, log: &Log) -> Result<SwapData, anyhow::Error> {
      let swap = v3::pool::decode_swap_log(log.data())?;

      let pair_address = log.address();
      let block = log.block_number;

      if pair_address != pool.address {
         return Err(anyhow::anyhow!("Pool Address mismatch"));
      }

      let (amount_in, token_in) = if swap.amount0.is_positive() {
         (swap.amount0, pool.token0().into_owned())
      } else {
         (swap.amount1, pool.token1().into_owned())
      };

      let (amount_out, token_out) = if swap.amount1.is_negative() {
         (swap.amount1, pool.token1().into_owned())
      } else {
         (swap.amount0, pool.token0().into_owned())
      };

      if block.is_none() {
         // this should never happen
         return Err(anyhow!("Block number is missing"));
      }

      let tx_hash = if let Some(hash) = log.transaction_hash {
         hash
      } else {
         return Err(anyhow!("Transaction hash is missing"));
      };

      let amount_in = U256::from_str(&amount_in.to_string())?;
      // remove the - sign
      let amount_out = amount_out
         .to_string()
         .trim_start_matches('-')
         .parse::<U256>()?;

      Ok(SwapData {
         token_in,
         token_out,
         amount_in,
         amount_out,
         block: block.unwrap(),
         tx_hash: tx_hash.to_string(),
      })
   }


#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;
    use currency::ERC20Token;
    use crate::DexKind;
    use alloy_provider::ProviderBuilder;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_simulate_position() {
        let url = "https://eth.merkle.io".parse().unwrap();
        let client = ProviderBuilder::new().on_http(url);

        let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let wst_eth = address!("7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0");
        let pool_address = address!("109830a1aaad605bbf02a9dfa7b0b92ec2fb7daa");

        let weth = ERC20Token::new(client.clone(), weth, 1).await.unwrap();
        let wst_eth = ERC20Token::new(client.clone(), wst_eth, 1).await.unwrap();

        let pool = UniswapV3Pool::new(1, pool_address, 100, weth, wst_eth, DexKind::UniswapV3);

        let position = PositionArgs {
            lower_range: 1.1062672693587939,
            upper_range: 1.1969094065772878,
            price_assumption: 1.167293589301331,
            deposit_amount: 500_000.0,
            pool,
        };

        let result = simulate_position(client, BlockTime::Days(1), position).await.unwrap();
        println!("{}", result.result_str());
    }
}