use abi::uniswap::v3::pool::{decode_burn_log, decode_collect_log, decode_mint_log, decode_swap_log};
use alloy_primitives::{Address, Signed, U256};

use alloy_contract::private::{Ethereum, Provider};
use alloy_rpc_types::{BlockId, Log};
use alloy_sol_types::SolEvent;

use std::collections::HashMap;

use crate::uniswap::v3::{get_tick_from_price, calculate_liquidity_amounts, calculate_liquidity_needed};


use super::{UniswapPool, pool::UniswapV3Pool};
use abi::uniswap::{
   nft_position::INonfungiblePositionManager,
   v3::pool::IUniswapV3Pool,
};
use currency::ERC20Token;
use types::BlockTime;
use utils::{NumericValue, address_book::uniswap_nft_position_manager, get_logs_for};

use revm_utils::{AccountType, DummyAccount, ForkFactory, new_evm, revm::state::Bytecode, simulate};

use anyhow::Context;
use tracing::trace;

#[derive(Debug, Clone, Default)]
pub struct SimPositionConfig {
   /// Lower price range (token0 in terms of token1)
   pub lower_range: f64,

   /// Upper price range (token0 in terms of token1)
   pub upper_range: f64,

   /// The total deposit amount in Token0
   pub deposit_amount: NumericValue,

   pub skip_simulating_mints: bool,
   pub skip_simulating_burns: bool,
}

#[derive(Debug, Clone)]
pub struct PositionResult {
   pub forked_block: u64,
   pub token0: ERC20Token,
   pub token1: ERC20Token,
   pub lower_tick: i32,
   pub upper_tick: i32,

   /// Amounts that are actually in the position
   pub amount0: NumericValue,
   pub amount1: NumericValue,

   /// Token0 USD Price at fork block
   pub past_token0_usd: f64,

   /// Token1 USD Price at fork block
   pub past_token1_usd: f64,

   /// Latest Token0 USD Price
   pub token0_usd: f64,

   /// Latest Token1 USD Price
   pub token1_usd: f64,

   /// Amount of Token0 earned
   pub token0_earned: NumericValue,

   /// Amount of Token1 earned
   pub token1_earned: NumericValue,

   /// Amount of Token0 earned in USD
   pub earned0_usd: NumericValue,

   /// Amount of Token1 earned in USD
   pub earned1_usd: NumericValue,

   pub buy_volume: NumericValue,

   pub sell_volume: NumericValue,

   pub total_volume_usd: NumericValue,

   /// Total Swaps that have occured
   pub total_swaps: usize,
   /// Total Mint that have occured
   pub total_mints: usize,
   /// Total Burn that have occured
   pub total_burns: usize,

   /// The times the position was active
   pub active_swaps: usize,

   pub failed_swaps: usize,
   pub failed_burns: usize,
   pub failed_mints: usize,

   /// APR of the position
   pub apr: f64,
}



pub async fn simulate_position<P>(
   client: P,
   block_time: BlockTime,
   config: SimPositionConfig,
   mut pool: UniswapV3Pool,
) -> Result<PositionResult, anyhow::Error>
where
   P: Provider<Ethereum> + Clone + 'static + Unpin,
{
   let token0 = pool.token0().into_owned();
   let token1 = pool.token1().into_owned();

   let total_supply_0_fut = token0.get_total_supply(client.clone());
   let total_supply_1_fut = token1.get_total_supply(client.clone());

   let full_block = client.get_block(BlockId::latest()).await?.unwrap();
   let chain_id = client.get_chain_id().await?;

   let latest_block = full_block.clone().header.number.clone();
   let fork_block_num = block_time.go_back(chain_id, latest_block)?;
   let fork_block = BlockId::number(fork_block_num);

   let swap_event = IUniswapV3Pool::Swap::SIGNATURE;
   let mint_event = IUniswapV3Pool::Mint::SIGNATURE;
   let collect_event = IUniswapV3Pool::Collect::SIGNATURE;
   let burn_event = IUniswapV3Pool::Burn::SIGNATURE;
   let events = vec![swap_event, mint_event, collect_event, burn_event];

   let logs = get_logs_for(
      client.clone(),
      chain_id,
      vec![pool.address],
      events,
      fork_block_num,
      1,
   )
   .await?;

   let sequenced_events = decode_events(&logs);

   pool
      .update_state(client.clone(), Some(fork_block.clone()))
      .await?;

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

   let lower_range = config.lower_range;
   let upper_range = config.upper_range;
   let deposit_amount = config.deposit_amount;

   let pool_state = pool.state().v3_state().unwrap();
   let sqrt_price = pool_state.sqrt_price;

   let lower_tick = get_tick_from_price(lower_range);
   let upper_tick = get_tick_from_price(upper_range);

   let sqrt_price_lower = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(lower_tick)?;
   let sqrt_price_upper = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(upper_tick)?;

   let liquidity = calculate_liquidity_needed(
      sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      deposit_amount.wei2(),
      true,
   )?;

   let (final_amount0, final_amount1) =
      calculate_liquidity_amounts(sqrt_price, sqrt_price_lower, sqrt_price_upper, liquidity)?;

   let amount0 = NumericValue::format_wei(final_amount0, pool.token0().decimals);
   let amount1 = NumericValue::format_wei(final_amount1, pool.token1().decimals);

   // prepare the fork enviroment
   let mut fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, Some(fork_block));

   // a simple router to simulate uniswap swaps
   let bytecode = Bytecode::new_raw(abi::misc::SWAP_ROUTER_BYTECODE.parse()?);
   let swap_router = DummyAccount::new(AccountType::Contract(bytecode), U256::ZERO);

   // a dummy account that act as the swapper
   let swapper = DummyAccount::new(AccountType::EOA, U256::ZERO);

   // a dummy account that act as the lp provider
   let lp_provider = DummyAccount::new(AccountType::EOA, U256::ZERO);

   // a dummy account that will mint and burn positions
   let minter_and_burner = DummyAccount::new(AccountType::EOA, U256::ZERO);

   // insert the accounts into the fork factory
   fork_factory.insert_dummy_account(swap_router.clone());
   fork_factory.insert_dummy_account(swapper.clone());
   fork_factory.insert_dummy_account(lp_provider.clone());
   fork_factory.insert_dummy_account(minter_and_burner.clone());

   let total_supply_0 = total_supply_0_fut.await?;
   let total_supply_1 = total_supply_1_fut.await?;

   let amount_to_fund_0 = total_supply_0;
   let amount_to_fund_1 = total_supply_1;

   // Fund the accounts
   fork_factory.give_token(swapper.address, pool.token0().address, amount_to_fund_0)?;
   fork_factory.give_token(swapper.address, pool.token1().address, amount_to_fund_1)?;
   fork_factory.give_token(
      minter_and_burner.address,
      pool.token0().address,
      amount_to_fund_0,
   )?;
   fork_factory.give_token(
      minter_and_burner.address,
      pool.token1().address,
      amount_to_fund_1,
   )?;

   // we give the lp provider just as much to create the position
   fork_factory.give_token(lp_provider.address, pool.token0().address, amount0.wei2())?;
   fork_factory.give_token(lp_provider.address, pool.token1().address, amount1.wei2())?;

   let fork_db = fork_factory.new_sandbox_fork();

   let fee = pool.fee.fee_u24();
   let lower_tick_i24: Signed<24, 1> = lower_tick
      .to_string()
      .parse()
      .context("Failed to parse tick")?;
   let upper_tick_i24: Signed<24, 1> = upper_tick
      .to_string()
      .parse()
      .context("Failed to parse tick")?;

   let mint_params = INonfungiblePositionManager::MintParams {
      token0: pool.token0().address,
      token1: pool.token1().address,
      fee,
      tickLower: lower_tick_i24,
      tickUpper: upper_tick_i24,
      amount0Desired: amount0.wei2(),
      amount1Desired: amount1.wei2(),
      amount0Min: U256::ZERO,
      amount1Min: U256::ZERO,
      recipient: lp_provider.address,
      deadline: U256::from(full_block.header.timestamp),
   };

   let nft_contract = uniswap_nft_position_manager(chain_id)?;

   // keep track of the times we were in the range
   let mut active_swaps = 0;

   // keep track of the amounts we have collected
   let mut total_collected0 = U256::ZERO;
   let mut total_collected1 = U256::ZERO;

   // Amounts that are actually in the position
   let amount0;
   let amount1;

   // Buy volume is when the quote token of the pool goes out and the base goes in
   // Amount of base token of the pool
   let mut buy_volume = U256::ZERO;

   // Sell volume is when the quote token of the pool goes in and the base goes out
   // Amount of quote token of the pool
   let mut sell_volume = U256::ZERO;

   let mut failed_swaps = 0;
   let mut failed_mints = 0;
   let mut failed_burns = 0;

   // Positions opened by the mint and fee collector
   let mut positions = Positions::new();

   let total_swaps = sequenced_events
      .iter()
      .filter(|e| matches!(e.event, PoolEvent::Swap(_)))
      .count();
   let total_mints = sequenced_events
      .iter()
      .filter(|e| matches!(e.event, PoolEvent::Mint(_)))
      .count();
   let total_burns = sequenced_events
      .iter()
      .filter(|e| matches!(e.event, PoolEvent::Burn(_)))
      .count();

   {
      let mut evm = new_evm(chain_id.into(), Some(&full_block), fork_db);

      // aprove the nft and swapper contract to spent the tokens
      let tokens = vec![pool.token0().clone(), pool.token1().clone()];
      for token in tokens {
         // approve the nft contract to spent LpProvider's tokens
         simulate::approve_token(
            &mut evm,
            token.address,
            lp_provider.address,
            nft_contract,
            U256::MAX,
         )?;

         // approve the swapper contract to spent Swapper's tokens
         simulate::approve_token(
            &mut evm,
            token.address,
            swapper.address,
            swap_router.address,
            U256::MAX,
         )?;

         // approve the nft contract to spent MinterAndBurner tokens
         simulate::approve_token(
            &mut evm,
            token.address,
            minter_and_burner.address,
            nft_contract,
            U256::MAX,
         )?;
      }

      // create the position
      let (_, mint_res) = simulate::mint_position(
         &mut evm,
         mint_params,
         lp_provider.address,
         nft_contract,
         true,
      )?;

      let token_id = mint_res.tokenId;

      amount0 = NumericValue::format_wei(mint_res.amount0, pool.token0().decimals);
      amount1 = NumericValue::format_wei(mint_res.amount1, pool.token1().decimals);


      // simulate all the swaps that occured
      trace!(target: "zeus_eth::amm::uniswap::v3::position", "Simulating {} swaps", total_swaps);
      for event in sequenced_events {
         match event.event {
            PoolEvent::Swap(swap) => {
               let (amount_in, amount_out, token_in, token_out) = if swap.amount0.is_positive() {
                  (swap.amount0, swap.amount1, pool.token0(), pool.token1())
               } else {
                  (swap.amount1, swap.amount0, pool.token1(), pool.token0())
               };

               let amount_in: U256 = amount_in.to_string().parse().unwrap();
               let amount_out: U256 = amount_out.to_string().trim_start_matches('-').parse().unwrap();

               if token_in.is_base() {
                  // Quote token is bought
                  buy_volume += amount_in;
               } else {
                  // Quote token is sold
                  sell_volume += amount_out;
               }

               let swap_params = abi::misc::SwapRouter::Params {
                  input_token: token_in.address,
                  output_token: token_out.address,
                  amount_in: amount_in,
                  pool: pool.address,
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
                  trace!(target: "zeus_eth::amm::uniswap::v3::position", "Swap Failed: {:?}", e);
                  continue;
               }
            }
            PoolEvent::Mint(mint) => {
               if config.skip_simulating_mints {
                  continue;
               }

               let mint_params = INonfungiblePositionManager::MintParams {
                  token0: pool.token0().address,
                  token1: pool.token1().address,
                  fee,
                  tickLower: mint.tickLower,
                  tickUpper: mint.tickUpper,
                  amount0Desired: mint.amount0,
                  amount1Desired: mint.amount1,
                  amount0Min: U256::ZERO,
                  amount1Min: U256::ZERO,
                  recipient: minter_and_burner.address,
                  deadline: U256::from(full_block.header.timestamp),
               };

               match simulate::mint_position(
                  &mut evm,
                  mint_params,
                  minter_and_burner.address,
                  nft_contract,
                  true,
               ) {
                  Ok((_res, mint_res)) => {
                     let token_id = mint_res.tokenId;
                     positions.insert((mint.owner, mint.tickLower, mint.tickUpper), token_id);
                  }
                  Err(e) => {
                     failed_mints += 1;
                     trace!(target: "zeus_eth::amm::uniswap::v3::position", "Mint Failed: {:?}", e);
                     continue;
                  }
               }
            }
            PoolEvent::Collect(_collect) => {
               // Skip for now, it shouldn't affect the performance of the position
            }
            PoolEvent::Burn(burn) => {
               if config.skip_simulating_burns {
                  continue;
               }
               
               let position = positions.get(&(burn.owner, burn.tickLower, burn.tickUpper)).cloned();
               if position.is_none() {
                //  eprintln!("No position found for Burn event with owner: {}", burn.owner);
                  continue;
               }

              // eprintln!("Simulating a Burn event with owner: {}", burn.owner);

               let position = position.unwrap();
               let params = INonfungiblePositionManager::DecreaseLiquidityParams {
                  tokenId: position,
                  liquidity: burn.amount,
                  amount0Min: burn.amount0,
                  amount1Min: burn.amount1,
                  deadline: U256::from(full_block.header.timestamp),
               };

               if let Err(e) = simulate::decrease_liquidity(
                  &mut evm,
                  params,
                  minter_and_burner.address,
                  nft_contract,
                  true,
               ) {
                  failed_burns += 1;
                 // eprintln!("Burn event failed with owner: {} reason: {}", burn.owner, e);
                  trace!("Decrease Liquidity Failed: {:?}", e);
                  continue;
               }
            }
         }

         // collect the fees
         let collect_params = INonfungiblePositionManager::CollectParams {
            tokenId: token_id,
            recipient: lp_provider.address,
            amount0Max: u128::MAX,
            amount1Max: u128::MAX,
         };

         let (_, amount0_collected, amount1_collected) = simulate::collect_fees(
            &mut evm,
            collect_params,
            lp_provider.address,
            nft_contract,
            false,
         )?;

         // compare the amount0 and amount1 with the collected amounts
         if amount0_collected > total_collected0 || amount1_collected > total_collected1 {
            total_collected0 = amount0_collected;
            total_collected1 = amount1_collected;
            active_swaps += 1;
         }

      }
   }

   let token0_earned = NumericValue::format_wei(total_collected0, pool.token0().decimals);
   let token1_earned = NumericValue::format_wei(total_collected1, pool.token1().decimals);

   // get the current usd price of token0 and token1
   pool.update_state(client.clone(), None).await?;

   let (base_usd, quote_usd) = pool.tokens_price(client.clone(), None).await?;

   // make sure we set the prices in the correct order
   let (latest_token0_usd, latest_token1_usd) = if pool.is_token0(pool.base_token().address) {
      (base_usd, quote_usd)
   } else {
      (quote_usd, base_usd)
   };

   let earned0_usd = NumericValue::value(token0_earned.f64(), latest_token0_usd);
   let earned1_usd = NumericValue::value(token1_earned.f64(), latest_token1_usd);

   let buy_volume = NumericValue::format_wei(buy_volume, pool.base_token().decimals);
   let sell_volume = NumericValue::format_wei(sell_volume, pool.quote_token().decimals);

   let buy_volume_usd = NumericValue::value(buy_volume.f64(), base_usd);
   let sell_volume_usd = NumericValue::value(sell_volume.f64(), quote_usd);
   let total_volume_usd = NumericValue::from_f64(buy_volume_usd.f64() + sell_volume_usd.f64());


   // calculate the APR of the position
   let total_earned_usd = earned0_usd.f64() + earned1_usd.f64();
   let deposit_usd_value = (past_token0_usd * amount0.f64()) + (past_token1_usd * amount1.f64());
   let mut apr = 0.0;

   match block_time {
      BlockTime::Days(days) => {
         apr = (total_earned_usd / deposit_usd_value) * (365.0 / days as f64) * 100.0;
      }
      BlockTime::Hours(hours) => {
         apr = (total_earned_usd / deposit_usd_value) * (8760.0 / hours as f64) * 100.0;
      }
      BlockTime::Block(_) => {
         // TODO
      }
      BlockTime::Minutes(_) => {
         // TODO
      }
   }

   let result = PositionResult {
      forked_block: fork_block_num,
      token0: pool.token0().into_owned(),
      token1: pool.token1().into_owned(),
      amount0,
      amount1,
      lower_tick,
      upper_tick,
      past_token0_usd,
      past_token1_usd,
      token0_usd: latest_token0_usd,
      token1_usd: latest_token1_usd,
      token0_earned: token0_earned,
      token1_earned: token1_earned,
      earned0_usd,
      earned1_usd,
      buy_volume,
      sell_volume,
      total_volume_usd,
      total_swaps,
      total_mints,
      total_burns,
      active_swaps,
      failed_swaps,
      failed_burns,
      failed_mints,
      apr: apr,
   };

   Ok(result)
}

/// Position ID
/// Key: owner, tick_lower, tick_upper -> tokenId
type Positions = HashMap<(Address, Signed<24, 1>, Signed<24, 1>), U256>;
enum PoolEvent {
   Swap(IUniswapV3Pool::Swap),
   Mint(IUniswapV3Pool::Mint),
   Collect(IUniswapV3Pool::Collect),
   Burn(IUniswapV3Pool::Burn),
}

struct SequencedEvent {
   block_number: u64,
   transaction_index: u64,
   log_index: u64,
   event: PoolEvent,
}

fn decode_events(logs: &Vec<Log>) -> Vec<SequencedEvent> {
   let mut sequenced_events: Vec<SequencedEvent> = Vec::new();
   let swap_event = IUniswapV3Pool::Swap::SIGNATURE_HASH;
   let mint_event = IUniswapV3Pool::Mint::SIGNATURE_HASH;
   let collect_event = IUniswapV3Pool::Collect::SIGNATURE_HASH;
   let burn_event = IUniswapV3Pool::Burn::SIGNATURE_HASH;

   for log in logs {
      let topic = log.inner.topics().first().cloned();
      let event = match topic {
         Some(t) if t == swap_event => decode_swap_log(&log.inner.data).ok().map(PoolEvent::Swap),
         Some(t) if t == mint_event => decode_mint_log(&log.inner.data).ok().map(PoolEvent::Mint),
         Some(t) if t == collect_event => decode_collect_log(&log.inner.data)
            .ok()
            .map(PoolEvent::Collect),
         Some(t) if t == burn_event => decode_burn_log(&log.inner.data).ok().map(PoolEvent::Burn),
         _ => None,
      };

      if let Some(evt) = event {
         sequenced_events.push(SequencedEvent {
            block_number: log.block_number.unwrap_or(0),
            transaction_index: log.transaction_index.unwrap_or(0),
            log_index: log.log_index.unwrap_or(0),
            event: evt,
         });
      }
   }

   sequenced_events.sort_by_key(|e| (e.block_number, e.transaction_index, e.log_index));
   sequenced_events
}





#[cfg(test)]
mod tests {
   use super::*;
   use crate::DexKind;
   use alloy_primitives::address;
   use alloy_provider::ProviderBuilder;
   use currency::ERC20Token;

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_simulate_position() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
      let wst_eth = address!("7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0");
      let pool_address = address!("109830a1aaad605bbf02a9dfa7b0b92ec2fb7daa");

      let weth = ERC20Token::new(client.clone(), weth, 1).await.unwrap();
      let wst_eth = ERC20Token::new(client.clone(), wst_eth, 1).await.unwrap();
      let amount0_deposit = NumericValue::parse_to_wei("10", wst_eth.decimals);

      let pool = UniswapV3Pool::new(1, pool_address, 100, weth, wst_eth, DexKind::UniswapV3);

      let skip_simulating_mints = true;
      let skip_simulating_burns = true;

      let position = SimPositionConfig {
         lower_range: 1.2044608725591641,
         upper_range: 1.210206495612689,
         deposit_amount: amount0_deposit,
         skip_simulating_mints,
         skip_simulating_burns,
      };

      let result = simulate_position(client, BlockTime::Days(1), position, pool)
         .await
         .unwrap();
      eprintln!("Result: {:#?}", result);
   }
}



/*


for (_i, swap) in swaps.iter().enumerate() {

      let mut in_range = false;

      let state_before_swap = pool.state().v3_or_v4_state().unwrap();

      let mut temp_position = Position::new(
         &state_before_swap,
         position.tick_lower,
         position.tick_upper,
         position.liquidity,
      )?;

      // Check if the position is active
      if state_before_swap.tick >= position.tick_lower && state_before_swap.tick < position.tick_upper {
         active_swaps += 1;
         in_range = true;
      }

      let token_in = swap.token_in.clone();
      pool.simulate_swap_mut(&token_in.into(), swap.amount_in)?;

      let state_after_swap = pool.state().v3_or_v4_state().unwrap();

      if in_range {
         // Update the temp position to see the fees from this ONE swap.
         let (fees_from_this_swap_0, fees_from_this_swap_1) = temp_position.update(&state_after_swap)?;

         total_fees_earned_0 += fees_from_this_swap_0;
         total_fees_earned_1 += fees_from_this_swap_1;
      }

      let fee0_earned = NumericValue::format_wei(total_fees_earned_0, pool.token0().decimals);
      let fee1_earned = NumericValue::format_wei(total_fees_earned_1, pool.token1().decimals);
   }


*/