use crate::tx::fees;
use crate::types::{BaseFee, PriorityFee};
use crate::{ZeusContext, ZeusCtx};
use anyhow::anyhow;
#[cfg(feature = "dev")]
use tracing::debug;
use tracing::warn;
use zeus_eth::{
   alloy_primitives::U256,
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag},
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

/// How long a recommendation is shown before it is re-read.
pub const FEE_TIERS_TTL: u64 = 60;

/// Read the chain's priority-fee recommendation into the context.
///
/// Mainnet asks the fee market for its own recent history; every other chain keeps the node's
/// estimate, floored per chain. A failed or empty history is not an error worth failing a send
/// over — it falls back to the estimate and leaves the previous recommendation in place.
pub async fn update_priority_fee(ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
   if ctx.is_chain_disabled(chain) {
      return Ok(());
   }

   let z_client = ctx.get_client_manager();
   let chain = ChainId::new(chain)?;

   if !chain.supports_type_2_tx() {
      return Ok(());
   }

   if chain.is_ethereum() {
      let history = z_client
         .request(chain.id(), |client| async move {
            client
               .get_fee_history(
                  fees::FEE_HISTORY_BLOCKS,
                  BlockNumberOrTag::Latest,
                  &fees::REWARD_PERCENTILES,
               )
               .await
               .map_err(|e| anyhow!("{:?}", e))
         })
         .await;

      match history {
         Ok(history) => {
            if let Some(rows) = history.reward.as_deref()
               && let Some(tiers) = fees::tiers_from_rewards(rows, PriorityFee::minimum(chain.id()))
            {
               ctx.update_fee_tiers(chain.id(), tiers);
               return Ok(());
            }

            warn!(
               "Rpc returned no usable fee history for chain {}, using its estimate",
               chain.id()
            );
         }
         Err(e) => warn!(
            "Fee history for chain {} failed ({:?}), using the rpc estimate",
            chain.id(),
            e
         ),
      }
   }

   let fee = z_client
      .request(chain.id(), |client| async move {
         client.get_max_priority_fee_per_gas().await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let fee_u256 = U256::from(fee).max(PriorityFee::minimum(chain.id()));
   let fee_fmt = NumericValue::format_to_gwei(fee_u256);

   if fee_fmt.is_zero() {
      return Err(anyhow!(
         "Rpc returned bad data, Fee (Wei) {} For Chain: {}",
         fee,
         chain.id()
      ));
   }

   #[cfg(feature = "dev")]
   debug!(
      "Priority fee for chain {} is {}",
      chain.id(),
      fee_fmt.formatted()
   );

   ctx.update_priority_fee(chain.id(), fee_fmt);
   Ok(())
}

/// Refresh a chain's recommendation when the one on hand is stale.
///
/// The periodic loop reads it every 300 s, which is minutes old by the time a window opens; the
/// confirm path calls this first. Never fatal — the window falls back to whatever is stored.
pub async fn ensure_priority_fee_fresh(ctx: ZeusCtx, chain: u64) {
   if ctx.is_priority_fee_fresh(chain, FEE_TIERS_TTL) {
      return;
   }

   if let Err(e) = update_priority_fee(ctx.clone(), chain).await {
      warn!("Could not refresh the priority fee for chain {chain}: {e:?}");
   }
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
