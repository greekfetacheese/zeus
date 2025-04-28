use super::UniswapPool;
use currency::Currency;
use alloy_primitives::{U256, Bytes};
use alloy_dyn_abi::TypedData;

pub mod v4;

/// The params for the execute function
pub struct ExecuteParams {
   pub call_data: Bytes,
   /// The eth to be sent along with the transaction
   pub value: U256,
   /// Through Permit2
   pub token_needs_approval: bool,
   /// The message to be signed
   /// 
   /// This is just to show it in a UI, the message if any already signed internally
   pub message: Option<TypedData>,
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

   pub fn set_message(&mut self, message: Option<TypedData>) {
      self.message = message;
   }
}


/// The route a swap will go through
#[derive(Debug, Clone, PartialEq)]
pub struct Route<P: UniswapPool> {
   pub pools: Vec<P>,
   pub currency_in: Currency,
   pub currency_out: Currency,
}

impl<P: UniswapPool> Route<P> {
   pub fn new(pools: Vec<P>, currency_in: Currency, currency_out: Currency) -> Self {
      Self {
         pools,
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