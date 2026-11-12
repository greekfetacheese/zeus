use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, Bytes, U256, address,
   utils::{format_units, keccak256, parse_units},
};
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolValue;

use crate::amm::uniswap::state::get_v4_pool_state;
use crate::amm::uniswap::{
   DexKind, FeeAmount, State, SwapResult, UniswapPool,
   v3::{calculate_price, calculate_swap, calculate_swap_mut},
};

use super::super::math::sqrt_price_math::Q96;
use crate::NumericValue;
use crate::abi::uniswap::v4::{
   PoolKey,
   quoter::{QuoteExactInputSingleParams, UniswapV4Quoter},
};
use crate::currency::{Currency, ERC20Token, NativeCurrency};
use crate::utils::{address_book, price_feed::get_base_token_price};

use anyhow::{anyhow, bail};
use core::cmp::Ordering;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Max fee to avoid overflows, It doesnt make any sense to swap on these pools anyway
pub const MAX_FEE: f32 = 10.0;

/// Denominator of [`HookFeeModel::fee_pips`].
pub const HOOK_FEE_DENOMINATOR: u32 = 10_000;

/// How long a fitted hook cut is trusted before it is fitted again.
pub const HOOK_FEE_TTL_SECS: u64 = 120;

/// How far the model may miss the chain at the reference amount before it is discarded.
///
/// A hook that is not a flat cut of the input — tiers, a fixed amount, oracle pricing — cannot be
/// described by one number. The fit says so with `linear: false` instead of guessing.
pub const HOOK_FEE_LINEARITY_TOLERANCE_PIPS: u32 = 25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV4Pool {
   pub chain_id: u64,
   pub fee: FeeAmount,
   pub dex: DexKind,
   pub currency0: Currency,
   pub currency1: Currency,
   #[serde(skip)]
   pub state: State,
   pub hooks: Address,
   /// On-chain tick spacing from `Initialize` when known.
   ///
   /// `0` means "unknown" (legacy pool data) → fall back to [`FeeAmount::tick_spacing_i32`].
   #[serde(default)]
   pub tick_spacing: i32,
   pub liquidity_amount0: U256,
   pub liquidity_amount1: U256,
   /// The chain's answer for one `(input currency, amount in)` pair; hook pools only.
   ///
   /// Not persisted: it is only valid for the amount it was fetched for, and the swap path refills
   /// it on every quote. See [`UniswapV4Pool::set_hook_quote`].
   #[serde(skip)]
   pub hook_quote: Option<HookSwapQuote>,
   /// The hook's cut, fitted from the chain and indexed by `zero_for_one`; hook pools only.
   ///
   /// Persisted so a hook pool can be priced without a request; refreshed by the swap and price
   /// paths when stale. See [`UniswapV4Pool::fit_hook_fee`].
   #[serde(default)]
   pub hook_fees: [Option<HookFeeModel>; 2],
}

/// What a hook takes off a swap, fitted from one on-chain quote.
///
/// `fee_pips` describes the whole cut — the pool's own fee and the hook's — as a fraction of the
/// input, because that is what the fit can observe. Only `linear` models may be applied to amounts
/// other than the ones they were fitted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookFeeModel {
   /// The cut, in pips of the input: `100` = 1%.
   pub fee_pips: u32,
   /// The model matched the chain at the reference amount, so the hook behaves like a flat cut.
   pub linear: bool,
   /// Unix seconds when the fit was made.
   pub observed_at: u64,
}

impl HookFeeModel {
   /// The input the pool would actually swap once the hook has taken its cut.
   fn input_after_cut(&self, amount_in: U256) -> U256 {
      let kept = HOOK_FEE_DENOMINATOR.saturating_sub(self.fee_pips);
      amount_in.saturating_mul(U256::from(kept)) / U256::from(HOOK_FEE_DENOMINATOR)
   }
}

/// Seconds since the unix epoch, for [`HookFeeModel::observed_at`].
fn now_secs() -> u64 {
   std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or_default()
      .as_secs()
}

/// The cut a hook takes, from what fee-0 local math would produce and what the chain produced.
fn implied_fee_pips(local_out: U256, chain_out: U256) -> Option<u32> {
   if local_out.is_zero() {
      return None;
   }

   // A hook that pays out instead of taking: fee-0 local math is already the conservative answer.
   if chain_out >= local_out {
      return Some(0);
   }

   let cut = (local_out - chain_out) * U256::from(HOOK_FEE_DENOMINATOR) / local_out;
   u32::try_from(cut).ok()
}

/// Does `model_out` miss the chain's `chain_out` by more than `tolerance_pips`?
fn misses_by_more_than(model_out: U256, chain_out: U256, tolerance_pips: u32) -> bool {
   if chain_out.is_zero() {
      return true;
   }

   let diff = model_out.abs_diff(chain_out);
   diff * U256::from(HOOK_FEE_DENOMINATOR) / chain_out > U256::from(tolerance_pips)
}

/// One on-chain quote for a hook pool.
///
/// A hook can change both sides of a swap, so the amount out is only known for the exact amount that
/// was asked for: `set_hook_quote` stores it, `hook_quote_out` only hands it back for the same
/// pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookSwapQuote {
   /// The pool-side input currency (`currency0` or `currency1`) the answer is for.
   pub currency_in: Address,
   pub amount_in: U256,
   pub amount_out: U256,
}

impl Ord for UniswapV4Pool {
   fn cmp(&self, other: &Self) -> Ordering {
      self.id().cmp(&other.id())
   }
}

impl PartialOrd for UniswapV4Pool {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
   }
}

impl Eq for UniswapV4Pool {}

impl PartialEq for UniswapV4Pool {
   fn eq(&self, other: &Self) -> bool {
      self.id() == other.id() && self.chain_id == other.chain_id
   }
}

impl Hash for UniswapV4Pool {
   fn hash<H: Hasher>(&self, state: &mut H) {
      self.chain_id.hash(state);
      self.currency0.hash(state);
      self.currency1.hash(state);
      self.fee.hash(state);
      self.resolved_tick_spacing().hash(state);
      self.hooks.hash(state);
      self.dex.hash(state);
   }
}

impl UniswapV4Pool {
   pub fn new(
      chain_id: u64,
      fee: FeeAmount,
      dex: DexKind,
      currency_a: Currency,
      currency_b: Currency,
      state: State,
      hooks: Address,
   ) -> Self {
      Self::new_with_spacing(
         chain_id,
         fee,
         fee.tick_spacing_i32(),
         dex,
         currency_a,
         currency_b,
         state,
         hooks,
      )
   }

   /// Create a V4 pool with an explicit on-chain tick spacing (from `Initialize`).
   ///
   /// The eight positional fields follow the `Initialize` event / `PoolKey` order and are
   /// forwarded verbatim by `new` and `from_components*`; a struct argument would obscure that.
   #[allow(clippy::too_many_arguments)]
   pub fn new_with_spacing(
      chain_id: u64,
      fee: FeeAmount,
      tick_spacing: i32,
      dex: DexKind,
      currency_a: Currency,
      currency_b: Currency,
      state: State,
      hooks: Address,
   ) -> Self {
      let (currency0, currency1) = if currency_a.address() < currency_b.address() {
         (currency_a, currency_b)
      } else {
         (currency_b, currency_a)
      };

      // Never store 0 — that would break pool id / StateView math.
      let tick_spacing = if tick_spacing == 0 {
         fee.tick_spacing_i32()
      } else {
         tick_spacing
      };

      Self {
         chain_id,
         fee,
         dex,
         currency0,
         currency1,
         state,
         hooks,
         tick_spacing,
         liquidity_amount0: U256::ZERO,
         liquidity_amount1: U256::ZERO,
         hook_quote: None,
         hook_fees: [None, None],
      }
   }

   pub fn from_components(
      chain_id: u64,
      currency_a: Currency,
      currency_b: Currency,
      fee: FeeAmount,
      dex: DexKind,
      hooks: Address,
   ) -> Self {
      Self::new(
         chain_id,
         fee,
         dex,
         currency_a,
         currency_b,
         State::none(),
         hooks,
      )
   }

   pub fn from_components_with_spacing(
      chain_id: u64,
      currency_a: Currency,
      currency_b: Currency,
      fee: FeeAmount,
      tick_spacing: i32,
      dex: DexKind,
      hooks: Address,
   ) -> Self {
      Self::new_with_spacing(
         chain_id,
         fee,
         tick_spacing,
         dex,
         currency_a,
         currency_b,
         State::none(),
         hooks,
      )
   }

   /// Resolved tick spacing (stored value, or fee heuristic if unset).
   pub fn resolved_tick_spacing(&self) -> i32 {
      if self.tick_spacing != 0 {
         self.tick_spacing
      } else {
         self.fee.tick_spacing_i32()
      }
   }

   pub fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one(currency_in);
      let price = calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   pub fn state_mut(&mut self) -> &mut State {
      &mut self.state
   }

   /// Does this pool's hook take part in swaps?
   ///
   /// Such a pool cannot be simulated locally — `beforeSwap`/`afterSwap` may change both amounts —
   /// so the swap path quotes it on chain and feeds the answer back through
   /// [`Self::set_hook_quote`].
   pub fn has_swap_hooks(&self) -> bool {
      super::has_swap_permissions(self.hooks)
   }

   /// Store the chain's answer for an exact-input swap through this pool.
   ///
   /// `currency_in` is the pool-side currency (`currency0`/`currency1`), not the trade currency: the
   /// quote is matched by address when the local quoter replays the hop.
   pub fn set_hook_quote(&mut self, currency_in: &Currency, amount_in: U256, amount_out: U256) {
      self.hook_quote = Some(HookSwapQuote {
         currency_in: currency_in.address(),
         amount_in,
         amount_out,
      });
   }

   /// The stored chain quote, when it answers for exactly this input.
   fn hook_quote_out(&self, currency_in: &Currency, amount_in: U256) -> Option<U256> {
      let quote = self.hook_quote?;
      let input = if self.zero_for_one(currency_in) {
         self.currency0.address()
      } else {
         self.currency1.address()
      };
      (quote.currency_in == input && quote.amount_in == amount_in).then_some(quote.amount_out)
   }

   /// The direction and the post-cut input a hook pool's swap should use, or why it cannot.
   ///
   /// The cut was fitted against fee-0 local math, so the pool's own fee is already inside it: the
   /// swap that follows runs at fee 0.
   fn hook_fee_swap_amount(
      &self,
      currency_in: &Currency,
      amount_in: U256,
   ) -> Result<(bool, U256), anyhow::Error> {
      let zero_for_one = self.zero_for_one(currency_in);
      let Some(model) = self.hook_fee(zero_for_one) else {
         return Err(anyhow!("Unsupported Hook"));
      };

      if !model.linear {
         return Err(anyhow!("Unsupported Hook"));
      }

      Ok((zero_for_one, model.input_after_cut(amount_in)))
   }

   /// The fitted hook cut for a swap direction, usable or not.
   pub fn hook_fee(&self, zero_for_one: bool) -> Option<&HookFeeModel> {
      self.hook_fees[usize::from(zero_for_one)].as_ref()
   }

   /// Does this direction need a fresh fit before the local quoter may use it?
   ///
   /// A non-linear hook never becomes usable: its cut depends on the amount, so the pool keeps
   /// asking the chain for the exact amount instead.
   pub fn hook_fee_is_stale(&self, zero_for_one: bool) -> bool {
      match self.hook_fee(zero_for_one) {
         None => true,
         Some(model) => {
            !model.linear || now_secs().saturating_sub(model.observed_at) > HOOK_FEE_TTL_SECS
         }
      }
   }

   /// Fit the hook's cut from the chain, for one swap direction.
   ///
   /// Two probes. The small one sits where the pool's own price impact is negligible, so the ratio
   /// between fee-0 local math and the chain *is* the hook's cut; the reference one — the amount the
   /// quote will be used at — is used to check that the flat-cut model reproduces the chain. A hook
   /// that fails the check is stored as `linear: false` and never applied to other amounts.
   pub async fn fit_hook_fee<P, N>(
      &self,
      client: P,
      currency_in: &Currency,
      reference: U256,
   ) -> Result<HookFeeModel, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      if !self.has_swap_hooks() {
         return Err(anyhow!("Pool has no swap hooks"));
      }

      let state = self.state().v3_state().ok_or_else(|| anyhow!("State not initialized"))?.clone();
      let zero_for_one = self.zero_for_one(currency_in);

      let small = reference / U256::from(1_000u64);
      if small.is_zero() {
         return Err(anyhow!(
            "Amount {reference} is too small to fit a hook fee against"
         ));
      }

      let local_small = calculate_swap(&state, 0, zero_for_one, small)?;
      let chain_small = self.quote_hook_swap(client.clone(), currency_in, small).await?;
      let fee_pips = implied_fee_pips(local_small, chain_small)
         .ok_or_else(|| anyhow!("Local simulation returned nothing to fit against"))?;

      let model = HookFeeModel {
         fee_pips,
         linear: false,
         observed_at: now_secs(),
      };

      let local_reference = calculate_swap(
         &state,
         0,
         zero_for_one,
         model.input_after_cut(reference),
      )?;
      let chain_reference = self.quote_hook_swap(client, currency_in, reference).await?;
      let linear = !misses_by_more_than(
         local_reference,
         chain_reference,
         HOOK_FEE_LINEARITY_TOLERANCE_PIPS,
      );

      Ok(HookFeeModel { linear, ..model })
   }

   /// Ask the chain what this pool returns for an exact-input swap.
   ///
   /// The deployed `V4Quoter` runs the same `PoolManager` code the swap will, so it is the only
   /// trustworthy price for a pool whose hook has swap permissions. Store the result with
   /// [`Self::set_hook_quote`].
   pub async fn quote_hook_swap<P, N>(
      &self,
      client: P,
      currency_in: &Currency,
      amount_in: U256,
   ) -> Result<U256, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let quoter = address_book::uniswap_v4_quoter(self.chain_id)?;

      let params = QuoteExactInputSingleParams {
         poolKey: self.key().into(),
         zeroForOne: self.zero_for_one(currency_in),
         exactAmount: amount_in.try_into()?,
         hookData: Bytes::default(),
      };

      let contract = UniswapV4Quoter::new(quoter, client);
      let quote = contract.quoteExactInputSingle(params).call().await?;

      Ok(quote.amountOut)
   }
}

/// `keccak256(abi.encode(currency0, currency1, fee, tickSpacing, hooks))`.
///
/// Takes raw addresses so a decoded `Initialize` log can compute its pool id without
/// materialising a `Currency` (which needs token metadata).
pub fn v4_pool_id(
   currency_a: Address,
   currency_b: Address,
   fee: FeeAmount,
   tick_spacing: i32,
   hooks: Address,
) -> B256 {
   let (address0, address1) = if currency_a < currency_b {
      (currency_a, currency_b)
   } else {
      (currency_b, currency_a)
   };

   let spacing = FeeAmount::i24_tick_spacing(tick_spacing).unwrap_or_else(|| fee.tick_spacing());

   keccak256((address0, address1, fee.fee_u24(), spacing, hooks).abi_encode())
}

impl UniswapPool for UniswapV4Pool {
   fn chain_id(&self) -> u64 {
      self.chain_id
   }

   fn address(&self) -> Address {
      Address::ZERO
   }

   fn fee(&self) -> FeeAmount {
      self.fee
   }

   fn tick_spacing(&self) -> alloy_primitives::aliases::I24 {
      FeeAmount::i24_tick_spacing(self.resolved_tick_spacing())
         .unwrap_or_else(|| self.fee.tick_spacing())
   }

   fn tick_spacing_i32(&self) -> i32 {
      self.resolved_tick_spacing()
   }

   fn id(&self) -> B256 {
      v4_pool_id(
         self.currency0().address(),
         self.currency1().address(),
         self.fee,
         self.resolved_tick_spacing(),
         self.hooks,
      )
   }

   fn key(&self) -> PoolKey {
      let (address0, address1) = if self.currency0().address() < self.currency1().address() {
         (
            self.currency0().address(),
            self.currency1().address(),
         )
      } else {
         (
            self.currency1().address(),
            self.currency0().address(),
         )
      };

      PoolKey {
         currency0: address0,
         currency1: address1,
         fee: self.fee.fee_u24(),
         tickSpacing: self.tick_spacing(),
         hooks: self.hooks,
      }
   }

   fn dex_kind(&self) -> DexKind {
      self.dex
   }

   fn hooks(&self) -> Address {
      self.hooks
   }

   fn zero_for_one(&self, currency_in: &Currency) -> bool {
      currency_in.address() == self.currency0.address()
   }

   fn currency0(&self) -> &Currency {
      &self.currency0
   }

   fn currency1(&self) -> &Currency {
      &self.currency1
   }

   fn have(&self, currency: &Currency) -> bool {
      self.is_currency0(currency) || self.is_currency1(currency)
   }

   fn is_currency0(&self, currency: &Currency) -> bool {
      &self.currency0 == currency
   }

   fn is_currency1(&self, currency: &Currency) -> bool {
      &self.currency1 == currency
   }

   fn state(&self) -> &State {
      &self.state
   }

   fn set_state(&mut self, state: State) {
      self.state = state;
   }

   fn set_state_res(&mut self, state: State) -> Result<(), anyhow::Error> {
      if state.is_v3() {
         self.state = state;
         Ok(())
      } else {
         Err(anyhow::anyhow!("Pool state is not for v4"))
      }
   }

   fn base_currency_exists(&self) -> bool {
      self.currency0().is_base() || self.currency1().is_base()
   }

   fn base_currency(&self) -> &Currency {
      if self.currency0.is_base() {
         &self.currency0
      } else {
         &self.currency1
      }
   }

   fn quote_currency(&self) -> &Currency {
      if self.currency0.is_base() {
         &self.currency1
      } else {
         &self.currency0
      }
   }

   fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one(currency_in);
      let price = calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   fn pool_balances(&self) -> (NumericValue, NumericValue) {
      let amount0 = NumericValue::format_wei(
         self.liquidity_amount0,
         self.currency0().decimals(),
      );
      let amount1 = NumericValue::format_wei(
         self.liquidity_amount1,
         self.currency1().decimals(),
      );
      (amount0, amount1)
   }

   fn base_balance(&self) -> NumericValue {
      if self.currency0().is_base() {
         NumericValue::format_wei(
            self.liquidity_amount0,
            self.currency0().decimals(),
         )
      } else {
         NumericValue::format_wei(
            self.liquidity_amount1,
            self.currency1().decimals(),
         )
      }
   }

   fn quote_balance(&self) -> NumericValue {
      if self.currency0().is_base() {
         NumericValue::format_wei(
            self.liquidity_amount1,
            self.currency1().decimals(),
         )
      } else {
         NumericValue::format_wei(
            self.liquidity_amount0,
            self.currency0().decimals(),
         )
      }
   }

   fn compute_virtual_reserves(&mut self) -> Result<(), anyhow::Error> {
      let state = self
         .state()
         .v3_state()
         .ok_or_else(|| anyhow::anyhow!("Pool state has not been initialized"))?;

      let liquidity = state.liquidity;

      if liquidity == 0 {
         self.liquidity_amount0 = U256::ZERO;
         self.liquidity_amount1 = U256::ZERO;
         return Ok(());
      }

      let sqrt_price_x96 = state.sqrt_price;

      if sqrt_price_x96.is_zero() {
         return Err(anyhow::anyhow!(
            "Invalid state: sqrt_price is zero"
         ));
      }

      let liquidity_u256 = U256::from(liquidity);

      // virtual amount of token0
      let amount0 = (liquidity_u256 * Q96) / sqrt_price_x96;

      // virtual amount of token1
      let amount1 = (liquidity_u256 * sqrt_price_x96) / Q96;

      // let amnt0 = NumericValue::format_wei(amount0, self.currency0().decimals());
      // let amnt1 = NumericValue::format_wei(amount1, self.currency1().decimals());

      /*
      eprintln!(
         "Pool {} / {} ({}) Virtual Reserves {} {} - {} {}",
         self.quote_currency().symbol(),
         self.base_currency().symbol(),
         self.fee().fee_percent(),
         self.currency0().symbol(),
         amnt0.abbreviated(),
         self.currency1().symbol(),
         amnt1.abbreviated()
      );
      */

      self.liquidity_amount0 = amount0;
      self.liquidity_amount1 = amount1;

      Ok(())
   }

   async fn update_state<P, N>(
      &mut self,
      client: P,
      block: Option<BlockId>,
   ) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let state = get_v4_pool_state(client, self, block).await?;
      self.set_state(state);
      self.compute_virtual_reserves()?;
      Ok(())
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let fee = self.fee.fee_percent();
      if fee > MAX_FEE {
         return Err(anyhow!(
            "Pool Fee {} exceeds max fee {}",
            fee,
            MAX_FEE
         ));
      }

      if self.has_swap_hooks() {
         if let Some(amount_out) = self.hook_quote_out(currency_in, amount_in) {
            return Ok(amount_out);
         }

         let (zero_for_one, amount_in) = self.hook_fee_swap_amount(currency_in, amount_in)?;
         let fee = 0;
         let state = self.state().v3_state().ok_or(anyhow!("State not initialized"))?;
         return calculate_swap(state, fee, zero_for_one, amount_in);
      }

      let zero_for_one = self.zero_for_one(currency_in);
      let fee = self.fee.fee();
      let state = self.state().v3_state().ok_or(anyhow!("State not initialized"))?;
      let amount_out = calculate_swap(state, fee, zero_for_one, amount_in)?;
      Ok(amount_out)
   }

   fn simulate_swap_mut(
      &mut self,
      currency_in: &Currency,
      amount_in: U256,
   ) -> Result<U256, anyhow::Error> {
      let fee = self.fee.fee_percent();
      if fee > MAX_FEE {
         return Err(anyhow!(
            "Pool Fee {} exceeds max fee {}",
            fee,
            MAX_FEE
         ));
      }

      if self.has_swap_hooks() {
         if let Some(amount_out) = self.hook_quote_out(currency_in, amount_in) {
            return Ok(amount_out);
         }

         let (zero_for_one, amount_in) = self.hook_fee_swap_amount(currency_in, amount_in)?;
         let fee = 0;
         let state = self.state_mut().v3_state_mut().ok_or(anyhow!("State not initialized"))?;
         return calculate_swap_mut(state, fee, zero_for_one, amount_in);
      }

      let zero_for_one = self.zero_for_one(currency_in);
      let fee = self.fee.fee();
      let state = self.state_mut().v3_state_mut().ok_or(anyhow!("State not initialized"))?;
      let amount_out = calculate_swap_mut(state, fee, zero_for_one, amount_in)?;

      Ok(amount_out)
   }

   fn simulate_swap_result(
      &self,
      currency_in: &Currency,
      currency_out: &Currency,
      amount_in: NumericValue,
   ) -> Result<SwapResult, anyhow::Error> {
      let fee = self.fee.fee_percent();
      if fee > MAX_FEE {
         return Err(anyhow::anyhow!(
            "Pool Fee {} exceeds max fee {}",
            fee,
            MAX_FEE
         ));
      }

      let amount_out = self.simulate_swap(currency_in, amount_in.wei())?;
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());
      let spot_price = self.calculate_price(currency_in)?;

      let fee_fraction = self.fee().fee_percent() as f64 / 100.0;
      let amount_in_after_fee = amount_in.f64() * (1.0 - fee_fraction);
      let ideal_amount_out = amount_in_after_fee * spot_price;

      let price_impact = (1.0 - (amount_out.f64() / ideal_amount_out)) * 100.0;

      Ok(SwapResult {
         amount_in,
         amount_out,
         ideal_amount_out: NumericValue::parse_to_wei(
            &ideal_amount_out.to_string(),
            currency_out.decimals(),
         ),
         price_impact,
      })
   }

   fn quote_price(&self, base_usd: f64) -> Result<f64, anyhow::Error> {
      if base_usd == 0.0 {
         return Ok(0.0);
      }

      if !self.base_currency_exists() {
         bail!("Base token not found in the pool");
      }

      let base = self.base_currency();
      let unit = parse_units("1", base.decimals())?.get_absolute();
      let amount_out = self.simulate_swap(base, unit)?;
      if amount_out == U256::ZERO {
         return Ok(0.0);
      }

      let amount_out =
         format_units(amount_out, self.quote_currency().decimals())?.parse::<f64>()?;
      let price = base_usd / amount_out;

      Ok(price)
   }

   /// Get the usd value of Base and Quote token at a given block
   /// If block is None, the latest block is used
   ///
   /// ## Returns
   ///
   /// - (base_price, quote_price)
   async fn tokens_price<P, N>(
      &self,
      client: P,
      block: Option<BlockId>,
   ) -> Result<(f64, f64), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      if !self.base_currency_exists() {
         bail!("Base token not found in the pool");
      }

      let base = self.base_currency().to_erc20();
      let base_price = get_base_token_price(client.clone(), base.into_owned(), block).await?;
      let quote_price = self.quote_price(base_price)?;
      Ok((base_price, quote_price))
   }
}

// Well known pools for quick testing

impl UniswapV4Pool {
   pub fn eth_uni() -> Self {
      let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
      let uni = ERC20Token {
         chain_id: 1,
         address: uni_addr,
         decimals: 18,
         symbol: "UNI".into(),
         name: "Uniswap Token".into(),
         total_supply: U256::ZERO,
      };
      let currency_a = Currency::from(uni);
      let currency_b = Currency::from(NativeCurrency::from(1));
      let fee = FeeAmount::MEDIUM;

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn uni_usdc() -> Self {
      let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
      let uni = ERC20Token {
         chain_id: 1,
         address: uni_addr,
         decimals: 18,
         symbol: "UNI".into(),
         name: "Uniswap Token".into(),
         total_supply: U256::ZERO,
      };
      let currency_a = Currency::from(uni);
      let currency_b = Currency::from(ERC20Token::usdc());
      let fee = FeeAmount::MEDIUM;

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn usdc_usdt() -> Self {
      let currency_a = Currency::from(ERC20Token::usdc());
      let currency_b = Currency::from(ERC20Token::usdt());
      let fee = FeeAmount::CUSTOM(10); // 0.001%

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn eth_usdt() -> Self {
      let currency_a = Currency::from(NativeCurrency::from(1));
      let currency_b = Currency::from(ERC20Token::usdt());
      let fee = FeeAmount::MEDIUM;

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   /// Mainnet ETH/USDC 0.05% V4 pool (currency0 = native ETH).
   pub fn eth_usdc() -> Self {
      let currency_a = Currency::from(NativeCurrency::from(1));
      let currency_b = Currency::from(ERC20Token::usdc());
      let fee = FeeAmount::LOW;

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn link_usdc() -> Self {
      let currency_a = Currency::from(ERC20Token::link());
      let currency_b = Currency::from(ERC20Token::usdc());
      let fee = FeeAmount::MEDIUM;

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn eth_virtual() -> Self {
      let currency_a = Currency::from(NativeCurrency::from(8453));
      let currency_b = ERC20Token {
         chain_id: 8453,
         address: address!("0x0b3e328455c4059EEb9e3f84b5543F74E24e7E1b"),
         decimals: 18,
         symbol: "VIRTUAL".into(),
         name: "Virtual Protocol".into(),
         total_supply: U256::ZERO,
      };

      // On-chain PoolKey (Initialize at Base block 25689401):
      // fee=500, tickSpacing=10, hooks=0. Swap logs emit fee=625 (charged swap
      // fee, LP + protocol) — that is not the PoolKey fee and must not be hashed.
      let fee = FeeAmount::new(500);

      Self::from_components(
         8453,
         currency_a,
         currency_b.into(),
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn usdc_wbtc() -> Self {
      let currency_a = Currency::from(ERC20Token::usdc());
      let currency_b = Currency::from(ERC20Token::wbtc());
      let fee = FeeAmount::MEDIUM;

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn wbtc_usdt() -> Self {
      let currency_a = Currency::from(ERC20Token::usdt());
      let currency_b = Currency::from(ERC20Token::wbtc());
      let fee = FeeAmount::MEDIUM;

      Self::from_components(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::amm::uniswap::{V4_FEE_CANDIDATES, v4_fee_spacing_candidates};
   use alloy_primitives::B256;
   use alloy_provider::ProviderBuilder;
   use std::collections::HashMap;
   use std::str::FromStr;

   #[test]
   fn correct_pool_creation() {
      let pool1 = UniswapV4Pool::link_usdc();
      let id1 =
         B256::from_str("0x50ae33c238824aa1937d5d9f1766c487bca39b548f8d957994e8357eeeca3280")
            .unwrap();
      assert_eq!(pool1.id(), id1);
   }

   #[test]
   fn correct_pool_creation_virtual() {
      let pool1 = UniswapV4Pool::eth_virtual();
      let id1 =
         B256::from_str("0xc344274edb26787476d6176a4d6666e5c2c424f7f39d82646fe2713796df85db")
            .unwrap();
      assert_eq!(pool1.id(), id1);
   }

   /// A pool whose hook has swap permissions cannot be priced locally: the local simulator refuses
   /// it until the chain has answered, and then only for the amount it answered for.
   #[test]
   fn hook_pool_swaps_need_a_chain_quote() {
      let mut pool = UniswapV4Pool::eth_usdc();
      // `0x…80` sets the `beforeSwap` permission bit in the hook address.
      pool.hooks = address!("0x0000000000000000000000000000000000000080");
      let currency_in = pool.currency0().clone();
      let amount_in = U256::from(1_000_000u64);

      assert!(pool.has_swap_hooks());
      assert!(pool.simulate_swap(&currency_in, amount_in).is_err());

      pool.set_hook_quote(&currency_in, amount_in, U256::from(7u64));
      assert_eq!(
         pool.simulate_swap(&currency_in, amount_in).unwrap(),
         U256::from(7u64)
      );

      // The answer belongs to one amount, and to one side of the pool.
      assert!(pool.simulate_swap(&currency_in, amount_in + U256::ONE).is_err());
      assert!(
         pool.simulate_swap(pool.currency1(), amount_in).is_err(),
         "an answer for one side must not be replayed for the other"
      );
   }

   #[test]
   fn hookless_pool_needs_no_chain_quote() {
      assert!(!UniswapV4Pool::eth_usdc().has_swap_hooks());
   }

   #[test]
   fn hook_fee_fit_math() {
      assert_eq!(
         implied_fee_pips(U256::from(10_000u64), U256::from(9_900u64)),
         Some(100)
      );
      assert_eq!(
         implied_fee_pips(U256::from(10_000u64), U256::from(10_000u64)),
         Some(0)
      );
      // A hook that pays out: fee-0 local math is the conservative answer, not a negative cut.
      assert_eq!(
         implied_fee_pips(U256::from(10_000u64), U256::from(10_500u64)),
         Some(0)
      );
      assert_eq!(
         implied_fee_pips(U256::ZERO, U256::from(1u64)),
         None
      );

      let model = HookFeeModel {
         fee_pips: 100,
         linear: true,
         observed_at: 0,
      };
      assert_eq!(
         model.input_after_cut(U256::from(10_000u64)),
         U256::from(9_900u64)
      );

      assert!(!misses_by_more_than(
         U256::from(9_900u64),
         U256::from(9_910u64),
         25
      ));
      assert!(misses_by_more_than(
         U256::from(9_900u64),
         U256::from(10_500u64),
         25
      ));
   }

   /// A pool with one tick range and no initialized ticks around it — enough for the swap math.
   fn hook_pool_with_state() -> UniswapV4Pool {
      let mut pool = UniswapV4Pool::eth_usdc();
      pool.hooks = address!("0x0000000000000000000000000000000000000080");

      let state = crate::amm::uniswap::state::V3PoolState {
         fee_growth_global_0_x128: U256::ZERO,
         fee_growth_global_1_x128: U256::ZERO,
         liquidity: 1_000_000_000_000_000_000_000_000u128,
         sqrt_price: U256::from(100u64) * (U256::ONE << 96),
         tick: 0,
         word_pos: 0,
         tick_spacing: 60,
         tick_bitmap: HashMap::new(),
         ticks: HashMap::new(),
      };

      pool.set_state(State::v3(state));
      pool
   }

   /// A linear model is applied to fee-0 local math on the post-cut input; anything else keeps asking.
   #[test]
   fn hook_fee_model_prices_local_math() {
      let mut pool = hook_pool_with_state();
      let currency_in = pool.currency0().clone();
      let amount_in = U256::from(1_000_000_000_000_000_000u64);

      let no_cut = calculate_swap(
         pool.state().v3_state().unwrap(),
         0,
         true,
         amount_in,
      )
      .unwrap();
      assert!(!no_cut.is_zero());

      // `currency0` is native ETH here, so this is the `zero_for_one` direction.
      pool.hook_fees[1] = Some(HookFeeModel {
         fee_pips: 100,
         linear: true,
         observed_at: now_secs(),
      });

      let quoted = pool.simulate_swap(&currency_in, amount_in).unwrap();
      assert!(
         quoted < no_cut,
         "the hook's cut must lower the quote"
      );

      let after_cut = amount_in * U256::from(99u64) / U256::from(100u64);
      let expected = calculate_swap(
         pool.state().v3_state().unwrap(),
         0,
         true,
         after_cut,
      )
      .unwrap();
      assert_eq!(quoted, expected);

      // A hook whose cut depends on the amount is never applied to another amount.
      pool.hook_fees[1] = Some(HookFeeModel {
         fee_pips: 100,
         linear: false,
         observed_at: now_secs(),
      });
      assert!(pool.simulate_swap(&currency_in, amount_in).is_err());

      // The model is only trusted for a while.
      pool.hook_fees[1] = Some(HookFeeModel {
         fee_pips: 100,
         linear: true,
         observed_at: now_secs(),
      });
      assert!(!pool.hook_fee_is_stale(true));
      assert!(
         pool.hook_fee_is_stale(false),
         "the other side was never fitted"
      );

      pool.hook_fees[1] = Some(HookFeeModel {
         fee_pips: 100,
         linear: true,
         observed_at: 0,
      });
      assert!(pool.hook_fee_is_stale(true));
   }

   #[test]
   fn swap_event_fee_is_not_pool_key_fee() {
      // Same pair as eth_virtual(), but hashed with the Swap-event fee (625)
      // and its fee/50 tick spacing (12). That is the id the old fixture produced.
      let mut pool = UniswapV4Pool::eth_virtual();
      pool.fee = FeeAmount::new(625);
      pool.tick_spacing = 12;
      let swapped_fee_id =
         B256::from_str("0xdde064c3358b4d4f32b5edce5ce4b655ad8b65bd9b934087946eb0b8b55ae533")
            .unwrap();
      assert_eq!(pool.id(), swapped_fee_id);
      assert_ne!(pool.id(), UniswapV4Pool::eth_virtual().id());
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn can_swap() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      // Swap 1 ETH for UNI
      let base = pool.base_currency();
      let quote = pool.quote_currency();

      let amount_in = NumericValue::parse_to_wei("10", base.decimals());
      let swap_result = pool.simulate_swap_result(base, quote, amount_in.clone()).unwrap();

      println!("=== V4 Swap Test ===");
      println!(
         "Ideal Output: {:.6} {}",
         swap_result.ideal_amount_out.f64(),
         quote.symbol()
      );
      println!(
         "Swapped {} {} For {} {}",
         amount_in.f64(),
         base.symbol(),
         swap_result.amount_out.f64(),
         quote.symbol()
      );
      println!(
         "With Price Impact: {:.4}%",
         swap_result.price_impact
      );
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_virtual_reserves_uni_usdc() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      let mut pool = UniswapV4Pool::uni_usdc();
      pool.update_state(client.clone(), None).await.unwrap();

      let usdc_balance = pool.base_balance();
      let uni_balance = pool.quote_balance();

      println!("USDC Balance: {}", usdc_balance.f64());
      println!("UNI Balance: {}", uni_balance.f64());
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_base_token_liquidity_eth_uni() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let eth_balance = pool.base_balance();
      let uni_balance = pool.quote_balance();

      println!("ETH Balance: {}", eth_balance.f64());
      println!("UNI Balance: {}", uni_balance.f64());
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_base_token_liquidity_usdc_usdt() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      let mut pool = UniswapV4Pool::usdc_usdt();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let (balance0, balance1) = pool.pool_balances();

      eprintln!(
         "{} {}",
         pool.currency0().symbol(),
         balance0.abbreviated()
      );
      eprintln!(
         "{} {}",
         pool.currency1().symbol(),
         balance1.abbreviated()
      );
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_base_token_liquidity_usdc_wbtc() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      let mut pool = UniswapV4Pool::usdc_wbtc();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let (balance0, balance1) = pool.pool_balances();

      eprintln!(
         "{} {}",
         pool.currency0().symbol(),
         balance0.abbreviated()
      );
      eprintln!(
         "{} {}",
         pool.currency1().symbol(),
         balance1.abbreviated()
      );
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn price_calculation() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let (base_price, quote_price) = pool.tokens_price(client.clone(), None).await.unwrap();

      let uni_in_eth = pool.calculate_price(pool.quote_currency()).unwrap();
      let eth_in_uni = pool.calculate_price(pool.base_currency()).unwrap();
      let eth_balance = pool.base_balance();

      println!(
         "{} Price: ${}",
         pool.base_currency().symbol(),
         base_price
      );
      println!(
         "{} Price: ${}",
         pool.quote_currency().symbol(),
         quote_price
      );
      println!("UNI in terms of ETH: {}", uni_in_eth);
      println!("ETH in terms of UNI: {}", eth_in_uni);
      println!("ETH Liquidity: {}", eth_balance.f64());
   }

   #[test]
   fn fee_spacing_candidates_include_zero_fee_with_standard_spacings() {
      let candidates = v4_fee_spacing_candidates(&V4_FEE_CANDIDATES);

      assert!(candidates.contains(&(FeeAmount::new(0), 60)));
      assert!(candidates.contains(&(FeeAmount::new(0), 1)));
      assert!(candidates.contains(&(FeeAmount::new(3000), 60)));
      // The fee/50 heuristic is still probed for non-standard fees.
      assert_eq!(FeeAmount::new(3000).tick_spacing_i32(), 60);
   }

   #[test]
   fn pool_order() {
      let pool = UniswapV4Pool::eth_uni();

      let eth = pool.base_currency().clone();
      let uni = pool.quote_currency().clone();

      let currency0 = pool.is_currency0(&eth);
      let currency1 = pool.is_currency1(&uni);
      assert!(currency0);
      assert!(currency1);

      let base_exists = pool.base_currency_exists();
      assert!(base_exists);
   }
}
