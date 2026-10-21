use crate::core::types::BaseFee;
use crate::core::{ZeusContext, ZeusCtx};
use anyhow::anyhow;
#[cfg(feature = "dev")]
use tracing::debug;
use zeus_eth::{
   alloy_primitives::U256,
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, NativeCurrency},
   types::ChainId,
   utils::{NumericValue, block::calculate_next_block_base_fee},
};

pub async fn get_base_fee(ctx: ZeusCtx, chain: u64) -> Result<BaseFee, anyhow::Error> {
   if ctx.is_chain_disabled(chain) {
      return Ok(BaseFee::new(0, 0));
   }

   let z_client = ctx.get_client_manager();
   let chain = ChainId::new(chain)?;

   if chain.is_ethereum() {
      let block = z_client
         .request(chain.id(), |client| async move {
            client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(block) = block {
         let base_fee = block.header.base_fee_per_gas.unwrap_or_default();
         let next_base_fee = calculate_next_block_base_fee(block);
         ctx.update_base_fee(chain.id(), base_fee, next_base_fee);

         return Ok(BaseFee::new(base_fee, next_base_fee));
      } else {
         return Err(anyhow!("Latest block not found"));
      }
   }

   let gas_price = z_client
      .request(chain.id(), |client| async move {
         client.get_gas_price().await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let fee: u64 = gas_price.try_into()?;

   #[cfg(feature = "dev")]
   let fee_gwei = NumericValue::format_to_gwei(U256::from(fee));

   #[cfg(feature = "dev")]
   debug!(
      "Base fee for chain {} is {}",
      chain.id(),
      fee_gwei.formatted()
   );

   ctx.update_base_fee(chain.id(), fee, fee);
   Ok(BaseFee::new(fee, fee))
}

/// Estimate the cost for a transaction
///
/// Returns (cost_in_wei, cost_in_usd)
pub fn estimate_tx_cost(
   ctx: &mut ZeusContext,
   chain: u64,
   gas_used: u64,
   priority_fee: U256,
) -> (NumericValue, NumericValue) {
   let base_fee = ctx.get_base_fee(chain).unwrap_or_default().next;
   let total_fee = priority_fee + U256::from(base_fee);

   // native currency price
   let native = NativeCurrency::from(chain);
   let price = ctx.get_currency_price(&Currency::from(native.clone()));

   let cost_in_wei = total_fee * U256::from(gas_used);
   let cost = NumericValue::format_wei(cost_in_wei, native.decimals);

   let cost_in_usd = NumericValue::value(cost.f64(), price.f64());

   (cost, cost_in_usd)
}
