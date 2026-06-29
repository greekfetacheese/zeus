//! Poseidon Merkle Tree for Railgun private state.
//!
//! This is a simple in-memory implementation of the Railgun Merkle tree
//! using light-poseidon. In a real wallet we would persist this (sled or similar)
//! and support incremental updates from the scanner.

use alloy_primitives::U256;
use anyhow::{anyhow, Result};
use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use light_poseidon::{Poseidon, PoseidonHasher};

const TREE_DEPTH: usize = 32; // Railgun typically uses 32 levels

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
    /// Create a new empty tree.
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

    #[test]
    fn test_empty_tree() {
        let tree = PoseidonMerkleTree::new().unwrap();
        assert_eq!(tree.len(), 0);
        // Root should be the all-zero hash
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
}