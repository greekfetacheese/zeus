//! Mainnet priority-fee recommendation.
//!
//! `eth_maxPriorityFeePerGas` is a single-sample node guess that under-reports — measured at
//! 1 800 wei (0.0000018 gwei) on public mainnet endpoints while the 20-block `eth_feeHistory`
//! percentiles were p50 0.009 / p75 0.15 / p90 0.78 gwei — so a transaction priced from it waits
//! behind everything else in a busy block and then sits out the 60 s receipt timeout. This module
//! turns the fee market's own recent history into three tips the user chooses between.
//!
//! L1 only: `eth_feeHistory` rewards carry no information on the other chains Zeus supports —
//! Arbitrum returns all-zero rewards and Base/Optimism a fixed ~0.001 gwei tip — so those keep
//! the single value they have.

use zeus_eth::alloy_primitives::U256;

/// Blocks sampled by `eth_feeHistory`.
pub const FEE_HISTORY_BLOCKS: u64 = 20;

/// Reward percentiles requested, ascending: one column per tier.
pub const REWARD_PERCENTILES: [f64; 3] = [50.0, 75.0, 90.0];

/// One gwei in wei.
const GWEI: u64 = 1_000_000_000;

/// How far each tier's tip must clear the one below it, in percent.
///
/// The raw percentiles collide whenever blocks are quiet, and three identical buttons read as a
/// broken control. The step is a display constraint, not a claim about the market: at those levels
/// every one of the three is includable in the next block.
const MIN_TIER_STEP_PERCENT: u64 = 125;

/// The column each tier reads.
const NORMAL_PERCENTILE: usize = 0;
const FAST_PERCENTILE: usize = 1;
const VERY_FAST_PERCENTILE: usize = 2;

/// How long a transaction is willing to wait, and what that costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PriorityTier {
   #[default]
   Normal,
   Fast,
   VeryFast,
}

impl PriorityTier {
   /// Every tier, slowest first.
   pub const ALL: [Self; 3] = [Self::Normal, Self::Fast, Self::VeryFast];

   pub fn label(self) -> &'static str {
      match self {
         Self::Normal => "Normal",
         Self::Fast => "Fast",
         Self::VeryFast => "Very fast",
      }
   }

   /// The stable name written to `MiscConfig`. Never a `Debug`/`Display` spelling: the store is
   /// unencrypted JSON and a rename would silently reset the user's choice.
   pub fn name(self) -> &'static str {
      match self {
         Self::Normal => "normal",
         Self::Fast => "fast",
         Self::VeryFast => "very_fast",
      }
   }

   pub fn from_name(name: &str) -> Option<Self> {
      Self::ALL.into_iter().find(|tier| tier.name() == name)
   }

   /// Multiplier for the transaction's max-fee ceiling, in basis points (10 000 = 1x).
   ///
   /// The ceiling only bounds what may be charged — the sender pays the base fee plus the tip
   /// actually used — but a 10% margin is spent by one block of base-fee growth (+12.5%/block),
   /// after which the tx is *invalid* ("max fee per gas less than block base fee") rather than
   /// merely slow, and it then sits out the 60 s receipt timeout.
   pub fn fee_ceiling_bps(self) -> u32 {
      match self {
         Self::Normal => 13_000,
         Self::Fast => 16_000,
         Self::VeryFast => 20_000,
      }
   }
}

/// The three tips for one chain, in wei.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeTiers {
   pub normal: U256,
   pub fast: U256,
   pub very_fast: U256,
}

impl FeeTiers {
   pub fn tip(self, tier: PriorityTier) -> U256 {
      match tier {
         PriorityTier::Normal => self.normal,
         PriorityTier::Fast => self.fast,
         PriorityTier::VeryFast => self.very_fast,
      }
   }
}

/// Three tips from `eth_feeHistory`'s per-block reward rows.
///
/// `rows[i][j]` is percentile `j` ([`REWARD_PERCENTILES`]) of block `i`'s effective priority
/// fees, in wei. Each percentile is reduced across blocks with the **median**: one MEV bundle can
/// pay several gwei in an otherwise quiet window (measured: p90 max 5.54 gwei against a p90 median
/// of 0.78 in the same 20 blocks), and a mean would carry that outlier into every tier.
///
/// `floor` is the chain's configured minimum (`PriorityFee::minimum`): a recommendation must never
/// undercut it.
///
/// `None` when the window says nothing — no rows, a requested column with no samples at all, or every
/// sample zero (a sequencer chain, or a node that answered without rewards) — so the caller keeps the
/// value it already had instead of offering three zeroes. A row that is merely too short for a column is
/// skipped for that column, not fatal: see `percentile_median`.
pub fn tiers_from_rewards(rows: &[Vec<u128>], floor: U256) -> Option<FeeTiers> {
   // A node that answered without rewards (or a sequencer chain) reports zeros, not an empty
   // window. The floor must not turn that silence into a recommendation: the caller keeps the
   // value it already had.
   if rows.iter().flatten().all(|reward| *reward == 0) {
      return None;
   }

   let normal = round_up_gwei(percentile_median(rows, NORMAL_PERCENTILE)?.max(floor));
   let fast = round_up_gwei(percentile_median(rows, FAST_PERCENTILE)?.max(step_above(normal)));
   let very_fast =
      round_up_gwei(percentile_median(rows, VERY_FAST_PERCENTILE)?.max(step_above(fast)));

   Some(FeeTiers {
      normal,
      fast,
      very_fast,
   })
}

/// Median of one percentile column across blocks.
///
/// A row too short for this column is skipped rather than shifting the column: a node returning
/// ragged rewards would otherwise have block 1's p75 read as its p50.
fn percentile_median(rows: &[Vec<u128>], index: usize) -> Option<U256> {
   let mut column: Vec<u128> = rows.iter().filter_map(|row| row.get(index).copied()).collect();

   if column.is_empty() {
      return None;
   }

   column.sort_unstable();

   let mid = column.len() / 2;
   let median = if column.len().is_multiple_of(2) {
      (U256::from(column[mid - 1]) + U256::from(column[mid])) / U256::from(2)
   } else {
      U256::from(column[mid])
   };

   Some(median)
}

/// The smallest tip that is a real step above `tip`.
fn step_above(tip: U256) -> U256 {
   let stepped = tip * U256::from(MIN_TIER_STEP_PERCENT) / U256::from(100);
   stepped.max(tip + U256::from(1))
}

/// Round a wei amount up to a step a user can read, so the three buttons do not flicker between
/// refreshes.
///
/// Steps: 0.005 gwei below 0.1, 0.01 gwei below 1, 0.1 gwei below 10, whole gwei above.
pub fn round_up_gwei(wei: U256) -> U256 {
   let step = if wei < U256::from(GWEI / 10) {
      U256::from(GWEI / 200)
   } else if wei < U256::from(GWEI) {
      U256::from(GWEI / 100)
   } else if wei < U256::from(GWEI * 10) {
      U256::from(GWEI / 10)
   } else {
      U256::from(GWEI)
   };

   wei.div_ceil(step) * step
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The three columns are p50, p75, p90.
   fn rows(row: [u128; 3]) -> Vec<Vec<u128>> {
      vec![row.to_vec(); 20]
   }

   #[test]
   fn a_quiet_window_still_gives_three_ordered_tiers() {
      // Every percentile is effectively zero, so the floor and the step alone build the ladder:
      // 0.01 gwei, then 1.25x rounded up to 0.015, then 1.25x rounded up to 0.02.
      let rows = rows([1, 1, 1]);
      let tiers = tiers_from_rewards(&rows, U256::from(10_000_000)).unwrap();

      assert_eq!(tiers.normal, U256::from(10_000_000));
      assert_eq!(tiers.fast, U256::from(15_000_000));
      assert_eq!(tiers.very_fast, U256::from(20_000_000));

      assert!(tiers.normal < tiers.fast);
      assert!(tiers.fast < tiers.very_fast);
   }

   #[test]
   fn an_mev_outlier_does_not_lift_the_ladder() {
      let mut rows = rows([9_000_000, 50_000_000, 100_000_000]);
      rows.push(vec![9_000_000, 50_000_000, 5_540_000_000u128]);

      let tiers = tiers_from_rewards(&rows, U256::from(10_000_000)).unwrap();

      // The median keeps the 19 quiet p90s: the mean over these 20 rows would have been
      // (19 * 0.1 + 5.54) / 20 = 0.372 gwei, i.e. nearly four times the 0.1 gwei it replaces.
      assert!(tiers.very_fast < U256::from(200_000_000));
   }

   #[test]
   fn all_zero_rewards_are_not_a_recommendation() {
      let rows = rows([0, 0, 0]);

      assert_eq!(
         tiers_from_rewards(&rows, U256::from(10_000_000)),
         None
      );
   }

   #[test]
   fn ragged_rows_are_skipped_per_column() {
      let mut rows = rows([9_000_000, 150_000_000, 780_000_000]);
      // Too short to carry p90: its values would drag the upper column down if a short row were
      // allowed to shift it.
      rows.push(vec![1, 1]);

      let tiers = tiers_from_rewards(&rows, U256::from(10_000_000)).unwrap();

      assert_eq!(tiers.fast, U256::from(150_000_000));
      assert_eq!(tiers.very_fast, U256::from(780_000_000));
   }

   #[test]
   fn an_empty_window_is_none() {
      assert_eq!(
         tiers_from_rewards(&[], U256::from(10_000_000)),
         None
      );
   }

   #[test]
   fn the_floor_wins_over_a_cheap_market() {
      let rows = rows([1, 1, 1]);
      let tiers = tiers_from_rewards(&rows, U256::from(10_000_000)).unwrap();

      assert_eq!(tiers.normal, U256::from(10_000_000));
   }

   #[test]
   fn round_up_gwei_steps() {
      assert_eq!(
         round_up_gwei(U256::from(1)),
         U256::from(5_000_000)
      );
      assert_eq!(
         round_up_gwei(U256::from(99_900_000)),
         U256::from(100_000_000)
      );
      // 0.99 gwei is already a whole number of the 0.01 gwei step its band uses, so there is
      // nothing to round up.
      assert_eq!(
         round_up_gwei(U256::from(990_000_000u64)),
         U256::from(990_000_000u64)
      );
      // 9.5 gwei is likewise a whole number of the 0.1 gwei step its band uses.
      assert_eq!(
         round_up_gwei(U256::from(9_500_000_000u64)),
         U256::from(9_500_000_000u64)
      );
      assert_eq!(
         round_up_gwei(U256::from(10_000_000_000u64)),
         U256::from(10_000_000_000u64)
      );
      assert_eq!(
         round_up_gwei(U256::from(11_000_000_000u64)),
         U256::from(11_000_000_000u64)
      );
   }

   #[test]
   fn fee_ceiling_grows_with_the_tier() {
      let ceilings = PriorityTier::ALL.map(PriorityTier::fee_ceiling_bps);

      assert_eq!(ceilings, [13_000, 16_000, 20_000]);

      for pair in ceilings.windows(2) {
         assert!(pair[0] < pair[1]);
      }
   }
}
