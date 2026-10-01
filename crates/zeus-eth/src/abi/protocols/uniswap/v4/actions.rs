use alloy_sol_types::sol;

sol! {

    type Currency is address;

    interface IHooks {}

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
        /// @notice Parameters for a single-hop exact-input swap
        struct ExactInputSingleParams {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountIn;
            uint128 amountOutMinimum;
            bytes hookData;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice Parameters for a multi-hop exact-input swap
        struct ExactInputParams {
            Currency currencyIn;
            PathKey[] path;
            uint128 amountIn;
            uint128 amountOutMinimum;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice Parameters for a single-hop exact-output swap
        struct ExactOutputSingleParams {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountOut;
            uint128 amountInMaximum;
            bytes hookData;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice Parameters for a multi-hop exact-output swap
        struct ExactOutputParams {
            address currencyOut;
            PathKey[] path;
            uint128 amountOut;
            uint128 amountInMaximum;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice `ExactInputSingleParams` as the current Universal Router generation decodes it
        ///
        /// The 2025+ generation inserts `uint256 minHopPriceX36` between `amountOutMinimum` and
        /// `hookData`. Given a legacy-shaped struct it reads the hookData *offset* word as
        /// `minHopPriceX36` and the hookData *length* word as the hookData offset — which points
        /// back at the struct start, so with a token `currency0` the calldata slice is out of
        /// bounds and the router reverts with **empty data** (a native pool has `currency0 == 0`
        /// and hides it). Verified against the deployed router on Robinhood Chain.
        struct ExactInputSingleParamsMinHopPrice {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountIn;
            uint128 amountOutMinimum;
            uint256 minHopPriceX36;
            bytes hookData;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice `ExactOutputSingleParams` as the current Universal Router generation decodes it
        struct ExactOutputSingleParamsMinHopPrice {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountOut;
            uint128 amountInMaximum;
            uint256 minHopPriceX36;
            bytes hookData;
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
