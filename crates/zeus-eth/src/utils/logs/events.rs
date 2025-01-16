use crate::defi::currency::erc20::ERC20Token;
use alloy_primitives::{utils::format_units, Address, U256};
use serde::{Deserialize, Serialize};

/// An Event that took place
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    Swap(SwapData),
    TokenTransfer(ERC20Transfer),
}

impl Event {
    pub fn is_swap(&self) -> bool {
        matches!(self, Event::Swap(_))
    }

    pub fn is_token_transfer(&self) -> bool {
        matches!(self, Event::TokenTransfer(_))
    }

    pub fn get_swap(&self) -> Option<&SwapData> {
        match self {
            Event::Swap(data) => Some(data),
            _ => None,
        }
    }

    pub fn get_token_transfer(&self) -> Option<&ERC20Transfer> {
        match self {
            Event::TokenTransfer(data) => Some(data),
            _ => None,
        }
    }
}

/// A swap that took place on a DEX (Uniswap)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapData {
    pub account: Option<Address>,
    pub token_in: ERC20Token,
    pub token_out: ERC20Token,
    pub amount_in: U256,
    pub amount_out: U256,
    pub block: u64,
    pub tx_hash: String,
}

impl SwapData {
    pub fn new(
        account: Option<Address>,
        token_in: ERC20Token,
        token_out: ERC20Token,
        amount_in: U256,
        amount_out: U256,
        block: u64,
        tx_hash: String,
    ) -> Self {
        Self {
            account,
            token_in,
            token_out,
            amount_in,
            amount_out,
            block,
            tx_hash,
        }
    }

    /// Return a formatted string to print in the console
    pub fn pretty(&self) -> Result<String, anyhow::Error> {
        let from = if let Some(account) = self.account {
            account.to_string()
        } else {
            "Unknown".to_string()
        };

        let s = format!(
            "Swap: {} -> {} | From: {} | Amount: {} -> {} | Block: {} | Tx: {}",
            self.token_in.symbol,
            self.token_out.symbol,
            from,
            format_units(self.amount_in, self.token_in.decimals)?,
            format_units(self.amount_out, self.token_out.decimals)?,
            self.block,
            self.tx_hash,
        );
        Ok(s)
    }
}

/// An ERC20 Transfer that took place
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ERC20Transfer {
    pub token: ERC20Token,
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub block: u64,
    pub tx_hash: String,
}

impl ERC20Transfer {
    pub fn new(
        token: ERC20Token,
        from: Address,
        to: Address,
        amount: U256,
        block: u64,
        tx_hash: String,
    ) -> Self {
        Self {
            token,
            from,
            to,
            amount,
            block,
            tx_hash,
        }
    }

    /// Return a formatted string to print in the console
    pub fn pretty(&self) -> Result<String, anyhow::Error> {
        let s = format!(
            "Transfer: {} | From: {} -> {} | Amount: {} | Block: {} | Tx: {}",
            self.token.symbol,
            self.from,
            self.to,
            format_units(self.amount, self.token.decimals)?,
            self.block,
            self.tx_hash,
        );
        Ok(s)
    }
}
