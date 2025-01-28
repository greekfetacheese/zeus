pub mod chain_link;
pub mod common_addr;

use crate::prelude::ERC20Token;

/// Is this token a common paired token?
///
/// Common paired tokens are WETH, WBNB, USDC, USDT, etc.
pub fn is_common_paired_token(token: &ERC20Token) -> bool {
    let weth = common_addr::weth(token.chain_id).is_ok_and(|addr| addr == token.address);
    let wbnb = common_addr::wbnb(token.chain_id).is_ok_and(|addr| addr == token.address);
    let usdc = common_addr::usdc(token.chain_id).is_ok_and(|addr| addr == token.address);
    let usdt = common_addr::usdt(token.chain_id).is_ok_and(|addr| addr == token.address);

    if weth || wbnb || usdc || usdt {
        return true;
    } else {
        return false;
    }
}