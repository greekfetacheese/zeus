use alloy_primitives::Address;

use crate::{
   account::signer::RailgunSigner,
   caip::AssetId,
   note::{OutputNote, transfer::TransferNote, unshield::UnshieldNote, utxo::UtxoNote},
};

/// An Operation represents a single "operation" within a railgun transaction.
/// Otherwise known as the `RailgunSmartWallet::Transaction` struct in solidity.
///
/// - An operation MUST only spend notes from a single tree.
/// - An operation MUST have fewer than to 12 out_notes (13 including unshield), which can be to
///   arbitrary addresses.
/// - An operation MUST only spend a single asset.
///   - The POI proof circuit inputs are designed around this assumption, since the token of the
///     spent notes is a private input.
/// - An operation MUST only spend notes from a single address.
///   - The POI proof circuit inputs are designed around this assumption, since the spender's public
///     and nullifying key are private inputs to the circuit.
/// - An operation MUST only have a single unshield note.
///   - The railgun smart contracts are designed around this assumption, since the
///     `RailgunSmartWallet::Transaction` struct only supports defining a single token/value pair
///     for unshielding.
#[derive(Debug, Clone)]
pub struct Operation {
   /// The UTXO tree number that the in_notes being spent are from
   pub utxo_tree_number: u32,

   /// The holder of the assets being spent.
   pub from: RailgunSigner,

   /// The asset this operation is spending.
   pub asset: AssetId,

   pub adapt_contract: Option<Address>,
   pub adapt_params: Option<[u8; 32]>,

   in_notes: Vec<UtxoNote>,
   out_notes: Vec<TransferNote>,
   unshield_note: Option<UnshieldNote>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum OperationVerificationError {
   Imbalanced(u128, u128, u128),
   TooManyOutputNotes(usize),
   TooManyInputNotes(usize),
}

impl std::fmt::Display for OperationVerificationError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::Imbalanced(input, output, unshield) => {
            write!(
               f,
               "Imbalanced operation: {input} != {output} + {unshield}"
            )
         }
         Self::TooManyOutputNotes(count) => {
            write!(f, "Too many output notes: {count} > 13")
         }
         Self::TooManyInputNotes(count) => {
            write!(f, "Too many input notes: {count} > 13")
         }
      }
   }
}

impl std::error::Error for OperationVerificationError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::Imbalanced(_, _, _) | Self::TooManyOutputNotes(_) | Self::TooManyInputNotes(_) => {
            None
         }
      }
   }
}

impl Operation {
   /// TODO: Add error checking to ensure that the operation is valid.
   ///
   /// - Spending and viewing keys are the same for all notes in
   /// - Tree number is the same for all notes in
   /// - AssetID is the same for all notes
   /// - notes_in.value = notes_out.value + unshield_note.value
   /// - notes_in.len() <= 13
   /// - notes_out.len() + unshield_note.is_some() <= 13
   pub fn new(
      tree_number: u32,
      from: RailgunSigner,
      asset: AssetId,
      in_notes: Vec<UtxoNote>,
      out_notes: Vec<TransferNote>,
      unshield: Option<UnshieldNote>,
   ) -> Self {
      Operation {
         utxo_tree_number: tree_number,
         from,
         asset,
         in_notes,
         out_notes,
         unshield_note: unshield,
         adapt_contract: None,
         adapt_params: None,
      }
   }

   pub fn new_empty(tree_number: u32, from: RailgunSigner, asset: AssetId) -> Self {
      Operation {
         utxo_tree_number: tree_number,
         from,
         asset,
         in_notes: Vec::new(),
         out_notes: Vec::new(),
         unshield_note: None,
         adapt_contract: None,
         adapt_params: None,
      }
   }

   pub fn add_in_note(&mut self, note: UtxoNote) {
      self.in_notes.push(note);
   }

   pub fn add_out_note(&mut self, note: TransferNote) {
      self.out_notes.push(note);
   }

   pub fn set_unshield_note(&mut self, note: UnshieldNote) {
      self.unshield_note = Some(note);
   }

   pub fn verify(&self) -> Result<(), OperationVerificationError> {
      let in_value: u128 = self.in_notes.iter().map(|n| n.value()).sum();
      let out_value: u128 = self.out_notes.iter().map(|n| n.value()).sum();
      let unshield_value: u128 = self.unshield_note.as_ref().map_or(0, |n| n.value());

      if in_value != out_value + unshield_value {
         return Err(OperationVerificationError::Imbalanced(
            in_value,
            out_value,
            unshield_value,
         ));
      }

      if self.out_notes.len() + self.unshield_note.is_some() as usize > 13 {
         return Err(OperationVerificationError::TooManyOutputNotes(
            self.out_notes.len(),
         ));
      }

      if self.in_notes.len() > 13 {
         return Err(OperationVerificationError::TooManyInputNotes(
            self.in_notes.len(),
         ));
      }

      Ok(())
   }
}

impl Operation {
   pub fn in_value(&self) -> u128 {
      self.in_notes.iter().map(|n| n.value()).sum()
   }

   /// Total value being transfered to other railgun addresses in this operation
   pub fn out_value(&self) -> u128 {
      let out_notes_value: u128 = self.out_notes.iter().map(|n| n.value()).sum();
      let unshield_value: u128 = self.unshield_note.as_ref().map_or(0, |n| n.value());
      out_notes_value + unshield_value
   }

   pub fn in_notes(&self) -> &[UtxoNote] {
      &self.in_notes
   }

   pub fn out_notes(&self) -> Vec<OutputNote> {
      let mut notes: Vec<OutputNote> = Vec::new();

      for transfer in &self.out_notes {
         notes.push(OutputNote::Transfer(transfer.clone()));
      }

      if let Some(unshield) = &self.unshield_note {
         notes.push(OutputNote::Unshield(*unshield));
      }

      notes.into_iter().filter(|n| n.value() > 0).collect()
   }

   pub fn unshield_note(&self) -> Option<UnshieldNote> {
      self.unshield_note
   }

   pub fn out_encryptable_notes(&self) -> Vec<TransferNote> {
      self.out_notes.iter().filter(|n| n.value() > 0).cloned().collect()
   }
}

#[cfg(test)]
mod error_tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         OperationVerificationError::Imbalanced(5, 3, 1).to_string(),
         "Imbalanced operation: 5 != 3 + 1"
      );
      assert_eq!(
         OperationVerificationError::TooManyOutputNotes(14).to_string(),
         "Too many output notes: 14 > 13"
      );
      assert_eq!(
         OperationVerificationError::TooManyInputNotes(14).to_string(),
         "Too many input notes: 14 > 13"
      );
   }

   #[test]
   fn the_payload_variants_have_no_source() {
      assert!(OperationVerificationError::Imbalanced(5, 3, 1).source().is_none());
      assert!(OperationVerificationError::TooManyOutputNotes(14).source().is_none());
      assert!(OperationVerificationError::TooManyInputNotes(14).source().is_none());
   }
}
