pub mod chain_link;
pub mod common_addr;

use crate::prelude::ERC20Token;
use crate::{ETH, BASE, BSC, OPTIMISM, ARBITRUM};

/// Is this token a base token?
///
/// We consider base tokens those that are mostly used for liquidity.
/// 
/// eg. WETH, WBNB, USDC, USDT, DAI are all base tokens.
pub fn is_base_token(token: &ERC20Token) -> bool {
    let weth = common_addr::weth(token.chain_id).is_ok_and(|addr| addr == token.address);
    let wbnb = common_addr::wbnb(token.chain_id).is_ok_and(|addr| addr == token.address);
    let usdc = common_addr::usdc(token.chain_id).is_ok_and(|addr| addr == token.address);
    let usdt = common_addr::usdt(token.chain_id).is_ok_and(|addr| addr == token.address);
    let dai = common_addr::dai(token.chain_id).is_ok_and(|addr| addr == token.address);

    if weth || wbnb || usdc || usdt || dai {
        return true;
    } else {
        return false;
    }
}

/// Return a list of base tokens based on the chain id.
pub fn base_tokens(chain_id: u64) -> Vec<ERC20Token> {
    let mut tokens = Vec::new();

    if chain_id == ETH {
        tokens.push(ERC20Token::weth());
        tokens.push(ERC20Token::usdc());
        tokens.push(ERC20Token::usdt());
        tokens.push(ERC20Token::dai());
    } else if chain_id == BSC {
        tokens.push(ERC20Token::wbnb());
        tokens.push(ERC20Token::usdc_bsc());
        tokens.push(ERC20Token::usdt_bsc());
        tokens.push(ERC20Token::dai_bsc());
    } else if chain_id == OPTIMISM {
        tokens.push(ERC20Token::weth_op());
        tokens.push(ERC20Token::usdc_op());
        tokens.push(ERC20Token::usdt_op());
        tokens.push(ERC20Token::dai_op());
    } else if chain_id == ARBITRUM {
        tokens.push(ERC20Token::weth_arbitrum());
        tokens.push(ERC20Token::usdc_arbitrum());
        tokens.push(ERC20Token::usdt_arbitrum());
        tokens.push(ERC20Token::dai_arbitrum());
    } else if chain_id == BASE {
        tokens.push(ERC20Token::weth_base());
        tokens.push(ERC20Token::usdc_base());
        tokens.push(ERC20Token::dai_base());
    }

    tokens
}