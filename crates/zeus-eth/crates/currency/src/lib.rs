pub mod erc20;
pub mod native;

use serde::{Deserialize, Serialize};

pub use erc20::ERC20Token;
pub use native::NativeCurrency;

/// Represents a Currency, this can be a [NativeCurrency] to its chain (eg ETH, BNB) or any [ERC20Token]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Currency {
   Native(NativeCurrency),
   ERC20(ERC20Token),
}

impl From<NativeCurrency> for Currency {
   fn from(native: NativeCurrency) -> Self {
      Self::Native(native)
   }
}

impl From<ERC20Token> for Currency {
   fn from(erc20: ERC20Token) -> Self {
      Self::ERC20(erc20)
   }
}

impl Currency {
   /// Create a new Currency from a [NativeCurrency]
   pub fn from_native(native: NativeCurrency) -> Self {
      Self::Native(native)
   }

   /// Create a new Currency from an [ERC20Token]
   pub fn from_erc20(erc20: ERC20Token) -> Self {
      Self::ERC20(erc20)
   }

   pub fn is_native(&self) -> bool {
      matches!(self, Self::Native(_))
   }

   pub fn is_erc20(&self) -> bool {
      matches!(self, Self::ERC20(_))
   }

   /// is it WETH or WBNB
   pub fn is_native_wrapped(&self) -> bool {
      matches!(self, Self::ERC20(erc20) if erc20.is_weth() || erc20.is_wbnb())
   }

   /// Get the ERC20 inside
   pub fn erc20(&self) -> Option<&ERC20Token> {
      match self {
         Self::ERC20(erc20) => Some(erc20),
         _ => None,
      }
   }

   /// Get the NativeCurrency inside
   pub fn native(&self) -> Option<&NativeCurrency> {
      match self {
         Self::Native(native) => Some(native),
         _ => None,
      }
   }

   pub fn chain_id(&self) -> u64 {
      match self {
         Self::Native(native) => native.chain_id,
         Self::ERC20(erc20) => erc20.chain_id,
      }
   }

   pub fn symbol(&self) -> &String {
      match self {
         Self::Native(native) => &native.symbol,
         Self::ERC20(erc20) => &erc20.symbol,
      }
   }

   pub fn name(&self) -> &String {
      match self {
         Self::Native(native) => &native.name,
         Self::ERC20(erc20) => &erc20.name,
      }
   }

   pub fn decimals(&self) -> u8 {
      match self {
         Self::Native(native) => native.decimals,
         Self::ERC20(erc20) => erc20.decimals,
      }
   }
}