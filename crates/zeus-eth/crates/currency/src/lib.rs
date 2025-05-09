pub mod erc20;
pub mod native;

use abi::alloy_primitives::Address;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use utils::is_base_token;

pub use erc20::ERC20Token;
pub use native::NativeCurrency;

/// Represents a Currency, this can be a [NativeCurrency] to its chain (eg ETH, BNB) or any [ERC20Token]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
   Native(NativeCurrency),
   ERC20(ERC20Token),
}

impl Default for Currency {
   fn default() -> Self {
      Self::Native(NativeCurrency::default())
   }
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
   pub fn is_native(&self) -> bool {
      matches!(self, Self::Native(_))
   }

   pub fn is_erc20(&self) -> bool {
      matches!(self, Self::ERC20(_))
   }

   /// eg. is it WETH or WBNB
   pub fn is_native_wrapped(&self) -> bool {
      matches!(self, Self::ERC20(erc20) if erc20.is_weth() || erc20.is_wbnb())
   }

   pub fn is_weth_or_eth(&self) -> bool {
      if self.is_native() {
         return true;
      }
      if self.is_native_wrapped() {
         return true;
      }
      false
   }

   /// Is this Currency a base currency?
   ///
   /// See [is_base_token]
   pub fn is_base(&self) -> bool {
      if self.is_native() {
         return true;
      }

      let erc20 = self.to_erc20();
      is_base_token(erc20.chain_id, erc20.address)
   }

   /// Convert this currency to a wrapped native currency
   /// eg. WETH
   ///
   /// Shortcut for [ERC20Token::wrapped_native_token]
   pub fn to_wrapped_native(&self) -> ERC20Token {
      ERC20Token::wrapped_native_token(self.chain_id())
   }

   pub fn to_weth_currency(&self) -> Currency {
      Currency::from(self.to_wrapped_native())
   }

   /// Convert this currency to an [ERC20Token]
   ///
   /// If it's already an `ERC20Token`, we just return it
   ///
   /// If it's a `NativeCurrency`, we convert it to it's `ERC20Token` version
   /// for example ETH will become WETH
   pub fn to_erc20(&self) -> Cow<ERC20Token> {
      match self {
         Currency::ERC20(erc20) => Cow::Borrowed(erc20),
         Currency::Native(_) => Cow::Owned(self.to_wrapped_native()),
      }
   }

   /// Get the address of the ERC20 token
   ///
   /// Shortcut for [Self::to_erc20()]
   pub fn address(&self) -> Address {
      self.to_erc20().address
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
