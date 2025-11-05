use zeus_eth::alloy_primitives::Address;
use zeus_wallet::Wallet;

// Argon2 parameters used to derive the seed from the credentials
// Hash lenght is always 64 bytes (512 bits)
pub const M_COST: u32 = 8192_000;
pub const T_COST: u32 = 96;
pub const P_COST: u32 = 1;

pub const DEV_M_COST: u32 = 256_000;
pub const DEV_T_COST: u32 = 16;
pub const DEV_P_COST: u32 = 1;

/// Helper struct to store info for a wallet (name, address, etc)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletInfo {
   pub address: Address,
   name: String,
   pub is_master: bool,
   pub is_child: bool,
   pub is_imported: bool,
}

impl WalletInfo {
   pub fn from_wallet(wallet: &Wallet) -> Self {
      Self {
         address: wallet.address(),
         name: wallet.name.clone(),
         is_master: wallet.is_master(),
         is_child: wallet.is_child(),
         is_imported: wallet.is_imported(),
      }
   }

   pub fn is_master(&self) -> bool {
      self.is_master
   }

   pub fn is_child(&self) -> bool {
      self.is_child
   }

   pub fn is_imported(&self) -> bool {
      self.is_imported
   }

   pub fn name(&self) -> String {
      let id = if self.is_master() {
         "(Master)"
      } else if self.is_child() {
         "(Child)"
      } else {
         "(Imported)"
      };

      format!("{} {}", self.name, id)
   }

   pub fn address_truncated(&self) -> String {
      format!(
         "{}...{}",
         &self.address.to_string()[..6],
         &self.address.to_string()[36..]
      )
   }
}