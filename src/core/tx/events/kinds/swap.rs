use crate::core::{ZeusCtx, types::Dapp};
use zeus_eth::amm::uniswap::UniswapPool;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::protocols::uniswap,
   alloy_primitives::{Address, Log, U256},
   currency::Currency,
   utils::NumericValue,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// USD values are the time of the tx
pub struct SwapParams {
   pub dapp: Dapp,
   pub input_currency: Currency,
   pub output_currency: Currency,
   pub amount_in: NumericValue,
   pub amount_in_usd: Option<NumericValue>,
   pub received: NumericValue,
   pub received_usd: Option<NumericValue>,
   pub min_received: Option<NumericValue>,
   pub min_received_usd: Option<NumericValue>,
   pub sender: Address,
   pub recipient: Option<Address>,
}

impl Default for SwapParams {
   fn default() -> Self {
      Self {
         dapp: Dapp::Uniswap,
         input_currency: Currency::default(),
         output_currency: Currency::default(),
         amount_in: NumericValue::default(),
         amount_in_usd: None,
         received: NumericValue::default(),
         received_usd: None,
         min_received: None,
         min_received_usd: None,
         sender: Address::default(),
         recipient: None,
      }
   }
}

impl SwapParams {
   pub async fn from_uniswap_v2(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      log: &Log,
   ) -> Result<Self, anyhow::Error> {
      let (swap_log, pool_address) = match uniswap::v2::pool::decode_swap_log(log) {
         Ok(decoded) => (decoded, log.address),
         Err(e) => {
            return Err(anyhow!("Failed to decode V2 swap log {}", e));
         }
      };

      let pool = ctx.get_v2_pool(chain, pool_address).await?;

      let (amount_in, currency_in) = if swap_log.amount0In > swap_log.amount1In {
         (swap_log.amount0In, pool.currency0().clone())
      } else {
         (swap_log.amount1In, pool.currency1().clone())
      };

      let (amount_out, currency_out) = if swap_log.amount0Out > swap_log.amount1Out {
         (swap_log.amount0Out, pool.currency0().clone())
      } else {
         (swap_log.amount1Out, pool.currency1().clone())
      };

      let amount_in = NumericValue::format_wei(amount_in, currency_in.decimals());
      let amount_in_usd = ctx.get_currency_value_for_amount(amount_in.f64(), &currency_in);

      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());
      let amount_out_usd = ctx.get_currency_value_for_amount(amount_out.f64(), &currency_out);

      let params = SwapParams {
         dapp: Dapp::Uniswap,
         input_currency: currency_in,
         output_currency: currency_out,
         amount_in,
         amount_in_usd: Some(amount_in_usd),
         received: amount_out,
         received_usd: Some(amount_out_usd),
         min_received: None,
         min_received_usd: None,
         sender: from,
         recipient: None,
      };

      Ok(params)
   }

   pub async fn from_uniswap_v3(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      log: &Log,
   ) -> Result<Self, anyhow::Error> {
      let (swap, pool_address) = match uniswap::v3::pool::decode_swap_log(log) {
         Ok(decoded) => (decoded, log.address),
         Err(e) => {
            return Err(anyhow!("Failed to decode V3 swap log {}", e));
         }
      };

      let pool = ctx.get_v3_pool(chain, pool_address).await?;

      let (amount_in, currency_in, amount_out, currency_out) = if swap.amount0.is_positive() {
         (
            swap.amount0,
            pool.currency0().clone(),
            swap.amount1,
            pool.currency1().clone(),
         )
      } else {
         (
            swap.amount1,
            pool.currency1().clone(),
            swap.amount0,
            pool.currency0().clone(),
         )
      };

      let amount_in = amount_in.to_string().parse::<U256>()?;
      let amount_out = amount_out.to_string().trim_start_matches('-').parse::<U256>()?;

      let amount_in = NumericValue::format_wei(amount_in, currency_in.decimals());
      let amount_in_usd = ctx.get_currency_value_for_amount(amount_in.f64(), &currency_in);

      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());
      let amount_out_usd = ctx.get_currency_value_for_amount(amount_out.f64(), &currency_out);

      let params = SwapParams {
         dapp: Dapp::Uniswap,
         input_currency: currency_in,
         output_currency: currency_out,
         amount_in,
         amount_in_usd: Some(amount_in_usd),
         received: amount_out,
         received_usd: Some(amount_out_usd),
         min_received: None,
         min_received_usd: None,
         sender: from,
         recipient: None,
      };

      Ok(params)
   }

   pub async fn from_uniswap_v4(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      log: &Log,
   ) -> Result<Self, anyhow::Error> {
      let swap = match uniswap::v4::decode_swap_log(log) {
         Ok(decoded) => decoded,
         Err(e) => {
            return Err(anyhow!("Failed to decode V4 swap log {}", e));
         }
      };

      let fee: u32 = swap.fee.try_into()?;
      let pool = ctx.get_v4_pool(chain, fee, swap.id).await?;

      // In V4 the negative amount is token amount we are selling
      let (amount_in, currency_in, amount_out, currency_out) = if swap.amount0.is_negative() {
         (
            swap.amount0,
            pool.currency0().clone(),
            swap.amount1,
            pool.currency1().clone(),
         )
      } else {
         (
            swap.amount1,
            pool.currency1().clone(),
            swap.amount0,
            pool.currency0().clone(),
         )
      };

      let amount_in = amount_in.to_string().trim_start_matches('-').parse::<U256>()?;
      let amount_out = amount_out.to_string().parse::<U256>()?;

      let amount_in = NumericValue::format_wei(amount_in, currency_in.decimals());
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());

      let amount_in_usd = ctx.get_currency_value_for_amount(amount_in.f64(), &currency_in);
      let amount_out_usd = ctx.get_currency_value_for_amount(amount_out.f64(), &currency_out);

      let params = SwapParams {
         dapp: Dapp::Uniswap,
         input_currency: currency_in.clone(),
         output_currency: currency_out.clone(),
         amount_in,
         amount_in_usd: Some(amount_in_usd),
         received: amount_out,
         received_usd: Some(amount_out_usd),
         min_received: None,
         min_received_usd: None,
         sender: from,
         recipient: None,
      };

      Ok(params)
   }
}
