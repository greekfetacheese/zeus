use alloy_sol_types::{sol, SolCall};
use alloy_primitives::{U256, Bytes, Uint, address, Address};

use INonfungiblePositionManager::MintParams;
use anyhow::Context;

pub const NFT_POSITION_CONTRACT: Address = address!("C36442b4a4522E871399CD717aBDD847Ab11FE88");


sol! {
    #[sol(rpc)]
    interface IMulticall {
        function multicall(bytes[] calldata data) external payable returns (bytes[] memory results);
    }

    interface INonfungiblePositionManager {
        function createAndInitializePoolIfNecessary(
            address token0,
            address token1,
            uint24 fee,
            uint160 sqrtPriceX96
        ) external payable returns (address pool);

        struct MintParams {
            address token0;
            address token1;
            uint24 fee;
            int24 tickLower;
            int24 tickUpper;
            uint256 amount0Desired;
            uint256 amount1Desired;
            uint256 amount0Min;
            uint256 amount1Min;
            address recipient;
            uint256 deadline;
        }

        function mint(MintParams calldata params)
            external
            payable
            returns (
                uint256 tokenId,
                uint128 liquidity,
                uint256 amount0,
                uint256 amount1
            );

        struct IncreaseLiquidityParams {
            uint256 tokenId;
            uint256 amount0Desired;
            uint256 amount1Desired;
            uint256 amount0Min;
            uint256 amount1Min;
            uint256 deadline;
        }

        function increaseLiquidity(IncreaseLiquidityParams calldata params)
            external
            payable
            returns (
                uint128 liquidity,
                uint256 amount0,
                uint256 amount1
            );

        struct DecreaseLiquidityParams {
            uint256 tokenId;
            uint128 liquidity;
            uint256 amount0Min;
            uint256 amount1Min;
            uint256 deadline;
        }

        function decreaseLiquidity(DecreaseLiquidityParams calldata params)
            external
            payable
            returns (uint256 amount0, uint256 amount1);

        struct CollectParams {
            uint256 tokenId;
            address recipient;
            uint128 amount0Max;
            uint128 amount1Max;
        }

        function collect(CollectParams calldata params) external payable returns (uint256 amount0, uint256 amount1);

        function burn(uint256 tokenId) external payable;

        function permit(
            address spender,
            uint256 tokenId,
            uint256 deadline,
            uint8 v,
            bytes32 r,
            bytes32 s
        ) external payable;

        function safeTransferFrom(address from, address to, uint256 tokenId) external;

        function safeTransferFrom(address from, address to, uint256 tokenId, bytes calldata data) external;

        function positions(uint256 tokenId)
        external
        view
        override
        returns (
            uint96 nonce,
            address operator,
            address token0,
            address token1,
            uint24 fee,
            int24 tickLower,
            int24 tickUpper,
            uint128 liquidity,
            uint256 feeGrowthInside0LastX128,
            uint256 feeGrowthInside1LastX128,
            uint128 tokensOwed0,
            uint128 tokensOwed1
        );
    }
}

#[derive(Debug, Clone)]
pub struct PositionsReturn {
    pub nonce: u128,
    pub operator: Address,
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub liquidity: u128,
    pub fee_growth_inside0_last_x128: U256,
    pub fee_growth_inside1_last_x128: U256,
    pub tokens_owed0: u128,
    pub tokens_owed1: u128,
}


// ABI Encode functions

pub fn encode_create_pool(token0: Address, token1: Address, fee: u32, sqrt_price_x96: U256) -> Result<Bytes, anyhow::Error> {
    let fee: Uint<24, 1> = fee.to_string().parse().context("Failed to parse fee")?;
    let sqrt_price_x96: Uint<160, 3> = sqrt_price_x96.to_string().parse().context("Failed to parse sqrt_price_x96")?;

    let abi = INonfungiblePositionManager::createAndInitializePoolIfNecessaryCall {
        token0,
        token1,
        fee,
        sqrtPriceX96: sqrt_price_x96,
    };
    Ok(Bytes::from(abi.abi_encode()))
}

pub fn encode_increase_liquidity(params: INonfungiblePositionManager::IncreaseLiquidityParams) -> Bytes {
    let abi = INonfungiblePositionManager::increaseLiquidityCall{params};
    Bytes::from(abi.abi_encode())
}

pub fn encode_decrease_liquidity(params: INonfungiblePositionManager::DecreaseLiquidityParams) -> Bytes {
    let abi = INonfungiblePositionManager::decreaseLiquidityCall{params};
    Bytes::from(abi.abi_encode())
}

pub fn encode_positions(token_id: U256) -> Bytes {
    let abi = INonfungiblePositionManager::positionsCall{tokenId: token_id};
    Bytes::from(abi.abi_encode())
}

pub fn encode_collect(params: INonfungiblePositionManager::CollectParams) -> Bytes {
    let abi = INonfungiblePositionManager::collectCall{params};
    Bytes::from(abi.abi_encode())
}

pub fn encode_burn(token_id: U256) -> Bytes {
    let abi = INonfungiblePositionManager::burnCall{tokenId: token_id};
    Bytes::from(abi.abi_encode())
}


/// Encode the Mint function for NFT Position Manager
pub fn encode_mint(params: MintParams) -> Vec<u8> {
    let contract = INonfungiblePositionManager::mintCall {
        params: MintParams {
            token0: params.token0,
            token1: params.token1,
            fee: params.fee,
            tickLower: params.tickLower,
            tickUpper: params.tickUpper,
            amount0Desired: params.amount0Desired,
            amount1Desired: params.amount1Desired,
            amount0Min: params.amount0Min,
            amount1Min: params.amount1Min,
            recipient: params.recipient,
            deadline: params.deadline,
        },
    };

    contract.abi_encode()
}


// ABI Decode functions

pub fn decode_create_pool(data: &Bytes) -> Result<Address, anyhow::Error> {
    let abi = INonfungiblePositionManager::createAndInitializePoolIfNecessaryCall::abi_decode_returns(data, true)?;
    Ok(abi.pool)
}

pub fn decode_increase_liquidity(data: &Bytes) -> Result<(u128, U256, U256), anyhow::Error> {
    let abi = INonfungiblePositionManager::increaseLiquidityCall::abi_decode_returns(data, true)?;
    Ok((abi.liquidity, abi.amount0, abi.amount1))
}

pub fn decode_decrease_liquidity(data: &Bytes) -> Result<(U256, U256), anyhow::Error> {
    let abi = INonfungiblePositionManager::decreaseLiquidityCall::abi_decode_returns(data, true)?;
    Ok((abi.amount0, abi.amount1))
}

pub fn decode_positions(data: &Bytes) -> Result<PositionsReturn, anyhow::Error> {
    let abi = INonfungiblePositionManager::positionsCall::abi_decode_returns(data, true)?;

    let nonce = abi.nonce.to_string().parse::<u128>().context("Failed to parse nonce")?;
    let fee = abi.fee.to_string().parse::<u32>().context("Failed to parse fee")?;
    let tick_lower = abi.tickLower.to_string().parse::<i32>().context("Failed to parse tick_lower")?;
    let tick_upper = abi.tickUpper.to_string().parse::<i32>().context("Failed to parse tick_upper")?;
    Ok(PositionsReturn {
        nonce,
        operator: abi.operator,
        token0: abi.token0,
        token1: abi.token1,
        fee,
        tick_lower,
        tick_upper,
        liquidity: abi.liquidity,
        fee_growth_inside0_last_x128: abi.feeGrowthInside0LastX128,
        fee_growth_inside1_last_x128: abi.feeGrowthInside1LastX128,
        tokens_owed0: abi.tokensOwed0,
        tokens_owed1: abi.tokensOwed1,
    })
}

pub fn decode_collect(data: &Bytes) -> Result<(U256, U256), anyhow::Error> {
    let abi = INonfungiblePositionManager::collectCall::abi_decode_returns(data, true)?;
    Ok((abi.amount0, abi.amount1))
}


/// Decode the output of the Mint function of the NFT Position Manager
pub fn decode_mint(bytes: &Bytes) -> Result<(U256, u128, U256, U256), anyhow::Error> {
    let res = INonfungiblePositionManager::mintCall::abi_decode_returns(&bytes, true)?;
    Ok((res.tokenId, res.liquidity, res.amount0, res.amount1))
}