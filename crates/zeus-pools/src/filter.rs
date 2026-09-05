use std::collections::HashSet;

use tracing::info;
use zeus_eth::{
   alloy_primitives::Address,
   amm::uniswap::{AnyUniswapPool, UniswapPool, state::batch_update_state},
   currency::{Currency, ERC20Token},
   types::ChainId,
   utils::{NumericValue, client::RpcClient, price_feed},
};

/// Same USD gate as `zeus-tokens` / `DEFAULT_POOL_MINIMUM_LIQUIDITY`.
const POOL_MINIMUM_LIQUIDITY: f64 = 10_000.0;

/// Drop pools whose fee is above this (Uniswap fee units / 10_000 = percent).
const MAX_FEE_PERCENT: f32 = 2.0;

pub async fn native_price(client: RpcClient, chain: ChainId) -> anyhow::Result<f64> {
   if chain.is_bsc() {
      price_feed::get_bnb_price(client, None).await
   } else {
      price_feed::get_eth_price(client, chain.id(), None).await
   }
}

/// WETH/USDC/USDT/DAI for the chain, plus WBTC and LINK when they exist on it.
pub fn known_token_addresses(chain_id: u64) -> HashSet<Address> {
   let mut addrs = HashSet::new();
   for token in ERC20Token::base_tokens(chain_id) {
      addrs.insert(token.address);
   }
   let wbtc = ERC20Token::wbtc();
   if wbtc.chain_id == chain_id {
      addrs.insert(wbtc.address);
   }
   let link = ERC20Token::link();
   if link.chain_id == chain_id {
      addrs.insert(link.address);
   }
   addrs
}

fn is_known_currency(currency: &Currency, quotes: &HashSet<Address>) -> bool {
   if currency.is_native() || currency.is_native_wrapped() {
      return true;
   }
   quotes.contains(&currency.address())
}

fn has_both_known_tokens(pool: &AnyUniswapPool, quotes: &HashSet<Address>) -> bool {
   is_known_currency(pool.currency0(), quotes) && is_known_currency(pool.currency1(), quotes)
}

fn fee_ok(pool: &AnyUniswapPool) -> bool {
   pool.fee().fee_percent() <= MAX_FEE_PERCENT
}

/// Cheap local cuts applied **before** `batch_update_state`.
///
/// Always drops fee > 2%. If `base_tokens_only`, also requires at least one
/// side to be native / WETH / USDC / USDT / DAI / WBTC / LINK.
pub fn prefilter_pools(
   chain: ChainId,
   pools: Vec<AnyUniswapPool>,
   base_tokens_only: bool,
) -> Vec<AnyUniswapPool> {
   let quotes = known_token_addresses(chain.id());
   let before = pools.len();
   let mut dropped_fee = 0usize;
   let mut dropped_base = 0usize;

   let kept: Vec<_> = pools
      .into_iter()
      .filter(|pool| {
         if !fee_ok(pool) {
            dropped_fee += 1;
            return false;
         }

         if base_tokens_only && !has_both_known_tokens(pool, &quotes) {
            dropped_base += 1;
            return false;
         }

         if pool.dex_kind().is_v4() {
            // Drop garbage pools with WETH
            if pool.currency0().is_native_wrapped() || pool.currency1().is_native_wrapped() {
               return false;
            }
         }

         true
      })
      .collect();

   info!(
      "Chain {}: prefilter {before} → {} (dropped fee>{}%: {dropped_fee}, no base token: {dropped_base})",
      chain.id(),
      kept.len(),
      MAX_FEE_PERCENT
   );
   kept
}

/// Drop pools whose base-side USD value is below $10k. Snapshot is left untouched.
pub async fn filter_liquid_pools(
   client: RpcClient,
   chain: ChainId,
   pools: Vec<AnyUniswapPool>,
   native_price: f64,
   concurrency: usize,
   batch_size: usize,
) -> anyhow::Result<Vec<AnyUniswapPool>> {
   let before = pools.len();
   let updated = batch_update_state(client, chain.id(), concurrency, batch_size, pools).await?;

   let mut kept = Vec::new();
   for pool in updated {
      if keep_pool(&pool, native_price) {
         kept.push(pool);
      }
   }

   info!(
      "Chain {}: kept {}/{} pools with base liquidity ≥ ${POOL_MINIMUM_LIQUIDITY}",
      chain.id(),
      kept.len(),
      before
   );
   Ok(kept)
}

fn keep_pool(pool: &AnyUniswapPool, native_price: f64) -> bool {
   if !pool.base_currency_exists() {
      return false;
   }

   let base_token = pool.base_currency();
   let price = if base_token.is_native() || base_token.is_native_wrapped() {
      native_price
   } else if base_token.is_stablecoin() {
      1.0
   } else {
      0.0
   };

   if price == 0.0 {
      return false;
   }

   let base_value = NumericValue::value(pool.base_balance().f64(), price);
   base_value.f64() >= POOL_MINIMUM_LIQUIDITY
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{
      alloy_primitives::Address,
      amm::uniswap::{DexKind, FeeAmount, UniswapV2Pool, UniswapV4Pool},
      currency::Currency,
      types::ChainId,
   };

   #[test]
   fn empty_state_is_not_liquid() {
      let pool: AnyUniswapPool = UniswapV2Pool::weth_uni().into();
      assert!(!keep_pool(&pool, 3000.0));
   }

   #[test]
   fn drops_fee_above_two_percent() {
      let mut pool = UniswapV4Pool::link_usdc();
      pool.fee = FeeAmount::CUSTOM(20_001); // 2.0001%
      let kept = prefilter_pools(ChainId::Ethereum, vec![pool.into()], true);
      assert!(kept.is_empty());
   }

   #[test]
   fn keeps_two_percent_fee() {
      let mut pool = UniswapV4Pool::link_usdc();
      pool.fee = FeeAmount::CUSTOM(20_000); // 2%
      let kept = prefilter_pools(ChainId::Ethereum, vec![pool.into()], true);
      assert_eq!(kept.len(), 1);
   }

   #[test]
   fn keeps_wbtc_and_link_pairs() {
      let kept = prefilter_pools(
         ChainId::Ethereum,
         vec![
            UniswapV4Pool::wbtc_usdt().into(),
            UniswapV4Pool::link_usdc().into(),
            UniswapV2Pool::weth_uni().into(),
         ],
         true,
      );
      assert_eq!(kept.len(), 3);
   }

   #[test]
   fn drops_pairs_with_no_base_token() {
      let junk = ERC20Token {
         chain_id: 1,
         address: Address::repeat_byte(0xab),
         decimals: 18,
         symbol: "JUNK".into(),
         name: "Junk".into(),
         total_supply: Default::default(),
      };
      let other = ERC20Token {
         chain_id: 1,
         address: Address::repeat_byte(0xcd),
         decimals: 18,
         symbol: "TRASH".into(),
         name: "Trash".into(),
         total_supply: Default::default(),
      };
      let pool = UniswapV4Pool::from_components(
         1,
         Currency::from(junk),
         Currency::from(other),
         FeeAmount::MEDIUM,
         DexKind::UniswapV4,
         Address::ZERO,
      );
      let kept = prefilter_pools(ChainId::Ethereum, vec![pool.into()], true);
      assert!(kept.is_empty());
   }

   #[test]
   fn base_tokens_only_can_be_disabled() {
      let junk = ERC20Token {
         chain_id: 1,
         address: Address::repeat_byte(0xab),
         decimals: 18,
         symbol: "JUNK".into(),
         name: "Junk".into(),
         total_supply: Default::default(),
      };
      let other = ERC20Token {
         chain_id: 1,
         address: Address::repeat_byte(0xcd),
         decimals: 18,
         symbol: "TRASH".into(),
         name: "Trash".into(),
         total_supply: Default::default(),
      };
      let pool = UniswapV4Pool::from_components(
         1,
         Currency::from(junk),
         Currency::from(other),
         FeeAmount::MEDIUM,
         DexKind::UniswapV4,
         Address::ZERO,
      );
      let kept = prefilter_pools(ChainId::Ethereum, vec![pool.into()], false);
      assert_eq!(kept.len(), 1);
   }
}
