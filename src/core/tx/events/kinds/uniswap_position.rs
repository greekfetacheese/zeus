use crate::core::ZeusCtx;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::amm::uniswap::UniswapPool;
use zeus_eth::{
   abi::protocols::uniswap,
   alloy_primitives::{Address, Log, U256},
   currency::Currency,
   utils::NumericValue,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PositionOperation {
   AddLiquidity,
   DecreaseLiquidity,
   CollectFees,
}

/// Struct to represent an operation on a Uniswap position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapPositionParams {
   pub position_operation: PositionOperation,
   pub currency0: Currency,
   pub currency1: Currency,
   pub amount0: NumericValue,
   pub amount0_usd: Option<NumericValue>,
   pub amount1: NumericValue,
   pub amount1_usd: Option<NumericValue>,
   pub min_amount0: Option<NumericValue>,
   pub min_amount0_usd: Option<NumericValue>,
   pub min_amount1: Option<NumericValue>,
   pub min_amount1_usd: Option<NumericValue>,
   pub sender: Address,
   /// If the operation is collect fees, this is the recipient
   pub recipient: Option<Address>,
}

impl UniswapPositionParams {
   pub fn op_is_add_liquidity(&self) -> bool {
      matches!(
         self.position_operation,
         PositionOperation::AddLiquidity
      )
   }

   pub fn op_is_decrease_liquidity(&self) -> bool {
      matches!(
         self.position_operation,
         PositionOperation::DecreaseLiquidity
      )
   }

   pub fn op_is_collect_fees(&self) -> bool {
      matches!(
         self.position_operation,
         PositionOperation::CollectFees
      )
   }

   /// Decode this Position operation if its CollectFees for Uniswap V3
   pub async fn collect_fees_for_v3_from_log(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      log: &Log,
   ) -> Result<Self, anyhow::Error> {
      let mut collect_fees_log = None;
      let mut pool_address = None;

      if let Ok(decoded_log) = uniswap::v3::pool::decode_collect_log(log) {
         collect_fees_log = Some(decoded_log);
         pool_address = Some(log.address);
      }

      if collect_fees_log.is_none() {
         return Err(anyhow!("Collect Fees log not found"));
      }

      let collect_fees = collect_fees_log.unwrap();
      let recipient = collect_fees.recipient;
      let pool_address = pool_address.unwrap();

      let pool = ctx.get_v3_pool(chain, pool_address).await?;

      let collected0 = U256::from(collect_fees.amount0);
      let collected1 = U256::from(collect_fees.amount1);

      let collected0 = NumericValue::format_wei(collected0, pool.currency0().decimals());
      let collected1 = NumericValue::format_wei(collected1, pool.currency1().decimals());

      let collected0_usd = ctx.get_currency_value_for_amount(collected0.f64(), pool.currency0());
      let collected1_usd = ctx.get_currency_value_for_amount(collected1.f64(), pool.currency1());

      Ok(Self {
         position_operation: PositionOperation::CollectFees,
         currency0: pool.currency0().clone(),
         currency1: pool.currency1().clone(),
         amount0: collected0,
         amount1: collected1,
         amount0_usd: Some(collected0_usd),
         amount1_usd: Some(collected1_usd),
         min_amount0: None,
         min_amount1: None,
         min_amount0_usd: None,
         min_amount1_usd: None,
         sender: from,
         recipient: Some(recipient),
      })
   }

   /// Decode this Position operation if it DecreaseLiquidity for Uniswap V3
   pub async fn decrease_liquidity_for_v3_from_logs(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      logs: &[Log],
   ) -> Result<Self, anyhow::Error> {
      let mut decrease_liquidity_log = None;
      let mut burn_log = None;
      let mut pool_address = None;

      for log in logs {
         if let Ok(decoded_log) = uniswap::v3::pool::decode_burn_log(log) {
            burn_log = Some(decoded_log);
            pool_address = Some(log.address);
         }

         if let Ok(decoded_log) = uniswap::nft_position::decode_decrease_liquidity_log(log) {
            decrease_liquidity_log = Some(decoded_log);
         }
      }

      if burn_log.is_none() {
         return Err(anyhow!("Burn log not found"));
      }

      if decrease_liquidity_log.is_none() {
         return Err(anyhow!("Decrease Liquidity log not found"));
      }

      let burn = burn_log.unwrap();
      let pool_address = pool_address.unwrap();

      let pool = ctx.get_v3_pool(chain, pool_address).await?;

      let amount0_removed = NumericValue::format_wei(burn.amount0, pool.currency0().decimals());
      let amount1_removed = NumericValue::format_wei(burn.amount1, pool.currency1().decimals());

      let amount0_usd_to_be_removed =
         ctx.get_currency_value_for_amount(amount0_removed.f64(), pool.currency0());
      let amount1_usd_to_be_removed =
         ctx.get_currency_value_for_amount(amount1_removed.f64(), pool.currency1());

      Ok(Self {
         position_operation: PositionOperation::DecreaseLiquidity,
         currency0: pool.currency0().clone(),
         currency1: pool.currency1().clone(),
         amount0: amount0_removed.clone(),
         amount1: amount1_removed.clone(),
         amount0_usd: Some(amount0_usd_to_be_removed),
         amount1_usd: Some(amount1_usd_to_be_removed),
         min_amount0: None,
         min_amount0_usd: None,
         min_amount1: None,
         min_amount1_usd: None,
         sender: from,
         recipient: None,
      })
   }

   /// Decode this Position operation if its AddLiqudity for Uniswap V3
   pub async fn add_liquidity_for_v3_from_logs(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      logs: &[Log],
   ) -> Result<Self, anyhow::Error> {
      let mut add_liquidity_log = None;
      let mut mint_log = None;
      let mut pool_address = None;

      for log in logs {
         if let Ok(decoded_log) = uniswap::v3::pool::decode_mint_log(log) {
            mint_log = Some(decoded_log);
            pool_address = Some(log.address);
         }

         if let Ok(decoded_log) = uniswap::nft_position::decode_increase_liquidity_log(log) {
            add_liquidity_log = Some(decoded_log);
         }
      }

      if mint_log.is_none() {
         return Err(anyhow!("Mint log not found"));
      }

      if add_liquidity_log.is_none() {
         return Err(anyhow!("Increase Liquidity log not found"));
      }

      let mint = mint_log.unwrap();
      let pool_address = pool_address.unwrap();
      let pool = ctx.get_v3_pool(chain, pool_address).await?;

      let amount0_minted = NumericValue::format_wei(mint.amount0, pool.currency0().decimals());
      let amount1_minted = NumericValue::format_wei(mint.amount1, pool.currency1().decimals());

      let amount0_usd = ctx.get_currency_value_for_amount(amount0_minted.f64(), pool.currency0());
      let amount1_usd = ctx.get_currency_value_for_amount(amount1_minted.f64(), pool.currency1());

      Ok(Self {
         position_operation: PositionOperation::AddLiquidity,
         currency0: pool.currency0().clone(),
         currency1: pool.currency1().clone(),
         amount0: amount0_minted,
         amount1: amount1_minted,
         amount0_usd: Some(amount0_usd),
         amount1_usd: Some(amount1_usd),
         min_amount0: None,
         min_amount1: None,
         min_amount0_usd: None,
         min_amount1_usd: None,
         sender: from,
         recipient: None,
      })
   }

   pub fn name(&self) -> String {
      match self.position_operation {
         PositionOperation::AddLiquidity => "Add Liquidity".to_string(),
         PositionOperation::DecreaseLiquidity => "Remove Liquidity".to_string(),
         PositionOperation::CollectFees => "Collect Fees".to_string(),
      }
   }
}
