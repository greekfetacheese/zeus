pub mod builders;
pub mod contracts;
pub mod merkle;
pub mod note;
pub mod scanner;

pub use contracts::{
   BoundParams, CommitmentCiphertext, CommitmentPreimage, RailgunEvent, RailgunSmartWallet,
   ShieldCiphertext, SnarkProof, TokenData as ContractTokenData, Transaction, UnshieldType,
   railgun_address,
};

pub use merkle::PoseidonMerkleTree;

// Re-export redb so users can easily open a Database for persistence
pub use redb;

pub use scanner::{OwnedNote, RailgunScanner};

// Preferred high-level API.
// Use `RailgunEngine` for almost all operations (one clean entry point per action).
pub use builders::{
   // Prepared data (return types from the high-level methods)
   PreparedBroadcasterUnshield,
   PreparedShield,
   PreparedUnshield,

   RailgunEngine,

   // Supporting helpers (for proof generation and advanced calldata)
   apply_shield_to_scanner,
   apply_unshield_to_scanner,
   build_unshield_proof_request,
   build_unshield_transact_calldata,
   snark_proof_from_sidecar,
};

pub use note::{
   BlindedViewingKeys, Note, NoteAnnotationData, TokenData, TokenType, compute_commitment,
   compute_note_public_key, compute_nullifier, compute_nullifier_for_note,
   compute_nullifying_key_from_viewing, compute_token_hash, create_note_with_keys,
   decrypt_annotation_data, decrypt_note_v2, derive_shared_symmetric_key, encrypt_annotation_data,
   encrypt_note_v2, get_blinding_scalar, get_note_blinding_keys,
};

// Re-export witness types so users of zeus-railgun can build ProofRequests without adding the prover crate explicitly.
pub use zeus_railgun_prover::{
   FormattedCircuitInputsRailgun, PrivateInputsRailgun, ProofRequest, PublicInputsRailgun,
};
