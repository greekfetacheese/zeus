pub mod actions;
pub mod state_view;

use alloy_primitives::LogData;
use alloy_sol_types::{SolEvent, sol};

pub use IPoolManager::{Initialize, Swap};

sol! {

    type PoolId is bytes32;
    type Currency is address;

   interface IHooks {}

    interface IPoolManager {
    /// @notice Emitted when a new pool is initialized
    /// @param id The abi encoded hash of the pool key struct for the new pool
    /// @param currency0 The first currency of the pool by address sort order
    /// @param currency1 The second currency of the pool by address sort order
    /// @param fee The fee collected upon every swap in the pool, denominated in hundredths of a bip
    /// @param tickSpacing The minimum number of ticks between initialized ticks
    /// @param hooks The hooks contract address for the pool, or address(0) if none
    /// @param sqrtPriceX96 The price of the pool on initialization
    /// @param tick The initial tick of the pool corresponding to the initialized price
    event Initialize(
        PoolId indexed id,
        Currency indexed currency0,
        Currency indexed currency1,
        uint24 fee,
        int24 tickSpacing,
        IHooks hooks,
        uint160 sqrtPriceX96,
        int24 tick
    );

    /// @notice Emitted for swaps between currency0 and currency1
    /// @param id The abi encoded hash of the pool key struct for the pool that was modified
    /// @param sender The address that initiated the swap call, and that received the callback
    /// @param amount0 The delta of the currency0 balance of the pool
    /// @param amount1 The delta of the currency1 balance of the pool
    /// @param sqrtPriceX96 The sqrt(price) of the pool after the swap, as a Q64.96
    /// @param liquidity The liquidity of the pool after the swap
    /// @param tick The log base 1.0001 of the price of the pool after the swap
    /// @param fee The swap fee in hundredths of a bip
    event Swap(
        PoolId indexed id,
        address indexed sender,
        int128 amount0,
        int128 amount1,
        uint160 sqrtPriceX96,
        uint128 liquidity,
        int24 tick,
        uint24 fee
    );
}




}

pub fn initialize_signature() -> &'static str {
   IPoolManager::Initialize::SIGNATURE
}

pub fn decode_initialize(log: &LogData) -> Result<Initialize, anyhow::Error> {
   let abi = IPoolManager::Initialize::decode_raw_log(log.topics(), &log.data)?;
   Ok(abi)
}

pub fn decode_swap_log(log: &LogData) -> Result<Swap, anyhow::Error> {
   let abi = IPoolManager::Swap::decode_raw_log(log.topics(), &log.data)?;
   Ok(abi)
}
