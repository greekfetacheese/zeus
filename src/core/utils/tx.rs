use zeus_eth::{
    wallet::{SafeSigner, SafeWallet},
    alloy_contract::private::Provider,
    alloy_primitives::{ Address, Bytes, U256, utils::{ format_ether, parse_units } },
    alloy_network::{ Ethereum, TransactionBuilder },
    alloy_rpc_types::{ BlockId, BlockTransactionsKind, TransactionRequest, TransactionReceipt },
    utils::block::calculate_next_block_base_fee,
    currency::Currency,
    types::ChainId,
};
use anyhow::anyhow;



pub type Amount = U256;

#[derive(Debug, Clone)]
pub enum TxType {
    /// ETH or ERC20 transfer
    Transfer(Amount, Currency),
    Other(Amount),
}

#[derive(Clone)]
pub struct TxParams {
    pub tx_type: TxType,
    pub signer: SafeSigner,
    pub recipient: Option<Address>,
    pub chain: u64,
    /// Priority fee in Gwei
    pub miner_tip: U256,
}

impl TxParams {
    pub fn new(
        tx_type: TxType,
        signer: SafeSigner,
        recipient: Option<Address>,
        chain: u64,
        miner_tip: U256,
    ) -> Self {
        Self {
            tx_type,
            signer,
            recipient,
            chain,
            miner_tip,
        }
    }

    pub fn transfer(
        tx_type: TxType,
        signer: SafeSigner,
        recipient: Address,
        chain: u64,
        miner_tip: U256,
    ) -> Self {
        Self {
            tx_type,
            signer,
            recipient: Some(recipient),
            chain,
            miner_tip,
        }
    }
}

fn has_funds(chain: ChainId, gas_cost: U256, balance: U256) -> Result<(), anyhow::Error> {
    let symbol = chain.coin_symbol();
    let gas_cost = format_ether(gas_cost);
    let balance = format_ether(balance);

    if balance < gas_cost {
        return Err(
            anyhow!(
                "Insufficient balance to cover gas fees, need at least {} {} but you have {} {}",
                gas_cost,
                symbol,
                balance,
                symbol
            )
        );
    }

    Ok(())
}

pub async fn send_tx<P>(client: P, params: TxParams) -> Result<TransactionReceipt, anyhow::Error>
    where P: Provider<(), Ethereum> + Clone + 'static
{
    let chain = ChainId::new(params.chain)?;
    let block = client.get_block(BlockId::latest(), BlockTransactionsKind::Hashes).await?;
    if block.is_none() {
        return Err(
            anyhow!("Latest block not found, check with your RPC provider or try again later")
        );
    }


    let nonce = client.get_transaction_count(params.signer.inner().address()).await?;
    let mut tx = build_tx(params.clone())?;
    tx.set_nonce(nonce);

    let signer = SafeWallet::from(params.signer.clone());

    let tx_envelope = tx.clone().build(&signer.inner()).await?;

    // calculate the estimated cost of the transaction
    let base_fee = calculate_next_block_base_fee(block.unwrap());
    let gas_used = client.estimate_gas(&tx).await?;
    let gas_cost = U256::from(gas_used * base_fee);
    let balance = client.get_balance(params.signer.inner().address()).await?;
    has_funds(chain, gas_cost, balance)?;

    let receipt = client
        .send_tx_envelope(tx_envelope).await?
        .with_required_confirmations(2)
        .with_timeout(Some(std::time::Duration::from_secs(30)))
        .get_receipt().await?;

    Ok(receipt)
}

fn build_tx(params: TxParams) -> Result<TransactionRequest, anyhow::Error> {
    let tx = match params.tx_type {
        TxType::Transfer(amount, currency) => {
            let amount = parse_units(&amount.to_string(), currency.decimals())?.get_absolute();

            let (call_data, gas_limit, to) = if currency.is_native() {
                let data = Bytes::default();
                let recipient = params.recipient.ok_or(
                    anyhow!("Recipient address is required for ETH transfers")
                )?;
                (data, 21_000, recipient)
            } else {
                let token = currency.erc20().unwrap();
                let recipient = params.recipient.ok_or(
                    anyhow!("Recipient address is required for ERC20 transfers")
                )?;
                let data = token.encode_transfer(recipient, amount);
                (data, 100_000, token.address)
            };

            TransactionRequest::default()
                .with_from(params.signer.inner().address())
                .with_to(to)
                .with_chain_id(params.chain)
                .with_value(amount)
                .with_gas_limit(gas_limit)
                .with_input(call_data)
                .with_max_priority_fee_per_gas(params.miner_tip.to::<u128>())
                .max_fee_per_gas(params.miner_tip.to::<u128>())
        }
        _ => todo!(),
    };

    Ok(tx)
}
