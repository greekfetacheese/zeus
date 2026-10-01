use anyhow::anyhow;
use serde_json::Value;

use super::{misc::TimeStamp, simulate::AccountPrefetch, swap_quoter::SwapStep};
use crate::core::{ZeusCtx, signature::Permit2Info};
use zeus_eth::{
   abi::uniswap::{universal_router_v2::*, v4::actions::*},
   alloy_primitives::{Address, Bytes, U256, address},
   alloy_sol_types::SolValue,
   amm::uniswap::{UniswapPool, v4::Actions},
   currency::Currency,
   types::ChainId,
   utils::{NumericValue, address_book},
};
use zeus_wallet::SecureKey;

#[cfg(feature = "dev")]
use tracing::debug;

/// Universal Router `ActionConstants.CONTRACT_BALANCE` (`1 << 255`).
///
/// V2/V3 exact-in treat this as "swap the router's full token balance".
/// Intermediate hops must use it: a quoted amount that is even 1 wei above the
/// previous hop's actual output makes UR `safeTransfer` revert `TRANSFER_FAILED`.
const CONTRACT_BALANCE: U256 = U256::from_limbs([0, 0, 0, 0x8000_0000_0000_0000]);

/// Universal Router / V4 `ActionConstants.OPEN_DELTA` (0).
///
/// In V4 actions it means "the open delta": for a swap, the credit the payer already holds on the
/// PoolManager; for a settle, the debt the swap left; for a take, the full credit.
const OPEN_DELTA: U256 = U256::ZERO;

/// True when this hop spends the user's trade input (or its wrapped native).
///
/// Split slices of ETH→WETH after `WRAP_ETH` still spend trade input: they must
/// keep the quoted amount. [`CONTRACT_BALANCE`] is only for tokens produced by
/// an earlier hop (e.g. USDC after USDT→USDC). Using it on a WETH split slice
/// spends the entire wrapped balance; later V3 hops then revert `AS`.
fn spends_trade_input(step_in: &Currency, trade_in: &Currency) -> bool {
   if step_in == trade_in {
      return true;
   }
   (trade_in.is_native() && step_in.is_native_wrapped())
      || (trade_in.is_native_wrapped() && step_in.is_native())
}

/// Amount to encode for a V2/V3 exact-in hop.
fn v2_v3_amount_in(spends_trade_input: bool, quoted: U256) -> U256 {
   if spends_trade_input {
      quoted
   } else {
      CONTRACT_BALANCE
   }
}

// https://docs.uniswap.org/contracts/universal-router/technical-reference
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Commands {
   V3_SWAP_EXACT_IN = 0x00,
   V3_SWAP_EXACT_OUT = 0x01,
   PERMIT2_TRANSFER_FROM = 0x02,
   PERMIT2_PERMIT_BATCH = 0x03,
   SWEEP = 0x04,
   TRANSFER = 0x05,
   PAY_PORTION = 0x06,
   V2_SWAP_EXACT_IN = 0x08,
   V2_SWAP_EXACT_OUT = 0x09,
   PERMIT2_PERMIT = 0x0a,
   WRAP_ETH = 0x0b,
   UNWRAP_WETH = 0x0c,
   PERMIT2_TRANSFER_FROM_BATCH = 0x0d,
   BALANCE_CHECK_ERC20 = 0x0e,
   V4_SWAP = 0x10,
   V3_POSITION_MANAGER_PERMIT = 0x11,
   V3_POSITION_MANAGER_CALL = 0x12,
   V4_INITIALIZE_POOL = 0x13,
   V4_POSITION_MANAGER_CALL = 0x14,
   EXECUTE_SUB_PLAN = 0x21,
}

/// The result of [encode_swap]
pub struct SwapExecuteParams {
   pub call_data: Bytes,
   /// The eth to be sent along with the transaction
   pub value: U256,

   pub permit2_info: Option<Permit2Info>,
}

impl Default for SwapExecuteParams {
   fn default() -> Self {
      Self::new()
   }
}

impl SwapExecuteParams {
   pub fn new() -> Self {
      Self {
         call_data: Bytes::default(),
         value: U256::ZERO,
         permit2_info: None,
      }
   }

   pub fn set_call_data(&mut self, call_data: Bytes) {
      self.call_data = call_data;
   }

   pub fn set_value(&mut self, value: U256) {
      self.value = value;
   }

   pub fn set_permit2_info(&mut self, permit2_details: Option<Permit2Info>) {
      self.permit2_info = permit2_details;
   }

   pub fn permit2_needs_approval(&self) -> bool {
      if let Some(permit2_details) = &self.permit2_info {
         permit2_details.needs_approval
      } else {
         false
      }
   }

   pub fn needs_new_signature(&self) -> bool {
      if let Some(permit2_details) = &self.permit2_info {
         permit2_details.needs_new_signature
      } else {
         false
      }
   }

   pub fn message(&self) -> Result<Value, anyhow::Error> {
      if let Some(permit2_details) = &self.permit2_info {
         if let Some(msg) = &permit2_details.msg {
            Ok(msg.clone())
         } else {
            Err(anyhow!("Permit2 Details found but no message"))
         }
      } else {
         Err(anyhow!("No Permit2 Details found"))
      }
   }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum SwapType {
   /// Indicates that the swap is based on an exact input amount.
   ExactInput,

   /// Indicates that the swap is based on an exact output amount.
   ExactOutput,
}

impl SwapType {
   pub fn is_exact_input(&self) -> bool {
      matches!(self, Self::ExactInput)
   }

   pub fn is_exact_output(&self) -> bool {
      matches!(self, Self::ExactOutput)
   }
}

/// Everything [`encode_swap`] needs that the calldata cannot imply.
///
/// This replaces thirteen positional arguments — `currency_in`/`currency_out`
/// were adjacent [`Currency`]s and `amount_in`/`amount_out_min` adjacent
/// [`U256`]s, so a swapped pair compiled and only the fork simulation could
/// catch it. Every field is named at the call site instead.
///
/// There is deliberately no `Default` and no partial builder: the default
/// [`Currency`] is native ETH, a defaulted `amount_out_min` is zero (the
/// calldata would enforce no slippage floor while the confirm window still shows
/// the one the quote produced) and a defaulted `recipient` is the zero address.
/// Being able to omit any of those is the bug this type removes, so each field
/// is required.
pub struct SwapRequest<P: UniswapPool + Clone> {
   pub chain_id: u64,
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in: U256,
   /// The slippage floor the encoded calldata enforces. `U256::ZERO` only for
   /// the pre-simulation encode, whose whole purpose is to learn the amount out.
   pub amount_out_min: U256,
   pub slippage: f64,
   pub swap_type: SwapType,
   pub steps: Vec<SwapStep<P>>,
   pub signer: SecureKey,
   /// Where the swap's proceeds are swept.
   pub recipient: Address,
   pub deadline_minutes: u64,
   /// A Permit2 payload the caller already built — a signature, allowance or
   /// nonce that must not be recomputed. `None` builds one.
   pub permit2_info: Option<Permit2Info>,
}

/// Encode the calldata for a swap using the universal router
pub async fn encode_swap<P: UniswapPool + Clone>(
   ctx: ZeusCtx,
   req: SwapRequest<P>,
) -> Result<SwapExecuteParams, anyhow::Error> {
   let SwapRequest {
      chain_id,
      currency_in,
      currency_out,
      amount_in,
      amount_out_min,
      slippage,
      swap_type,
      steps: swap_steps,
      signer: secure_signer,
      recipient,
      deadline_minutes: deadline_in_minutes,
      permit2_info,
   } = req;

   if swap_steps.is_empty() {
      return Err(anyhow!("No swap steps provided"));
   }
   if !swap_type.is_exact_input() {
      return Err(anyhow!("Only support exact input"));
   }

   let owner = secure_signer.address();
   let router_addr = address_book::universal_router_v2(chain_id)?;
   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut execute_params = SwapExecuteParams::new();

   // Handle Permit2 approvals (signature only; transfer is in encode_swap_plan)
   if currency_in.is_erc20() {
      let token_in = currency_in.to_erc20();

      let permit_info = if let Some(info) = permit2_info {
         info
      } else {
         let info = Permit2Info::new(
            ctx.clone(),
            chain_id,
            &token_in,
            amount_in,
            owner,
            router_addr,
         )
         .await?;
         info
      };

      if permit_info.needs_new_signature {
         let signature = permit_info.sign(secure_signer).await?;

         let permit_input = encode_permit2_permit(
            token_in.address,
            amount_in,
            permit_info.expiration,
            permit_info.allowance.nonce,
            router_addr,
            permit_info.sig_deadline,
            signature,
         );

         commands.push(Commands::PERMIT2_PERMIT as u8);
         inputs.push(permit_input);
      }

      execute_params.set_permit2_info(Some(permit_info));
   }

   let plan = encode_swap_plan(
      chain_id,
      &swap_steps,
      swap_type,
      amount_in,
      amount_out_min,
      slippage,
      &currency_in,
      &currency_out,
      recipient,
   )?;

   commands.extend(plan.commands);
   inputs.extend(plan.inputs);
   execute_params.set_value(plan.value);

   let command_bytes = Bytes::from(commands);
   #[cfg(feature = "dev")]
   debug!("Command Bytes: {:?}", command_bytes);

   let deadline = TimeStamp::now_as_secs()?.saturating_add_secs(deadline_in_minutes * 60);
   let data = encode_execute_with_deadline(
      command_bytes,
      inputs,
      U256::from(deadline.timestamp()),
   );

   execute_params.set_call_data(data);

   Ok(execute_params)
}

struct SwapPlan {
   commands: Vec<u8>,
   inputs: Vec<Bytes>,
   value: U256,
}

fn sum_amount_in<P: UniswapPool>(
   steps: &[SwapStep<P>],
   pred: impl Fn(&SwapStep<P>) -> bool,
) -> U256 {
   steps.iter().filter(|s| pred(s)).map(|s| s.amount_in.wei()).sum()
}

/// Quoted ETH/WETH left on the router after the hops (ignores WRAP/UNWRAP cmds).
fn quoted_router_eth_weth<P: UniswapPool>(steps: &[SwapStep<P>]) -> (U256, U256) {
   let mut eth = U256::ZERO;
   let mut weth = U256::ZERO;
   for swap in steps {
      if swap.currency_in.is_native() && eth >= swap.amount_in.wei() {
         eth -= swap.amount_in.wei();
      }
      if swap.currency_in.is_native_wrapped() && weth >= swap.amount_in.wei() {
         weth -= swap.amount_in.wei();
      }
      if swap.currency_out.is_native() {
         eth += swap.amount_out.wei();
      }
      if swap.currency_out.is_native_wrapped() {
         weth += swap.amount_out.wei();
      }
   }
   (eth, weth)
}

/// True when this hop's input token is WETH, so the router — not the user — must hold it.
///
/// A native-ETH trade has to `WRAP_ETH` every such slice first, whatever the dex: the quoter
/// treats WETH and ETH as one asset, so the hop can be the WETH pool of a V4 dex (only the
/// native pool of a V4 dex is settled straight from `msg.value`). V2/V3 pools are always WETH.
///
/// The same predicate restores the slices after [`Commands::UNWRAP_WETH`], which unwraps the
/// router's *entire* WETH balance.
fn needs_weth_balance<P: UniswapPool>(step: &SwapStep<P>) -> bool {
   step.currency_in.is_native_wrapped()
}

/// A hop that takes native ETH out of the router's `msg.value` (a V4 native pool).
fn needs_v4_native_unwrap<P: UniswapPool>(step: &SwapStep<P>) -> bool {
   step.currency_in.is_native() && step.pool.dex_kind().is_uniswap_v4()
}

/// Command sequence without Permit2 signature or deadline wrapping.
fn encode_swap_plan(
   chain_id: u64,
   swap_steps: &[SwapStep<impl UniswapPool + Clone>],
   swap_type: SwapType,
   amount_in: U256,
   amount_out_min: U256,
   slippage: f64,
   currency_in: &Currency,
   currency_out: &Currency,
   recipient: Address,
) -> Result<SwapPlan, anyhow::Error> {
   let router_addr = address_book::universal_router_v2(chain_id)?;
   let router_version = address_book::universal_router_version(chain_id)?;
   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut value = U256::ZERO;

   if currency_in.is_native() {
      value = amount_in;
      let amount_to_wrap = sum_amount_in(swap_steps, needs_weth_balance);
      if amount_to_wrap > U256::ZERO {
         commands.push(Commands::WRAP_ETH as u8);
         inputs.push(encode_wrap_eth(router_addr, amount_to_wrap));
      }
   }

   let first_step_uses_permit2 = currency_in.is_erc20();
   if first_step_uses_permit2 {
      commands.push(Commands::PERMIT2_TRANSFER_FROM as u8);
      inputs.push(encode_permit2_transfer_from(
         currency_in.to_erc20().address,
         router_addr,
         amount_in,
      ));
   }

   // WETH in + V4 native hops: Permit2 left WETH on the router, V4 SETTLE wants ETH.
   // UNWRAP_WETH unwraps the entire WETH balance, so re-WRAP every slice that needs WETH back.
   if currency_in.is_native_wrapped() {
      let amount_to_unwrap = sum_amount_in(swap_steps, needs_v4_native_unwrap);
      if amount_to_unwrap > U256::ZERO {
         commands.push(Commands::UNWRAP_WETH as u8);
         inputs.push(encode_unwrap_weth(router_addr, amount_to_unwrap));

         let amount_to_wrap = sum_amount_in(swap_steps, needs_weth_balance);
         if amount_to_wrap > U256::ZERO {
            commands.push(Commands::WRAP_ETH as u8);
            inputs.push(encode_wrap_eth(router_addr, amount_to_wrap));
         }
      }
   }

   let weth = Currency::wrapped_native(chain_id);

   for swap in swap_steps {
      #[cfg(feature = "dev")]
      {
         debug!("|=== Swap Step ===|");
         debug!(
            "Swap Step: {} {} -> {} {} {} ({}) {} {}",
            swap.amount_in.abbreviated(),
            swap.currency_in.symbol(),
            swap.amount_out.abbreviated(),
            swap.currency_out.symbol(),
            swap.pool.dex_kind().as_str(),
            swap.pool.fee().fee_percent(),
            swap.pool.address(),
            swap.pool.id(),
         );
      }

      let uses_initial_funds = swap.currency_in == *currency_in;
      let amount_is_trade_input = spends_trade_input(&swap.currency_in, currency_in);
      let payer_is_user = uses_initial_funds && !first_step_uses_permit2;
      let step_amount_out_min = U256::ZERO;

      let step_currency_in = if swap.currency_in.is_native() {
         &weth
      } else {
         &swap.currency_in
      };

      if swap.pool.dex_kind().is_uniswap_v2() {
         let path = vec![step_currency_in.address(), swap.currency_out.address()];
         let input = encode_v2_swap_exact_in(
            router_addr,
            v2_v3_amount_in(amount_is_trade_input, swap.amount_in.wei()),
            step_amount_out_min,
            path,
            payer_is_user,
         )?;
         commands.push(Commands::V2_SWAP_EXACT_IN as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v3() {
         let path = vec![step_currency_in.address(), swap.currency_out.address()];
         let fees = vec![swap.pool.fee().fee_u24()];
         let input = encode_v3_swap_exact_in(
            router_addr,
            v2_v3_amount_in(amount_is_trade_input, swap.amount_in.wei()),
            step_amount_out_min,
            path,
            fees,
            payer_is_user,
         )?;
         commands.push(Commands::V3_SWAP_EXACT_IN as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v4() {
         let input = encode_v4_internal_actions(
            router_version,
            &swap.pool,
            swap_type,
            &swap.currency_in,
            &swap.currency_out,
            swap.amount_in.wei(),
            step_amount_out_min,
            router_addr,
            payer_is_user,
            amount_is_trade_input,
         )?;
         commands.push(Commands::V4_SWAP as u8);
         inputs.push(input);
      }
   }

   let (router_eth_balance, router_weth_balance) = quoted_router_eth_weth(swap_steps);
   let ur_has_weth_balance = router_weth_balance > U256::ZERO;
   let ur_has_eth_balance = router_eth_balance > U256::ZERO;
   let mut should_sweep = true;

   if currency_out.is_native_wrapped() && ur_has_eth_balance {
      commands.push(Commands::WRAP_ETH as u8);
      inputs.push(encode_wrap_eth(router_addr, CONTRACT_BALANCE));
   }

   if currency_out.is_native() && ur_has_weth_balance && !ur_has_eth_balance {
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(encode_unwrap_weth(recipient, amount_out_min));
      should_sweep = false;
   }

   if currency_out.is_native() && ur_has_weth_balance && ur_has_eth_balance {
      let weth_amount = NumericValue::format_wei(router_weth_balance, currency_out.decimals());
      let amount_min = weth_amount.calc_slippage(slippage, currency_out.decimals());
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(encode_unwrap_weth(router_addr, amount_min.wei()));
   }

   if should_sweep {
      let sweep_params = Sweep {
         token: currency_out.address(),
         recipient,
         amountMin: amount_out_min,
      };

      #[cfg(feature = "dev")]
      debug!("Sweep Params: {:?}", sweep_params);

      commands.push(Commands::SWEEP as u8);
      inputs.push(sweep_params.abi_encode_params().into());
   }

   Ok(SwapPlan {
      commands,
      inputs,
      value,
   })
}

fn encode_v4_internal_actions(
   router_version: address_book::UniversalRouterVersion,
   pool: &impl UniswapPool,
   swap_type: SwapType,
   currency_in: &Currency,
   currency_out: &Currency,
   amount_in: U256,
   amount_out_min: U256,
   router_addr: Address,
   payer_is_user: bool,
   amount_is_trade_input: bool,
) -> Result<Bytes, anyhow::Error> {
   // A hop fed by an earlier hop cannot put its input in wei into the calldata: the quote is an
   // estimate, and a multi-tick V3 hop lands below it. V4 actions work on PoolManager deltas rather
   // than balances, so the UR's `CONTRACT_BALANCE` is what makes such a hop independent of the quote:
   // settle the router's whole balance of the input token (which is exactly what the previous hop
   // delivered), then swap the credit that settle just created (`OPEN_DELTA`). Settling the quoted
   // amount instead asks the pool for more than the router holds — `TRANSFER_FAILED`.
   //
   // `CONTRACT_BALANCE` means "everything of this token this router holds", the same caveat the
   // V2/V3 rule accepts: correct as long as nothing else of that token is already sitting in the
   // router, which is why the trade input (and its wrapped form, shared across split slices) is
   // settled by amount instead.
   let from_router_balance = swap_type.is_exact_input() && !amount_is_trade_input;

   // OPEN_DELTA for both: spend the settled credit, take whatever the swap produced.
   let swap_amount_in = if from_router_balance {
      OPEN_DELTA
   } else {
      amount_in
   };
   let swap_amount_out_min = if from_router_balance {
      OPEN_DELTA
   } else {
      amount_out_min
   };

   let (swap_command, swap_input) = encode_v4_swap_single_command_input(
      router_version,
      pool,
      swap_type,
      currency_in,
      swap_amount_in,
      swap_amount_out_min,
   )?;

   // Settle tells the V4 contract how to receive the input tokens
   let settle = SettleParams {
      currency: currency_in.address(),
      amount: if from_router_balance {
         CONTRACT_BALANCE
      } else {
         amount_in
      },
      payerIsUser: payer_is_user && !from_router_balance,
   };

   let settle_action = Actions::SETTLE(settle);
   let settle_input = settle_action.abi_encode();

   // A zero take amount is `OPEN_DELTA`: take the whole credit, whatever the pool produced. The
   // user's floor is the sweep's `amountMin`, not a per-hop minimum.
   let take_params = TakeParams {
      currency: currency_out.address(),
      recipient: router_addr,
      amount: amount_out_min,
   };

   let take_action = Actions::TAKE(take_params);
   let take_input = take_action.abi_encode();

   // A balance-fed hop settles *before* it swaps: the settle is what gives the router the credit the
   // swap then spends. Every other shape swaps first and settles the amount it consumed.
   let (v4_commands, v4_action_params) = if from_router_balance {
      (
         vec![settle_action.command(), swap_command, take_action.command()],
         vec![settle_input, swap_input, take_input],
      )
   } else {
      (
         vec![swap_command, settle_action.command(), take_action.command()],
         vec![swap_input, settle_input, take_input],
      )
   };

   encode_v4_router_command_input(v4_commands, v4_action_params)
}

fn encode_v4_swap_single_command_input(
   router_version: address_book::UniversalRouterVersion,
   pool: &impl UniswapPool,
   swap_type: SwapType,
   currency_in: &Currency,
   amount_in: U256,
   amount_out: U256,
) -> Result<(u8, Bytes), anyhow::Error> {
   use address_book::UniversalRouterVersion;

   // `Actions` is consulted only for the action id — the params come from whichever struct layout
   // the deployed router decodes (the generation is a per-chain deployment fact).
   let command = if swap_type.is_exact_input() {
      Actions::SWAP_EXACT_IN_SINGLE(Default::default()).command()
   } else {
      Actions::SWAP_EXACT_OUT_SINGLE(Default::default()).command()
   };

   let params_bytes: Bytes = match (swap_type.is_exact_input(), router_version) {
      (true, UniversalRouterVersion::Legacy) => ExactInputSingleParams {
         poolKey: pool.key(),
         zeroForOne: pool.zero_for_one(currency_in),
         amountIn: amount_in.try_into()?,
         amountOutMinimum: amount_out.try_into()?,
         hookData: Bytes::default(),
      }
      .abi_encode(),
      (true, UniversalRouterVersion::MinHopPriceX36) => ExactInputSingleParamsMinHopPrice {
         poolKey: pool.key(),
         zeroForOne: pool.zero_for_one(currency_in),
         amountIn: amount_in.try_into()?,
         amountOutMinimum: amount_out.try_into()?,
         // No per-hop price guard: the sweep's amountMin is the slippage floor.
         minHopPriceX36: U256::ZERO,
         hookData: Bytes::default(),
      }
      .abi_encode(),
      (false, UniversalRouterVersion::Legacy) => ExactOutputSingleParams {
         poolKey: pool.key(),
         zeroForOne: pool.zero_for_one(currency_in),
         amountOut: amount_out.try_into()?,
         amountInMaximum: amount_in.try_into()?,
         hookData: Bytes::default(),
      }
      .abi_encode(),
      (false, UniversalRouterVersion::MinHopPriceX36) => ExactOutputSingleParamsMinHopPrice {
         poolKey: pool.key(),
         zeroForOne: pool.zero_for_one(currency_in),
         amountOut: amount_out.try_into()?,
         amountInMaximum: amount_in.try_into()?,
         minHopPriceX36: U256::ZERO,
         hookData: Bytes::default(),
      }
      .abi_encode(),
   }
   .into();

   Ok((command, params_bytes))
}

/// Encodes the input for the Universal Router's V4_SWAP command (0x10).
/// This input is itself an ABI-encoded tuple: (bytes actions, bytes[] params)
fn encode_v4_router_command_input(
   v4_commands: Vec<u8>,
   v4_action_params: Vec<Bytes>,
) -> Result<Bytes, anyhow::Error> {
   if v4_commands.len() != v4_action_params.len() {
      return Err(anyhow::anyhow!(
         "V4 actions and params length mismatch: {} != {}",
         v4_commands.len(),
         v4_action_params.len()
      ));
   }

   let actions_bytes = Bytes::from(v4_commands);

   let params = ActionsParams {
      actions: actions_bytes,
      params: v4_action_params,
   }
   .abi_encode_params();

   Ok(params.into())
}

/// Accounts a Universal Router swap touches: the signer, the router stack, the traded tokens, the
/// burn address Base/Optimism require, and every hop pool.
///
/// Shared with the swap regressions in `src/tests/swap.rs` so a test prefetches exactly what the app
/// prefetches: an account missing here is an RPC call in the middle of the simulation.
pub fn swap_prefetch_accounts<P: UniswapPool>(
   chain: ChainId,
   signer_address: Address,
   router_addr: Address,
   permit2_addr: Address,
   beneficiary: Address,
   currency_in: &Currency,
   currency_out: &Currency,
   swap_steps: &[SwapStep<P>],
) -> Vec<AccountPrefetch> {
   let first_pool = &swap_steps.first().unwrap().pool;
   let last_pool = &swap_steps.last().unwrap().pool;
   let burn_addr = address!("0000000000000000000000000000000000000001");

   let mut accounts = Vec::new();
   accounts.push(AccountPrefetch::eoa(signer_address));
   accounts.push(AccountPrefetch::contract(router_addr));
   accounts.push(AccountPrefetch::contract(permit2_addr));
   accounts.push(AccountPrefetch::eoa(beneficiary));

   if currency_in.is_erc20() {
      accounts.push(AccountPrefetch::contract(currency_in.address()));

      if chain.is_base() || chain.is_optimism() {
         accounts.push(AccountPrefetch::contract(burn_addr));
      }
   }

   if currency_in.is_native() && !first_pool.dex_kind().is_v4() {
      accounts.push(AccountPrefetch::contract(
         currency_in.to_erc20().address,
      ));
   }

   if currency_out.is_erc20() {
      accounts.push(AccountPrefetch::contract(currency_out.address()));
   }

   if currency_out.is_native() && !last_pool.dex_kind().is_v4() {
      accounts.push(AccountPrefetch::contract(
         currency_out.to_erc20().address,
      ));
   }

   let pools_addr = swap_steps.iter().map(|s| s.pool.address()).collect::<Vec<_>>();
   for pool in pools_addr {
      if !pool.is_zero() {
         accounts.push(AccountPrefetch::contract(pool));
      }
   }

   accounts
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{
      alloy_primitives::address,
      amm::uniswap::{AnyUniswapPool, DexKind, FeeAmount, UniswapV3Pool, UniswapV4Pool},
      currency::{Currency, ERC20Token, NativeCurrency},
   };

   fn eth() -> Currency {
      Currency::from(NativeCurrency::from(1u64))
   }

   fn weth() -> Currency {
      Currency::wrapped_native(1)
   }

   fn usdc() -> Currency {
      Currency::from(ERC20Token::usdc())
   }

   fn usdt() -> Currency {
      Currency::from(ERC20Token::usdt())
   }

   fn recipient() -> Address {
      address!("2222222222222222222222222222222222222222")
   }

   fn wei(amount: &str, decimals: u8) -> NumericValue {
      NumericValue::parse_to_wei(amount, decimals)
   }

   fn v3_weth_usdc() -> UniswapV3Pool {
      UniswapV3Pool::new(
         1,
         address!("1111111111111111111111111111111111111111"),
         500,
         ERC20Token::weth(),
         ERC20Token::usdc(),
         DexKind::UniswapV3,
      )
   }

   /// The WETH pool of the same pair the `eth_usdc()` V4 pool quotes — the quoter treats WETH and
   /// native ETH as one asset, so an ETH trade can be routed through this one.
   fn v4_weth_usdc() -> UniswapV4Pool {
      UniswapV4Pool::from_components(
         1,
         weth(),
         usdc(),
         FeeAmount::LOW,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   fn v4_usdc_usdt() -> UniswapV4Pool {
      UniswapV4Pool::from_components(
         1,
         usdc(),
         usdt(),
         FeeAmount::LOW,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   fn step(
      pool: impl Into<AnyUniswapPool>,
      cin: Currency,
      cout: Currency,
      amt_in: NumericValue,
      amt_out: NumericValue,
   ) -> SwapStep<AnyUniswapPool> {
      SwapStep::new(pool.into(), amt_in, amt_out, cin, cout)
   }

   fn plan(
      currency_in: Currency,
      currency_out: Currency,
      amount_in: U256,
      steps: Vec<SwapStep<AnyUniswapPool>>,
   ) -> SwapPlan {
      plan_on_chain(1, currency_in, currency_out, amount_in, steps)
   }

   fn plan_on_chain(
      chain_id: u64,
      currency_in: Currency,
      currency_out: Currency,
      amount_in: U256,
      steps: Vec<SwapStep<AnyUniswapPool>>,
   ) -> SwapPlan {
      encode_swap_plan(
         chain_id,
         &steps,
         SwapType::ExactInput,
         amount_in,
         U256::from(1u64),
         0.5,
         &currency_in,
         &currency_out,
         recipient(),
      )
      .unwrap()
   }

   fn assert_commands(got: &SwapPlan, expected: &[Commands]) {
      let exp: Vec<u8> = expected.iter().map(|c| *c as u8).collect();
      assert_eq!(
         got.commands, exp,
         "got {:#04x?} expected {:#04x?}",
         got.commands, exp
      );
   }

   fn nth_input(got: &SwapPlan, cmd: Commands, nth: usize) -> &Bytes {
      got.commands
         .iter()
         .enumerate()
         .filter(|(_, c)| **c == cmd as u8)
         .nth(nth)
         .map(|(i, _)| &got.inputs[i])
         .expect("command missing from plan")
   }

   #[test]
   fn initial_v2_v3_hop_uses_quoted_amount() {
      let quoted = U256::from(499_993_647_689u64);
      assert_eq!(v2_v3_amount_in(true, quoted), quoted);
   }

   #[test]
   fn intermediate_v2_v3_hop_uses_contract_balance() {
      let quoted = U256::from(499_993_647_689u64);
      assert_eq!(v2_v3_amount_in(false, quoted), CONTRACT_BALANCE);
      assert_eq!(CONTRACT_BALANCE, U256::from(1u8) << 255);
   }

   #[test]
   fn eth_in_weth_split_slice_is_trade_input() {
      assert!(spends_trade_input(&weth(), &eth()));
      assert!(spends_trade_input(&eth(), &eth()));
      assert!(!spends_trade_input(&usdc(), &eth()));
   }

   #[test]
   fn weth_in_native_v4_hop_is_trade_input() {
      assert!(spends_trade_input(&eth(), &weth()));
      assert!(spends_trade_input(&weth(), &weth()));
   }

   #[test]
   fn usdt_to_usdc_hop_is_not_trade_input() {
      assert!(!spends_trade_input(&usdc(), &usdt()));
      assert!(spends_trade_input(&usdt(), &usdt()));
   }

   #[test]
   fn weth_to_usdc_via_v4_native_unwraps_before_swap() {
      let amount_in = wei("1", 18);
      let got = plan(
         weth(),
         usdc(),
         amount_in.wei(),
         vec![step(
            UniswapV4Pool::eth_usdc(),
            eth(),
            usdc(),
            amount_in.clone(),
            wei("2500", 6),
         )],
      );

      assert_eq!(got.value, U256::ZERO);
      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::UNWRAP_WETH,
            Commands::V4_SWAP,
            Commands::SWEEP,
         ],
      );

      let unwrap =
         UnwrapWeth::abi_decode_params(nth_input(&got, Commands::UNWRAP_WETH, 0)).unwrap();
      assert_eq!(unwrap.amountMin, amount_in.wei());
   }

   #[test]
   fn usdc_to_weth_via_v4_native_wraps_before_sweep() {
      let amount_in = wei("2500", 6);
      let got = plan(
         usdc(),
         weth(),
         amount_in.wei(),
         vec![step(
            UniswapV4Pool::eth_usdc(),
            usdc(),
            eth(),
            amount_in,
            wei("1", 18),
         )],
      );

      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::V4_SWAP,
            Commands::WRAP_ETH,
            Commands::SWEEP,
         ],
      );

      let wrap = WrapEth::abi_decode_params(nth_input(&got, Commands::WRAP_ETH, 0)).unwrap();
      assert_eq!(wrap.amount, CONTRACT_BALANCE);
   }

   #[test]
   fn weth_to_usdc_via_v3_does_not_unwrap() {
      let amount_in = wei("1", 18);
      let got = plan(
         weth(),
         usdc(),
         amount_in.wei(),
         vec![step(
            v3_weth_usdc(),
            weth(),
            usdc(),
            amount_in,
            wei("2500", 6),
         )],
      );

      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::V3_SWAP_EXACT_IN,
            Commands::SWEEP,
         ],
      );
   }

   #[test]
   fn eth_to_usdc_via_v4_sends_value_without_wrap() {
      let amount_in = wei("1", 18);
      let got = plan(
         eth(),
         usdc(),
         amount_in.wei(),
         vec![step(
            UniswapV4Pool::eth_usdc(),
            eth(),
            usdc(),
            amount_in.clone(),
            wei("2500", 6),
         )],
      );

      assert_eq!(got.value, amount_in.wei());
      assert_commands(&got, &[Commands::V4_SWAP, Commands::SWEEP]);
   }

   #[test]
   fn eth_into_a_v4_weth_pool_wraps_the_slice() {
      // The quoter treats WETH and ETH as one asset, so an ETH trade can be routed through the
      // *WETH* pool of a V4 dex. Its SETTLE pulls WETH out of the router, so the ETH has to be
      // wrapped first — only a native pool (`currency0 == address(0)`) settles from `msg.value`.
      let amount_in = wei("1", 18);
      let got = plan(
         eth(),
         usdc(),
         amount_in.wei(),
         vec![step(
            v4_weth_usdc(),
            weth(),
            usdc(),
            amount_in.clone(),
            wei("2500", 6),
         )],
      );

      assert_eq!(got.value, amount_in.wei());
      assert_commands(
         &got,
         &[Commands::WRAP_ETH, Commands::V4_SWAP, Commands::SWEEP],
      );

      let wrap = WrapEth::abi_decode_params(nth_input(&got, Commands::WRAP_ETH, 0)).unwrap();
      assert_eq!(wrap.amount, amount_in.wei());
   }

   #[test]
   fn weth_split_v4_native_and_v4_weth_rewraps_both_slices() {
      // UNWRAP_WETH unwraps the router's whole WETH balance, so the WETH-backed slice has to be
      // wrapped again before its hop — not just V2/V3 slices.
      let native_in = wei("0.6", 18);
      let weth_in = wei("0.4", 18);
      let amount_in = NumericValue::format_wei(native_in.wei() + weth_in.wei(), 18);
      let got = plan(
         weth(),
         usdc(),
         amount_in.wei(),
         vec![
            step(
               UniswapV4Pool::eth_usdc(),
               eth(),
               usdc(),
               native_in.clone(),
               wei("1500", 6),
            ),
            step(
               v4_weth_usdc(),
               weth(),
               usdc(),
               weth_in.clone(),
               wei("1000", 6),
            ),
         ],
      );

      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::UNWRAP_WETH,
            Commands::WRAP_ETH,
            Commands::V4_SWAP,
            Commands::V4_SWAP,
            Commands::SWEEP,
         ],
      );

      let unwrap =
         UnwrapWeth::abi_decode_params(nth_input(&got, Commands::UNWRAP_WETH, 0)).unwrap();
      assert_eq!(unwrap.amountMin, native_in.wei());

      let wrap = WrapEth::abi_decode_params(nth_input(&got, Commands::WRAP_ETH, 0)).unwrap();
      assert_eq!(wrap.amount, weth_in.wei());
   }

   #[test]
   fn v3_swap_input_carries_the_min_hop_price_array() {
      // The current router generation decodes a 6th param word (`uint256[] minHopPriceX36`) and
      // reverts `SliceOutOfBounds()` without it; the previous one stops after `permit2`.
      let amount_in = wei("1", 18);
      let got = plan(
         eth(),
         usdc(),
         amount_in.wei(),
         vec![step(
            v3_weth_usdc(),
            weth(),
            usdc(),
            amount_in.clone(),
            wei("2500", 6),
         )],
      );

      let input = nth_input(&got, Commands::V3_SWAP_EXACT_IN, 0);
      let decoded = V3SwapExactIn::abi_decode_params(input).unwrap();
      assert!(decoded.minHopPriceX36.is_empty());
      assert_eq!(decoded.amountIn, amount_in.wei());
      assert_eq!(decoded.permit2, false);
      assert_eq!(
         decoded.path.len(),
         43,
         "WETH(20) + fee(3) + USDC(20)"
      );
   }

   #[test]
   fn v4_swap_params_follow_the_chain_router_generation() {
      let amount_in = wei("1", 18);
      let steps = || {
         vec![step(
            v4_weth_usdc(),
            weth(),
            usdc(),
            amount_in.clone(),
            wei("2500", 6),
         )]
      };

      let legacy = plan_on_chain(1, eth(), usdc(), amount_in.wei(), steps());
      let current = plan_on_chain(4663, eth(), usdc(), amount_in.wei(), steps());

      assert_eq!(
         nth_input(&current, Commands::V4_SWAP, 0).len(),
         nth_input(&legacy, Commands::V4_SWAP, 0).len() + 32,
         "the current generation inserts `minHopPriceX36` before the hookData offset"
      );
   }

   /// A hop whose input is an earlier hop's output cannot put that amount in wei into the calldata:
   /// the quote is an estimate and a multi-tick V3 hop lands below it. V4 actions work on PoolManager
   /// deltas, so the hop has to settle the router's balance (`CONTRACT_BALANCE`) *first* and then swap
   /// the credit that settle created (`OPEN_DELTA`) — settling the quoted amount instead asks the pool
   /// for more USDC than the router holds, which is the `TRANSFER_FAILED` this shape used to revert.
   #[test]
   fn v4_hop_fed_by_an_earlier_hop_settles_the_router_balance_first() {
      let amount_in = wei("1", 18);
      let quote_out = wei("2500", 6);
      let got = plan(
         eth(),
         usdt(),
         amount_in.wei(),
         vec![
            step(
               v3_weth_usdc(),
               weth(),
               usdc(),
               amount_in.clone(),
               quote_out.clone(),
            ),
            step(
               v4_usdc_usdt(),
               usdc(),
               usdt(),
               quote_out.clone(),
               quote_out,
            ),
         ],
      );

      assert_commands(
         &got,
         &[
            Commands::WRAP_ETH,
            Commands::V3_SWAP_EXACT_IN,
            Commands::V4_SWAP,
            Commands::SWEEP,
         ],
      );

      let v4 = ActionsParams::abi_decode_params(nth_input(&got, Commands::V4_SWAP, 0)).unwrap();
      // SETTLE (0x0b), SWAP_EXACT_IN_SINGLE (0x06), TAKE (0x0e)
      assert_eq!(
         v4.actions.to_vec(),
         vec![0x0b, 0x06, 0x0e],
         "the settle comes first: it is what credits the router"
      );

      let settle = SettleParams::abi_decode_params(&v4.params[0]).unwrap();
      assert_eq!(settle.currency, usdc().address());
      assert_eq!(settle.amount, CONTRACT_BALANCE);
      assert!(
         !settle.payerIsUser,
         "the router pays out of its own balance"
      );

      // `hookData` makes the swap struct dynamic, so its params carry a leading struct offset (the
      // offset the router's `CalldataDecoder` reads); the static settle/take params do not.
      let swap = ExactInputSingleParams::abi_decode(&v4.params[1]).unwrap();
      assert_eq!(
         swap.amountIn, 0,
         "OPEN_DELTA: spend the settled credit"
      );

      let take = TakeParams::abi_decode_params(&v4.params[2]).unwrap();
      assert_eq!(
         take.amount, 0,
         "OPEN_DELTA: take the whole credit"
      );
      assert_eq!(
         take.recipient,
         address_book::universal_router_v2(1).unwrap()
      );
   }

   /// A hop that spends the trade input knows its amount, so it keeps the quoted settle and the
   /// swap-first order the router's own examples use.
   #[test]
   fn v4_hop_that_spends_the_trade_input_keeps_the_quoted_settle() {
      let amount_in = wei("1", 18);
      let got = plan(
         eth(),
         usdc(),
         amount_in.wei(),
         vec![step(
            UniswapV4Pool::eth_usdc(),
            eth(),
            usdc(),
            amount_in.clone(),
            wei("2500", 6),
         )],
      );

      let v4 = ActionsParams::abi_decode_params(nth_input(&got, Commands::V4_SWAP, 0)).unwrap();
      // SWAP_EXACT_IN_SINGLE (0x06), SETTLE (0x0b), TAKE (0x0e)
      assert_eq!(v4.actions.to_vec(), vec![0x06, 0x0b, 0x0e]);

      let swap = ExactInputSingleParams::abi_decode(&v4.params[0]).unwrap();
      assert_eq!(U256::from(swap.amountIn), amount_in.wei());

      let settle = SettleParams::abi_decode_params(&v4.params[1]).unwrap();
      assert_eq!(settle.amount, amount_in.wei());
   }

   #[test]
   fn eth_to_usdc_via_v3_wraps_first() {
      let amount_in = wei("1", 18);
      let got = plan(
         eth(),
         usdc(),
         amount_in.wei(),
         vec![step(
            v3_weth_usdc(),
            weth(),
            usdc(),
            amount_in.clone(),
            wei("2500", 6),
         )],
      );

      assert_eq!(got.value, amount_in.wei());
      assert_commands(
         &got,
         &[
            Commands::WRAP_ETH,
            Commands::V3_SWAP_EXACT_IN,
            Commands::SWEEP,
         ],
      );

      let wrap = WrapEth::abi_decode_params(nth_input(&got, Commands::WRAP_ETH, 0)).unwrap();
      assert_eq!(wrap.amount, amount_in.wei());
   }

   #[test]
   fn weth_split_v4_native_and_v3_unwraps_then_rewraps() {
      let v4_in = wei("0.6", 18);
      let v3_in = wei("0.4", 18);
      let amount_in = NumericValue::format_wei(v4_in.wei() + v3_in.wei(), 18);
      let got = plan(
         weth(),
         usdc(),
         amount_in.wei(),
         vec![
            step(
               UniswapV4Pool::eth_usdc(),
               eth(),
               usdc(),
               v4_in.clone(),
               wei("1500", 6),
            ),
            step(
               v3_weth_usdc(),
               weth(),
               usdc(),
               v3_in.clone(),
               wei("1000", 6),
            ),
         ],
      );

      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::UNWRAP_WETH,
            Commands::WRAP_ETH,
            Commands::V4_SWAP,
            Commands::V3_SWAP_EXACT_IN,
            Commands::SWEEP,
         ],
      );

      let unwrap =
         UnwrapWeth::abi_decode_params(nth_input(&got, Commands::UNWRAP_WETH, 0)).unwrap();
      assert_eq!(unwrap.amountMin, v4_in.wei());

      let wrap = WrapEth::abi_decode_params(nth_input(&got, Commands::WRAP_ETH, 0)).unwrap();
      assert_eq!(wrap.amount, v3_in.wei());
   }

   #[test]
   fn v3_weth_out_to_native_unwraps_to_recipient_without_sweep() {
      let amount_in = wei("2500", 6);
      let got = plan(
         usdc(),
         eth(),
         amount_in.wei(),
         vec![step(
            v3_weth_usdc(),
            usdc(),
            weth(),
            amount_in,
            wei("1", 18),
         )],
      );

      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::V3_SWAP_EXACT_IN,
            Commands::UNWRAP_WETH,
         ],
      );

      let unwrap =
         UnwrapWeth::abi_decode_params(nth_input(&got, Commands::UNWRAP_WETH, 0)).unwrap();
      assert_eq!(unwrap.recipient, recipient());
      assert_eq!(unwrap.amountMin, U256::from(1u64));
   }

   #[test]
   fn mixed_eth_and_weth_out_unwraps_then_sweeps() {
      let amount_in = wei("5000", 6);
      let got = plan(
         usdc(),
         eth(),
         amount_in.wei(),
         vec![
            step(
               UniswapV4Pool::eth_usdc(),
               usdc(),
               eth(),
               wei("2500", 6),
               wei("1", 18),
            ),
            step(
               v3_weth_usdc(),
               usdc(),
               weth(),
               wei("2500", 6),
               wei("1", 18),
            ),
         ],
      );

      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::V4_SWAP,
            Commands::V3_SWAP_EXACT_IN,
            Commands::UNWRAP_WETH,
            Commands::SWEEP,
         ],
      );

      let unwrap =
         UnwrapWeth::abi_decode_params(nth_input(&got, Commands::UNWRAP_WETH, 0)).unwrap();
      assert_ne!(unwrap.recipient, recipient());
   }

   #[test]
   fn erc20_to_erc20_v4_has_no_wrap_or_unwrap() {
      let amount_in = wei("10000", 6);
      let got = plan(
         usdc(),
         usdt(),
         amount_in.wei(),
         vec![step(
            UniswapV4Pool::usdc_usdt(),
            usdc(),
            usdt(),
            amount_in,
            wei("10000", 6),
         )],
      );

      assert_commands(
         &got,
         &[
            Commands::PERMIT2_TRANSFER_FROM,
            Commands::V4_SWAP,
            Commands::SWEEP,
         ],
      );
   }
}
