use zeus_eth::{
   abi::zeus as zeus_abi,
   alloy_primitives::{Address, Bytes, U256},
   alloy_sol_types::SolValue,
   amm::uniswap::UniswapPool,
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};

use super::swap_quoter::SwapStep;
use anyhow::anyhow;

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum Commands {
   V2_SWAP = 0x01,
   V3_SWAP = 0x02,
   V4_SWAP = 0x03,
   WRAP_ETH = 0x04,
   UNWRAP_WETH = 0x05,
   WRAP_ETH_NO_CHECK = 0x06,
}

pub struct ZeusSwapDelegatorParams {
   pub call_data: Bytes,
   pub value: U256,
}

impl Default for ZeusSwapDelegatorParams {
   fn default() -> Self {
      Self::new()
   }
}

impl ZeusSwapDelegatorParams {
   pub fn new() -> Self {
      Self {
         call_data: Bytes::default(),
         value: U256::ZERO,
      }
   }

   pub fn set_call_data(&mut self, call_data: Bytes) {
      self.call_data = call_data;
   }

   pub fn set_value(&mut self, value: U256) {
      self.value = value;
   }
}

pub async fn encode_swap_delegate(
   chain_id: u64,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   amount_in: U256,
   amount_out_min: U256,
   slippage: f64,
   currency_in: Currency,
   currency_out: Currency,
   recipient: Address,
) -> Result<ZeusSwapDelegatorParams, anyhow::Error> {
   if swap_steps.is_empty() {
      return Err(anyhow!("No swap steps provided"));
   }

   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut execute_params = ZeusSwapDelegatorParams::new();

   let weth_currency = currency_in.to_weth();

   if currency_in.is_native() {
      // Always set the tx value to the total input amount when dealing with native ETH.
      execute_params.set_value(amount_in);

      // Calculate how much ETH needs to be wrapped for V2/V3 pools.
      let amount_to_wrap: U256 = swap_steps
         .iter()
         .filter(|s| s.currency_in == weth_currency && !s.pool.dex_kind().is_uniswap_v4())
         .map(|s| s.amount_in.wei())
         .sum();

      if amount_to_wrap > U256::ZERO {
         let data = zeus_abi::ZeusDelegate::WrapETHNoCheck {
            amount: amount_to_wrap,
         }
         .abi_encode()
         .into();
         commands.push(Commands::WRAP_ETH_NO_CHECK as u8);
         inputs.push(data);
      }
   }

   // Track ETH and WETH balances after the swaps
   let mut router_eth_balance = U256::ZERO;
   let mut router_weth_balance = U256::ZERO;

   for swap in &swap_steps {
      if swap.currency_in.is_native() && router_eth_balance >= swap.amount_in.wei() {
         router_eth_balance -= swap.amount_in.wei();
      }

      if swap.currency_in.is_native_wrapped() && router_weth_balance >= swap.amount_in.wei() {
         router_weth_balance -= swap.amount_in.wei();
      }

      if swap.currency_out.is_native() {
         router_eth_balance += swap.amount_out.wei();
      }

      if swap.currency_out.is_native_wrapped() {
         router_weth_balance += swap.amount_out.wei();
      }

      // For V2/V3, the input currency should always be the WETH address, even if the user starts with ETH.
      // The WRAP_ETH command ensures the router has the WETH.
      let step_currency_in = if swap.currency_in.is_native() {
         &weth_currency
      } else {
         &swap.currency_in
      };

      if swap.pool.dex_kind().is_v2() {
         let variant = 0 as u8;
         let input = zeus_abi::ZeusDelegate::V2V3SwapParams {
            amountIn: swap.amount_in.wei(),
            tokenIn: step_currency_in.address(),
            tokenOut: swap.currency_out.address(),
            pool: swap.pool.address(),
            poolVariant: variant.into(),
            fee: swap.pool.fee().fee_u24(),
         }
         .abi_encode()
         .into();

         commands.push(Commands::V2_SWAP as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_v3() {
         let variant = 1 as u8;
         let input: Bytes = zeus_abi::ZeusDelegate::V2V3SwapParams {
            amountIn: swap.amount_in.wei(),
            tokenIn: step_currency_in.address(),
            tokenOut: swap.currency_out.address(),
            pool: swap.pool.address(),
            poolVariant: variant.into(),
            fee: swap.pool.fee().fee_u24(),
         }
         .abi_encode()
         .into();
         commands.push(Commands::V3_SWAP as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v4() {
         let input: Bytes = zeus_abi::ZeusDelegate::V4SwapArgs {
            currencyIn: swap.currency_in.address(),
            currencyOut: swap.currency_out.address(),
            amountIn: swap.amount_in.wei(),
            fee: swap.pool.fee().fee_u24(),
            tickSpacing: swap.pool.fee().tick_spacing(),
            zeroForOne: swap.pool.zero_for_one_v4(&swap.currency_in),
            hooks: Address::ZERO,
            hookData: Bytes::default(),
            recipient: recipient,
         }
         .abi_encode()
         .into();
         commands.push(Commands::V4_SWAP as u8);
         inputs.push(input);
      }
   }

   handle_eth_payment(
      chain_id,
      &swap_steps,
      &currency_out,
      slippage,
      router_weth_balance,
      &mut commands,
      &mut inputs,
   );

   handle_weth_payment(
      chain_id,
      &swap_steps,
      &currency_out,
      slippage,
      router_eth_balance,
      &mut commands,
      &mut inputs,
   );

   let command_bytes = Bytes::from(commands);

   let calldata = zeus_abi::encode_z_swap(
      command_bytes,
      inputs,
      currency_out.address(),
      amount_out_min,
   );

   execute_params.set_call_data(calldata);

   Ok(execute_params)
}

fn handle_eth_payment(
   chain: u64,
   swap_steps: &Vec<SwapStep<impl UniswapPool>>,
   currency_out: &Currency,
   slippage: f64,
   router_weth_balance: U256,
   commands: &mut Vec<u8>,
   inputs: &mut Vec<Bytes>,
) {
   let v4_pools: usize = swap_steps.iter().filter(|s| s.pool.dex_kind().is_v4()).count();

   // Pool will send directly to EOA
   if swap_steps.len() == 1 && currency_out.is_erc20() {
      return;
   }

   // V4 Pool will send directly to EOA
   if swap_steps.len() == 1 && v4_pools > 0 {
      return;
   }

   let router_has_weth_balance = router_weth_balance > U256::ZERO;
   let should_pay_eth = currency_out.is_native() && router_has_weth_balance;

   if !should_pay_eth {
      return;
   }

   let weth = ERC20Token::wrapped_native_token(chain);
   let weth_balance = NumericValue::format_wei(router_weth_balance, weth.decimals);
   let amount_min = weth_balance.calc_slippage(slippage, weth.decimals);

   let data = zeus_abi::ZeusDelegate::UnwrapWETH {
      amountMin: amount_min.wei(),
   }
   .abi_encode()
   .into();
   commands.push(Commands::UNWRAP_WETH as u8);
   inputs.push(data);
}

fn handle_weth_payment(
   chain: u64,
   swap_steps: &Vec<SwapStep<impl UniswapPool>>,
   currency_out: &Currency,
   slippage: f64,
   router_eth_balance: U256,
   commands: &mut Vec<u8>,
   inputs: &mut Vec<Bytes>,
) {
   // Pool will send directly to EOA
   if swap_steps.len() == 1 && currency_out.is_erc20() {
      return;
   }

   let router_has_eth_balance = router_eth_balance > U256::ZERO;
   let should_pay_weth = currency_out.is_native_wrapped() && router_has_eth_balance;

   if !should_pay_weth {
      return;
   }

   let eth = Currency::from(NativeCurrency::from(chain));
   let eth_balance = NumericValue::format_wei(router_eth_balance, eth.decimals());
   let amount_min = eth_balance.calc_slippage(slippage, eth.decimals());

   let data = zeus_abi::ZeusDelegate::WrapETH {
      amountMin: amount_min.wei(),
   }
   .abi_encode()
   .into();
   commands.push(Commands::WRAP_ETH as u8);
   inputs.push(data);
}
