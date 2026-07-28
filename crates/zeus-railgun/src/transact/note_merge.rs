//! Suggest private self-transfer packs that merge fragmented UTXO notes.

use std::collections::BTreeMap;

use crate::caip::AssetId;
use crate::circuit::remote_artifact_loader::ARTIFACT_MAX_INPUTS;
use crate::transact::transaction_builder::MAX_CIRCUIT_INPUTS;

/// One unspent note considered for merge planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeNoteRef {
   pub amount: u128,
   pub tree_number: u32,
   pub leaf_index: u32,
}

/// A single recommended merge: spend these notes via private self-transfer
/// into one output note (exact sum → circuit `Nx01`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeSuggestion {
   pub asset: AssetId,
   pub tree_number: u32,
   /// Notes that will be spent (≤ `max_inputs` used when suggesting).
   pub notes: Vec<MergeNoteRef>,
   /// Exact sum of `notes` — pass this as the transfer amount with
   /// [`super::NoteSelectionMode::SmallestFirst`].
   pub amount: u128,
   /// Cap used for this suggestion (from available circuits).
   pub max_inputs: usize,
   /// Note count for this asset on this tree before the merge.
   pub notes_on_tree_before: usize,
   /// Expected note count on this tree after the merge (spent + 1 new).
   pub notes_on_tree_after: usize,
   /// Total notes for this asset across all trees before.
   pub notes_total_before: usize,
   /// Estimated total notes for this asset after this one merge.
   pub notes_total_after: usize,
   /// True if another merge would still help after this one lands.
   pub more_merges_available: bool,
}

impl MergeSuggestion {
   pub fn input_count(&self) -> usize {
      self.notes.len()
   }

   /// Circuit name for the consolidate transfer (exact → one out).
   pub fn circuit_label(&self) -> String {
      format!("{:02}x01", self.input_count())
   }
}

/// Lightweight note view used by [`suggest_merge`].
#[derive(Debug, Clone, Copy)]
pub struct MergeCandidate {
   pub asset: AssetId,
   pub amount: u128,
   pub tree_number: u32,
   pub leaf_index: u32,
}

/// Suggest the best **single** merge pack for `asset`.
///
/// `max_inputs` should be the largest `N` for which the `Nx01` circuit is
/// available (see [`crate::circuit::remote_artifact_loader::AvailableCircuits`]).
/// Values are clamped to `[2, MAX_CIRCUIT_INPUTS]`.
///
/// Strategy (per tree, pick the greediest pack overall):
/// 1. Only notes with `amount > 0` on the same UTXO tree can share an op.
/// 2. If a tree has 2..=`max_inputs` notes → merge **all** of them.
/// 3. If a tree has more → pack up to `max_inputs` **smallest** notes
///    (dust-first) into one `Nx01` transfer.
/// 4. Prefer the pack that removes the most notes (largest pack size), then the
///    most fragmented tree, then lowest tree number.
///
/// Returns `None` when every tree already has ≤1 note for the asset, or when
/// `max_inputs < 2`.
pub fn suggest_merge(
   asset: AssetId,
   candidates: &[MergeCandidate],
   max_inputs: usize,
) -> Option<MergeSuggestion> {
   let max_inputs = max_inputs.clamp(0, MAX_CIRCUIT_INPUTS.min(ARTIFACT_MAX_INPUTS));
   if max_inputs < 2 {
      return None;
   }

   let asset_notes: Vec<&MergeCandidate> =
      candidates.iter().filter(|n| n.asset == asset && n.amount > 0).collect();
   if asset_notes.len() < 2 {
      return None;
   }

   let notes_total_before = asset_notes.len();

   let mut by_tree: BTreeMap<u32, Vec<&MergeCandidate>> = BTreeMap::new();
   for n in &asset_notes {
      by_tree.entry(n.tree_number).or_default().push(*n);
   }

   let mut best: Option<(usize, usize, Vec<MergeNoteRef>, u128, u32)> = None;
   // tuple: (pack_len, tree_note_count, pack, amount, tree)

   for (tree, mut notes) in by_tree {
      if notes.len() < 2 {
         continue;
      }

      // Smallest first for dust packing.
      notes.sort_by(|a, b| a.amount.cmp(&b.amount).then_with(|| a.leaf_index.cmp(&b.leaf_index)));

      let pack_len = notes.len().min(max_inputs);
      if pack_len < 2 {
         continue;
      }

      let pack_src = &notes[..pack_len];
      let amount: u128 = pack_src.iter().map(|n| n.amount).sum();
      let pack: Vec<MergeNoteRef> = pack_src
         .iter()
         .map(|n| MergeNoteRef {
            amount: n.amount,
            tree_number: n.tree_number,
            leaf_index: n.leaf_index,
         })
         .collect();

      let tree_count = notes.len();
      // Prefer larger pack, then more fragmented tree, then lower tree id.
      let better = match &best {
         None => true,
         Some((bl, bt_count, _, _, bt)) => {
            if pack_len != *bl {
               pack_len > *bl
            } else if tree_count != *bt_count {
               tree_count > *bt_count
            } else {
               tree < *bt
            }
         }
      };
      if better {
         best = Some((pack_len, tree_count, pack, amount, tree));
      }
   }

   let (_pack_len, notes_on_tree_before, notes, amount, tree_number) = best?;
   let notes_on_tree_after = notes_on_tree_before - notes.len() + 1;
   let notes_total_after = notes_total_before - notes.len() + 1;

   // After this merge, any tree with ≥2 notes (including residual on this tree) needs more.
   let more_merges_available = notes_on_tree_after >= 2
      || asset_notes
         .iter()
         .filter(|n| n.tree_number != tree_number)
         .fold(BTreeMap::<u32, usize>::new(), |mut m, n| {
            *m.entry(n.tree_number).or_default() += 1;
            m
         })
         .values()
         .any(|&c| c >= 2);

   Some(MergeSuggestion {
      asset,
      tree_number,
      notes,
      amount,
      max_inputs,
      notes_on_tree_before,
      notes_on_tree_after,
      notes_total_before,
      notes_total_after,
      more_merges_available,
   })
}

/// Convenience: suggest using the full Zeus artifact input cap.
pub fn suggest_merge_default(
   asset: AssetId,
   candidates: &[MergeCandidate],
) -> Option<MergeSuggestion> {
   suggest_merge(asset, candidates, MAX_CIRCUIT_INPUTS)
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::address;

   fn weth() -> AssetId {
      AssetId::Erc20(address!(
         "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
      ))
   }

   fn n(amount: u128, tree: u32, leaf: u32) -> MergeCandidate {
      MergeCandidate {
         asset: weth(),
         amount,
         tree_number: tree,
         leaf_index: leaf,
      }
   }

   #[test]
   fn merges_all_four_small_notes() {
      let notes = vec![
         n(300_000_000_000_000, 0, 0), // 0.0003
         n(400_000_000_000_000, 0, 1),
         n(100_000_000_000_000, 0, 2),
         n(200_000_000_000_000, 0, 3),
      ];
      let s = suggest_merge(weth(), &notes, 5).expect("suggestion");
      assert_eq!(s.notes.len(), 4);
      assert_eq!(s.amount, 1_000_000_000_000_000);
      assert_eq!(s.circuit_label(), "04x01");
      assert_eq!(s.notes_total_after, 1);
      assert!(!s.more_merges_available);
   }

   #[test]
   fn packs_five_smallest_when_more_than_max() {
      let mut notes = Vec::new();
      // 6 tiny + 1 large
      for i in 0..6 {
         notes.push(n(1_000 + i as u128, 0, i));
      }
      notes.push(n(1_000_000_000, 0, 99));
      let s = suggest_merge(weth(), &notes, 5).expect("suggestion");
      assert_eq!(s.notes.len(), 5);
      assert!(s.notes.iter().all(|n| n.amount < 1_000_000_000));
      assert!(s.more_merges_available);
   }

   #[test]
   fn respects_smaller_available_circuits() {
      let mut notes = Vec::new();
      for i in 0..5 {
         notes.push(n(1_000 + i as u128, 0, i));
      }
      // Only 03x01 available → pack 3 smallest.
      let s = suggest_merge(weth(), &notes, 3).expect("suggestion");
      assert_eq!(s.notes.len(), 3);
      assert_eq!(s.max_inputs, 3);
      assert_eq!(s.circuit_label(), "03x01");
      assert!(s.more_merges_available);
   }

   #[test]
   fn none_when_max_inputs_too_small() {
      let notes = vec![n(1_000, 0, 0), n(2_000, 0, 1)];
      assert!(suggest_merge(weth(), &notes, 1).is_none());
   }

   #[test]
   fn none_when_single_note() {
      let notes = vec![n(1_000, 0, 0)];
      assert!(suggest_merge(weth(), &notes, 5).is_none());
   }
}
