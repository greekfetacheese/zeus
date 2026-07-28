//! Because Railgun's transaction-within-transaction language is confusing, I'm
//! setting some ground rules.
//!
//! A "Note" is an already-on-chain note, which can be used as an input to an Operation.
//!
//! A "Operation" means a single railgun transaction (IE `RailgunSmartWallet.Transaction` object).
//!  - An operation can have many input notes, but they must all be on the same tree and held by the
//!    same address.
//!  - An operation may have many output notes, which can be to different addresses and on different
//!    trees.
//!  - An operation may only have one unshield note, since the `RailgunSmartWallet.Transaction`
//!    struct only
//!
//! A "Transaction" means an EVM transaction.
//!  - A transaction can have many operations across many trees and addresses.

use std::collections::{BTreeMap, HashSet};

use alloy_primitives::{Address, U256};
use rand::Rng;
use thiserror::Error;

use crate::{
   abi,
   account::{address::RailgunAddress, signer::RailgunSigner},
   caip::AssetId,
   circuit::{
      groth16_prover::Groth16Prover,
      inputs::transact_inputs::{TransactCircuitInputs, TransactCircuitInputsError},
   },
   merkle_tree::UtxoMerkleTree,
   note::{
      Note,
      encrypt::EncryptError,
      operation::{Operation, OperationVerificationError},
      transfer::TransferNote,
      unshield::UnshieldNote,
      utxo::UtxoNote,
   },
   transact::proved_transaction::ProvedOperation,
};

/// Max inputs/outputs per operation that we have proving artifacts for
/// Protocol max is 13 but we fail earlier.
pub const MAX_CIRCUIT_INPUTS: usize = 5;
pub const MAX_CIRCUIT_OUTPUTS: usize = 5;

/// Basic builder for constructing railgun transactions. Transactions are sets
/// of shielded operations (transfers and unshield) that are proved together
/// and can be executed in a single on-chain transaction.
#[derive(Clone, Default)]
pub struct TransactionBuilder {
   intents: Vec<Intent>,

   //? Used to track unshield intents to ensure we don't have multiple unshields
   //? for the same from / asset.
   unshields: HashSet<(RailgunAddress, AssetId)>,

   adapt_contract: Option<Address>,
   adapt_params: Option<[u8; 32]>,
}

#[derive(Debug, Error)]
pub enum TransactionBuilderError {
   #[error(
      "Multiple unshield operations from the same address and asset are not supported: from {from}, asset {asset}"
   )]
   MultipleUnshields {
      from: RailgunAddress,
      asset: AssetId,
   },
   #[error(
      "Insufficient balance for intent with from {from}, asset {asset}, value {value} (available matched notes total {available})"
   )]
   InsufficientBalance {
      from: RailgunAddress,
      asset: AssetId,
      value: u128,
      available: u128,
   },
   /// Enough total value exists, but not within `MAX_CIRCUIT_INPUTS` notes on one tree.
   #[error(
      "Private notes are too fragmented for asset {asset}: need {value}, but at most {best_with_max} can be spent with {max_inputs} input notes ({note_count} notes available). Consolidate private notes first."
   )]
   NotesFragmented {
      asset: AssetId,
      value: u128,
      max_inputs: usize,
      best_with_max: u128,
      note_count: usize,
   },
   #[error(
      "Too many circuit outputs for asset {asset}: {outputs} > {max_outputs} (Zeus artifact limit)"
   )]
   TooManyOutputs {
      asset: AssetId,
      outputs: usize,
      max_outputs: usize,
   },
   #[error("Encryption error: {0}")]
   Encryption(#[from] EncryptError),
   #[error("Prover error: {0}")]
   Prover(Box<dyn std::error::Error + Send + Sync>),
   #[error("Missing tree for number {0}")]
   MissingTree(u32),
   #[error("No input notes")]
   NoInputNotes,
   #[error("Transact circuit input error: {0}")]
   TransactCircuitInput(#[from] TransactCircuitInputsError),
   #[error("Operation verification error: {0}")]
   OperationVerification(#[from] OperationVerificationError),
}

#[derive(Clone)]
struct Intent {
   pub from: RailgunSigner,
   pub asset: AssetId,
   pub value: u128,
   pub kind: IntentKind,
}

#[derive(Clone)]
enum IntentKind {
   Transfer { to: RailgunAddress, memo: String },
   Unshield { to: Address },
}

impl TransactionBuilder {
   pub fn new() -> Self {
      Self {
         intents: Vec::new(),
         unshields: HashSet::new(),
         adapt_contract: None,
         adapt_params: None,
      }
   }
}

impl TransactionBuilder {
   /// Adds a transfer operation to this transaction.
   pub fn transfer(
      mut self,
      from: RailgunSigner,
      to: RailgunAddress,
      asset: AssetId,
      value: u128,
      memo: &str,
   ) -> Self {
      self.intents.push(Intent {
         from,
         asset,
         value,
         kind: IntentKind::Transfer {
            to,
            memo: memo.to_string(),
         },
      });
      self
   }

   /// Adds an unshield operation to this transaction.
   pub fn unshield(
      mut self,
      from: RailgunSigner,
      to: Address,
      asset: AssetId,
      value: u128,
   ) -> Result<Self, TransactionBuilderError> {
      if self.unshields.contains(&(from.address().clone(), asset)) {
         return Err(TransactionBuilderError::MultipleUnshields {
            from: from.address().clone(),
            asset,
         });
      }
      self.unshields.insert((from.address().clone(), asset));

      self.intents.push(Intent {
         from,
         asset,
         value,
         kind: IntentKind::Unshield { to },
      });
      Ok(self)
   }

   /// Sets the adapt contract and parameters for this transaction.
   pub fn adapt(mut self, contract: Address, params: [u8; 32]) -> Self {
      self.adapt_contract = Some(contract);
      self.adapt_params = Some(params);
      self
   }

   /// True if any intent spends `asset` (used by the fee loop to decide whether
   /// the paymaster fee merges into an existing operation or is a separate prove).
   pub fn spends_asset(&self, asset: AssetId) -> bool {
      self.intents.iter().any(|intent| intent.asset == asset)
   }

   /// Builds and proves a set of operations for railgun, without packaging into a transaction.
   pub(crate) async fn build<R: Rng>(
      &self,
      prover: &Groth16Prover,
      chain_id: u64,
      in_notes: &[UtxoNote],
      utxo_trees: &BTreeMap<u32, UtxoMerkleTree>,
      rng: &mut R,
   ) -> Result<Vec<ProvedOperation>, TransactionBuilderError> {
      let groups = self.group_intents();
      let mut operations = build_groups(in_notes, groups, rng)?;

      for op in &mut operations {
         op.adapt_contract = self.adapt_contract;
         op.adapt_params = self.adapt_params;
         op.verify()?;
         let outputs = op.out_notes().len();
         if outputs > MAX_CIRCUIT_OUTPUTS {
            return Err(TransactionBuilderError::TooManyOutputs {
               asset: op.asset,
               outputs,
               max_outputs: MAX_CIRCUIT_OUTPUTS,
            });
         }
      }

      let proved = prove_operations(prover, utxo_trees, chain_id, &operations, rng).await?;
      Ok(proved)
   }

   /// Group intents with the following rules:
   ///
   /// 1. Each group has a single asset.
   /// 2. Each group has a single signer.
   /// 3. Each group has at most one unshield.
   fn group_intents(&self) -> BTreeMap<(RailgunAddress, AssetId), Vec<Intent>> {
      let mut groups = BTreeMap::new();
      for intent in &self.intents {
         groups
            .entry((intent.from.address().clone(), intent.asset))
            .or_insert_with(Vec::new)
            .push(intent.clone());
      }

      groups
   }
}

/// Build the operations for each group of intents.
fn build_groups<R: Rng>(
   in_notes: &[UtxoNote],
   groups: BTreeMap<(RailgunAddress, AssetId), Vec<Intent>>,
   rng: &mut R,
) -> Result<Vec<Operation>, TransactionBuilderError> {
   let mut operations = Vec::new();
   for ((from, asset), intents) in groups {
      let ops = build_group(in_notes, from, asset, intents, rng)?;
      operations.extend(ops);
   }
   Ok(operations)
}

/// Build the operations for a single group of intents.
fn build_group<R: Rng>(
   in_notes: &[UtxoNote],
   from: RailgunAddress,
   asset: AssetId,
   mut intents: Vec<Intent>,
   rng: &mut R,
) -> Result<Vec<Operation>, TransactionBuilderError> {
   // Sort intents smallest to largest. Helps to ensure small intents don't
   // ever need to span across multiple trees.
   intents.sort_by(|a, b| a.value.cmp(&b.value));

   // Filter notes for this asset and signer, and group by tree number.
   let tree_number = in_notes
      .iter()
      .filter(|n| n.asset == asset && n.viewing_pubkey == from.viewing_pubkey())
      .fold(BTreeMap::new(), |mut acc, n| {
         acc.entry(n.tree_number).or_insert_with(Vec::new).push(n);
         acc
      });

   let mut balances: BTreeMap<u32, u128> = tree_number
      .iter()
      .map(|(tree_number, notes)| {
         let balance = notes.iter().map(|n| n.value()).sum();
         (*tree_number, balance)
      })
      .collect();

   // Value spendable under the circuit input cap (sum of the largest N notes).
   let coverable: BTreeMap<u32, u128> = tree_number
      .iter()
      .map(|(tree_number, notes)| (*tree_number, max_selectable_value(notes)))
      .collect();

   let available_total: u128 = balances.values().sum();

   // Fit intents to trees.
   let mut operations: BTreeMap<u32, Operation> = BTreeMap::new();
   for intent in intents {
      // Prefer a single tree that can fund this intent with ≤ MAX_CIRCUIT_INPUTS notes.
      // Oldest tree first among those with residual balance + coverable value.
      let single = balances.iter().find_map(|(&tree, &bal)| {
         if bal < intent.value {
            return None;
         }
         let cov = coverable.get(&tree).copied().unwrap_or(0);
         // Residual after prior intents on this tree is tracked in `balances`.
         // Coverable is static (pre-reservation); if prior intents already claimed
         // value on this tree, the final select_notes on combined out_value enforces
         // the hard cap. For the first assignment, require coverable >= intent.
         let already = operations.get(&tree).map(|op| op.out_value()).unwrap_or(0);
         if cov < already.saturating_add(intent.value) {
            return None;
         }
         Some(tree)
      });

      if let Some(tree) = single {
         *balances.get_mut(&tree).unwrap() -= intent.value;
         insert_operation(&mut operations, tree, intent, rng);
         continue;
      }

      split_intent(
         from.clone(),
         asset,
         intent,
         &mut balances,
         &coverable,
         &mut operations,
         rng,
         available_total,
      )?;
   }

   // Add in notes to operations
   for (tree, op) in operations.iter_mut() {
      let Some(notes) = tree_number.get(tree) else {
         debug_assert!(false, "Tree {} should exist in tree_number", tree);
         continue;
      };

      let selected = select_notes(notes, op.out_value(), &from, asset)?;
      for note in selected {
         op.add_in_note(note.clone());
      }
      add_change_note(op, asset, rng);
   }

   Ok(operations.into_values().collect())
}

/// Helper for fitting an intent to multiple trees when it can't fit on a single tree.
fn split_intent<R: Rng>(
   from: RailgunAddress,
   asset: AssetId,
   intent: Intent,
   balances: &mut BTreeMap<u32, u128>,
   coverable: &BTreeMap<u32, u128>,
   operations: &mut BTreeMap<u32, Operation>,
   rng: &mut R,
   available_total: u128,
) -> Result<(), TransactionBuilderError> {
   let mut remaining = intent.value;
   let trees: Vec<u32> = balances.keys().copied().collect();
   for tree in trees {
      if remaining == 0 {
         break;
      }

      let available = *balances.get(&tree).unwrap();
      if available == 0 {
         continue;
      }

      let already = operations.get(&tree).map(|op| op.out_value()).unwrap_or(0);
      let cov = coverable.get(&tree).copied().unwrap_or(0);
      let room = cov.saturating_sub(already).min(available);
      if room == 0 {
         continue;
      }

      let take = remaining.min(room);
      *balances.get_mut(&tree).unwrap() -= take;

      let mut partial = intent.clone();
      partial.value = take;
      insert_operation(operations, tree, partial, rng);

      remaining -= take;
   }

   if remaining > 0 {
      // Distinguish true insolvency from circuit-capped fragmentation.
      if available_total >= intent.value {
         let best = coverable.values().copied().max().unwrap_or(0);
         return Err(TransactionBuilderError::NotesFragmented {
            asset,
            value: intent.value,
            max_inputs: MAX_CIRCUIT_INPUTS,
            best_with_max: best,
            note_count: 0,
         });
      }
      return Err(TransactionBuilderError::InsufficientBalance {
         from,
         asset,
         value: intent.value,
         available: available_total,
      });
   }
   Ok(())
}

/// Helper to insert an intent into an operation, creating the operation if it
/// doesn't exist.
fn insert_operation<R: Rng>(
   operations: &mut BTreeMap<u32, Operation>,
   tree: u32,
   intent: Intent,
   rng: &mut R,
) {
   let from = intent.from.clone();
   let asset = intent.asset;
   let op = operations.entry(tree).or_insert(Operation::new_empty(tree, from, asset));

   match intent.kind {
      IntentKind::Transfer { to, memo } => op.add_out_note(TransferNote::new(
         intent.from.keys().viewing_private_key.clone(),
         to,
         intent.asset,
         intent.value,
         rng.random(),
         &memo,
      )),
      IntentKind::Unshield { to } => {
         op.set_unshield_note(UnshieldNote::new(to, intent.asset, intent.value))
      }
   }
}

/// Sum of the largest `MAX_CIRCUIT_INPUTS` notes upper bound spendable in one op.
fn max_selectable_value(notes: &[&UtxoNote]) -> u128 {
   let mut values: Vec<u128> = notes.iter().map(|n| n.value()).collect();
   values.sort_unstable_by(|a, b| b.cmp(a));
   values.into_iter().take(MAX_CIRCUIT_INPUTS).sum()
}

/// Select the fewest notes that cover `value`, preferring larger notes, capped at
/// [`MAX_CIRCUIT_INPUTS`] (artifact limit).
///
/// Largest-first is optimal for minimizing input count (and thus circuit size /
/// prove time / download size). The maximum value achievable with ≤K notes is
/// exactly the sum of the K largest, so failure here is definitive for this tree.
fn select_notes<'a>(
   notes: &'a [&UtxoNote],
   value: u128,
   from: &RailgunAddress,
   asset: AssetId,
) -> Result<Vec<&'a UtxoNote>, TransactionBuilderError> {
   if value == 0 {
      return Ok(Vec::new());
   }

   let mut sorted: Vec<&UtxoNote> = notes.to_vec();
   // Largest first; stable tie-break on leaf_index for determinism.
   sorted.sort_by(|a, b| b.value().cmp(&a.value()).then_with(|| a.leaf_index.cmp(&b.leaf_index)));

   let mut selected: Vec<&UtxoNote> = Vec::with_capacity(MAX_CIRCUIT_INPUTS.min(sorted.len()));
   let mut total = 0u128;
   for note in sorted.iter().take(MAX_CIRCUIT_INPUTS) {
      selected.push(*note);
      total = total.saturating_add(note.value());
      if total >= value {
         return Ok(selected);
      }
   }

   let available: u128 = notes.iter().map(|n| n.value()).sum();
   if available < value {
      return Err(TransactionBuilderError::InsufficientBalance {
         from: from.clone(),
         asset,
         value,
         available,
      });
   }

   Err(TransactionBuilderError::NotesFragmented {
      asset,
      value,
      max_inputs: MAX_CIRCUIT_INPUTS,
      best_with_max: total,
      note_count: notes.len(),
   })
}

/// Helper to add a change note to an operation if there is excess value.
fn add_change_note<R: Rng>(operation: &mut Operation, asset: AssetId, rng: &mut R) {
   let signer = operation.from.clone();
   let change = operation.in_value().saturating_sub(operation.out_value());
   if change > 0 {
      let change_note = TransferNote::new(
         signer.keys().viewing_private_key.clone(),
         signer.address().clone(),
         asset,
         change,
         rng.random(),
         "change",
      );
      operation.add_out_note(change_note);
   }
}

async fn prove_operations(
   prover: &Groth16Prover,
   utxo_trees: &BTreeMap<u32, UtxoMerkleTree>,
   chain_id: u64,
   operations: &[Operation],
   rng: &mut impl Rng,
) -> Result<Vec<ProvedOperation>, TransactionBuilderError> {
   let mut proved = Vec::new();
   for op in operations {
      let tree = op.utxo_tree_number;
      let Some(utxo_tree) = utxo_trees.get(&tree) else {
         return Err(TransactionBuilderError::MissingTree(tree));
      };
      let proved_op = prove_operation(prover, utxo_tree, chain_id, op, rng).await?;
      proved.push(proved_op);
   }
   Ok(proved)
}

async fn prove_operation(
   prover: &Groth16Prover,
   utxo_tree: &UtxoMerkleTree,
   chain_id: u64,
   operation: &Operation,
   rng: &mut impl Rng,
) -> Result<ProvedOperation, TransactionBuilderError> {
   let unshield_note = operation.unshield_note();
   let unshield_type = unshield_note.map(|n| n.unshield_type()).unwrap_or_default();
   let unshield_preimage = unshield_note.map(|n| n.preimage()).unwrap_or_default();

   let commitment_ciphertexts: Vec<abi::railgun::CommitmentCiphertext> = operation
      .out_encryptable_notes()
      .iter()
      .map(|n| n.encrypt(rng))
      .collect::<Result<_, _>>()?;

   //? min_gas_price, adapt_contract, and adapt_input are all vestigial fields for
   //? railgun relayers.
   let bound_params = abi::railgun::BoundParams::new(
      utxo_tree.number() as u16,
      0,
      unshield_type,
      chain_id,
      operation.adapt_contract.unwrap_or(Address::ZERO),
      &operation.adapt_params.unwrap_or([0u8; 32]),
      commitment_ciphertexts,
   );

   let inputs = TransactCircuitInputs::from_inputs(
      utxo_tree,
      bound_params.hash(),
      &operation.from,
      operation.asset,
      operation.in_notes(),
      &operation.out_notes(),
   )?;
   let proof = prover
      .prove_transact(&inputs)
      .await
      .map_err(|e| TransactionBuilderError::Prover(Box::new(e)))?;

   let merkleroot: U256 = inputs.merkleroot.into();
   let transaction = abi::railgun::Transaction::new(
      proof.into(),
      merkleroot.into(),
      inputs.nullifiers.iter().map(|n| n.clone().into()).collect(),
      inputs.commitments_out.iter().map(|c| c.clone().into()).collect(),
      bound_params,
      unshield_preimage,
   );

   Ok(ProvedOperation::new(
      operation.clone(),
      inputs,
      transaction,
   ))
}
