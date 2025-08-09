pub mod pool;
pub use pool::UniswapV4Pool;

use crate::abi::uniswap::universal_router_v2::IV4Router::*;
use alloy_primitives::{Address, Bytes};
use alloy_sol_types::SolValue;

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum Actions {
   // Pool actions
   // Liquidity actions
   INCREASE_LIQUIDITY(IncreaseLiquidityParams) = 0x00,
   DECREASE_LIQUIDITY(DecreaseLiquidityParams) = 0x01,
   MINT_POSITION(MintPositionParams) = 0x02,
   BURN_POSITION(BurnPositionParams) = 0x03,
   // Swapping
   SWAP_EXACT_IN_SINGLE(ExactInputSingleParams) = 0x06,
   SWAP_EXACT_IN(ExactInputParams) = 0x07,
   SWAP_EXACT_OUT_SINGLE(ExactOutputSingleParams) = 0x08,
   SWAP_EXACT_OUT(ExactOutputParams) = 0x09,

   // Closing deltas on the pool manager
   // Settling
   SETTLE(SettleParams) = 0x0b,
   SETTLE_ALL(SettleAllParams) = 0x0c,
   SETTLE_PAIR(SettlePairParams) = 0x0d,
   // Taking
   TAKE(TakeParams) = 0x0e,
   TAKE_ALL(TakeAllParams) = 0x0f,
   TAKE_PORTION(TakePortionParams) = 0x10,
   TAKE_PAIR(TakePairParams) = 0x11,

   CLOSE_CURRENCY(CloseCurrencyParams) = 0x12,
   SWEEP(SweepParams) = 0x14,
}

/// https://doc.rust-lang.org/error_codes/E0732.html
#[inline]
const fn discriminant(v: &Actions) -> u8 {
   unsafe { *(v as *const Actions as *const u8) }
}

impl Actions {
   #[inline]
   pub const fn command(&self) -> u8 {
      discriminant(self)
   }

   #[inline]
   pub fn abi_encode(&self) -> Bytes {
      match self {
         Self::INCREASE_LIQUIDITY(params) => params.abi_encode_params(),
         Self::DECREASE_LIQUIDITY(params) => params.abi_encode_params(),
         Self::MINT_POSITION(params) => params.abi_encode_params(),
         Self::BURN_POSITION(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_IN_SINGLE(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_IN(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_OUT_SINGLE(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_OUT(params) => params.abi_encode_params(),
         Self::SETTLE(params) => params.abi_encode_params(),
         Self::SETTLE_ALL(params) => params.abi_encode_params(),
         Self::SETTLE_PAIR(params) => params.abi_encode_params(),
         Self::TAKE(params) => params.abi_encode_params(),
         Self::TAKE_ALL(params) => params.abi_encode_params(),
         Self::TAKE_PORTION(params) => params.abi_encode_params(),
         Self::TAKE_PAIR(params) => params.abi_encode_params(),
         Self::CLOSE_CURRENCY(params) => params.abi_encode_params(),
         Self::SWEEP(params) => params.abi_encode_params(),
      }
      .into()
   }

   #[inline]
   pub fn abi_decode(command: u8, data: &Bytes) -> Result<Self, anyhow::Error> {
      let data = data.iter().as_slice();
      Ok(match command {
         0x00 => Self::INCREASE_LIQUIDITY(IncreaseLiquidityParams::abi_decode(data)?),
         0x01 => Self::DECREASE_LIQUIDITY(DecreaseLiquidityParams::abi_decode(data)?),
         0x02 => Self::MINT_POSITION(MintPositionParams::abi_decode(data)?),
         0x03 => Self::BURN_POSITION(BurnPositionParams::abi_decode(data)?),
         0x06 => Self::SWAP_EXACT_IN_SINGLE(ExactInputSingleParams::abi_decode(data)?),
         0x07 => Self::SWAP_EXACT_IN(ExactInputParams::abi_decode(data)?),
         0x08 => Self::SWAP_EXACT_OUT_SINGLE(ExactOutputSingleParams::abi_decode(data)?),
         0x09 => Self::SWAP_EXACT_OUT(ExactOutputParams::abi_decode(data)?),
         0x0b => Self::SETTLE(SettleParams::abi_decode(data)?),
         0x0c => Self::SETTLE_ALL(SettleAllParams::abi_decode(data)?),
         0x0d => Self::SETTLE_PAIR(SettlePairParams::abi_decode(data)?),
         0x0e => Self::TAKE(TakeParams::abi_decode(data)?),
         0x0f => Self::TAKE_ALL(TakeAllParams::abi_decode(data)?),
         0x10 => Self::TAKE_PORTION(TakePortionParams::abi_decode(data)?),
         0x11 => Self::TAKE_PAIR(TakePairParams::abi_decode(data)?),
         0x12 => Self::CLOSE_CURRENCY(CloseCurrencyParams::abi_decode(data)?),
         0x14 => Self::SWEEP(SweepParams::abi_decode(data)?),
         _ => return Err(anyhow::anyhow!("Invalid action")),
      })
   }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HookOptions {
   AfterRemoveLiquidityReturnsDelta = 0,
   AfterAddLiquidityReturnsDelta = 1,
   AfterSwapReturnsDelta = 2,
   BeforeSwapReturnsDelta = 3,
   AfterDonate = 4,
   BeforeDonate = 5,
   AfterSwap = 6,
   BeforeSwap = 7,
   AfterRemoveLiquidity = 8,
   BeforeRemoveLiquidity = 9,
   AfterAddLiquidity = 10,
   BeforeAddLiquidity = 11,
   AfterInitialize = 12,
   BeforeInitialize = 13,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HookPermissions {
   pub after_remove_liquidity_returns_delta: bool,
   pub after_add_liquidity_returns_delta: bool,
   pub after_swap_returns_delta: bool,
   pub before_swap_returns_delta: bool,
   pub after_donate: bool,
   pub before_donate: bool,
   pub after_swap: bool,
   pub before_swap: bool,
   pub after_remove_liquidity: bool,
   pub before_remove_liquidity: bool,
   pub after_add_liquidity: bool,
   pub before_add_liquidity: bool,
   pub after_initialize: bool,
   pub before_initialize: bool,
}

#[inline]
#[must_use]
pub const fn permissions(address: Address) -> HookPermissions {
   HookPermissions {
      before_initialize: has_permission(address, HookOptions::BeforeInitialize),
      after_initialize: has_permission(address, HookOptions::AfterInitialize),
      before_add_liquidity: has_permission(address, HookOptions::BeforeAddLiquidity),
      after_add_liquidity: has_permission(address, HookOptions::AfterAddLiquidity),
      before_remove_liquidity: has_permission(address, HookOptions::BeforeRemoveLiquidity),
      after_remove_liquidity: has_permission(address, HookOptions::AfterRemoveLiquidity),
      before_swap: has_permission(address, HookOptions::BeforeSwap),
      after_swap: has_permission(address, HookOptions::AfterSwap),
      before_donate: has_permission(address, HookOptions::BeforeDonate),
      after_donate: has_permission(address, HookOptions::AfterDonate),
      before_swap_returns_delta: has_permission(address, HookOptions::BeforeSwapReturnsDelta),
      after_swap_returns_delta: has_permission(address, HookOptions::AfterSwapReturnsDelta),
      after_add_liquidity_returns_delta: has_permission(address, HookOptions::AfterAddLiquidityReturnsDelta),
      after_remove_liquidity_returns_delta: has_permission(address, HookOptions::AfterRemoveLiquidityReturnsDelta),
   }
}

#[inline]
#[must_use]
pub const fn has_permission(address: Address, hook_option: HookOptions) -> bool {
   let mask = ((address.0.0[18] as u64) << 8) | (address.0.0[19] as u64);
   let hook_flag_index = hook_option as u64;
   mask & (1 << hook_flag_index) != 0
}

#[inline]
#[must_use]
pub const fn has_initialize_permissions(address: Address) -> bool {
   has_permission(address, HookOptions::BeforeInitialize) || has_permission(address, HookOptions::AfterInitialize)
}

#[inline]
#[must_use]
pub const fn has_liquidity_permissions(address: Address) -> bool {
   has_permission(address, HookOptions::BeforeAddLiquidity)
      || has_permission(address, HookOptions::AfterAddLiquidity)
      || has_permission(address, HookOptions::BeforeRemoveLiquidity)
      || has_permission(address, HookOptions::AfterRemoveLiquidity)
}

#[inline]
#[must_use]
pub const fn has_swap_permissions(address: Address) -> bool {
   // this implicitly encapsulates swap delta permissions
   has_permission(address, HookOptions::BeforeSwap) || has_permission(address, HookOptions::AfterSwap)
}

#[inline]
#[must_use]
pub const fn has_donate_permissions(address: Address) -> bool {
   has_permission(address, HookOptions::BeforeDonate) || has_permission(address, HookOptions::AfterDonate)
}
