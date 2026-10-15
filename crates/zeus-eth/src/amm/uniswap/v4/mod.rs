pub mod pool;
pub use pool::UniswapV4Pool;

use crate::abi::uniswap::v4::actions::*;
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
         // V4Router CalldataDecoder decodes these as `abi.decode(params, (Struct))`,
         // which expects an outer offset word. `abi_encode_params()` starts at
         // currency0, so ETH pools (currency0 == address(0)) work by accident
         // and ERC20/ERC20 pools revert.
         Self::SWAP_EXACT_IN_SINGLE(params) => params.abi_encode(),
         Self::SWAP_EXACT_IN(params) => params.abi_encode(),
         Self::SWAP_EXACT_OUT_SINGLE(params) => params.abi_encode(),
         Self::SWAP_EXACT_OUT(params) => params.abi_encode(),
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
         // The liquidity params reach the position manager as `abi.encodeParams` — the deployed
         // `CalldataDecoder` reads `tokenId` / `poolKey` straight at the params' first word — so
         // they decode with `abi_decode_params`, the shape `abi_encode_params` writes. The swap
         // structs below are the other way around: the router looks up an outer offset word
         // first, which is why they keep `abi_decode`.
         0x00 => Self::INCREASE_LIQUIDITY(IncreaseLiquidityParams::abi_decode_params(data)?),
         0x01 => Self::DECREASE_LIQUIDITY(DecreaseLiquidityParams::abi_decode_params(data)?),
         0x02 => Self::MINT_POSITION(MintPositionParams::abi_decode_params(data)?),
         0x03 => Self::BURN_POSITION(BurnPositionParams::abi_decode_params(data)?),
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

/// Decode the `(bytes actions, bytes[] params)` envelope a `modifyLiquidities` call carries.
pub fn actions_from_unlock_data(unlock_data: &Bytes) -> Result<Vec<Actions>, anyhow::Error> {
   let (actions, params) = <(Bytes, Vec<Bytes>)>::abi_decode(unlock_data)?;
   actions_from_params(&actions, &params)
}

/// Decode the action / params arrays of a `modifyLiquidities` call.
///
/// Action byte `i` consumes `params[i]` (the position manager's own invariant), so a command this
/// crate cannot decode — one the deployed contract may know but [`Actions`] does not — is skipped
/// without losing the walk: the remaining actions still line up with their own params. `Err` when
/// the actions buffer is empty or a listed action has no params.
pub fn actions_from_params(
   actions: &Bytes,
   params: &[Bytes],
) -> Result<Vec<Actions>, anyhow::Error> {
   if actions.is_empty() {
      return Err(anyhow::anyhow!("Empty V4 actions buffer"));
   }

   let mut decoded = Vec::with_capacity(actions.len());

   for (i, command) in actions.iter().enumerate() {
      let data = params
         .get(i)
         .ok_or_else(|| anyhow::anyhow!("Missing params for V4 action {i}"))?;

      if let Ok(action) = Actions::abi_decode(*command, data) {
         decoded.push(action);
      }
   }

   Ok(decoded)
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
      after_add_liquidity_returns_delta: has_permission(
         address,
         HookOptions::AfterAddLiquidityReturnsDelta,
      ),
      after_remove_liquidity_returns_delta: has_permission(
         address,
         HookOptions::AfterRemoveLiquidityReturnsDelta,
      ),
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
   has_permission(address, HookOptions::BeforeInitialize)
      || has_permission(address, HookOptions::AfterInitialize)
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
   has_permission(address, HookOptions::BeforeSwap)
      || has_permission(address, HookOptions::AfterSwap)
}

#[inline]
#[must_use]
pub const fn has_donate_permissions(address: Address) -> bool {
   has_permission(address, HookOptions::BeforeDonate)
      || has_permission(address, HookOptions::AfterDonate)
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::abi::uniswap::v4::actions::{
      ExactInputSingleParams, ExactInputSingleParamsMinHopPrice, PoolKey,
   };
   use alloy_primitives::{U256, address, aliases::I24};

   fn weth_usdc_key() -> PoolKey {
      PoolKey {
         currency0: address!("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
         currency1: address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
         fee: 500.try_into().unwrap(),
         tickSpacing: 10.try_into().unwrap(),
         hooks: Address::ZERO,
      }
   }

   /// The envelope `modifyLiquidities` carries: `abi.encode(bytes actions, bytes[] params)`.
   fn unlock_data(actions: &[u8], params: Vec<Bytes>) -> Bytes {
      (Bytes::from(actions.to_vec()), params).abi_encode().into()
   }

   fn mint_params() -> MintPositionParams {
      MintPositionParams {
         poolKey: weth_usdc_key(),
         tickLower: I24::try_from(-887_220).unwrap(),
         tickUpper: I24::try_from(887_220).unwrap(),
         liquidity: U256::from(1_000_000u64),
         amount0Max: u128::MAX,
         amount1Max: u128::MAX,
         owner: address!("46efbaedc92067e6d60e84ed6395099723252496"),
         hookData: Bytes::default(),
      }
   }

   /// A routed mint is one batch: mint, settle both currencies, take both. Every action must come
   /// back with its own params — the positional invariant — including `MINT_POSITION`, whose
   /// params reach the position manager without an outer offset word.
   #[test]
   fn unlock_data_decodes_a_mint_settle_take_batch() {
      let mint = mint_params();
      let settle = SettlePairParams {
         currency0: address!("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
         currency1: address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
      };
      let take = TakeAllParams {
         currency: address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
         minAmount: U256::from(1_000_000u64),
      };

      let params = vec![
         Actions::MINT_POSITION(mint.clone()).abi_encode(),
         Actions::SETTLE_PAIR(settle.clone()).abi_encode(),
         Actions::TAKE_ALL(take.clone()).abi_encode(),
      ];

      let decoded = actions_from_unlock_data(&unlock_data(&[0x02, 0x0d, 0x0f], params)).unwrap();

      assert_eq!(
         decoded,
         vec![
            Actions::MINT_POSITION(mint),
            Actions::SETTLE_PAIR(settle),
            Actions::TAKE_ALL(take),
         ]
      );
   }

   /// A command the enum does not model (0x13 `DONATE`) is skipped and the others survive: action
   /// `i` consumes `params[i]`, so the walk never shifts.
   #[test]
   fn unlock_data_skips_unknown_commands_without_losing_the_walk() {
      let mint = mint_params();
      let take = TakeAllParams {
         currency: weth_usdc_key().currency1,
         minAmount: U256::from(1_000_000u64),
      };

      let params = vec![
         Actions::MINT_POSITION(mint.clone()).abi_encode(),
         Bytes::from(vec![0u8; 32]),
         Actions::TAKE_ALL(take.clone()).abi_encode(),
      ];

      let decoded = actions_from_unlock_data(&unlock_data(&[0x02, 0x13, 0x0f], params)).unwrap();

      assert_eq!(
         decoded,
         vec![Actions::MINT_POSITION(mint), Actions::TAKE_ALL(take)]
      );
   }

   /// Malformed unlock data is an error rather than an empty action list: an empty actions buffer,
   /// and an action whose params are missing.
   #[test]
   fn unlock_data_rejects_malformed_input() {
      assert!(actions_from_unlock_data(&unlock_data(&[], vec![])).is_err());
      assert!(actions_from_unlock_data(&unlock_data(&[0x02], vec![])).is_err());
      assert!(actions_from_unlock_data(&Bytes::from_static(&[0u8; 8])).is_err());
   }

   #[test]
   fn swap_exact_in_single_encoding_starts_with_struct_offset() {
      let params = ExactInputSingleParams {
         poolKey: weth_usdc_key(),
         zeroForOne: true,
         amountIn: 1_000_000,
         amountOutMinimum: 0,
         hookData: Bytes::default(),
      };
      let encoded = Actions::SWAP_EXACT_IN_SINGLE(params).abi_encode();
      let offset = U256::from_be_slice(&encoded[..32]);
      assert_eq!(
         offset,
         U256::from(32u64),
         "CalldataDecoder reads the first word as the struct offset"
      );
   }

   /// The current router generation inserts `uint256 minHopPriceX36` between `amountOutMinimum`
   /// and `hookData`. Encoding the legacy struct against it makes the router read the hookData
   /// offset word as `minHopPriceX36` and the hookData length word as the hookData offset — a
   /// calldata slice that starts at the struct itself.
   #[test]
   fn swap_exact_in_single_min_hop_price_word_precedes_hook_data() {
      let legacy = ExactInputSingleParams {
         poolKey: weth_usdc_key(),
         zeroForOne: true,
         amountIn: 1_000_000,
         amountOutMinimum: 0,
         hookData: Bytes::default(),
      }
      .abi_encode();

      let current = ExactInputSingleParamsMinHopPrice {
         poolKey: weth_usdc_key(),
         zeroForOne: true,
         amountIn: 1_000_000,
         amountOutMinimum: 0,
         minHopPriceX36: U256::ZERO,
         hookData: Bytes::default(),
      }
      .abi_encode();

      assert_eq!(current.len(), legacy.len() + 32, "one extra word");
      assert_eq!(
         U256::from_be_slice(&current[..32]),
         U256::from(32u64)
      );

      let struct_start = 32;
      let field = |index: usize| {
         let from = struct_start + index * 32;
         U256::from_be_slice(&current[from..from + 32])
      };

      // 5 poolKey words, zeroForOne, amountIn, amountOutMinimum, minHopPriceX36, hookData offset
      assert_eq!(
         field(8),
         U256::ZERO,
         "minHopPriceX36 = 0 disables the per-hop price guard"
      );
      assert_eq!(
         field(9),
         U256::from(10 * 32u64),
         "hookData offset shifted one word past the struct head"
      );
      assert_eq!(field(10), U256::ZERO, "empty hookData");
   }
}
