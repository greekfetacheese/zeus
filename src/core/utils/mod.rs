use tokio::runtime::Runtime;


use std::path::PathBuf;
use lazy_static::lazy_static;
use super::{ Wallet, ZeusCtx };
use anyhow::anyhow;

use alloy_network::{TransactionBuilder, EthereumWallet};
use zeus_eth::{
    types::*,
    alloy_provider::Provider,
    alloy_primitives::{Address, U256, utils::parse_units},
    currency::{Currency, ERC20Token},
    alloy_rpc_types::TransactionRequest,
};

pub mod trace;

lazy_static! {
    pub static ref RT: Runtime = Runtime::new().unwrap();
}


pub mod fetch;

/// Zeus data directory
pub fn data_dir() -> Result<PathBuf, anyhow::Error> {
    let dir = std::env::current_dir()?.join("data");

    if !dir.exists() {
        std::fs::create_dir_all(dir.clone())?;
    }

    Ok(dir)
}

/// Pool data directory
pub fn pool_data_dir() -> Result<PathBuf, anyhow::Error> {
    let dir = data_dir()?.join("pool_data.json");
    Ok(dir)
}



pub async fn send_crypto(
    ctx: ZeusCtx,
    sender: Wallet,
    to: Address,
    currency: Currency,
    amount: U256,
    fee: String,
    chain: u64
) -> Result<(), anyhow::Error> {
    let client = ctx.get_client_with_id(chain)?;

    if to.is_zero() {
        return Err(anyhow!("Invalid recipient address"));
    }

    if amount.is_zero() {
        return Err(anyhow!("Amount cannot be 0"));
    }

    let fee = if fee.is_empty() {
        parse_units("1", "gwei")?.get_absolute()
    } else {
        parse_units(&fee, "gwei")?.get_absolute()
    };

    let miner_tip = U256::from(fee);
    let from = sender.key.address();
    let nonce = client.get_transaction_count(from).await?;

    let tx = if currency.is_native() {
        let amount = parse_units(&amount.to_string(), currency.decimals())?.get_absolute();

        TransactionRequest::default()
            .with_from(from)
            .with_to(to)
            .with_chain_id(chain)
            .with_value(amount)
            .with_nonce(nonce)
            .with_gas_limit(21_000)
            .with_max_priority_fee_per_gas(miner_tip.to::<u128>())
            .with_max_fee_per_gas(miner_tip.to::<u128>())
    } else {
        let token = currency.erc20().unwrap();
        let amount = parse_units(&amount.to_string(), token.decimals)?.get_absolute();
        let call_data = token.encode_transfer(to, amount);

        TransactionRequest::default()
            .with_from(from)
            .with_to(token.address)
            .with_chain_id(chain)
            .with_value(U256::ZERO)
            .with_nonce(nonce)
            .with_gas_limit(100_000)
            .with_max_priority_fee_per_gas(miner_tip.to::<u128>())
            .with_max_fee_per_gas(miner_tip.to::<u128>())
            .with_input(call_data)
    };

    let signer = EthereumWallet::new(sender.key.clone());
    let tx_envelope = tx.build(&signer).await?;
    
    let receipt = client
        .send_tx_envelope(tx_envelope).await?
        .with_required_confirmations(2)
        .with_timeout(Some(std::time::Duration::from_secs(30)))
        .get_receipt().await?;

    Ok(())
}

/// Sync all the V2 & V3 pools for all the tokens
pub async fn sync_pools(ctx: ZeusCtx, chains: Vec<u64>) -> Result<(), anyhow::Error> {
    const MAX_RETRY: usize = 5;

    for chain in chains {
        let currencies = ctx.get_currencies(chain);

        for currency in &*currencies {
            if currency.is_native() {
                continue;
            }

            let token = currency.erc20().unwrap();
            let ctx = ctx.clone();

            let mut retry = 0;
            let mut v2_pools = None;
            let mut v3_pools = None;

            while v2_pools.is_none() && retry < MAX_RETRY {
                match fetch::get_v2_pools_for_token(ctx.clone(), token.clone()).await {
                    Ok(pools) => {
                        v2_pools = Some(pools);
                    }
                    Err(e) => tracing::error!("Error getting v2 pools: {:?}", e),
                }
                retry += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            retry = 0;
            while v3_pools.is_none() && retry < MAX_RETRY {
                match fetch::get_v3_pools_for_token(ctx.clone(), token.clone()).await {
                    Ok(pools) => {
                        v3_pools = Some(pools);
                    }
                    Err(e) => tracing::error!("Error getting v3 pools: {:?}", e),
                }
                retry += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            if let Some(v2_pools) = v2_pools {
                tracing::info!("Got {} v2 pools for: {}", v2_pools.len(), token.symbol);
                ctx.add_v2_pools(v2_pools);
            }

            if let Some(v3_pools) = v3_pools {
                tracing::info!("Got {} v3 pools for: {}", v3_pools.len(), token.symbol);
                ctx.add_v3_pools(v3_pools);
            }
        }
    }

    ctx.save_pool_data()?;

    Ok(())
}

/// Update the necceary data
pub async fn update(ctx: ZeusCtx) {
    RT.spawn(async move {
        update_price_manager(ctx.clone()).await;
    });
}

pub async fn update_price_manager(ctx: ZeusCtx) {
    const INTERVAL: u64 = 600;

    let mut time_passed = std::time::Instant::now();

    loop {
        if time_passed.elapsed().as_secs() > INTERVAL {
            let pool_manager = ctx.pool_manager();

            for chain in SUPPORTED_CHAINS {
                let client = ctx.get_client_with_id(chain).unwrap();
                let base_tokens = ERC20Token::base_tokens(chain);
                let res = pool_manager.update(client.clone(), chain, base_tokens).await;
                if let Err(e) = res {
                    tracing::error!("Error updating pool manager: {:?}", e);
                }
            }
            time_passed = std::time::Instant::now();

            ctx.save_pool_data().unwrap();
            tracing::info!("Pool State Manager updated");
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
