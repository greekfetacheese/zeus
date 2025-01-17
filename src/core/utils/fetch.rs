use crate::core::ZeusCtx;
use zeus_eth::alloy_primitives::{ Address, U256 };
use zeus_eth::defi::currency::Currency;
use zeus_eth::prelude::ERC20Token;

/// Get the ERC20 Token from the blockchain and update the db
pub async fn get_erc20_token(ctx: ZeusCtx, token_address: Address, chain_id: u64) -> Result<ERC20Token, anyhow::Error> {
    let client = ctx.get_client()?;
    let owner = ctx.wallet().key.address();

    let token = ERC20Token::new(client.clone(), token_address, chain_id).await?;

    let balance = if owner != Address::ZERO {
        token.balance_of(owner, client.clone(), None).await?
    } else {
        U256::ZERO
    };

    // Update the db
    ctx.write(|ctx| {
        let currency = Currency::from_erc20(token.clone());

        ctx.db.insert_currency(chain_id, currency);
        ctx.db.insert_token_balance(chain_id, owner, token.address, balance);

        let time = std::time::Instant::now();
        ctx.db.save_to_file().unwrap();
        tracing::info!("Time to save zeus db {:?}", time.elapsed().as_millis());
    });

    Ok(token)
}
