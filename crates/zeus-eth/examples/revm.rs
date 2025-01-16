use alloy_provider::{ProviderBuilder, Provider, WsConnect};
use alloy_primitives::{address, U256, utils::{parse_units, format_units}};
use revm::db::{CacheDB, EmptyDB};
use std::sync::Arc;

use zeus_eth::prelude::{AccountType, ERC20Token, ForkFactory, weth, usdc, usdt, DummyAccount, new_evm};
use zeus_eth::revm_utils::simulate::{can_tranfer_erc20, erc20_balance};


#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let url = "wss://eth.merkle.io";
    let client = ProviderBuilder::new().on_ws(WsConnect::new(url)).await?;
    let client = Arc::new(client);
    let chain_id = client.get_chain_id().await?;

    let weth = ERC20Token::new(client.clone(), weth(chain_id)?, chain_id).await?;
    let usdc = ERC20Token::new(client.clone(), usdc(chain_id)?, chain_id).await?;
    let usdt = ERC20Token::new(client.clone(), usdt(chain_id)?, chain_id).await?;

    // create a dummy account and fund it with 1000 ETH
    let amount = parse_units("1000", 18)?.get_absolute();
    let alice = DummyAccount::new(AccountType::EOA, amount);

    // create a new fork based on the latest block
    let db = CacheDB::new(EmptyDB::new());
    let mut fork_factory = ForkFactory::new_sandbox_factory(client, db, None);

    // insert alice in the fork and fund the weth, usdc and usdt balances

    // for most tokens the total supply as amount works fine if you need a really high amount
    // ! However be careful using [U256::MAX] as can lead to Blacklist errors for tokens like USDC
    alice.insert(&mut fork_factory, weth.clone(), weth.total_supply)?;
    alice.insert(&mut fork_factory, usdc.clone(), usdc.total_supply)?;
    alice.insert(&mut fork_factory, usdt.clone(), usdt.total_supply)?;

    // create the forkdb
    let fork_db = fork_factory.new_sandbox_fork();

    // create a new evm instance
    let mut evm = new_evm(fork_db, None);

    let tokens = vec![weth.clone(), usdc.clone(), usdt.clone()];

    // check the erc20 balance of alice
    for token in &tokens {
        let balance = erc20_balance(&mut evm, token.clone(), alice.address)?;
        let bal = format_units(balance, token.decimals)?;
        println!("Alice's balance of {} is: {}", token.symbol, bal);
    }

    // see if we can transfer the tokens
    let dead = address!("000000000000000000000000000000000000dEaD");
    for token in &tokens {
        let (success, err) = can_tranfer_erc20(&mut evm, token.clone(), alice.address, dead, U256::from(1))?;
        if success {
            println!("Alice can transfer {}", token.symbol);
        } else {
            println!("Alice CANNOT transfer {} Reason: {}", token.symbol, err);
        }
    }
    println!("===============================");

    // now lets try this again but will fund alice with U256::MAX on the tokens
    let alice = DummyAccount::new(AccountType::EOA, U256::MAX);

    // Now we fund alice with U256::MAX on the tokens
    // The USDC transfer here should fail
    alice.insert(&mut fork_factory, weth.clone(), U256::MAX)?;
    alice.insert(&mut fork_factory, usdc.clone(), U256::MAX)?;
    alice.insert(&mut fork_factory, usdt.clone(), U256::MAX)?;

    let fork_db = fork_factory.new_sandbox_fork();
    let mut evm = new_evm(fork_db, None);

    for token in tokens {
        let (success, err) = can_tranfer_erc20(&mut evm, token.clone(), alice.address, dead, U256::from(1))?;
        if success {
            println!("Alice can transfer {}", token.symbol);
        } else {
            println!("Alice CANNOT transfer {} Reason: {}", token.symbol, err);
        }
    }


    Ok(())
}