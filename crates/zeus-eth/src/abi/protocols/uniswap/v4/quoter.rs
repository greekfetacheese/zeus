//! `V4Quoter` — the periphery contract that runs a swap through the real `PoolManager` and reports
//! what it returned.
//!
//! Zeus prices hookless V4 pools with local tick math. That math cannot cover a pool whose hook has
//! swap permissions: `beforeSwap`/`afterSwap` may return deltas that change both amounts, and only
//! the deployed contract knows what they are. The quoter runs the same `PoolManager` code the swap
//! will, so its answer is the one to use.

use alloy_sol_types::sol;

sol! {
    /// The pool the quoter is asked about.
    ///
    /// Same layout as [`super::actions::PoolKey`], repeated because a `sol!` block can only name
    /// structs declared inside it; [`From`] converts between the two.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct PoolKey {
        address currency0;
        address currency1;
        uint24 fee;
        int24 tickSpacing;
        address hooks;
    }

    /// Argument of [`UniswapV4Quoter::quoteExactInputSingle`].
    #[derive(Debug, Default, PartialEq, Eq)]
    struct QuoteExactInputSingleParams {
        PoolKey poolKey;
        bool zeroForOne;
        uint128 exactAmount;
        bytes hookData;
    }

    #[sol(rpc)]
    contract UniswapV4Quoter {
        /// Simulates an exact-input swap against the `PoolManager` and returns what it produced.
        function quoteExactInputSingle(
            QuoteExactInputSingleParams params
        ) external returns (uint256 amountOut, uint256 gasEstimate);
    }
}

impl From<super::actions::PoolKey> for PoolKey {
   fn from(key: super::actions::PoolKey) -> Self {
      Self {
         currency0: key.currency0,
         currency1: key.currency1,
         fee: key.fee,
         tickSpacing: key.tickSpacing,
         hooks: key.hooks,
      }
   }
}
