use alloy_rpc_types::BlockId;
use alloy_sol_types::{ sol, SolCall };
use alloy_primitives::{ Address, Bytes, U256 };

use alloy_provider::Provider;
use alloy_transport::Transport;
use alloy_contract::private::Network;

sol! {
    #[sol(rpc)]
    contract IERC20 {
        event Approval(address indexed owner, address indexed spender, uint value);
        event Transfer(address indexed from, address indexed to, uint value);

        function balanceOf(address owner) external view returns (uint256 balance);
        function approve(address spender, uint256 amount) external returns (bool);
        function transfer(address recipient, uint256 amount) external returns (bool);
        function transferFrom(address from, address recipient, uint256 amount) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function name() external view returns (string memory);
        function symbol() external view returns (string memory);
        function decimals() external view returns (uint8);
        function totalSupply() external view returns (uint256);
}
}

pub async fn balance_of<T, P, N>(
    token: Address,
    owner: Address,
    client: P,
    block: Option<BlockId>
)
    -> Result<U256, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let block = block.unwrap_or(BlockId::latest());
    let contract = IERC20::new(token, client);
    let b = contract.balanceOf(owner).block(block).call().await?;
    Ok(b.balance)
}

pub async fn allowance<T, P, N>(
    token: Address,
    owner: Address,
    spender: Address,
    client: P
)
    -> Result<U256, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let contract = IERC20::new(token, client);
    let a = contract.allowance(owner, spender).call().await?;
    Ok(a._0)
}

pub async fn symbol<T, P, N>(token: Address, client: P) -> Result<String, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let contract = IERC20::new(token, client);
    let s = contract.symbol().call().await?;
    Ok(s._0)
}

pub async fn name<T, P, N>(token: Address, client: P) -> Result<String, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let contract = IERC20::new(token, client);
    let n = contract.name().call().await?;
    Ok(n._0)
}

pub async fn decimals<T, P, N>(token: Address, client: P) -> Result<u8, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let contract = IERC20::new(token, client);
    let d = contract.decimals().call().await?;
    Ok(d._0)
}

pub async fn total_supply<T, P, N>(token: Address, client: P) -> Result<U256, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let contract = IERC20::new(token, client);
    let t = contract.totalSupply().call().await?;
    Ok(t._0)
}

// ** ABI Encode Functions

pub fn encode_balance_of(owner: Address) -> Bytes {
    let c = IERC20::balanceOfCall { owner };
    Bytes::from(c.abi_encode())
}

pub fn encode_allowance(owner: Address, spender: Address) -> Bytes {
    let c = IERC20::allowanceCall { owner, spender };
    Bytes::from(c.abi_encode())
}

pub fn encode_approve(spender: Address, amount: U256) -> Bytes {
    let c = IERC20::approveCall { spender, amount };
    Bytes::from(c.abi_encode())
}

pub fn encode_transfer(recipient: Address, amount: U256) -> Bytes {
    let c = IERC20::transferCall { recipient, amount };
    Bytes::from(c.abi_encode())
}

// ** ABI Decode Functions

pub fn decode_balance_of(bytes: &Bytes) -> Result<U256, anyhow::Error> {
    let b = IERC20::balanceOfCall::abi_decode_returns(&bytes, true)?;
    Ok(b.balance)
}

pub fn decode_allowance(bytes: &Bytes) -> Result<U256, anyhow::Error> {
    let a = IERC20::allowanceCall::abi_decode_returns(&bytes, true)?;
    Ok(a._0)
}