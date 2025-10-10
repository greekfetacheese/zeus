pub mod erc20;
pub mod native;

use crate::utils::is_base_token;
use alloy_primitives::Address;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub use erc20::ERC20Token;
pub use native::NativeCurrency;

/// Represents a Currency, this can be a [NativeCurrency] to its chain (eg ETH, BNB) or any [ERC20Token]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
   pub fn native(chain: u64) -> Self {
      Self::Native(NativeCurrency::from(chain))
   }

   pub fn wrapped_native(chain: u64) -> Self {
      Self::ERC20(ERC20Token::wrapped_native_token(chain))
   }

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

   pub fn is_stablecoin(&self) -> bool {
      if self.is_native() {
         return false;
      }

      self.to_erc20().is_stablecoin()
   }

   /// Convert this currency to a wrapped native currency
   /// 
   /// For example if the chain_id is 1 this will become [ERC20Token::weth()]
   pub fn to_wrapped_native(&self) -> ERC20Token {
      ERC20Token::wrapped_native_token(self.chain_id())
   }

   /// Convert this currency to an [ERC20Token]
   ///
   /// If it's already an `ERC20Token`, we just return it
   ///
   /// If it's a `NativeCurrency`, it converts it's wrapped version
   /// for example ETH will become WETH
   pub fn to_erc20(&self) -> Cow<'_, ERC20Token> {
      match self {
         Currency::ERC20(erc20) => Cow::Borrowed(erc20),
         Currency::Native(_) => Cow::Owned(self.to_wrapped_native()),
      }
   }

   /// Get the address of this Currency
   ///
   /// If it's a `NativeCurrency`, it returns [Address::ZERO]
   pub fn address(&self) -> Address {
      if self.is_erc20() {
         self.to_erc20().address
      } else {
         Address::ZERO
      }
   }

   /// Get the ERC20 inside
   pub fn erc20_opt(&self) -> Option<&ERC20Token> {
      match self {
         Self::ERC20(erc20) => Some(erc20),
         _ => None,
      }
   }

   /// Get the NativeCurrency inside
   pub fn native_opt(&self) -> Option<&NativeCurrency> {
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
