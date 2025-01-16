pub mod logs;
pub mod batch_request;

use anyhow::anyhow;

/*
Legend:
1 Hour in Eth = 300 blocks
1 Day in Eth = 7200 blocks

1 Hour in Bsc = 1200 blocks
1 Day in Bsc = 28800 blocks

1 Hour in OP Chains = 1800 blocks
1 Day in OP Chains = 43200 blocks
*/

/// Enum to express time in blocks (hours, days, block number)
#[derive(Debug, Clone)]
pub enum BlockTime {
    /// Go back X hours
    Hours(u64),

    /// Go back X days
    Days(u64),

    /// Go back at X block
    Block(u64),

    // TODO
    // Choose a start and end time period
   // Period(Date, Date),
}

impl BlockTime {
    /// Go back X blocks from the current block
    pub fn go_back(&self, chain_id: u64, current_block: u64) -> Result<u64, anyhow::Error> {
        let blocks_to_subtract = match self {
            BlockTime::Hours(hours) => match chain_id {
                1 => hours * 300,
                56 => hours * 1200,
                8453 => hours * 1800,
                _ => return Err(anyhow!("Unsupported chain_id: {}", chain_id)),
            },
            BlockTime::Days(days) => match chain_id {
                1 => days * 7200,
                56 => days * 28800,
                8453 => days * 43200,
                _ => return Err(anyhow!("Unsupported chain_id: {}", chain_id)),
            },
            BlockTime::Block(block) => return Ok(*block),
        };

        if blocks_to_subtract > current_block {
            return Err(anyhow!("Starting block is greater than the current block"));
        }

        Ok(current_block - blocks_to_subtract)
    }

    /// Go forward X blocks from the start block
    pub fn go_forward(&self, chain_id: u64, start_block: u64) -> Result<u64, anyhow::Error> {
        let blocks_to_add = match self {
            BlockTime::Hours(hours) => match chain_id {
                1 => hours * 300,
                56 => hours * 1200,
                8453 => hours * 1800,
                _ => return Err(anyhow!("Unsupported chain_id: {}", chain_id)),
            },
            BlockTime::Days(days) => match chain_id {
                1 => days * 7200,
                56 => days * 28800,
                8453 => days * 43200,
                _ => return Err(anyhow!("Unsupported chain_id: {}", chain_id)),
            },
            BlockTime::Block(block) => *block,
        };

        Ok(start_block + blocks_to_add)
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