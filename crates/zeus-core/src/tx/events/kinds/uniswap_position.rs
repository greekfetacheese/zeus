use crate::ZeusCtx;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::amm::uniswap::UniswapPool;
use zeus_eth::amm::uniswap::v4::{Actions, actions_from_params};
use zeus_eth::{
   abi::{erc20, protocols::uniswap},
   alloy_primitives::{Address, B256, Bytes, I256, Log, U256},
   currency::{Currency, NativeCurrency},
   utils::{NumericValue, address_book},
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

   /// Decode this Position operation if it is an AddLiquidity for Uniswap V4.
   ///
   /// V4 is a singleton: the position manager's `modifyLiquidities` call carries a batch of
   /// actions and the pool manager emits one core `ModifyLiquidity` event per liquidity change.
   /// That event has no amounts — every payment settles as a delta through the singleton — so the
   /// amounts shown are best-effort, derived from the currency `Transfer` logs (and, for the
   /// native currency, the call's own `value`).
   pub async fn add_liquidity_for_v4_from_logs(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      logs: &[Log],
      call_data: Bytes,
      value: U256,
   ) -> Result<Self, anyhow::Error> {
      // Not a position manager `modifyLiquidities` call → not a V4 position operation.
      let (actions, params) = uniswap::v4::decode_modify_liquidities(&call_data)?;
      let actions = actions_from_params(&actions, &params)?;

      let mint = actions.iter().find_map(|action| match action {
         Actions::MINT_POSITION(params) => Some(params),
         _ => None,
      });

      let has_increase =
         actions.iter().any(|action| matches!(action, Actions::INCREASE_LIQUIDITY(_)));

      if mint.is_none() && !has_increase {
         return Err(anyhow!("No V4 add liquidity action"));
      }

      // The pool must have genuinely moved, and only an add moves it up.
      let pool_id = v4_liquidity_change_pool_id(chain, logs, true)?;

      let (currency0, currency1) = if let Some(mint) = mint {
         // A mint carries the PoolKey, so the currencies are known without a pool lookup.
         let currency0 = v4_currency(&ctx, chain, mint.poolKey.currency0).await?;
         let currency1 = v4_currency(&ctx, chain, mint.poolKey.currency1).await?;
         (currency0, currency1)
      } else {
         // An increase names only the position NFT: the log's pool id leads back to the pool.
         let pool = ctx.get_v4_pool(chain, 0, pool_id).await?;
         (pool.currency0().clone(), pool.currency1().clone())
      };

      let recipients = [
         address_book::uniswap_v4_nft_position_manager(chain)?,
         address_book::uniswap_v4_pool_manager(chain)?,
      ];

      let amount0 = v4_leg_amount(&currency0, logs, value, &recipients);
      let amount1 = v4_leg_amount(&currency1, logs, value, &recipients);

      let amount0_usd = ctx.get_currency_value_for_amount(amount0.f64(), &currency0);
      let amount1_usd = ctx.get_currency_value_for_amount(amount1.f64(), &currency1);

      Ok(Self {
         position_operation: PositionOperation::AddLiquidity,
         currency0,
         currency1,
         amount0,
         amount1,
         amount0_usd: Some(amount0_usd),
         amount1_usd: Some(amount1_usd),
         // A mint's `amount0Max/amount1Max` is a cap, not a floor: it must not be shown as the
         // "Minimum Liquidity to be added".
         min_amount0: None,
         min_amount0_usd: None,
         min_amount1: None,
         min_amount1_usd: None,
         sender: from,
         recipient: None,
      })
   }

   /// Decode this Position operation if it is a DecreaseLiquidity for Uniswap V4.
   pub async fn decrease_liquidity_for_v4_from_logs(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      logs: &[Log],
      call_data: Bytes,
      value: U256,
   ) -> Result<Self, anyhow::Error> {
      // Not a position manager `modifyLiquidities` call → not a V4 position operation.
      let (actions, params) = uniswap::v4::decode_modify_liquidities(&call_data)?;
      let actions = actions_from_params(&actions, &params)?;

      let decrease = actions.iter().find_map(|action| match action {
         Actions::DECREASE_LIQUIDITY(params) => Some((params.amount0Min, params.amount1Min)),
         Actions::BURN_POSITION(params) => Some((params.amount0Min, params.amount1Min)),
         _ => None,
      });

      let Some((min_amount0_wei, min_amount1_wei)) = decrease else {
         return Err(anyhow!("No V4 decrease liquidity action"));
      };

      // The pool must have genuinely moved, and a removal moves it down.
      let pool_id = v4_liquidity_change_pool_id(chain, logs, false)?;

      // A decrease names only the position NFT: the log's pool id leads back to the pool.
      let pool = ctx.get_v4_pool(chain, 0, pool_id).await?;
      let currency0 = pool.currency0().clone();
      let currency1 = pool.currency1().clone();

      // The tokens coming back to the user. The native side of a removal has no transfer log, so
      // it reads the call's own `value`, which a removal normally leaves at zero.
      let amount0 = v4_leg_amount(&currency0, logs, value, &[from]);
      let amount1 = v4_leg_amount(&currency1, logs, value, &[from]);

      let amount0_usd = ctx.get_currency_value_for_amount(amount0.f64(), &currency0);
      let amount1_usd = ctx.get_currency_value_for_amount(amount1.f64(), &currency1);

      let min_amount0 = NumericValue::format_wei(U256::from(min_amount0_wei), currency0.decimals());
      let min_amount1 = NumericValue::format_wei(U256::from(min_amount1_wei), currency1.decimals());

      let min_amount0_usd = ctx.get_currency_value_for_amount(min_amount0.f64(), &currency0);
      let min_amount1_usd = ctx.get_currency_value_for_amount(min_amount1.f64(), &currency1);

      Ok(Self {
         position_operation: PositionOperation::DecreaseLiquidity,
         currency0,
         currency1,
         amount0,
         amount1,
         amount0_usd: Some(amount0_usd),
         amount1_usd: Some(amount1_usd),
         // Unlike a mint's cap, `amount0Min/amount1Min` is the removal's real floor.
         min_amount0: Some(min_amount0),
         min_amount0_usd: Some(min_amount0_usd),
         min_amount1: Some(min_amount1),
         min_amount1_usd: Some(min_amount1_usd),
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

/// The id of the V4 pool whose liquidity moved, from the core `ModifyLiquidity` event the pool
/// manager emits for every change.
///
/// `positive` selects the direction: an add raises the pool's liquidity, a removal lowers it. A
/// `modifyLiquidities` call whose `ModifyLiquidity` events move the pool the other way is not the
/// operation its action list claims to be.
fn v4_liquidity_change_pool_id(
   chain: u64,
   logs: &[Log],
   positive: bool,
) -> Result<B256, anyhow::Error> {
   let pool_manager = address_book::uniswap_v4_pool_manager(chain)?;

   for log in logs {
      if log.address != pool_manager {
         continue;
      }

      let Ok(decoded) = uniswap::v4::decode_modify_liquidity(&log.data) else {
         continue;
      };

      let moved = if positive {
         decoded.liquidityDelta > I256::ZERO
      } else {
         decoded.liquidityDelta < I256::ZERO
      };

      if moved {
         return Ok(decoded.id);
      }
   }

   Err(anyhow!(
      "No V4 ModifyLiquidity log with a {} liquidity delta",
      if positive { "positive" } else { "negative" }
   ))
}

/// One leg of a V4 position operation's amount, best-effort.
///
/// V4 emits no amounts: the pool manager is a singleton and every payment settles as a delta
/// through it, so the operation's params carry only caps/floors. What is observable in the
/// receipt is the currency's own `Transfer` logs, plus — for the native currency — the call's
/// `value`.
///
/// `recipients` picks the transfers that belong to this leg: the final hop into the position
/// manager or the pool manager on an add (a Permit2-routed payment has an earlier hop that this
/// deliberately does not count), or the payout back to the user on a removal.
fn v4_leg_amount(
   currency: &Currency,
   logs: &[Log],
   value: U256,
   recipients: &[Address],
) -> NumericValue {
   if currency.is_native() {
      return NumericValue::format_wei(value, currency.decimals());
   }

   let mut amount = U256::ZERO;

   for log in logs {
      if log.address != currency.address() {
         continue;
      }

      if let Ok(transfer) = erc20::decode_transfer_log(log)
         && recipients.contains(&transfer.to)
      {
         amount += transfer.value;
      }
   }

   NumericValue::format_wei(amount, currency.decimals())
}

/// Resolve one V4 currency: `address(0)` is the chain's native currency, anything else an ERC-20
/// looked up (and cached) through the context.
async fn v4_currency(
   ctx: &ZeusCtx,
   chain: u64,
   address: Address,
) -> Result<Currency, anyhow::Error> {
   if address.is_zero() {
      return Ok(NativeCurrency::from(chain).into());
   }

   Ok(Currency::from(
      ctx.get_token(chain, address).await?,
   ))
}
