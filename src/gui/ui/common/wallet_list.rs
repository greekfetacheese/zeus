//! Shared wallet list: sort by USD value and collect per-wallet totals.

use crate::core::{WalletInfo, ZeusCtx};
use std::collections::HashMap;
use zeus_eth::{alloy_primitives::Address, utils::NumericValue};

/// Wallets sorted by USD value (public or private, matching privacy mode).
pub struct WalletListByValue {
   pub wallets: Vec<WalletInfo>,
   pub values: HashMap<Address, NumericValue>,
   pub chains: HashMap<Address, Vec<u64>>,
}

impl WalletListByValue {
   pub fn collect(ctx: &ZeusCtx) -> Self {
      let mut wallets = ctx.get_all_wallets_info();
      let include_testnets = ctx.chain().is_testnet();
      let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);

      wallets.sort_by(|a, b| {
         let value_a = ctx.get_total_value(a.address, include_testnets);
         let value_b = ctx.get_total_value(b.address, include_testnets);
         value_b
            .for_mode(privacy_mode)
            .f64()
            .partial_cmp(&value_a.for_mode(privacy_mode).f64())
            .unwrap_or(std::cmp::Ordering::Equal)
      });

      let mut values = HashMap::new();
      let mut chains = HashMap::new();
      for wallet in &wallets {
         let value = ctx.get_total_value(wallet.address, include_testnets);
         values.insert(
            wallet.address,
            value.for_mode(privacy_mode).clone(),
         );
         chains.insert(
            wallet.address,
            ctx.get_chains_that_have_balance(wallet.address),
         );
      }

      Self {
         wallets,
         values,
         chains,
      }
   }
}
