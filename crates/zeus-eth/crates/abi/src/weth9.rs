use alloy_primitives::{Bytes, LogData, U256};
use alloy_sol_types::{SolCall, SolEvent, sol};

pub use IWETH9::{Deposit, Withdrawal};

sol! {
    #[sol(rpc)]
    contract IWETH9 {
        event Deposit(address indexed dst, uint wad);
        event Withdrawal(address indexed src, uint wad);

        function deposit() external payable;
        function withdraw(uint256 amount) external;
}
}

pub fn deposit_selector() -> [u8; 4] {
   IWETH9::depositCall::SELECTOR
}

pub fn withdraw_selector() -> [u8; 4] {
   IWETH9::withdrawCall::SELECTOR
}

pub fn deposit_signature() -> &'static str {
   IWETH9::depositCall::SIGNATURE
}

pub fn withdraw_signature() -> &'static str {
   IWETH9::withdrawCall::SIGNATURE
}

pub fn decode_deposit_log(log: &LogData) -> Result<Deposit, anyhow::Error> {
   let b = IWETH9::Deposit::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_withdraw_log(log: &LogData) -> Result<Withdrawal, anyhow::Error> {
   let b = IWETH9::Withdrawal::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn encode_deposit() -> Bytes {
   let c = IWETH9::depositCall {};
   Bytes::from(c.abi_encode())
}

pub fn encode_withdraw(amount: U256) -> Bytes {
   let c = IWETH9::withdrawCall { amount };
   Bytes::from(c.abi_encode())
}
