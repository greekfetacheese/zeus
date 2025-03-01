use super::{ARBITRUM, BASE, BSC, ETH, OPTIMISM};
use anyhow::bail;

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
#[derive(Debug, Clone, Copy)]
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
   /// Go back in time from the current block by X Minutes / Hours / Days or Blocks
   pub fn go_back(&self, chain_id: u64, current_block: u64) -> Result<u64, anyhow::Error> {
      let blocks = match self {
         BlockTime::Minutes(mins) => minutes_to_blocks(*mins, chain_id)?,
         BlockTime::Hours(hours) => hours_to_blocks(*hours, chain_id)?,
         BlockTime::Days(days) => days_to_blocks(*days, chain_id)?,
         BlockTime::Block(block) => *block,
      };

      if blocks > current_block {
         bail!(
            "Cannot go back {} blocks from block {}",
            blocks,
            current_block
         );
      }

      Ok(current_block - blocks)
   }

   /// Go forward in time from the current block by X Minutes / Hours / Days or Blocks
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
      _ => {
         bail!("Unsupported Chain Id: {}", chain_id);
      }
   })
}

fn hours_to_blocks(hours: u64, chain_id: u64) -> Result<u64, anyhow::Error> {
   Ok(match chain_id {
      ETH => hours * 300,
      BSC => hours * 1200,
      BASE | OPTIMISM => hours * 1800,
      ARBITRUM => hours * 14400,
      _ => {
         bail!("Unsupported Chain Id: {}", chain_id);
      }
   })
}

fn days_to_blocks(days: u64, chain_id: u64) -> Result<u64, anyhow::Error> {
   Ok(match chain_id {
      ETH => days * 7200,
      BSC => days * 28800,
      BASE | OPTIMISM => days * 43200,
      ARBITRUM => days * 345600,
      _ => {
         bail!("Unsupported Chain Id: {}", chain_id);
      }
   })
}
