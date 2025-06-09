pub mod state_view;

use alloy_sol_types::{sol, SolEvent};
use alloy_primitives::LogData;

pub use IPoolManager::Initialize;

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
}

    #[derive(Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct PoolKey {
        address currency0;
        address currency1;
        uint24 fee;
        int24 tickSpacing;
        address hooks;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct PathKey {
        address intermediateCurrency;
        uint24 fee;
        int24 tickSpacing;
        IHooks hooks;
        bytes hookData;
    }


    #[derive(Debug, Default, PartialEq, Eq)]
    struct IncreaseLiquidityParams {
        uint256 tokenId;
        uint256 liquidity;
        uint128 amount0Max;
        uint128 amount1Max;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct DecreaseLiquidityParams {
        uint256 tokenId;
        uint256 liquidity;
        uint128 amount0Min;
        uint128 amount1Min;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct MintPositionParams {
        PoolKey poolKey;
        int24 tickLower;
        int24 tickUpper;
        uint256 liquidity;
        uint128 amount0Max;
        uint128 amount1Max;
        address owner;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct BurnPositionParams {
        uint256 tokenId;
        uint128 amount0Min;
        uint128 amount1Min;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactInSingleParams {
        PoolKey poolKey;
        bool zeroForOne;
        uint128 amountIn;
        uint128 amountOutMinimum;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactInParams {
        address currencyIn;
        PathKey[] path;
        uint128 amountIn;
        uint128 amountOutMinimum;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactOutSingleParams {
        PoolKey poolKey;
        bool zeroForOne;
        uint128 amountOut;
        uint128 amountInMaximum;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactOutParams {
        address currencyOut;
        PathKey[] path;
        uint128 amountOut;
        uint128 amountInMaximum;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettleParams {
        address currency;
        uint256 amount;
        bool payerIsUser;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettleAllParams {
        address currency;
        uint256 maxAmount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettlePairParams {
        address currency0;
        address currency1;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakeParams {
        address currency;
        address recipient;
        uint256 amount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakeAllParams {
        address currency;
        uint256 minAmount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakePortionParams {
        address currency;
        address recipient;
        uint256 bips;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakePairParams {
        address currency0;
        address currency1;
        address recipient;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettleTakePairParams {
        address settleCurrency;
        address takeCurrency;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct CloseCurrencyParams {
        address currency;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SweepParams {
        address currency;
        address recipient;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct ActionsParams {
        bytes actions;
        bytes[] params;
    }


}

pub fn initialize_signature() -> &'static str {
   IPoolManager::Initialize::SIGNATURE
}

pub fn decode_initialize(log: &LogData) -> Result<Initialize, anyhow::Error> {
   let abi = IPoolManager::Initialize::decode_raw_log(log.topics(), &log.data)?;
   Ok(abi)
}