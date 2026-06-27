//! Best broadcaster selection logic.
//! Ported/adapted from TS `search/best-broadcaster.ts`.
//! For Phase 1B we keep it simple and focus on historical data.

use crate::Chain;
use crate::fees::{BroadcasterFeeCache, CachedTokenFee, fee_is_usable};

/// A selected broadcaster for a specific token.
#[derive(Debug, Clone)]
pub struct SelectedBroadcaster {
   pub railgun_address: String,
   pub token_fee: CachedTokenFee,
   pub token_address: String,
}

/// Finds the best (lowest fee) broadcaster for a given token on the chain.
/// Returns None if no usable fees are cached.
pub fn find_best_broadcaster(
   cache: &BroadcasterFeeCache,
   chain: &Chain,
   token_address: &str,
   _use_relay_adapt: bool,
) -> Option<SelectedBroadcaster> {
   let token_lc = token_address.to_lowercase();

   let token_fees = cache.fees_for_token(chain, &token_lc)?;

   let mut candidates: Vec<SelectedBroadcaster> = Vec::new();

   for (broadcaster_addr, identifiers) in token_fees {
      for (_identifier, fee) in identifiers {
         if !fee_is_usable(fee) {
            continue;
         }

         // TODO: later add version check, POI check, trusted signer variance etc.

         candidates.push(SelectedBroadcaster {
            railgun_address: broadcaster_addr.clone(),
            token_fee: fee.clone(),
            token_address: token_address.to_string(),
         });
      }
   }

   if candidates.is_empty() {
      return None;
   }

   // Sort by fee (ascending) then by reliability if available (higher better).
   // For now we just take lowest fee. In TS they have more sophisticated sorting.
   candidates.sort_by(|a, b| {
      let fee_a = hex_to_u128(&a.token_fee.fee_per_unit_gas);
      let fee_b = hex_to_u128(&b.token_fee.fee_per_unit_gas);
      fee_a.cmp(&fee_b)
   });

   candidates.into_iter().next()
}

fn hex_to_u128(hex: &str) -> u128 {
   let s = hex.trim_start_matches("0x");
   u128::from_str_radix(s, 16).unwrap_or(u128::MAX)
}

/// Returns all usable broadcasters for a token (useful for UI selection).
pub fn find_broadcasters_for_token(
   cache: &BroadcasterFeeCache,
   chain: &Chain,
   token_address: &str,
   _use_relay_adapt: bool,
) -> Vec<SelectedBroadcaster> {
   let token_lc = token_address.to_lowercase();

   let Some(token_fees) = cache.fees_for_token(chain, &token_lc) else {
      return vec![];
   };

   let mut result = Vec::new();

   for (broadcaster_addr, identifiers) in token_fees {
      for (_id, fee) in identifiers {
         if fee_is_usable(fee) {
            result.push(SelectedBroadcaster {
               railgun_address: broadcaster_addr.clone(),
               token_fee: fee.clone(),
               token_address: token_address.to_string(),
            });
         }
      }
   }

   // Sort by fee ascending
   result.sort_by(|a, b| {
      let fa = hex_to_u128(&a.token_fee.fee_per_unit_gas);
      let fb = hex_to_u128(&b.token_fee.fee_per_unit_gas);
      fa.cmp(&fb)
   });

   result
}
