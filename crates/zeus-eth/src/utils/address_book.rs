use crate::types::ChainId;
use alloy_primitives::{Address, address};
use anyhow::bail;

pub fn vitalik() -> Address {
   address!("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045")
}

pub fn zeus_stateview(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0xa2377ec17cB3dBBFF82A1fFE9D3E29D9ea990f9B")),
      ChainId::Optimism => Ok(address!("0x443e22658d645C30629acf24552e80823fDb85e8")),
      ChainId::BinanceSmartChain => Ok(address!("0x443e22658d645C30629acf24552e80823fDb85e8")),
      ChainId::Base => Ok(address!("0x443e22658d645C30629acf24552e80823fDb85e8")),
      ChainId::Arbitrum => Ok(address!("0x443e22658d645C30629acf24552e80823fDb85e8")),
   }
}

/// https://docs.uniswap.org/contracts/v3/reference/deployments/
///
/// Returns the Permit2 contract address for the given chain id.
pub fn permit2_contract(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0x000000000022D473030F116dDEE9F6B43aC78BA3")),
      ChainId::Optimism => Ok(address!("0x000000000022D473030F116dDEE9F6B43aC78BA3")),
      ChainId::BinanceSmartChain => Ok(address!("0x000000000022D473030F116dDEE9F6B43aC78BA3")),
      ChainId::Base => Ok(address!("0x000000000022D473030F116dDEE9F6B43aC78BA3")),
      ChainId::Arbitrum => Ok(address!("0x000000000022D473030F116dDEE9F6B43aC78BA3")),
   }
}

/// ETH-USD Price Feed Chainlink
pub fn eth_usd_price_feed(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("5f4eC3Df9cbd43714FE2740f5E3616155c5b8419")),
      ChainId::Optimism => Ok(address!("13e3Ee699D1909E989722E753853AE30b17e08c5")),
      ChainId::BinanceSmartChain => Ok(address!("9ef1B8c0E4F7dc8bF5719Ea496883DC6401d5b2e")),
      ChainId::Base => Ok(address!("71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70")),
      ChainId::Arbitrum => Ok(address!("639Fe6ab55C921f74e7fac1ee960C0B6293ba612")),
   }
}

/// USDC-USD Price Feed Chainlink
pub fn usdc_usd_price_feed(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0x8fFfFfd4AfB6115b954Bd326cbe7B4BA576818f6")),
      ChainId::Optimism => Ok(address!("0x16a9FA2FDa030272Ce99B29CF780dFA30361E0f3")),
      ChainId::BinanceSmartChain => Ok(address!("0x51597f405303C4377E36123cBc172b13269EA163")),
      ChainId::Base => Ok(address!("0x7e860098F58bBFC8648a4311b374B1D669a2bc6B")),
      ChainId::Arbitrum => Ok(address!("0x50834F3163758fcC1Df9973b6e91f0F0F0434aD3")),
   }
}

/// USDT-USD Price Feed Chainlink
pub fn usdt_usd_price_feed(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0x3E7d1eAB13ad0104d2750B8863b489D65364e32D")),
      ChainId::Optimism => Ok(address!("0xECef79E109e997bCA29c1c0897ec9d7b03647F5E")),
      ChainId::BinanceSmartChain => Ok(address!("0xB97Ad0E74fa7d920791E90258A6E2085088b4320")),
      ChainId::Base => Ok(address!("0xf19d560eB8d2ADf07BD6D13ed03e1D11215721F9")),
      ChainId::Arbitrum => Ok(address!("0x3f3f5dF88dC9F13eac63DF89EC16ef6e7E25DdE7")),
   }
}

/// DAI-USD Price Feed Chainlink
pub fn dai_usd_price_feed(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0xAed0c38402a5d19df6E4c03F4E2DceD6e29c1ee9")),
      ChainId::Optimism => Ok(address!("0x8dBa75e83DA73cc766A7e5a0ee71F656BAb470d6")),
      ChainId::BinanceSmartChain => Ok(address!("0x132d3C0B1D2cEa0BC552588063bdBb210FDeecfA")),
      ChainId::Base => Ok(address!("0x591e79239a7d679378eC8c847e5038150364C78F")),
      ChainId::Arbitrum => Ok(address!("0xc5C8E77B397E531B8EC06BFb0048328B30E9eCfB")),
   }
}

/// BNB-USD Price Feed Chainlink (BSC ONLY)
pub fn bnb_usd_price_feed() -> Address {
   address!("0567F2323251f0Aab15c8dFb1967E4e8A7D42aeE")
}

/// Returns the address of the WETH token for the given chain id.
pub fn weth(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")),
      ChainId::Optimism => Ok(address!("4200000000000000000000000000000000000006")),
      ChainId::BinanceSmartChain => Ok(address!("2170Ed0880ac9A755fd29B2688956BD959F933F8")),
      ChainId::Base => Ok(address!("4200000000000000000000000000000000000006")),
      ChainId::Arbitrum => Ok(address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1")),
   }
}

/// Returns the address of the WBNB token on the given chain id.
pub fn wbnb(chain_id: u64) -> Result<Address, anyhow::Error> {
   if chain_id != crate::types::BSC {
      bail!("WBNB is only available on Binance Smart Chain")
   }
   Ok(address!("bb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"))
}

/// Returns the address of the USDC token for the given chain id.
pub fn usdc(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")),
      ChainId::Optimism => Ok(address!("7F5c764cBc14f9669B88837ca1490cCa17c31607")),
      ChainId::BinanceSmartChain => Ok(address!("8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d")),
      ChainId::Base => Ok(address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")),
      ChainId::Arbitrum => Ok(address!("af88d065e77c8cC2239327C5EDb3A432268e5831")),
   }
}

/// Returns the address of the USDT token for the given chain id.
pub fn usdt(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("dAC17F958D2ee523a2206206994597C13D831ec7")),
      ChainId::Optimism => Ok(address!("94b008aA00579c1307B0EF2c499aD98a8ce58e58")),
      ChainId::BinanceSmartChain => Ok(address!("55d398326f99059fF775485246999027B3197955")),
      ChainId::Base => bail!("USDT is not available on Base Chain"),
      ChainId::Arbitrum => Ok(address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9")),
   }
}

/// Returns the address of the DAI token for the given chain id.
pub fn dai(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("6B175474E89094C44Da98b954EedeAC495271d0F")),
      ChainId::Optimism => Ok(address!("DA10009cBd5D07dd0CeCc66161FC93D7c9000da1")),
      ChainId::BinanceSmartChain => Ok(address!("1AF3F329e8BE154074D8769D1FFa4eE058B1DBc3")),
      ChainId::Base => Ok(address!("50c5725949A6F0c72E6C4a641F24049A917DB0Cb")),
      ChainId::Arbitrum => Ok(address!("DA10009cBd5D07dd0CeCc66161FC93D7c9000da1")),
   }
}

/// Returns the address of the WBTC token for the given chain id.
pub fn wbtc(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599")),
      ChainId::Optimism => Ok(address!("68f180fcCe6836688e9084f035309E29Bf0A2095")),
      ChainId::BinanceSmartChain => Ok(address!("0555E30da8f98308EdB960aa94C0Db47230d2B9c")),
      ChainId::Base => Ok(address!("0555E30da8f98308EdB960aa94C0Db47230d2B9c")),
      ChainId::Arbitrum => Ok(address!("2f2a2543B76A4166549F7aaB2e75Bef0aefC5B0f")),
   }
}

/// Returns the address of the stETH token
///
/// Ethereum Mainnet Only
pub fn steth() -> Address {
   address!("0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84")
}

/// Return the address of the UniswapV4 Stateview contract on the given chain id.
pub fn uniswap_v4_stateview(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0x7fFE42C4a5DEeA5b0feC41C94C136Cf115597227")),
      ChainId::Optimism => Ok(address!("0xc18a3169788F4F75A170290584ECA6395C75Ecdb")),
      ChainId::BinanceSmartChain => Ok(address!("0xd13Dd3D6E93f276FAfc9Db9E6BB47C1180aeE0c4")),
      ChainId::Base => Ok(address!("0xA3c0c9b65baD0b08107Aa264b0f3dB444b867A71")),
      ChainId::Arbitrum => Ok(address!("0x76Fd297e2D437cd7f76d50F01AfE6160f86e9990")),
   }
}

/// Return the address of the UniswapV4 PoolManager contract on the given chain id.
pub fn uniswap_v4_pool_manager(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0x000000000004444c5dc75cB358380D2e3dE08A90")),
      ChainId::Optimism => Ok(address!("0x9a13F98Cb987694C9F086b1F5eB990EeA8264Ec3")),
      ChainId::BinanceSmartChain => Ok(address!("0x28e2ea090877bf75740558f6bfb36a5ffee9e9df")),
      ChainId::Base => Ok(address!("0x498581fF718922c3f8e6A244956aF099B2652b2b")),
      ChainId::Arbitrum => Ok(address!("0x360e68faccca8ca495c1b759fd9eee466db9fb32")),
   }
}

/// Return the address of the Uniswap UniversalRouter V2 contract on the given chain id.
pub fn universal_router_v2(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0x66a9893cc07d91d95644aedd05d03f95e1dba8af")),
      ChainId::Optimism => Ok(address!("0x851116d9223fabed8e56c0e6b8ad0c31d98b3507")),
      ChainId::BinanceSmartChain => Ok(address!("0x1906c1d672b88cd1b9ac7593301ca990f94eae07")),
      ChainId::Base => Ok(address!("0x6ff5693b99212da76ad316178a184ab56d299b43")),
      ChainId::Arbitrum => Ok(address!("0xa51afafe0263b40edaef0df8781ea9aa03e381a3")),
   }
}

/// Return the address of the UniswapV4 Quoter contract on the given chain id.
pub fn uniswap_v4_quoter(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0x52f0e24d1c21c8a0cb1e5a5dd6198556bd9e1203")),
      ChainId::Optimism => Ok(address!("0x1f3131a13296fb91c90870043742c3cdbff1a8d7")),
      ChainId::BinanceSmartChain => Ok(address!("0x9f75dd27d6664c475b90e105573e550ff69437b0")),
      ChainId::Base => Ok(address!("0x0d5e0f971ed27fbff6c2837bf31316121532048d")),
      ChainId::Arbitrum => Ok(address!("0x76fd297e2d437cd7f76d50f01afe6160f86e9990")),
   }
}

/// Return the Uniswap's NonfungiblePositionManager contract address for the given chain id.
pub fn uniswap_nft_position_manager(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("C36442b4a4522E871399CD717aBDD847Ab11FE88")),
      ChainId::Optimism => Ok(address!("C36442b4a4522E871399CD717aBDD847Ab11FE88")),
      ChainId::BinanceSmartChain => Ok(address!("7b8A01B39D58278b5DE7e48c8449c9f4F5170613")),
      ChainId::Base => Ok(address!("03a520b32C04BF3bEEf7BEb72E919cf822Ed34f1")),
      ChainId::Arbitrum => Ok(address!("C36442b4a4522E871399CD717aBDD847Ab11FE88")),
   }
}

/// Returns the Uniswap V2 Factory address for the given chain id.
pub fn uniswap_v2_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f")),
      ChainId::Optimism => Ok(address!("0c3c1c532F1e39EdF36BE9Fe0bE1410313E074Bf")),
      ChainId::BinanceSmartChain => Ok(address!("8909Dc15e40173Ff4699343b6eB8132c65e18eC6")),
      ChainId::Base => Ok(address!("8909Dc15e40173Ff4699343b6eB8132c65e18eC6")),
      ChainId::Arbitrum => Ok(address!("f1D7CC64Fb4452F05c498126312eBE29f30Fbcf9")),
   }
}

/// Returns the Uniswap V2 Router address for the given chain id.
pub fn uniswap_v2_router(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("7a250d5630B4cF539739dF2C5dAcb4c659F2488D")),
      ChainId::Optimism => Ok(address!("4A7b5Da61326A6379179b40d00F57E5bbDC962c2")),
      ChainId::BinanceSmartChain => Ok(address!("4752ba5dbc23f44d87826276bf6fd6b1c372ad24")),
      ChainId::Base => Ok(address!("4752ba5dbc23f44d87826276bf6fd6b1c372ad24")),
      ChainId::Arbitrum => Ok(address!("4752ba5dbc23f44d87826276bf6fd6b1c372ad24")),
   }
}

/// Returns the Uniswap V3 Factory address for the given chain id.
pub fn uniswap_v3_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("1F98431c8aD98523631AE4a59f267346ea31F984")),
      ChainId::Optimism => Ok(address!("1F98431c8aD98523631AE4a59f267346ea31F984")),
      ChainId::BinanceSmartChain => Ok(address!("dB1d10011AD0Ff90774D0C6Bb92e5C5c8b4461F7")),
      ChainId::Base => Ok(address!("33128a8fC17869897dcE68Ed026d694621f6FDfD")),
      ChainId::Arbitrum => Ok(address!("1F98431c8aD98523631AE4a59f267346ea31F984")),
   }
}

/// Returns the PancakeSwap V2 Factory address for the given chain id.
pub fn pancakeswap_v2_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("1097053Fd2ea711dad45caCcc45EfF7548fCB362")),
      ChainId::BinanceSmartChain => Ok(address!("cA143Ce32Fe78f1f7019d7d551a6402fC5350c73")),
      ChainId::Base => Ok(address!("02a84c1b3BBD7401a5f7fa98a384EBC70bB5749E")),
      ChainId::Arbitrum => Ok(address!("02a84c1b3BBD7401a5f7fa98a384EBC70bB5749E")),
      ChainId::Optimism => bail!("PancakeSwap V2 is not available on Optimism"),
   }
}

/// Returns the PancakeSwap V2 Router address for the given chain id.
pub fn pancakeswap_v2_router(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("EfF92A263d31888d860bD50809A8D171709b7b1c")),
      ChainId::BinanceSmartChain => Ok(address!("10ED43C718714eb63d5aA57B78B54704E256024E")),
      ChainId::Base => Ok(address!("8cFe327CEc66d1C090Dd72bd0FF11d690C33a2Eb")),
      ChainId::Arbitrum => Ok(address!("8cFe327CEc66d1C090Dd72bd0FF11d690C33a2Eb")),
      ChainId::Optimism => bail!("PancakeSwap V2 is not available on Optimism"),
   }
}

/// Returns the PancakeSwap V3 Factory address for the given chain id.
pub fn pancakeswap_v3_factory(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
      ChainId::BinanceSmartChain => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
      ChainId::Base => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
      ChainId::Arbitrum => Ok(address!("0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865")),
      ChainId::Optimism => bail!("PancakeSwap V3 is not available on Optimism"),
   }
}

/// Returns the PancakeSwap V3 Smart Router address for the given chain id.
pub fn pancakeswap_v3_router(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("13f4EA83D0bd40E75C8222255bc855a974568Dd4")),
      ChainId::BinanceSmartChain => Ok(address!("13f4EA83D0bd40E75C8222255bc855a974568Dd4")),
      ChainId::Base => Ok(address!("678Aa4bF4E210cf2166753e054d5b7c31cc7fa86")),
      ChainId::Arbitrum => Ok(address!("32226588378236Fd0c7c4053999F88aC0e5cAc77")),
      ChainId::Optimism => bail!("PancakeSwap V3 is not available on Optimism"),
   }
}

/// Return the address of the Across SpokePool V2 contract on the specified chain
pub fn across_spoke_pool_v2(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum => Ok(address!("5c7BCd6E7De5423a257D81B442095A1a6ced35C5")),
      ChainId::Optimism => Ok(address!("6f26Bf09B1C792e3228e5467807a900A503c0281")),
      ChainId::Base => Ok(address!("09aea4b2242abC8bb4BB78D537A67a245A7bEC64")),
      ChainId::Arbitrum => Ok(address!("e35e9842fceaca96570b734083f4a58e8f7c5f2a")),
      ChainId::BinanceSmartChain => bail!("Across Protocol does not support BSC"),
   }
}
