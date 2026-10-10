//! Scoring a requested unshield amount against the pool's recent deposits.
//!
//! This is a Rust port of the engine in [railcheck](https://github.com/ddddubbby/railcheck) (MIT),
//! which asks the same question for RAILGUN withdrawals on Ethereum. The model, in short: a
//! withdrawal is linked to a deposit when the amount *is* that deposit, or a small sum of them. So
//! the scorer looks for 1-, 2- and 3-deposit subsets that add up to the requested amount, weights
//! them by how many such subsets the pool admits (a pool of 10,000 deposits where thousands of
//! combinations add up to your amount hides you; a pool of 10,000 where only one deposit does not),
//! and adds a term for how crowded the region around the amount is.
//!
//! What differs from railcheck, and why:
//!
//! - railcheck asks the user three questions (address reuse, prior transfers, withdrawing the rest
//!   of a partly withdrawn deposit) and puts a floor under the score from the answers. Zeus has the
//!   user's own history, so [`user_exposure`] *computes* those answers from it.
//! - railcheck's pool stores amounts rounded to nano-ETH, so its match tolerance is `±5000`
//!   nano-ETH. Zeus keeps full wei, so the tolerance here is relative (`1e-9`): it only has to
//!   absorb rounding, not a lossy store.
//! - The suggestion search is the same: try a smaller amount, and keep it only when the pool has a
//!   crowd to hide in (at least [`CROWD_FLOOR`] deposits inside a 10% band) *and* the smaller amount
//!   itself matches nothing.

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::privacy::activity::Deposit;

/// The protocol-activity window both halves of a check run over.
///
/// 180 days is railcheck's window, and it is a judgement call, not a derivation: long enough that a
/// withdrawal is judged against a season of deposits, short enough that the pool still resembles
/// what an observer would compare against today.
pub const ACTIVITY_WINDOW_SECONDS: u64 = 180 * 86_400;

/// How many matching sets the UI is given to show.
pub const MATCH_LIMIT: usize = 5;

/// Deposits that must sit inside a 10% band for a suggested amount to be worth suggesting. Below
/// this the suggestion would be a different distinctive amount.
const CROWD_FLOOR: usize = 20;

/// The band a crowd is counted in: `[candidate, candidate / CROWD_BAND]` — a 10% window.
const CROWD_BAND: f64 = 0.9;

/// Combination weights, smallest set first: railcheck's.
const SET_WEIGHTS: [f64; 3] = [0.25, 0.1, 0.05];

/// Percentages of the requested amount to try when looking for a safer one, in order.
const SAFER_PERCENTAGES: [u64; 21] = [
   90, 85, 80, 75, 70, 89, 88, 87, 86, 84, 83, 82, 81, 79, 78, 77, 76, 74, 73, 72, 71,
];

/// A per-deposit posterior at or above this (in percent) is worth reporting as a match.
const REPORTABLE_PERCENT: f64 = 6.0;

/// The match tolerance, relative to the requested amount: one part in a billion, never below a wei.
///
/// A tolerance exists to absorb rounding, and the only rounding left here is the caller's own
/// arithmetic — the pool is compared in full wei. A deposit that differs by more than this is a
/// different amount, which is the whole point of the check.
const RELATIVE_TOLERANCE: i128 = 1_000_000_000;

/// The largest a suggested amount is rounded down to: four decimals, as railcheck reports it.
const SUGGESTION_DECIMALS: u8 = 4;

/// A set's dust leg — a part of a sum smaller than this restates a smaller set rather than being one.
///
/// railcheck uses twice its absolute nano-ETH tolerance; expressed here against the asset's whole
/// unit, so it means the same thing for a 6-decimal token as for an 18-decimal one.
const DUST_LEG_DIVISOR: i128 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskBand {
   Low,
   Medium,
   High,
   Critical,
}

impl RiskBand {
   pub fn from_score(score: u8) -> Self {
      match score {
         0..=5 => Self::Low,
         6..=20 => Self::Medium,
         21..=50 => Self::High,
         _ => Self::Critical,
      }
   }

   pub fn label(&self) -> &'static str {
      match self {
         Self::Low => "Low",
         Self::Medium => "Medium",
         Self::High => "High",
         Self::Critical => "Critical",
      }
   }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PrivacyError {
   #[error("enter an amount more than zero")]
   ZeroAmount,
   #[error("this amount is too large to assess")]
   AmountTooLarge,
   #[error("no deposits of this asset in the last {0} days")]
   NoRecentActivity(u64),
}

/// What the user's own history says about the requested amount.
///
/// These are railcheck's questions with the answers read from the chain instead of asked: a wallet
/// that shields and then withdraws the same amount has already published the link to anyone watching
/// the pool, whatever the rest of the pool looks like.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UserExposure {
   /// The wallet's own shields in the window whose value is this amount.
   pub duplicate_shields: u32,
   /// The amount is what remains of a shield this wallet has already partly withdrawn.
   pub withdraws_remainder: bool,
}

impl UserExposure {
   /// The floor this puts under the score, on railcheck's scale: a duplicate deposit is the whole
   /// story (100), a split withdrawal is strong evidence (90), and otherwise the pool decides.
   pub fn floor(&self) -> u8 {
      if self.duplicate_shields > 0 {
         100
      } else if self.withdraws_remainder {
         90
      } else {
         0
      }
   }

   pub fn is_exposed(&self) -> bool {
      self.floor() > 0
   }
}

/// The matching deposits a withdrawal resembles, largest set last.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchSets {
   pub singles: Vec<Deposit>,
   pub pairs: Vec<[Deposit; 2]>,
   pub triples: Vec<[Deposit; 3]>,
}

impl MatchSets {
   pub fn is_empty(&self) -> bool {
      self.len() == 0
   }

   pub fn len(&self) -> usize {
      self.singles.len() + self.pairs.len() + self.triples.len()
   }
}

/// How many deposits sit close to the requested size.
///
/// The exact-sum matches answer "could this be one deposit, or a sum of them?" — which is rare, and
/// silence there reads as reassurance. These answer "is this a common size at all?", which is the
/// question a slider amount always fails: a size nobody else uses stands out even when nothing adds
/// up to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Crowding {
   /// Deposits within ±1% of the amount.
   pub within_one_percent: u32,
   /// Deposits within ±10% of the amount.
   pub within_ten_percent: u32,
}

/// The verdict on one requested amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnshieldAmountAdvice {
   /// The risk the user is shown: the pool's score, floored by their own history.
   pub score: u8,
   /// The pool's score on its own, before the floor.
   pub amount_score: u8,
   pub band: RiskBand,
   /// How many deposits of the asset the window held.
   pub pool_size: u32,
   /// Matching combinations by size: single deposits, pairs, triples.
   pub matches_by_size: [u32; 3],
   pub sets: MatchSets,
   /// How many deposits are near this size, whether or not any of them add up to it.
   pub crowding: Crowding,
   /// A smaller amount the pool hides better, when one was found.
   pub suggestion: Option<u128>,
   pub user: UserExposure,
}

impl UnshieldAmountAdvice {
   /// How many 1/2/3-deposit combinations add up to the amount.
   pub fn matches(&self) -> u32 {
      self.matches_by_size.iter().fold(0u32, |sum, count| sum.saturating_add(*count))
   }
}

/// Score `requested_wei` against `pool` (the protocol's deposits) and the wallet's own history.
///
/// `now_ts` decides the window: only deposits from the last [`ACTIVITY_WINDOW_SECONDS`] count, which
/// is also the only place the pool is trimmed to time — a caller may hand over a wider block range
/// than the window needs.
pub fn assess_amount(
   pool: &[Deposit],
   requested_wei: u128,
   decimals: u8,
   now_ts: u64,
   own_shields: &[Deposit],
   own_unshields: &[Deposit],
) -> Result<UnshieldAmountAdvice, PrivacyError> {
   let target = i128::try_from(requested_wei).map_err(|_| PrivacyError::AmountTooLarge)?;
   if target <= 0 {
      return Err(PrivacyError::ZeroAmount);
   }

   let start_ts = now_ts.saturating_sub(ACTIVITY_WINDOW_SECONDS);
   let window: Vec<Deposit> = pool
      .iter()
      .copied()
      .filter(|deposit| deposit.timestamp >= start_ts && deposit.timestamp < now_ts)
      .collect();

   if window.is_empty() {
      return Err(PrivacyError::NoRecentActivity(
         ACTIVITY_WINDOW_SECONDS / 86_400,
      ));
   }

   let scored = amount_score(&window, target, decimals);
   let user = user_exposure(own_shields, own_unshields, requested_wei, now_ts);
   let score = scored.score.max(user.floor());

   Ok(UnshieldAmountAdvice {
      score,
      amount_score: scored.score,
      band: RiskBand::from_score(score),
      pool_size: window.len().min(u32::MAX as usize) as u32,
      matches_by_size: scored.matches_by_size,
      sets: scored.sets,
      crowding: scored.crowding,
      suggestion: safer_amount(&window, target, decimals, scored.score),
      user,
   })
}

/// What the wallet's own history says about `requested_wei`.
///
/// Only the window matters for the deposit being repeated — an amount shielded years ago and never
/// withdrawn is not the link an observer is looking at today — while a partial withdrawal is judged
/// against the shield it came out of, whenever that was.
pub fn user_exposure(
   own_shields: &[Deposit],
   own_unshields: &[Deposit],
   requested_wei: u128,
   now_ts: u64,
) -> UserExposure {
   let Ok(target) = i128::try_from(requested_wei) else {
      return UserExposure::default();
   };

   let tolerance = tolerance(target);
   let start_ts = now_ts.saturating_sub(ACTIVITY_WINDOW_SECONDS);

   let duplicate_shields = own_shields
      .iter()
      .filter(|deposit| deposit.timestamp >= start_ts && deposit.timestamp < now_ts)
      .filter(|deposit| within(deposit, target, tolerance))
      .count()
      .min(u32::MAX as usize) as u32;

   // The rest of a partly withdrawn shield: what is left after every earlier, smaller withdrawal of
   // the same asset from this wallet. Only shields larger than each withdrawal count — a withdrawal
   // larger than the shield cannot have come out of it.
   let mut withdraws_remainder = false;
   for shield in own_shields.iter().filter(|deposit| deposit.amount_wei > 0) {
      let Some(value) = shield.amount_i128() else {
         continue;
      };
      let drawn: i128 = own_unshields
         .iter()
         .filter(|withdrawal| withdrawal.timestamp >= shield.timestamp)
         .filter_map(|withdrawal| withdrawal.amount_i128())
         .filter(|amount| *amount > 0 && *amount < value)
         .sum();

      if drawn <= 0 {
         continue;
      }

      let remaining = value.saturating_sub(drawn.min(value));
      if remaining > 0 && (target - remaining).abs() <= tolerance {
         withdraws_remainder = true;
         break;
      }
   }

   UserExposure {
      duplicate_shields,
      withdraws_remainder,
   }
}

/// A deposit is this amount, so far as rounding is concerned.
fn within(deposit: &Deposit, target: i128, tolerance: i128) -> bool {
   match deposit.amount_i128() {
      Some(amount) => (amount - target).abs() <= tolerance,
      None => false,
   }
}

/// The match tolerance for `target`: relative, never zero.
fn tolerance(target: i128) -> i128 {
   (target / RELATIVE_TOLERANCE).max(1)
}

/// A part of a sum smaller than this restates a smaller set instead of being a set of its own.
fn dust_leg(decimals: u8, target: i128) -> i128 {
   let unit = 10i128.saturating_pow(u32::from(decimals));
   2 * (unit / DUST_LEG_DIVISOR).max(tolerance(target))
}

/// The whole units `amount` is worth, for the crowd band.
fn whole_units(amount: i128, decimals: u8) -> f64 {
   amount as f64 / 10f64.powi(i32::from(decimals))
}

struct Scored {
   score: u8,
   /// Matching combinations by size: single deposits, pairs, triples.
   matches_by_size: [u32; 3],
   sets: MatchSets,
   crowding: Crowding,
}

/// railcheck's `amountScore`: the posterior per deposit, the reported score, and the match table.
fn amount_score(pool: &[Deposit], target: i128, decimals: u8) -> Scored {
   let tolerance = tolerance(target);
   let high = target + tolerance;

   let mut sorted: Vec<Deposit> = pool
      .iter()
      .copied()
      .filter(|deposit| deposit.amount_i128().is_some_and(|amount| amount <= high))
      .collect();
   sorted.sort_by_key(|deposit| deposit.amount_wei);

   let amounts: Vec<i128> = sorted.iter().filter_map(Deposit::amount_i128).collect();
   let n = amounts.len();

   let counts = count_sets(&amounts, target, tolerance);

   // How crowded the region around the amount is, against the whole pool: a withdrawal of a common
   // size is hidden by everyone else withdrawing the same size.
   let a = whole_units(target, decimals);
   let m = pool
      .iter()
      .filter_map(|deposit| deposit.amount_i128())
      .filter(|amount| {
         let value = whole_units(*amount, decimals);
         value >= 0.8 * a && value <= 1.25 * a
      })
      .count();
   let q = if pool.is_empty() || a <= 0.0 {
      1.0
   } else {
      (((1 + m) as f64 / pool.len() as f64) * 0.00001 / (0.45 * a)).min(1.0)
   };

   // The same neighbourhood `m` counts, reported instead of only weighted: symmetric bands, because
   // "within 10% of this amount" is a sentence the user can check, while `q`'s window is deliberately
   // not symmetric. An amount too small to have whole units has no neighbourhood to speak of.
   let near = |band: f64| -> u32 {
      pool
         .iter()
         .filter_map(|deposit| deposit.amount_i128())
         .filter(|amount| {
            let value = whole_units(*amount, decimals);
            value >= (1.0 - band) * a && value <= (1.0 + band) * a
         })
         .count()
         .min(u32::MAX as usize) as u32
   };
   let crowding = if a <= 0.0 {
      Crowding::default()
   } else {
      Crowding {
         within_one_percent: near(0.01),
         within_ten_percent: near(0.10),
      }
   };

   let choose = [
      n as f64,
      n as f64 * (n as f64 - 1.0) / 2.0,
      n as f64 * (n as f64 - 1.0) * (n as f64 - 2.0) / 6.0,
   ];
   let weights: [f64; 3] = std::array::from_fn(|i| {
      if choose[i] > 0.0 {
         SET_WEIGHTS[i] / choose[i]
      } else {
         0.0
      }
   });

   let denominator = 0.6 * q + (0..3).map(|k| counts.totals[k] as f64 * weights[k]).sum::<f64>();

   let posterior: Vec<f64> = (0..n)
      .map(|i| {
         if denominator > 0.0 {
            (0..3).map(|k| counts.counts[k][i] as f64 * weights[k]).sum::<f64>() / denominator
         } else {
            0.0
         }
      })
      .collect();

   let highest = posterior.iter().copied().fold(0.0, f64::max);
   let score = (100.0 * highest).round().clamp(0.0, 100.0) as u8;
   let matches_by_size = counts.totals.map(|total| total.min(u32::MAX as u64) as u32);
   let reportable = posterior.iter().any(|p| (100.0 * p).round() >= REPORTABLE_PERCENT);

   let sets = if reportable {
      match_sets(
         &sorted, &amounts, target, tolerance, decimals, &posterior,
      )
   } else {
      MatchSets::default()
   };

   Scored {
      score,
      matches_by_size,
      sets,
      crowding,
   }
}

/// Count the 1-, 2- and 3-deposit subsets that sum to `target` within `tolerance`.
///
/// `amounts` must be ascending. railcheck's sliding-window scan, kept as it is: for each pair of
/// outer deposits the eligible third is a contiguous slice, so a moving lower bound and a running
/// difference array cover the range without enumerating it.
fn count_sets(amounts: &[i128], target: i128, tolerance: i128) -> SetCounts {
   let n = amounts.len();
   let mut counts = [vec![0u64; n], vec![0u64; n], vec![0u64; n]];
   let mut totals = [0u64; 3];

   if n == 0 {
      return SetCounts { counts, totals };
   }

   let low = target - tolerance;
   let high = target + tolerance;

   for i in 0..n {
      if amounts[i] >= low && amounts[i] <= high {
         counts[0][i] += 1;
         totals[0] += 1;
      }
   }

   for k in 2..=3usize {
      let mut diff = vec![0i64; n + 1];
      let outer = if k == 2 { 1 } else { n.saturating_sub(2) };

      for p in 0..outer {
         if k == 3 && p + 2 < n && amounts[p] + amounts[p + 1] + amounts[p + 2] > high {
            break;
         }

         let base = if k == 2 { 0 } else { amounts[p] };
         let mut lo = n;
         let mut hi = n as isize - 1;
         let mut j = if k == 2 { 0 } else { p + 1 };

         while j + 1 < n {
            if base + amounts[j] + amounts[j + 1] > high {
               break;
            }

            let min = low - base - amounts[j];
            let max = high - base - amounts[j];

            while lo > 0 && amounts[lo - 1] >= min {
               lo -= 1;
            }
            while hi >= 0 && amounts[hi as usize] > max {
               hi -= 1;
            }

            let start = (j + 1).max(lo);
            let count = if hi >= start as isize {
               (hi - start as isize + 1) as u64
            } else {
               0
            };

            if count > 0 {
               totals[k - 1] += count;
               counts[k - 1][j] += count;
               if k == 3 {
                  counts[k - 1][p] += count;
               }
               diff[start] += 1;
               diff[hi as usize + 1] -= 1;
            }

            j += 1;
         }
      }

      let mut running = 0i64;
      for i in 0..n {
         running += diff[i];
         if running > 0 {
            counts[k - 1][i] = counts[k - 1][i].saturating_add(running as u64);
         }
      }
   }

   SetCounts { counts, totals }
}

struct SetCounts {
   counts: [Vec<u64>; 3],
   totals: [u64; 3],
}

/// The matching sets to show: `limit` of them, smallest set first, most probable within a size.
fn match_sets(
   sorted: &[Deposit],
   amounts: &[i128],
   target: i128,
   tolerance: i128,
   decimals: u8,
   posterior: &[f64],
) -> MatchSets {
   let n = amounts.len();
   let low = target - tolerance;
   let high = target + tolerance;
   let dust = dust_leg(decimals, target);
   let score = |index: usize| posterior.get(index).copied().unwrap_or(0.0);

   let mut table = SetTable::default();

   for i in 0..n {
      if amounts[i] >= low && amounts[i] <= high {
         table.push(vec![i], score(i), amounts, dust);
      }
   }

   let singles = table.sets.len();

   if singles < MATCH_LIMIT {
      for i in 0..n.saturating_sub(1) {
         let mut j = lower_bound(amounts, low - amounts[i], i + 1);
         while j < n && amounts[j] <= high - amounts[i] {
            table.push(vec![i, j], score(i) + score(j), amounts, dust);
            j += 1;
         }
      }
   }

   if table.sets.len() < MATCH_LIMIT {
      let mut order: Vec<usize> = (0..n).collect();
      order.sort_by(|a, b| score(*b).partial_cmp(&score(*a)).unwrap_or(Ordering::Equal));

      'outer: for i in order.iter().take(40) {
         for j in 0..n.saturating_sub(1) {
            if j == *i {
               continue;
            }

            let remainder_low = low - amounts[*i] - amounts[j];
            let remainder_high = high - amounts[*i] - amounts[j];
            if remainder_high < 0 {
               continue;
            }

            let mut k = lower_bound(amounts, remainder_low, j + 1);
            while k < n && amounts[k] <= remainder_high {
               if k != *i {
                  let mut indices = vec![*i, j, k];
                  indices.sort_by_key(|index| (amounts[*index], *index));
                  let rank = indices.iter().map(|index| score(*index)).sum();
                  table.push(indices, rank, amounts, dust);

                  if table.sets.len() >= MATCH_LIMIT * 8 {
                     break 'outer;
                  }
               }
               k += 1;
            }
         }
      }
   }

   table.finish(sorted, MATCH_LIMIT)
}

/// The candidate sets, before they are ordered, deduplicated and cut down to `limit`.
#[derive(Default)]
struct SetTable {
   sets: Vec<(Vec<usize>, f64, i128)>,
}

impl SetTable {
   fn push(&mut self, indices: Vec<usize>, rank: f64, amounts: &[i128], dust: i128) {
      // A sum with a dust part is a smaller set said twice: it is not a distinct match.
      if indices.len() > 1 && indices.iter().any(|index| amounts[*index] <= dust) {
         return;
      }
      let max = indices.iter().map(|index| amounts[*index]).max().unwrap_or(0);
      self.sets.push((indices, rank, max));
   }

   /// Smallest set first, then most probable, then largest — matching railcheck's order.
   fn finish(self, sorted: &[Deposit], limit: usize) -> MatchSets {
      let mut sets = self.sets;
      sets.sort_by(|a, b| {
         a.0.len()
            .cmp(&b.0.len())
            .then(b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal))
            .then(b.2.cmp(&a.2))
      });

      let mut seen = HashSet::new();
      let mut out = MatchSets::default();

      for (indices, _, _) in sets {
         let mut key = indices.clone();
         key.sort_unstable();
         if !seen.insert(key) {
            continue;
         }

         match indices.as_slice() {
            [i] => out.singles.push(sorted[*i]),
            [i, j] => out.pairs.push([sorted[*i], sorted[*j]]),
            [i, j, k] => out.triples.push([sorted[*i], sorted[*j], sorted[*k]]),
            _ => continue,
         }

         if out.len() >= limit {
            break;
         }
      }

      out
   }
}

/// The first index at or after `from` whose amount is `>= x`.
fn lower_bound(amounts: &[i128], x: i128, from: usize) -> usize {
   let mut lo = from.min(amounts.len());
   let mut hi = amounts.len();
   while lo < hi {
      let mid = (lo + hi) / 2;
      if amounts[mid] < x {
         lo = mid + 1;
      } else {
         hi = mid;
      }
   }
   lo
}

/// A smaller amount the pool hides better, or `None` when the requested amount is already common.
fn safer_amount(pool: &[Deposit], target: i128, decimals: u8, score: u8) -> Option<u128> {
   if f64::from(score) < REPORTABLE_PERCENT {
      return None;
   }

   let step = 10i128
      .saturating_pow(u32::from(
         decimals.saturating_sub(SUGGESTION_DECIMALS),
      ))
      .max(1);

   for percentage in SAFER_PERCENTAGES {
      let candidate = (target * i128::from(percentage) / 100 / step) * step;
      if candidate <= 0 {
         continue;
      }

      let whole = whole_units(candidate, decimals);
      let crowd = pool
         .iter()
         .filter_map(Deposit::amount_i128)
         .filter(|amount| {
            let value = whole_units(*amount, decimals);
            value >= whole && value <= whole / CROWD_BAND
         })
         .count();

      if crowd < CROWD_FLOOR {
         continue;
      }

      if amount_score(pool, candidate, decimals).score <= 5 {
         return u128::try_from(candidate).ok();
      }
   }

   None
}

#[cfg(test)]
mod tests {
   use super::*;

   const END_TS: u64 = 1_791_292_643;
   const WEI: i128 = 1_000_000_000_000_000_000;

   fn deposit(amount_wei: u128, days_ago: u64) -> Deposit {
      Deposit {
         amount_wei,
         timestamp: END_TS - days_ago * 86_400,
         block: 26_133_618 - days_ago * 7_200,
      }
   }

   /// Deposits of `amount` ETH, `count` of them, `days_ago` days back.
   fn eth(amount: f64, days_ago: u64) -> Deposit {
      deposit((amount * WEI as f64) as u128, days_ago)
   }

   /// Exact matches are not the whole picture: a size nobody else uses stands out even when nothing
   /// adds up to it, which is what a slider amount always does.
   #[test]
   fn crowding_counts_the_deposits_close_to_the_amount() {
      let pool = vec![
         eth(1.0, 1),
         eth(1.004, 2),
         eth(1.009, 3),
         eth(1.05, 4),
         eth(1.09, 5),
         eth(1.5, 6),
         eth(0.98, 7),
         eth(0.99, 8),
      ];

      let advice = assess_amount(&pool, WEI as u128, 18, END_TS, &[], &[]).unwrap();

      assert_eq!(
         advice.crowding.within_one_percent, 4,
         "1.0, 1.004, 1.009 and 0.99 are within 1%"
      );
      assert_eq!(
         advice.crowding.within_ten_percent, 7,
         "all but 1.5"
      );
   }

   /// railcheck's `brute`: every 1/2/3-deposit subset, counted the slow way.
   fn brute(amounts: &[i128], target: i128, tolerance: i128) -> SetCounts {
      let n = amounts.len();
      let mut counts = [vec![0u64; n], vec![0u64; n], vec![0u64; n]];
      let mut totals = [0u64; 3];

      let mut visit = |indices: &[usize]| {
         let sum: i128 = indices.iter().map(|i| amounts[*i]).sum();
         if (sum - target).abs() <= tolerance {
            totals[indices.len() - 1] += 1;
            for i in indices {
               counts[indices.len() - 1][*i] += 1;
            }
         }
      };

      for i in 0..n {
         visit(&[i]);
         for j in i + 1..n {
            visit(&[i, j]);
            for k in j + 1..n {
               visit(&[i, j, k]);
            }
         }
      }

      SetCounts { counts, totals }
   }

   /// The scan that counts matching sets without enumerating them must agree with the enumeration,
   /// on pools with duplicate amounts and amounts sitting exactly on the tolerance edges.
   #[test]
   fn count_sets_agrees_with_brute_force() {
      // A deterministic LCG, so a failure can be reproduced.
      let mut seed: u64 = 61_956;
      let mut rand = move || {
         seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
         (seed >> 32) as f64 / 4_294_967_296.0
      };

      for round in 0..400 {
         let count = 1 + (rand() * 40.0) as usize;
         let mut amounts: Vec<i128> =
            (0..count).map(|_| (rand() * 100_000.0) as i128 * 1_000_000_000).collect();
         amounts.sort_unstable();

         let target = (rand() * 300_000.0) as i128 * 1_000_000_000 + (rand() * 1e9) as i128;
         // The tolerance is relative here, so the brute force has to be told the same one.
         let tolerance = tolerance(target.max(1));

         let fast = count_sets(&amounts, target, tolerance);
         let slow = brute(&amounts, target, tolerance);

         assert_eq!(
            fast.totals, slow.totals,
            "totals disagree on round {round} (target {target})"
         );
         assert_eq!(
            fast.counts, slow.counts,
            "per-deposit counts disagree on round {round} (target {target})"
         );
      }
   }

   /// Every exact pair and triple in a pool is found — the scan's boundaries are inclusive.
   #[test]
   fn exact_pairs_and_triples_are_always_found() {
      let mut seed: u64 = 987_654_321;
      let mut rand = move || {
         seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
         (seed >> 32) as f64 / 4_294_967_296.0
      };

      let mut amounts: Vec<i128> = (0..32).map(|_| 1 + (rand() * 1e9) as i128).collect();
      amounts.sort_unstable();

      for round in 0..2_000 {
         let wanted = if round % 2 == 0 { 2 } else { 3 };
         let mut picked = HashSet::new();
         while picked.len() < wanted {
            picked.insert((rand() * amounts.len() as f64) as usize);
         }

         let target: i128 = picked.iter().map(|i| amounts[*i]).sum();
         let counts = count_sets(&amounts, target, tolerance(target));

         assert!(
            counts.totals[wanted - 1] > 0,
            "the {wanted}-deposit set summing to {target} was not found"
         );
         for i in picked {
            assert!(
               counts.counts[wanted - 1][i] > 0,
               "deposit {i} was not credited with the set it belongs to"
            );
         }
      }
   }

   /// A set is listed only when it says something: a sum whose part is dust is a smaller set.
   #[test]
   fn a_dust_part_is_not_a_listed_set() {
      let pool = vec![
         deposit(3_502_749_352 * 1_000_000_000, 1),
         deposit(0, 1),
         deposit(400 * 1_000_000_000, 1),
         deposit(9_000 * 1_000_000_000, 1),
      ];
      let target = 3_502_749_352 * 1_000_000_000;

      let scored = amount_score(&pool, target, 18);

      assert!(
         scored.matches_by_size[1] > 0,
         "the dust pair does sum to the amount"
      );
      assert_eq!(
         scored.sets.len(),
         1,
         "only the exact deposit is worth showing"
      );
      assert_eq!(scored.sets.singles.len(), 1);
      assert_eq!(scored.sets.singles[0].amount_wei, target as u128);
      assert_eq!(scored.score, 100);
   }

   /// Sets that sum to the amount are listed; lone tiny legs that cannot are not.
   #[test]
   fn sets_that_sum_to_the_amount_are_listed() {
      let pool = vec![
         deposit(50_000_000 * 1_000_000_000, 1),
         deposit(19_950_000_000 * 1_000_000_000, 1),
         deposit(50_000_000 * 1_000_000_000, 1),
         deposit(19_950_000_000 * 1_000_000_000, 1),
         deposit(3_500_000_000 * 1_000_000_000, 1),
         deposit(1_000_000_000 * 1_000_000_000, 1),
         deposit(3_500_000_000 * 1_000_000_000, 1),
         deposit(1_000_000_000 * 1_000_000_000, 1),
      ];

      let twenty = amount_score(&pool, 20 * WEI, 18);
      assert_eq!(
         twenty.matches_by_size[0], 0,
         "no single deposit is 20 ETH"
      );
      assert!(
         twenty.matches_by_size[1] >= 2,
         "two pairs of deposits are 20 ETH"
      );
      assert!(!twenty.sets.is_empty());

      for pair in &twenty.sets.pairs {
         let sum = pair[0].amount_wei + pair[1].amount_wei;
         assert_eq!(sum, 20 * WEI as u128);
         assert!(
            pair.iter().any(|deposit| deposit.amount_wei == 19_950_000_000 * 1_000_000_000),
            "the large leg is what makes the pair distinctive"
         );
      }
      assert!(twenty.sets.singles.is_empty());
      assert!(twenty.sets.triples.is_empty());

      let four_five = amount_score(&pool, (4.5 * WEI as f64) as i128, 18);
      assert!(!four_five.sets.is_empty());
      for pair in &four_five.sets.pairs {
         let mut amounts = [pair[0].amount_wei, pair[1].amount_wei];
         amounts.sort_unstable();
         assert_eq!(
            amounts,
            [1_000_000_000 * 1_000_000_000, 3_500_000_000 * 1_000_000_000]
         );
      }
   }

   /// An amount that exactly one deposit matches, in a pool of otherwise unrelated amounts, is the
   /// link the check exists to catch.
   #[test]
   fn a_distinctive_amount_in_a_diverse_pool_is_high_risk() {
      let mut pool: Vec<Deposit> =
         (1..=300).map(|i| eth(i as f64 * 0.01, 1 + i as u64 % 100)).collect();
      pool.push(eth(0.25, 2));

      let advice = assess_amount(
         &pool,
         (0.25 * WEI as f64) as u128,
         18,
         END_TS,
         &[],
         &[],
      )
      .unwrap();

      assert!(
         advice.band == RiskBand::High || advice.band == RiskBand::Critical,
         "expected a high score, got {} ({})",
         advice.score,
         advice.band.label()
      );
      assert!(!advice.sets.is_empty());
      assert!(!advice.user.is_exposed());
   }

   /// The same amount is *not* distinctive when the pool is full of it: hiding is the point.
   #[test]
   fn an_amount_the_pool_is_full_of_is_low_risk() {
      let mut pool: Vec<Deposit> = (1..=25).map(|i| eth(0.25, 1 + i as u64 % 100)).collect();
      pool.extend((1..=275).map(|i| eth(i as f64 * 0.01, 1 + i as u64 % 100)));

      let advice = assess_amount(
         &pool,
         (0.25 * WEI as f64) as u128,
         18,
         END_TS,
         &[],
         &[],
      )
      .unwrap();

      assert_eq!(
         advice.band,
         RiskBand::Low,
         "a common amount scored {}",
         advice.score
      );
      assert_eq!(
         advice.suggestion, None,
         "there is nothing safer to suggest"
      );
   }

   /// The wallet's own duplicate shield is the whole story, whatever the pool says.
   #[test]
   fn a_duplicate_of_the_wallets_own_shield_scores_critical() {
      let pool: Vec<Deposit> = (1..=25).map(|i| eth(0.25, 1 + i as u64 % 100)).collect();
      let own = vec![eth(0.25, 3)];

      let advice = assess_amount(
         &pool,
         (0.25 * WEI as f64) as u128,
         18,
         END_TS,
         &own,
         &[],
      )
      .unwrap();

      assert_eq!(advice.user.duplicate_shields, 1);
      assert_eq!(advice.user.floor(), 100);
      assert_eq!(advice.score, 100);
      assert_eq!(advice.band, RiskBand::Critical);
      assert!(advice.user.is_exposed());
   }

   /// Withdrawing the rest of a shield that was already partly withdrawn is the other own-history
   /// link: the two withdrawals sum to a deposit of theirs.
   #[test]
   fn withdrawing_the_remainder_of_a_partly_withdrawn_shield_is_flagged() {
      let own_shields = vec![eth(2.0, 30)];
      let own_unshields = vec![eth(0.5, 20)];

      let exposure = user_exposure(
         &own_shields,
         &own_unshields,
         (1.5 * WEI as f64) as u128,
         END_TS,
      );
      assert!(exposure.withdraws_remainder);
      assert_eq!(exposure.floor(), 90);

      // The whole 2 ETH is a duplicate of the wallet's own shield — flagged, but as a repeat of the
      // amount, not as a remainder.
      let full = user_exposure(
         &own_shields,
         &own_unshields,
         (2.0 * WEI as f64) as u128,
         END_TS,
      );
      assert!(!full.withdraws_remainder);
      assert_eq!(full.duplicate_shields, 1);

      // An amount that is neither the shield nor what is left of it is not the wallet's own link.
      let other = user_exposure(
         &own_shields,
         &own_unshields,
         (1.8 * WEI as f64) as u128,
         END_TS,
      );
      assert_eq!(other.floor(), 0);

      // A withdrawal larger than the shield cannot have come out of it, so there is no remainder.
      let too_big = user_exposure(
         &own_shields,
         &[eth(3.0, 20)],
         (1.5 * WEI as f64) as u128,
         END_TS,
      );
      assert_eq!(too_big.floor(), 0);
   }

   /// A deposit outside the window is not a duplicate: the link an observer sees is a recent one.
   #[test]
   fn only_own_history_inside_the_window_counts() {
      let own_shields = vec![eth(1.0, 200)];

      let exposure = user_exposure(&own_shields, &[], WEI as u128, END_TS);
      assert_eq!(exposure.duplicate_shields, 0);
   }

   /// A pool with nothing recent in it is an error the caller must handle, not a zero score.
   #[test]
   fn an_empty_window_is_an_error() {
      let pool = vec![eth(1.0, 200)];

      assert_eq!(
         assess_amount(&pool, WEI as u128, 18, END_TS, &[], &[]),
         Err(PrivacyError::NoRecentActivity(180))
      );
      assert_eq!(
         assess_amount(&[], 0, 18, END_TS, &[], &[]),
         Err(PrivacyError::ZeroAmount)
      );
   }

   /// When the amount is distinctive and the pool has somewhere to hide, a smaller amount is
   /// offered — and it is one that matches nothing.
   #[test]
   fn a_safer_amount_is_offered_when_the_pool_supports_it() {
      let mut pool: Vec<Deposit> = (1..=30).map(|i| eth(1.05, 1 + i as u64 % 50)).collect();
      pool.push(eth(1.2, 5));

      let advice = assess_amount(
         &pool,
         (1.2 * WEI as f64) as u128,
         18,
         END_TS,
         &[],
         &[],
      )
      .unwrap();

      assert_eq!(advice.score, 100);
      assert_eq!(advice.band, RiskBand::Critical);

      let suggestion = advice.suggestion.expect("a crowd sits below the amount");
      assert_eq!(suggestion, (1.02 * WEI as f64) as u128);
      assert!(
         advice.sets.singles.iter().all(|d| d.amount_wei != suggestion),
         "the suggestion must not be a deposit of its own"
      );
   }

   /// Everything the scorer produces is bounded and consistent, whatever the pool.
   #[test]
   fn the_verdict_is_always_in_range() {
      let mut seed: u64 = 4_242;
      let mut rand = move || {
         seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
         (seed >> 32) as f64 / 4_294_967_296.0
      };

      for _ in 0..200 {
         let count = 1 + (rand() * 60.0) as usize;
         let pool: Vec<Deposit> = (0..count)
            .map(|_| deposit((rand() * 1e19) as u128, (rand() * 170.0) as u64))
            .collect();
         let requested = (rand() * 1e19) as u128;

         let advice = assess_amount(&pool, requested, 18, END_TS, &[], &[]).unwrap();
         assert_eq!(advice.band, RiskBand::from_score(advice.score));
         assert!(advice.score <= 100);
         assert!(advice.pool_size <= count as u32);
         assert_eq!(advice.sets.len() <= MATCH_LIMIT, true);
         if let Some(suggestion) = advice.suggestion {
            assert!(suggestion > 0 && suggestion < requested);
         }
      }
   }
}
