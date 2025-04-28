pub mod pool;

use alloy_primitives::{
   Address,
   aliases::{I24, U24},
};
use serde::{Deserialize, Serialize};

/// The default factory enabled fee amounts, denominated in hundredths of bips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum FeeAmount {
   LOWEST = 100,
   LOW_200 = 200,
   LOW_300 = 300,
   LOW_400 = 400,
   LOW = 500,
   MEDIUM = 3000,
   HIGH = 10000,
   CUSTOM(u32),
}

impl FeeAmount {
   pub fn fee(&self) -> u32 {
      match self {
         Self::LOWEST => 100,
         Self::LOW_200 => 200,
         Self::LOW_300 => 300,
         Self::LOW_400 => 400,
         Self::LOW => 500,
         Self::MEDIUM => 3000,
         Self::HIGH => 10000,
         Self::CUSTOM(fee) => *fee,
      }
   }

   pub fn fee_u24(&self) -> U24 {
      U24::from(self.fee())
   }

   /// The default factory tick spacings by fee amount.
   pub fn tick_spacing(&self) -> I24 {
      match self {
         Self::LOWEST => I24::ONE,
         Self::LOW_200 => I24::from_limbs([4]),
         Self::LOW_300 => I24::from_limbs([6]),
         Self::LOW_400 => I24::from_limbs([8]),
         Self::LOW => I24::from_limbs([10]),
         Self::MEDIUM => I24::from_limbs([60]),
         Self::HIGH => I24::from_limbs([200]),
         Self::CUSTOM(fee) => I24::from_limbs([(fee / 50) as u64]),
      }
   }

   pub fn tick_spacing_i32(&self) -> i32 {
      match self {
         Self::LOWEST => 1,
         Self::LOW_200 => 10,
         Self::LOW_300 => 60,
         Self::LOW_400 => 200,
         Self::LOW => 400,
         Self::MEDIUM => 1000,
         Self::HIGH => 2000,
         Self::CUSTOM(fee) => *fee as i32 / 50,
      }
   }
}

impl From<u32> for FeeAmount {
   fn from(fee: u32) -> Self {
      match fee {
         100 => Self::LOWEST,
         200 => Self::LOW_200,
         300 => Self::LOW_300,
         400 => Self::LOW_400,
         500 => Self::LOW,
         3000 => Self::MEDIUM,
         10000 => Self::HIGH,
         _ => Self::CUSTOM(fee),
      }
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
