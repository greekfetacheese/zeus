use bincode_next::{Decode, Encode};

/// On-disk default token list entry. Field layout must stay in lockstep with
/// `zeus::core::context::currencies::TokenData` so the wallet can decode this blob.
#[derive(Clone, Encode, Decode)]
pub struct TokenData {
   pub chain_id: u64,
   pub address: String,
   pub name: String,
   pub symbol: String,
   pub decimals: u8,
   pub icon_data_x32: Vec<u8>,
   pub icon_data_x24: Vec<u8>,
}

impl TokenData {
   pub fn new(
      chain_id: u64,
      address: String,
      name: String,
      symbol: String,
      decimals: u8,
      icon_data_x32: Vec<u8>,
      icon_data_x24: Vec<u8>,
   ) -> Self {
      Self {
         chain_id,
         address,
         name,
         symbol,
         decimals,
         icon_data_x32,
         icon_data_x24,
      }
   }
}
