use super::UniswapPool;
use alloy_primitives::{Bytes, U256};
use currency::Currency;
use utils::NumericValue;
use serde_json::Value;

pub mod v4;
mod tests;

pub struct ExecuteParams {
   pub call_data: Bytes,
   /// The eth to be sent along with the transaction
   pub value: U256,
   /// Through Permit2
   pub token_needs_approval: bool,
   /// The message to be signed
   ///
   /// This is just to show it in a UI, the message if any already signed internally
   pub message: Option<Value>,
}

impl ExecuteParams {
   pub fn new() -> Self {
      Self {
         call_data: Bytes::default(),
         value: U256::ZERO,
         token_needs_approval: false,
         message: None,
      }
   }

   pub fn set_call_data(&mut self, call_data: Bytes) {
      self.call_data = call_data;
   }

   pub fn set_value(&mut self, value: U256) {
      self.value = value;
   }

   pub fn set_token_needs_approval(&mut self, token_needs_approval: bool) {
      self.token_needs_approval = token_needs_approval;
   }

   pub fn set_message(&mut self, message: Option<Value>) {
      self.message = message;
   }
}

/// Represents a single atomic swap step within a potentially larger route.
#[derive(Debug, Clone, PartialEq)]
pub struct SwapStep<P: UniswapPool> {
   /// The specific pool used for this swap step.
   pub pool: P,
   /// The exact amount of `currency_in` being swapped in this step.
   pub amount_in: NumericValue,
   /// The simulated amount of `currency_out` received from this step.
   pub amount_out: NumericValue,
   /// The currency being provided to the pool.
   pub currency_in: Currency,
   /// The currency being received from the pool.
   pub currency_out: Currency,
}

impl<P: UniswapPool> SwapStep<P> {
   pub fn new(pool: P, amount_in: NumericValue, amount_out: NumericValue, currency_in: Currency, currency_out: Currency) -> Self {
      Self {
         pool,
         amount_in,
         amount_out,
         currency_in,
         currency_out,
      }
   }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum SwapType {
   /// Indicates that the swap is based on an exact input amount.
   ExactInput,

   /// Indicates that the swap is based on an exact output amount.
   ExactOutput,
}

impl SwapType {
   pub fn is_exact_input(&self) -> bool {
      matches!(self, Self::ExactInput)
   }

   pub fn is_exact_output(&self) -> bool {
      matches!(self, Self::ExactOutput)
   }
}

/*
pub fn encode_route_to_path(route: &Route<impl UniswapPool>, exact_output: bool) -> Result<Bytes, anyhow::Error> {
   let mut path: Vec<u8> = Vec::with_capacity(23 * route.pools.len() + 20);
   if exact_output {
      let mut token_out = route.currency_out.to_erc20();
      for pool in route.pools.iter().rev() {
         let (token_in, leg) = encode_leg(pool, &token_out)?;
         token_out = token_in;
         path.extend(leg);
      }
      path.extend(route.currency_in.to_erc20().address.abi_encode_packed());
   } else {
      let mut token_in = route.currency_in.to_erc20();
      for pool in route.pools.iter() {
         let (token_out, leg) = encode_leg(pool, &token_in)?;
         token_in = token_out;
         path.extend(leg);
      }
      path.extend(route.currency_out.to_erc20().address.abi_encode_packed());
   }
   Ok(path.into())
}

fn encode_leg(pool: &impl UniswapPool, token_in: &ERC20Token) -> Result<(ERC20Token, Vec<u8>), anyhow::Error> {
   let token_out;
   let leg: (Address, U24) = if pool.token0().address == token_in.address {
      token_out = pool.token1().clone();
      (pool.token0().address, pool.fee().try_into()?)
   } else {
      token_out = pool.token0().clone();
      (pool.token1().address, pool.fee().try_into()?)
   };
   Ok((token_out, leg.abi_encode_packed()))
}

*/
