use std::collections::HashMap;

use crate::{merkle_tree::RailgunMerkleProof, note::utxo::UtxoNote, poi::types::ListKey};

#[derive(Clone)]
pub struct PoiNote {
    pub inner: UtxoNote,
    pub pois: HashMap<ListKey, RailgunMerkleProof>,
}

impl PoiNote {
    pub fn new(note: UtxoNote, pois: HashMap<ListKey, RailgunMerkleProof>) -> Self {
        Self { inner: note, pois }
    }
}
