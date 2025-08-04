pub mod wallet;
pub mod vault;

pub use wallet::{Wallet, WalletInfo, SecureHDWallet};
pub use vault::Vault;

/// Saved contact by the user
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Contact {
   pub name: String,
   pub address: String,
}

impl Contact {
   pub fn new(name: String, address: String) -> Self {
      Self { name, address }
   }

   pub fn address_short(&self, start: usize, end: usize) -> String {
      let address_str = self.address.as_str();

      if address_str.len() < start + end {
         return address_str.to_string();
      }

      let start_part = &address_str[..start];
      let end_part = &address_str[address_str.len() - end..];

      format!("{}...{}", start_part, end_part)
   }
}
