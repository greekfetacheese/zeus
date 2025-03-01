use alloy_primitives::{Bytes, U256};
use alloy_sol_types::{SolCall, sol};

sol! {
    #[sol(rpc)]
    contract IWETH9 {
        event Deposit(address indexed dst, uint wad);
        event Withdrawal(address indexed src, uint wad);

        function deposit() external payable;
        function withdraw(uint256 amount) external;
}
}

pub fn encode_deposit() -> Bytes {
   let c = IWETH9::depositCall {};
   Bytes::from(c.abi_encode())
}

pub fn encode_withdraw(amount: U256) -> Bytes {
   let c = IWETH9::withdrawCall { amount };
   Bytes::from(c.abi_encode())
}
