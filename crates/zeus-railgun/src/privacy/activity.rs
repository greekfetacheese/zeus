//! Protocol activity: the deposits a privacy check compares a withdrawal against.
//!
//! Two facts about the persisted events snapshot decide this module's shape.
//!
//! - `Shield.value` is the note's value **after** the protocol fee, which is exactly what the
//!   `Unshield` event reports as its `amount`. The two quantities are directly comparable, so the
//!   check needs no fee math in either direction.
//! - A shield whose transaction also carries a `Nullified` event is a *reshield*: a private note put
//!   back under a fresh commitment rather than a new entry into the pool. railcheck excludes those
//!   ("internal reshields"), and so do we — a reshield is not a deposit anyone can be matched to.
//!
//! `timestamp` is filled by the RPC syncer from the log's `blockTimestamp`, which not every endpoint
//! returns. A zero therefore means "this RPC did not say", never "unknown time": the window's own end
//! block and end timestamp estimate it, so a deposit is not dropped, nor pulled in, for the wrong
//! reason.

use std::collections::HashSet;

use alloy_primitives::B256;

use crate::{caip::AssetId, indexer::syncer::types::SyncEvent};

/// A deposit into the pool, in the asset's own units (wei for an ERC-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deposit {
   pub amount_wei: u128,
   /// Unix seconds. Estimated from the block when the event carried no timestamp.
   pub timestamp: u64,
   pub block: u64,
}

impl Deposit {
   /// The deposit, when its value fits the signed arithmetic the scorer uses. A Railgun value is
   /// bounded by `uint120`, so this only ever rejects data that is not a real deposit.
   pub fn amount_i128(&self) -> Option<i128> {
      i128::try_from(self.amount_wei).ok()
   }
}

/// The window a check runs over: blocks `[start_block, end_block]`, times `[start_ts, end_ts)`.
///
/// Both ends are needed. Blocks select the snapshot range to read (cheap, indexed by chunk), while
/// times decide membership exactly — including for the estimated timestamps above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepositWindow {
   pub start_ts: u64,
   pub end_ts: u64,
   pub start_block: u64,
   pub end_block: u64,
}

impl DepositWindow {
   pub fn new(start_ts: u64, end_ts: u64, start_block: u64, end_block: u64) -> Self {
      Self {
         start_ts,
         end_ts,
         start_block,
         end_block,
      }
   }

   pub fn contains_block(&self, block: u64) -> bool {
      block >= self.start_block && block <= self.end_block
   }

   pub fn contains_ts(&self, timestamp: u64) -> bool {
      timestamp >= self.start_ts && timestamp < self.end_ts
   }
}

/// Unix seconds for `block`, walked back from the window's end at the chain's block time.
///
/// Only ever used for an event whose timestamp the RPC did not return. Being off by a slot is
/// immaterial for a window measured in days, and the block is the one thing the event always has.
pub fn estimated_timestamp(block: u64, end_block: u64, end_ts: u64, block_time_secs: u64) -> u64 {
   let behind = end_block.saturating_sub(block);
   end_ts.saturating_sub(behind.saturating_mul(block_time_secs))
}

/// Every deposit of `asset` inside `window`, oldest first, reshields removed.
///
/// `events` is whatever range the caller loaded (typically the window's blocks); the filter here is
/// the authority on membership, so a caller that over-reads costs only work, never correctness.
pub fn deposits_from_events(
   events: &[SyncEvent],
   asset: AssetId,
   window: &DepositWindow,
   block_time_secs: u64,
) -> Vec<Deposit> {
   // Collected first, in one pass: the nullifier of a reshield may sit anywhere in the range, so the
   // shield cannot be judged while walking events.
   let reshielded: HashSet<B256> = events
      .iter()
      .filter_map(|event| match event {
         SyncEvent::Nullified(nullified, _) => Some(nullified.tx_hash),
         _ => None,
      })
      .collect();

   let mut deposits = Vec::new();

   for event in events {
      let SyncEvent::Shield(shield, block) = event else {
         continue;
      };

      if !window.contains_block(*block) || shield.token != asset {
         continue;
      }

      // A zero hash is an event that never carried one (the Subsquid source), and cannot be matched
      // against anything — it is not a reshield.
      if shield.tx_hash != B256::ZERO && reshielded.contains(&shield.tx_hash) {
         continue;
      }

      let timestamp = if shield.timestamp != 0 {
         shield.timestamp
      } else {
         estimated_timestamp(
            *block,
            window.end_block,
            window.end_ts,
            block_time_secs,
         )
      };

      if !window.contains_ts(timestamp) {
         continue;
      }

      let Ok(amount_wei) = u128::try_from(shield.value) else {
         continue;
      };
      if amount_wei == 0 {
         continue;
      }

      deposits.push(Deposit {
         amount_wei,
         timestamp,
         block: *block,
      });
   }

   deposits.sort_by_key(|deposit| (deposit.block, deposit.timestamp));
   deposits
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{
      crypto::aes::Ciphertext,
      indexer::syncer::types::{Nullified, Shield as SyncShield},
   };
   use alloy_primitives::{B256, address};
   use ruint::aliases::U256;

   const WETH: AssetId = AssetId::Erc20(address!(
      "C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
   ));
   const USDC: AssetId = AssetId::Erc20(address!(
      "A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
   ));

   const END_BLOCK: u64 = 26_133_618;
   const END_TS: u64 = 1_791_292_643;
   const BLOCK_TIME: u64 = 12;

   fn window() -> DepositWindow {
      // 180 days of blocks at mainnet block time, ending at the window's end.
      let start_block = END_BLOCK - 180 * 86_400 / BLOCK_TIME;
      DepositWindow::new(
         END_TS - 180 * 86_400,
         END_TS,
         start_block,
         END_BLOCK,
      )
   }

   fn ciphertext() -> Ciphertext {
      Ciphertext {
         iv: [0u8; 16],
         tag: [0u8; 16],
         data: Vec::new(),
      }
   }

   fn shield(token: AssetId, value: u128, timestamp: u64, block: u64, tx_hash: B256) -> SyncEvent {
      SyncEvent::Shield(
         SyncShield {
            tree_number: 0,
            leaf_index: 0,
            npk: U256::ZERO,
            token,
            value: U256::from(value),
            ciphertext: ciphertext(),
            shield_key: [0u8; 32],
            hash: None,
            timestamp,
            tx_hash,
         },
         block,
      )
   }

   fn nullified(tx_hash: B256, block: u64) -> SyncEvent {
      SyncEvent::Nullified(
         Nullified {
            tree_number: 0,
            nullifier: Default::default(),
            timestamp: 0,
            tx_hash,
         },
         block,
      )
   }

   fn inside_block(days_ago: u64) -> u64 {
      END_BLOCK - days_ago * 86_400 / BLOCK_TIME
   }

   fn inside_ts(days_ago: u64) -> u64 {
      END_TS - days_ago * 86_400
   }

   /// Only the asset asked for is activity: another token's shield is a different pool.
   #[test]
   fn only_the_requested_asset_counts() {
      let events = vec![
         shield(
            WETH,
            1_000,
            inside_ts(3),
            inside_block(3),
            B256::from([1u8; 32]),
         ),
         shield(
            USDC,
            1_000,
            inside_ts(3),
            inside_block(3),
            B256::from([2u8; 32]),
         ),
      ];

      let deposits = deposits_from_events(&events, WETH, &window(), BLOCK_TIME);

      assert_eq!(deposits.len(), 1);
      assert_eq!(deposits[0].amount_wei, 1_000);

      let other = deposits_from_events(&events, USDC, &window(), BLOCK_TIME);
      assert_eq!(other.len(), 1);
   }

   /// A shield and a nullifier in the same transaction is a reshield: the note went back into the
   /// pool, so it is not a deposit anyone else can be matched to.
   #[test]
   fn a_reshield_is_not_a_deposit() {
      let reshield = B256::from([7u8; 32]);
      let events = vec![
         shield(
            WETH,
            4_000,
            inside_ts(1),
            inside_block(1),
            reshield,
         ),
         nullified(reshield, inside_block(1)),
         shield(
            WETH,
            5_000,
            inside_ts(1),
            inside_block(1),
            B256::from([8u8; 32]),
         ),
      ];

      let deposits = deposits_from_events(&events, WETH, &window(), BLOCK_TIME);

      assert_eq!(deposits.len(), 1);
      assert_eq!(deposits[0].amount_wei, 5_000);
      assert_eq!(deposits[0].block, inside_block(1));

      // The nullifier may be read before or after the shield it belongs to — same answer either way.
      let reordered = vec![
         nullified(reshield, inside_block(1)),
         shield(
            WETH,
            4_000,
            inside_ts(1),
            inside_block(1),
            reshield,
         ),
      ];
      assert!(deposits_from_events(&reordered, WETH, &window(), BLOCK_TIME).is_empty());
   }

   /// A shield outside the block range is not read, and one inside the blocks but outside the times
   /// is not counted — the two halves of the window are both binding.
   #[test]
   fn the_window_binds_on_blocks_and_on_time() {
      let events = vec![
         shield(
            WETH,
            1_000,
            inside_ts(200),
            inside_block(200),
            B256::from([1u8; 32]),
         ),
         shield(
            WETH,
            2_000,
            inside_ts(1),
            inside_block(200),
            B256::from([2u8; 32]),
         ),
         shield(
            WETH,
            3_000,
            inside_ts(1),
            inside_block(1),
            B256::from([3u8; 32]),
         ),
      ];

      let deposits = deposits_from_events(&events, WETH, &window(), BLOCK_TIME);

      assert_eq!(deposits.len(), 1);
      assert_eq!(deposits[0].amount_wei, 3_000);
   }

   /// An RPC that never returned `blockTimestamp` leaves the event at zero. That is not "unknown
   /// time": the block estimates it, so the deposit still lands in the window — and next to its
   /// neighbour, in block order, rather than at the epoch.
   #[test]
   fn a_missing_timestamp_is_estimated_from_the_block() {
      let events = vec![
         shield(
            WETH,
            1_000,
            0,
            inside_block(90),
            B256::from([1u8; 32]),
         ),
         shield(
            WETH,
            2_000,
            inside_ts(90),
            inside_block(89),
            B256::from([2u8; 32]),
         ),
      ];

      let deposits = deposits_from_events(&events, WETH, &window(), BLOCK_TIME);

      assert_eq!(deposits.len(), 2);
      let estimated = deposits[0].timestamp;
      assert_eq!(
         estimated,
         END_TS - (END_BLOCK - inside_block(90)) * BLOCK_TIME
      );
      assert_eq!(
         estimated,
         inside_ts(90),
         "an exact block time must land on the real timestamp"
      );
      assert_eq!(deposits[1].timestamp, inside_ts(90));
   }

   /// The oldest deposit is the one the estimate can push out of the window: a block just inside it
   /// is still inside, and there is no reason to drop it.
   #[test]
   fn an_estimated_timestamp_at_the_window_edge_stays_in() {
      let first_block = window().start_block;
      let events = vec![shield(WETH, 1_000, 0, first_block, B256::from([1u8; 32]))];

      let deposits = deposits_from_events(&events, WETH, &window(), BLOCK_TIME);
      assert_eq!(deposits.len(), 1);
   }

   /// Zero-value commitments are not deposits, and a value beyond `u128` (impossible on chain) is
   /// skipped rather than truncated.
   #[test]
   fn a_zero_or_oversized_value_is_not_a_deposit() {
      let events = vec![
         shield(
            WETH,
            0,
            inside_ts(1),
            inside_block(1),
            B256::from([1u8; 32]),
         ),
         SyncEvent::Shield(
            SyncShield {
               tree_number: 0,
               leaf_index: 0,
               npk: U256::ZERO,
               token: WETH,
               value: U256::MAX,
               ciphertext: ciphertext(),
               shield_key: [0u8; 32],
               hash: None,
               timestamp: inside_ts(1),
               tx_hash: B256::from([2u8; 32]),
            },
            inside_block(1),
         ),
      ];

      assert!(deposits_from_events(&events, WETH, &window(), BLOCK_TIME).is_empty());
   }

   /// Deposits come back oldest first, which is what the match table and the tests read.
   #[test]
   fn deposits_are_ordered_by_block() {
      let events = vec![
         shield(
            WETH,
            1_000,
            inside_ts(1),
            inside_block(1),
            B256::from([1u8; 32]),
         ),
         shield(
            WETH,
            2_000,
            inside_ts(5),
            inside_block(5),
            B256::from([2u8; 32]),
         ),
         shield(
            WETH,
            3_000,
            inside_ts(3),
            inside_block(3),
            B256::from([3u8; 32]),
         ),
      ];

      let deposits = deposits_from_events(&events, WETH, &window(), BLOCK_TIME);
      let amounts: Vec<u128> = deposits.iter().map(|deposit| deposit.amount_wei).collect();
      assert_eq!(amounts, vec![2_000, 3_000, 1_000]);
   }
}
