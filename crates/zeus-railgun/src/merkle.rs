//! Poseidon Merkle Tree for Railgun private state.
//!
//! This module provides an in-memory Poseidon Merkle tree with optional
//! disk persistence using redb (a stable, pure-Rust embedded database).
//!
//! The tree stores only the list of leaves. On load it fully reconstructs
//! the tree layers and root (the tree is small — Railgun uses depth 16).
//!
//! NOTE: The actual Railgun contracts use TREE_DEPTH = 16 (see Commitments.sol).
//! We are currently using 32 for future-proofing, but should align eventually.

use alloy_primitives::U256;
use anyhow::{anyhow, Result};
use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use light_poseidon::{Poseidon, PoseidonHasher};
use redb::{Database, ReadableDatabase, TableDefinition};
use std::path::Path;

const TREE_DEPTH: usize = 32; // TODO: align to 16 to match real Railgun contracts

/// Table that stores the serialized leaves for each tree id (e.g. "mainnet" or "chain:1")
const MERKLE_LEAVES_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("railgun_merkle_leaves");

/// A simple Poseidon Merkle Tree (binary).
#[derive(Clone, Debug)]
pub struct PoseidonMerkleTree {
    /// The leaves (commitments) in order of insertion.
    leaves: Vec<U256>,

    /// Current root (cached).
    root: U256,

    /// Zero value at each level (for empty subtrees).
    zeros: [U256; TREE_DEPTH + 1],

    /// The full tree layers (for proof generation later).
    /// layers[0] = leaves, layers[1] = level above, etc.
    layers: Vec<Vec<U256>>,
}

impl PoseidonMerkleTree {
    /// Create a new empty tree (in-memory only).
    pub fn new() -> Result<Self> {
        let mut zeros = [U256::ZERO; TREE_DEPTH + 1];
        let mut current_zero = U256::ZERO;

        // Precompute the zero values for each level
        for i in 0..=TREE_DEPTH {
            zeros[i] = current_zero;
            if i < TREE_DEPTH {
                current_zero = poseidon_pair(current_zero, current_zero)?;
            }
        }

        let mut tree = Self {
            leaves: Vec::new(),
            root: zeros[TREE_DEPTH],
            zeros,
            layers: vec![Vec::new()],
        };

        tree.layers[0] = Vec::new();
        Ok(tree)
    }

    /// Insert a new commitment (leaf) into the tree.
    pub fn insert(&mut self, commitment: U256) -> Result<U256> {
        self.leaves.push(commitment);

        // Rebuild the tree (simple but correct for now; later we can make it incremental)
        self.rebuild()?;

        Ok(self.root)
    }

    /// Insert multiple commitments at once (more efficient for batch events).
    pub fn insert_batch(&mut self, commitments: &[U256]) -> Result<U256> {
        self.leaves.extend_from_slice(commitments);
        self.rebuild()?;
        Ok(self.root)
    }

    fn rebuild(&mut self) -> Result<()> {
        if self.leaves.is_empty() {
            self.root = self.zeros[TREE_DEPTH];
            self.layers = vec![Vec::new()];
            return Ok(());
        }

        let mut current_level = self.leaves.clone();

        let mut layers = vec![current_level.clone()];

        for level in 0..TREE_DEPTH {
            let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

            for chunk in current_level.chunks(2) {
                let left = chunk[0];
                let right = if chunk.len() > 1 {
                    chunk[1]
                } else {
                    self.zeros[level]
                };

                let parent = poseidon_pair(left, right)?;
                next_level.push(parent);
            }

            layers.push(next_level.clone());
            current_level = next_level;

            if current_level.len() == 1 {
                break;
            }
        }

        self.root = current_level[0];
        self.layers = layers;

        Ok(())
    }

    /// Get the current Merkle root.
    pub fn root(&self) -> U256 {
        self.root
    }

    /// Number of leaves inserted so far.
    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    /// Get a leaf by index.
    pub fn get_leaf(&self, index: usize) -> Option<U256> {
        self.leaves.get(index).copied()
    }

    /// Generate a Merkle proof for a given leaf index.
    /// Returns (leaf, path elements, path indices)
    pub fn get_proof(&self, index: usize) -> Result<(U256, Vec<U256>, Vec<u8>)> {
        if index >= self.leaves.len() {
            return Err(anyhow!("Leaf index out of range"));
        }

        let mut proof = Vec::new();
        let mut indices = Vec::new();
        let mut current_idx = index;

        for level in 0..self.layers.len().saturating_sub(1) {
            let level_nodes = &self.layers[level];
            let sibling_idx = if current_idx % 2 == 0 {
                current_idx + 1
            } else {
                current_idx - 1
            };

            let sibling = if sibling_idx < level_nodes.len() {
                level_nodes[sibling_idx]
            } else {
                self.zeros[level]
            };

            proof.push(sibling);
            indices.push((current_idx % 2) as u8);

            current_idx /= 2;
        }

        Ok((self.leaves[index], proof, indices))
    }

    // ======================== Persistence (redb) ========================

    /// Load a tree from disk (or create an empty one if it doesn't exist).
    ///
    /// `db_path` - path to the redb database file (e.g. "zeus-railgun.redb")
    /// `tree_id` - unique identifier for this tree, e.g. "mainnet" or "chain:137"
    ///
    /// This is the most convenient entry point for persistent usage.
    pub fn open(db_path: impl AsRef<Path>, tree_id: &str) -> Result<Self> {
        let db = Database::create(db_path)?;
        Self::load(&db, tree_id)
    }

    /// Load the tree from an already-opened redb Database.
    ///
    /// Preferred when you want to share one database file between the merkle tree
    /// and other persistent state (nullifiers, owned notes, etc.).
    pub fn load(db: &Database, tree_id: &str) -> Result<Self> {
        let leaves = load_leaves(db, tree_id)?;
        Self::from_leaves(leaves)
    }

    /// Persist the current leaves to the given database under `tree_id`.
    ///
    /// Call this after `insert` / `insert_batch` when you want durability.
    pub fn save(&self, db: &Database, tree_id: &str) -> Result<()> {
        save_leaves(db, tree_id, &self.leaves)
    }

    /// Returns a serializable snapshot of just the leaves.
    /// This is the minimal state needed to reconstruct the tree.
    pub fn leaves(&self) -> &[U256] {
        &self.leaves
    }

    /// Create a tree by loading a list of leaves (used when restoring from disk).
    pub fn from_leaves(leaves: Vec<U256>) -> Result<Self> {
        let mut tree = Self::new()?;
        if !leaves.is_empty() {
            tree.insert_batch(&leaves)?;
        }
        Ok(tree)
    }
}

// ======================== redb helpers ========================

fn serialize_leaves(leaves: &[U256]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(leaves.len() * 32 + 8);
    buf.extend_from_slice(&(leaves.len() as u64).to_le_bytes());
    for leaf in leaves {
        buf.extend_from_slice(&leaf.to_be_bytes::<32>());
    }
    buf
}

fn deserialize_leaves(data: &[u8]) -> Result<Vec<U256>> {
    if data.len() < 8 {
        return Ok(Vec::new());
    }

    let len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    let mut leaves = Vec::with_capacity(len);

    let mut offset = 8;
    for _ in 0..len {
        if offset + 32 > data.len() {
            return Err(anyhow!("corrupted merkle leaves data"));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data[offset..offset + 32]);
        leaves.push(U256::from_be_bytes(bytes));
        offset += 32;
    }

    Ok(leaves)
}

fn load_leaves(db: &Database, tree_id: &str) -> Result<Vec<U256>> {
    let read_txn = db.begin_read()?;
    // Table may not exist yet (first run)
    let table = match read_txn.open_table(MERKLE_LEAVES_TABLE) {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()),
    };

    match table.get(tree_id)? {
        Some(value) => deserialize_leaves(value.value()),
        None => Ok(Vec::new()),
    }
}

fn save_leaves(db: &Database, tree_id: &str, leaves: &[U256]) -> Result<()> {
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(MERKLE_LEAVES_TABLE)?;
        let serialized = serialize_leaves(leaves);
        table.insert(tree_id, serialized.as_slice())?;
    }
    write_txn.commit()?;
    Ok(())
}

/// Poseidon hash of two values (as used in Railgun Merkle tree).
fn poseidon_pair(left: U256, right: U256) -> Result<U256> {
    let mut poseidon = Poseidon::<Fr>::new_circom(2)?;

    let left_fr = u256_to_fr(left)?;
    let right_fr = u256_to_fr(right)?;

    let hash = poseidon.hash(&[left_fr, right_fr])?;
    fr_to_u256(hash)
}

fn u256_to_fr(value: U256) -> Result<Fr> {
    let bytes = value.to_le_bytes::<32>();
    Ok(Fr::from_le_bytes_mod_order(&bytes))
}

fn fr_to_u256(fr: Fr) -> Result<U256> {
    let bigint = fr.into_bigint();
    let be = bigint.to_bytes_be();
    let mut padded = [0u8; 32];
    let start = 32 - be.len();
    padded[start..].copy_from_slice(&be);
    Ok(U256::from_be_bytes(padded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    fn temp_db_path() -> PathBuf {
        let mut path = env::temp_dir();
        path.push(format!("zeus-railgun-test-{}.redb", std::process::id()));
        // Clean up previous test run if it exists
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn test_empty_tree() {
        let tree = PoseidonMerkleTree::new().unwrap();
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_insert_and_root() {
        let mut tree = PoseidonMerkleTree::new().unwrap();
        let c1 = U256::from(12345u64);
        let root1 = tree.insert(c1).unwrap();
        assert_eq!(tree.len(), 1);
        assert!(root1 != U256::ZERO);

        let c2 = U256::from(67890u64);
        let root2 = tree.insert(c2).unwrap();
        assert_eq!(tree.len(), 2);
        assert!(root2 != root1);
    }

    #[test]
    fn test_proof() {
        let mut tree = PoseidonMerkleTree::new().unwrap();
        tree.insert(U256::from(1u64)).unwrap();
        tree.insert(U256::from(2u64)).unwrap();

        let (leaf, proof, indices) = tree.get_proof(0).unwrap();
        assert_eq!(leaf, U256::from(1u64));
        assert_eq!(proof.len(), indices.len());
    }

    #[test]
    fn test_from_leaves_roundtrip() {
        let mut original = PoseidonMerkleTree::new().unwrap();
        original.insert(U256::from(42u64)).unwrap();
        original.insert(U256::from(99u64)).unwrap();

        let leaves = original.leaves().to_vec();
        let restored = PoseidonMerkleTree::from_leaves(leaves).unwrap();

        assert_eq!(restored.root(), original.root());
        assert_eq!(restored.len(), original.len());
    }

    #[test]
    fn test_persist_and_reload() {
        let db_path = temp_db_path();
        let tree_id = "test-chain";

        let db = Database::create(&db_path).unwrap();

        // Create and populate
        let mut tree1 = PoseidonMerkleTree::load(&db, tree_id).unwrap();
        tree1.insert(U256::from(111u64)).unwrap();
        tree1.insert(U256::from(222u64)).unwrap();
        tree1.save(&db, tree_id).unwrap();

        // Re-load from same handle and verify
        let tree2 = PoseidonMerkleTree::load(&db, tree_id).unwrap();
        assert_eq!(tree2.len(), 2);
        assert_eq!(tree2.root(), tree1.root());

        // Cleanup
        drop(db);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_load_save_with_shared_db() {
        let db_path = temp_db_path();
        let db = Database::create(&db_path).unwrap();
        let tree_id = "shared-test";

        let mut tree = PoseidonMerkleTree::load(&db, tree_id).unwrap();
        tree.insert(U256::from(777u64)).unwrap();
        tree.save(&db, tree_id).unwrap();

        let reloaded = PoseidonMerkleTree::load(&db, tree_id).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded.get_leaf(0), Some(U256::from(777u64)));

        drop(db);
        let _ = std::fs::remove_file(&db_path);
    }
}