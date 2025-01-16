// ! Commonly used addresses

use alloy_primitives::{address, Address};
use anyhow::anyhow;

/// Return the address of the Native wrapped token on the given chain id.
pub fn native_wrapped_token(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(weth(chain_id)?),
        // Optimism
        10 => Ok(weth(chain_id)?),
        // BSC Mainnet
        56 => Ok(wbnb(chain_id)?),
        // OP Base
        8453 => Ok(wbnb(chain_id)?),
        // Arbitrum
        42161 => Ok(weth(chain_id)?),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}

/// Returns the address of the WETH token on the given chain id.
pub fn weth(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")),
        // Optimism
        10 => Ok(address!("4200000000000000000000000000000000000006")),
        // OP Base
        8453 => Ok(address!("4200000000000000000000000000000000000006")),
        // Arbitrum
        42161 => Ok(address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
}
}

/// Returns the address of the WBNB token on the given chain id.
pub fn wbnb(chain_id: u64) -> Result<Address, anyhow::Error> {
    if chain_id != 56 {
        return Err(anyhow!("Wrong ChainId expected 56 but got {}", chain_id));
    }
    Ok(address!("bb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"))
}

/// Returns the address of the USDC token on the given chain id.
pub fn usdc(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")),
        // Optimism
        10 => Ok(address!("7F5c764cBc14f9669B88837ca1490cCa17c31607")),
        // BSC Mainnet
        56 => Ok(address!("8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d")),
        // OP Base
        8453 => Ok(address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")),
        // Arbitrum
        42161 => Ok(address!("af88d065e77c8cC2239327C5EDb3A432268e5831")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}

/// Returns the address of the USDT token on the given chain id.
pub fn usdt(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("dAC17F958D2ee523a2206206994597C13D831ec7")),
        // Optimism
        10 => Ok(address!("94b008aA00579c1307B0EF2c499aD98a8ce58e58")),
        // BSC Mainnet
        56 => Ok(address!("55d398326f99059fF775485246999027B3197955")),
        // OP Base
        8453 => Err(anyhow!("USDT is not available on chain id: {}", chain_id)),
        // Arbitrum
        42161 => Ok(address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}

/// Returns the address of the DAI token on the given chain id.
pub fn dai(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("6B175474E89094C44Da98b954EedeAC495271d0F")),
        // Optimism
        10 => Ok(address!("DA10009cBd5D07dd0CeCc66161FC93D7c9000da1")),
        // BSC Mainnet
        56 => Ok(address!("1AF3F329e8BE154074D8769D1FFa4eE058B1DBc3")),
        // OP Base
        8453 => Ok(address!("50c5725949A6F0c72E6C4a641F24049A917DB0Cb")),
        // Arbitrum
        42161 => Ok(address!("DA10009cBd5D07dd0CeCc66161FC93D7c9000da1")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}

/// Returns the address of the WBTC token on the given chain id.
pub fn wbtc(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599")),
        // Optimism
        10 => Ok(address!("68f180fcCe6836688e9084f035309E29Bf0A2095")),
        // BSC Mainnet
        56 => Ok(address!("0555E30da8f98308EdB960aa94C0Db47230d2B9c")),
        // OP Base
        8453 => Ok(address!("0555E30da8f98308EdB960aa94C0Db47230d2B9c")),
        // Arbitrum
        42161 => Ok(address!("2f2a2543B76A4166549F7aaB2e75Bef0aefC5B0f")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}


/// Returns the Uniswap V2 Factory address on the given chain id.
pub fn uniswap_v2_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f")),
        // Optimism
        10 => Ok(address!("0c3c1c532F1e39EdF36BE9Fe0bE1410313E074Bf")),
        // BSC Mainnet
        56 => Ok(address!("8909Dc15e40173Ff4699343b6eB8132c65e18eC6")),
        // OP Base
        8453 => Ok(address!("8909Dc15e40173Ff4699343b6eB8132c65e18eC6")),
        // Arbitrum
        42161 => Ok(address!("f1D7CC64Fb4452F05c498126312eBE29f30Fbcf9")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}

/// Returns the Uniswap V2 Router address on the given chain id.
pub fn uniswap_v2_router(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("7a250d5630B4cF539739dF2C5dAcb4c659F2488D")),
        // Optimism
        10 => Ok(address!("4A7b5Da61326A6379179b40d00F57E5bbDC962c2")),
        // BSC Mainnet
        56 => Ok(address!("4752ba5dbc23f44d87826276bf6fd6b1c372ad24")),
        // OP Base
        8453 => Ok(address!("4752ba5dbc23f44d87826276bf6fd6b1c372ad24")),
        // Arbitrum
        42161 => Ok(address!("4752ba5dbc23f44d87826276bf6fd6b1c372ad24")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}


/// Returns the Uniswap V3 Factory address on the given chain id.
pub fn uniswap_v3_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("1F98431c8aD98523631AE4a59f267346ea31F984")),
        // Optimism
        10 => Ok(address!("1F98431c8aD98523631AE4a59f267346ea31F984")),
        // BSC Mainnet
        56 => Ok(address!("dB1d10011AD0Ff90774D0C6Bb92e5C5c8b4461F7")),
        // OP Base
        8453 => Ok(address!("33128a8fC17869897dcE68Ed026d694621f6FDfD")),
        // Arbitrum
        42161 => Ok(address!("1F98431c8aD98523631AE4a59f267346ea31F984")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}


/// Returns the PancakeSwap V2 Factory address on the given chain id.
pub fn pancakeswap_v2_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("1097053Fd2ea711dad45caCcc45EfF7548fCB362")),
        // BSC Mainnet
        56 => Ok(address!("cA143Ce32Fe78f1f7019d7d551a6402fC5350c73")),
        // OP Base
        8453 => Ok(address!("02a84c1b3BBD7401a5f7fa98a384EBC70bB5749E")),
        // Arbitrum
        42161 => Ok(address!("02a84c1b3BBD7401a5f7fa98a384EBC70bB5749E")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}

/// Returns the PancakeSwap V2 Router address on the given chain id.
pub fn pancakeswap_v2_router(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("EfF92A263d31888d860bD50809A8D171709b7b1c")),
        // BSC Mainnet
        56 => Ok(address!("10ED43C718714eb63d5aA57B78B54704E256024E")),
        // OP Base
        8453 => Ok(address!("8cFe327CEc66d1C090Dd72bd0FF11d690C33a2Eb")),
        // Arbitrum
        42161 => Ok(address!("8cFe327CEc66d1C090Dd72bd0FF11d690C33a2Eb")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}


/// Returns the PancakeSwap V3 Factory address on the given chain id.
pub fn pancakeswap_v3_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
        // BSC Mainnet
        56 => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
        // OP Base
        8453 => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
        // Arbitrum
        42161 => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}


/// Returns the PancakeSwap V3 Smart Router address on the given chain id.
pub fn pancakeswap_v3_router(chain_id: u64) -> Result<Address, anyhow::Error> {
    match chain_id {
        // ETH Mainnet
        1 => Ok(address!("13f4EA83D0bd40E75C8222255bc855a974568Dd4")),
        // BSC Mainnet
        56 => Ok(address!("13f4EA83D0bd40E75C8222255bc855a974568Dd4")),
        // OP Base
        8453 => Ok(address!("678Aa4bF4E210cf2166753e054d5b7c31cc7fa86")),
        // Arbitrum
        42161 => Ok(address!("32226588378236Fd0c7c4053999F88aC0e5cAc77")),
        _ => Err(anyhow!("Unsupported chain id: {}", chain_id)),
    }
}