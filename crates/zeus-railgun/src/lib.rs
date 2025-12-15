use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{Filter, Log};
use alloy_sol_types::{sol, SolEvent};
use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Nonce};
use curve25519_dalek::{EdwardsPoint, Scalar as DalekScalar};
use sha2::{Sha256, Digest};
use sled::Db;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;


pub mod address;

/* 
sol! {
    event CommitmentBatch(uint256 indexed treeNumber, uint256 startPosition, bytes32[] hashes, bytes ciphertext);
    event GeneratedCommitmentBatch(uint256 indexed treeNumber, uint256 startPosition, (bytes32 hash)[] commitments);
    event Nullifier(bytes32 indexed nullifier);
}
    */