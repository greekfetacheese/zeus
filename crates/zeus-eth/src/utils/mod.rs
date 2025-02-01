pub mod logs;
pub mod batch_request;


use crate::prelude::{ETH, BSC, BASE, OPTIMISM, ARBITRUM};
use anyhow::anyhow;

/*
Legend:
1 Hour in Eth = 300 blocks
1 Day in Eth = 7200 blocks

1 Hour in Bsc = 1200 blocks
1 Day in Bsc = 28800 blocks

1 Hour in OP Chains = 1800 blocks
1 Day in OP Chains = 43200 blocks

Assuming 250ms block time on average
1 Hour in Arbitrum = 14400 blocks
1 Day in Arbitrum = 345600 blocks
*/

/// Enum to express time in blocks (hours, days, block number)
#[derive(Debug, Clone)]
pub enum BlockTime {
    Minutes(u64),
    Hours(u64),
    Days(u64),
    Block(u64),

    // TODO
    // Choose a start and end time period
   // Period(Date, Date),
}

impl BlockTime {
    /// Go back X blocks from the current block
    pub fn go_back(&self, chain_id: u64, current_block: u64) -> Result<u64, anyhow::Error> {
        let blocks = match self {
            BlockTime::Minutes(mins) => minutes_to_blocks(*mins, chain_id)?,
            BlockTime::Hours(hours) => hours_to_blocks(*hours, chain_id)?,
            BlockTime::Days(days) => days_to_blocks(*days, chain_id)?,
            BlockTime::Block(block) => *block,
        };

        if blocks > current_block {
            return Err(anyhow!("Block offset exceeds current block"));
        }

        Ok(current_block - blocks)
    }

    /// Go forward X blocks from the current block
    pub fn go_forward(&self, chain_id: u64, current_block: u64) -> Result<u64, anyhow::Error> {
        let blocks_to_add = match self {
            BlockTime::Minutes(mins) => minutes_to_blocks(*mins, chain_id)?,
            BlockTime::Hours(hours) => hours_to_blocks(*hours, chain_id)?,
            BlockTime::Days(days) => days_to_blocks(*days, chain_id)?,
            BlockTime::Block(block) => *block,
        };

        Ok(current_block + blocks_to_add)
    }

    pub fn is_day(&self) -> bool {
        match self {
            BlockTime::Days(_) => true,
            _ => false,
        }
    }

    pub fn is_hour(&self) -> bool {
        match self {
            BlockTime::Hours(_) => true,
            _ => false,
        }
    }

    pub fn is_block(&self) -> bool {
        match self {
            BlockTime::Block(_) => true,
            _ => false,
        }
    }
}

// Conversion functions
fn minutes_to_blocks(minutes: u64, chain_id: u64) -> Result<u64, anyhow::Error> {
    Ok(match chain_id {
        ETH => minutes * 5,
        BSC => minutes * 20,
        BASE | OPTIMISM => minutes * 30,
        ARBITRUM => minutes * 240,
        _ => return Err(anyhow!("Unsupported chain ID: {}", chain_id)),
    })
}

fn hours_to_blocks(hours: u64, chain_id: u64) -> Result<u64, anyhow::Error> {
    Ok(match chain_id {
        ETH => hours * 300,
        BSC => hours * 1200,
        BASE | OPTIMISM => hours * 1800,
        ARBITRUM => hours * 14400,
        _ => return Err(anyhow!("Unsupported chain ID: {}", chain_id)),
    })
}

fn days_to_blocks(days: u64, chain_id: u64) -> Result<u64, anyhow::Error> {
    Ok(match chain_id {
        ETH => days * 7200,
        BSC => days * 28800,
        BASE | OPTIMISM => days * 43200,
        ARBITRUM => days * 345600,
        _ => return Err(anyhow!("Unsupported chain ID: {}", chain_id)),
    })
}