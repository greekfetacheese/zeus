use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, LogData, U256, Uint};
use alloy_sol_types::{SolCall, SolEvent, sol};

use INonfungiblePositionManager::MintParams;
use anyhow::Context;

sol! {

    #[sol(rpc)]
    contract INonfungiblePositionManager {
        function createAndInitializePoolIfNecessary(
            address token0,
            address token1,
            uint24 fee,
            uint160 sqrtPriceX96
        ) external payable returns (address pool);

      function multicall(bytes[] calldata data) public payable override returns (bytes[] memory results);

      function selfPermit(address token, uint256 value, uint256 deadline, uint8 v, bytes32 r, bytes32 s) public payable override;


        #[derive(Debug)]
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
            /// Owner of the position
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

        function ownerOf(uint256 tokenId) external view returns (address);

    event IncreaseLiquidity(
    uint256 tokenId,
    uint128 liquidity,
    uint256 amount0,
    uint256 amount1
  );

    event DecreaseLiquidity(
    uint256 tokenId,
    uint128 liquidity,
    uint256 amount0,
    uint256 amount1
  );

    /// @notice Emitted when fees are collected by the owner of a position
    /// @dev Collect events may be emitted with zero amount0 and amount1 when the caller chooses not to collect fees
    /// @param owner The owner of the position for which fees are collected
    /// @param tickLower The lower tick of the position
    /// @param tickUpper The upper tick of the position
    /// @param amount0 The amount of token0 fees collected
    /// @param amount1 The amount of token1 fees collected
    event Collect(
        address indexed owner,
        address recipient,
        int24 indexed tickLower,
        int24 indexed tickUpper,
        uint128 amount0,
        uint128 amount1
    );
    }
}

pub struct CollectLog {
   /// Owner of the position
   pub owner: Address,
   /// Recipient of the collected amounts
   pub recipient: Address,
   pub tick_lower: i32,
   pub tick_upper: i32,
   /// Collected token0 amount
   pub amount0: u128,
   /// Collected token1 amount
   pub amount1: u128,
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

pub type MintReturn = IncreaseLiquidityReturn;
pub type DecreaseLiquidityReturn = IncreaseLiquidityReturn;
#[derive(Debug, Clone)]
pub struct IncreaseLiquidityReturn {
   pub token_id: U256,
   pub liquidity: u128,
   pub amount0: U256,
   pub amount1: U256,
}

#[derive(Debug, Clone)]
pub struct PositionParams {
   /// Nonce for permits
   pub nonce: U256,
   /// Address that is approved for spending
   pub operator: Address,
   pub token0: Address,
   pub token1: Address,
   pub fee: u32,
   pub tick_lower: i32,
   pub tick_upper: i32,
   pub liquidity: U256,
   pub fee_growth_inside0_last_x128: U256,
   pub fee_growth_inside1_last_x128: U256,
   /// Unclaimed fees
   pub tokens_owed0: U256,
   /// Unclaimed fees
   pub tokens_owed1: U256,
}

pub async fn owner_of<P, N>(client: P, contract_address: Address, token_id: U256) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = INonfungiblePositionManager::new(contract_address, client);
   let owner = contract.ownerOf(token_id).call().await?;
   Ok(owner)
}

pub async fn positions<P, N>(
   client: P,
   contract_address: Address,
   token_id: U256,
) -> Result<PositionParams, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = INonfungiblePositionManager::new(contract_address, client);
   let positions = contract.positions(token_id).call().await?;
   let nonce = U256::from(positions.nonce);
   let fee: u32 = positions.fee.to_string().parse()?;
   let tick_lower: i32 = positions.tickLower.to_string().parse()?;
   let tick_upper: i32 = positions.tickUpper.to_string().parse()?;
   let liquidity = U256::from(positions.liquidity);
   let tokens_owed0 = U256::from(positions.tokensOwed0);
   let tokens_owed1 = U256::from(positions.tokensOwed1);

   let params = PositionParams {
      nonce,
      fee,
      tick_lower,
      tick_upper,
      operator: positions.operator,
      token0: positions.token0,
      token1: positions.token1,
      liquidity,
      fee_growth_inside0_last_x128: positions.feeGrowthInside0LastX128,
      fee_growth_inside1_last_x128: positions.feeGrowthInside1LastX128,
      tokens_owed0,
      tokens_owed1,
   };

   Ok(params)
}

// ABI Encode functions

pub fn encode_create_pool(
   token0: Address,
   token1: Address,
   fee: u32,
   sqrt_price_x96: U256,
) -> Result<Bytes, anyhow::Error> {
   let fee: Uint<24, 1> = fee.to_string().parse().context("Failed to parse fee")?;
   let sqrt_price_x96: Uint<160, 3> = sqrt_price_x96
      .to_string()
      .parse()
      .context("Failed to parse sqrt_price_x96")?;

   let abi = INonfungiblePositionManager::createAndInitializePoolIfNecessaryCall {
      token0,
      token1,
      fee,
      sqrtPriceX96: sqrt_price_x96,
   };
   Ok(Bytes::from(abi.abi_encode()))
}

pub fn encode_increase_liquidity(params: INonfungiblePositionManager::IncreaseLiquidityParams) -> Bytes {
   let abi = INonfungiblePositionManager::increaseLiquidityCall { params };
   Bytes::from(abi.abi_encode())
}


pub fn encode_positions(token_id: U256) -> Bytes {
   let abi = INonfungiblePositionManager::positionsCall { tokenId: token_id };
   Bytes::from(abi.abi_encode())
}

pub fn encode_collect(params: INonfungiblePositionManager::CollectParams) -> Bytes {
   let abi = INonfungiblePositionManager::collectCall { params };
   Bytes::from(abi.abi_encode())
}

pub fn encode_decrease_liquidity(params: INonfungiblePositionManager::DecreaseLiquidityParams) -> Bytes {
   let abi = INonfungiblePositionManager::decreaseLiquidityCall { params };
   Bytes::from(abi.abi_encode())
}

pub fn encode_burn(token_id: U256) -> Bytes {
   let abi = INonfungiblePositionManager::burnCall { tokenId: token_id };
   Bytes::from(abi.abi_encode())
}

/// Encode the Mint function for NFT Position Manager
pub fn encode_mint(params: MintParams) -> Bytes {
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

   contract.abi_encode().into()
}

// ABI Decode functions

pub fn decode_create_pool(data: &Bytes) -> Result<Address, anyhow::Error> {
   let abi = INonfungiblePositionManager::createAndInitializePoolIfNecessaryCall::abi_decode_returns(data)?;
   Ok(abi)
}

pub fn decode_increase_liquidity(data: &Bytes) -> Result<(u128, U256, U256), anyhow::Error> {
   let abi = INonfungiblePositionManager::increaseLiquidityCall::abi_decode_returns(data)?;
   Ok((abi.liquidity, abi.amount0, abi.amount1))
}

pub fn decode_decrease_liquidity_call(data: &Bytes) -> Result<(U256, U256), anyhow::Error> {
   let abi = INonfungiblePositionManager::decreaseLiquidityCall::abi_decode_returns(data)?;
   Ok((abi.amount0, abi.amount1))
}

pub fn decode_positions(data: &Bytes) -> Result<PositionsReturn, anyhow::Error> {
   let abi = INonfungiblePositionManager::positionsCall::abi_decode_returns(data)?;

   let nonce = abi
      .nonce
      .to_string()
      .parse::<u128>()
      .context("Failed to parse nonce")?;
   let fee = abi
      .fee
      .to_string()
      .parse::<u32>()
      .context("Failed to parse fee")?;
   let tick_lower = abi
      .tickLower
      .to_string()
      .parse::<i32>()
      .context("Failed to parse tick_lower")?;
   let tick_upper = abi
      .tickUpper
      .to_string()
      .parse::<i32>()
      .context("Failed to parse tick_upper")?;
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
   let abi = INonfungiblePositionManager::collectCall::abi_decode_returns(data)?;
   Ok((abi.amount0, abi.amount1))
}

/// Decode the output of the Mint function of the NFT Position Manager
pub fn decode_mint(bytes: &Bytes) -> Result<MintReturn, anyhow::Error> {
   let res = INonfungiblePositionManager::mintCall::abi_decode_returns(&bytes)?;
   Ok(MintReturn {
      token_id: res.tokenId,
      liquidity: res.liquidity,
      amount0: res.amount0,
      amount1: res.amount1,
   })
}

pub fn decode_collect_log(log: &LogData) -> Result<CollectLog, anyhow::Error> {
   let res = INonfungiblePositionManager::Collect::decode_raw_log(log.topics(), &log.data)?;
   Ok(CollectLog {
      owner: res.owner,
      recipient: res.recipient,
      tick_lower: i32::try_from(res.tickLower)?,
      tick_upper: i32::try_from(res.tickUpper)?,
      amount0: res.amount0,
      amount1: res.amount1,
   })
}

pub fn decode_increase_liquidity_log(log: &LogData) -> Result<IncreaseLiquidityReturn, anyhow::Error> {
   let res = INonfungiblePositionManager::IncreaseLiquidity::decode_raw_log(log.topics(), &log.data)?;
   Ok(IncreaseLiquidityReturn {
      token_id: res.tokenId,
      liquidity: res.liquidity,
      amount0: res.amount0,
      amount1: res.amount1,
   })
}

pub fn decode_decrease_liquidity_log(log: &LogData) -> Result<DecreaseLiquidityReturn, anyhow::Error> {
   let res = INonfungiblePositionManager::DecreaseLiquidity::decode_raw_log(log.topics(), &log.data)?;
   Ok(DecreaseLiquidityReturn {
      token_id: res.tokenId,
      liquidity: res.liquidity,
      amount0: res.amount0,
      amount1: res.amount1,
   })
}
