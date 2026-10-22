//! The [`DecodedEvent`] enum and its non-mechanical methods.
//!
//! The `dummy_*` fixtures and the mechanical `*_params` / `as_*` / `is_*`
//! accessors live in [`super::accessors`].

use super::kinds::*;

use crate::ZeusCtx;
use serde::{Deserialize, Serialize};
use zeus_eth::currency::{Currency, NativeCurrency};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum DecodedEvent {
   /// Cross-chain bridge
   Bridge(BridgeParams),

   /// Swap
   SwapToken(SwapParams),

   /// An operation on a Uniswap position
   UniswapPositionOperation(UniswapPositionParams),

   /// ERC20 token approval
   TokenApprove(TokenApproveParams),

   /// ETH or ERC20 transfer
   Transfer(TransferParams),

   /// NFT (ERC-721 / ERC-1155) transfer, mint or burn
   NftTransfer(NftTransferParams),

   /// NFT (ERC-721 / ERC-1155) approval — per token or collection-wide
   NftApprove(NftApproveParams),

   /// Wrap ETH
   WrapETH(WrapETHParams),

   /// Unwrap WETH
   UnwrapWETH(UnwrapWETHParams),

   /// EOA delegate
   EOADelegate(EOADelegateParams),

   /// Permit2 approval
   Permit(PermitParams),

   /// Railgun Shield
   Shield(ShieldParams),

   /// Railgun Unshield
   Unshield(UnshieldParams),

   /// Railgun private (zk → zk) transfer
   PrivateTransfer(PrivateTransferParams),

   #[default]
   Other,
}

impl DecodedEvent {
   /// We consider any action to be MEV vulnerable that involves some kind of slippage
   pub fn is_mev_vulnerable(&self) -> bool {
      if self.is_swap() {
         return true;
      }

      if self.is_uniswap_position_op() {
         let params = self.uniswap_position_params();
         return !params.op_is_collect_fees();
      }

      if self.is_other() {
         return true;
      }

      false
   }

   pub fn name(&self) -> String {
      match self {
         Self::Transfer(p) => p.name(),
         Self::NftTransfer(p) => p.name(),
         Self::NftApprove(p) => p.name().to_string(),
         Self::WrapETH(_) => "Wrap ETH".to_string(),
         Self::UnwrapWETH(_) => "Unwrap WETH".to_string(),
         Self::Bridge(_) => "Bridge".to_string(),
         Self::SwapToken(_) => "Swap".to_string(),
         Self::UniswapPositionOperation(p) => p.name(),
         Self::EOADelegate(_) => "Wallet Delegation".to_string(),
         Self::Permit(p) => p.title(),
         Self::TokenApprove(p) => p.name().to_string(),
         Self::Shield(_) => "Shield".to_string(),
         Self::Unshield(_) => "Unshield".to_string(),
         Self::PrivateTransfer(_) => "Private Transfer".to_string(),
         Self::Other => "Unknown Interaction".to_string(),
      }
   }

   /// Recompute USD snapshots from the current price manager.
   ///
   /// Token amounts stay as decoded; only `*_usd` fields are overwritten.
   pub fn refresh_usd(&mut self, ctx: &ZeusCtx) {
      match self {
         Self::Bridge(p) => {
            p.amount_usd =
               Some(ctx.get_currency_value_for_amount(p.amount.f64(), &p.input_currency));
            p.received_usd =
               Some(ctx.get_currency_value_for_amount(p.received.f64(), &p.output_currency));
         }
         Self::SwapToken(p) => {
            p.amount_in_usd =
               Some(ctx.get_currency_value_for_amount(p.amount_in.f64(), &p.input_currency));
            p.received_usd =
               Some(ctx.get_currency_value_for_amount(p.received.f64(), &p.output_currency));
            if let Some(min) = &p.min_received {
               p.min_received_usd =
                  Some(ctx.get_currency_value_for_amount(min.f64(), &p.output_currency));
            }
         }
         Self::UniswapPositionOperation(p) => {
            p.amount0_usd = Some(ctx.get_currency_value_for_amount(p.amount0.f64(), &p.currency0));
            p.amount1_usd = Some(ctx.get_currency_value_for_amount(p.amount1.f64(), &p.currency1));
            if let Some(min) = &p.min_amount0 {
               p.min_amount0_usd = Some(ctx.get_currency_value_for_amount(min.f64(), &p.currency0));
            }
            if let Some(min) = &p.min_amount1 {
               p.min_amount1_usd = Some(ctx.get_currency_value_for_amount(min.f64(), &p.currency1));
            }
         }
         Self::TokenApprove(p) => {
            p.amount_usd = Some(ctx.get_token_value_for_amount(p.amount.f64(), &p.token));
         }
         Self::Transfer(p) => {
            p.amount_usd = Some(ctx.get_currency_value_for_amount(p.amount.f64(), &p.currency));
            if let Some(real) = &p.real_amount_sent {
               p.real_amount_sent_usd =
                  Some(ctx.get_currency_value_for_amount(real.f64(), &p.currency));
            }
         }
         Self::WrapETH(p) => {
            let currency = Currency::from(NativeCurrency::from(p.chain));
            p.eth_wrapped_usd =
               Some(ctx.get_currency_value_for_amount(p.eth_wrapped.f64(), &currency));
         }
         Self::UnwrapWETH(p) => {
            let currency = Currency::from(NativeCurrency::from(p.chain));
            p.weth_unwrapped_usd =
               Some(ctx.get_currency_value_for_amount(p.weth_unwrapped.f64(), &currency));
            p.eth_received_usd =
               Some(ctx.get_currency_value_for_amount(p.eth_received.f64(), &currency));
         }
         Self::Permit(p) => {
            p.amount_usd = Some(ctx.get_currency_value_for_amount(p.amount.f64(), &p.token));
         }
         Self::Shield(p) => {
            if let (Some(amount), Some(token)) = (&p.amount, &p.erc20) {
               p.amount_usd = Some(ctx.get_token_value_for_amount(amount.f64(), token));
            }
            if let (Some(fee), Some(token)) = (&p.fee, &p.erc20) {
               p.fee_usd = Some(ctx.get_token_value_for_amount(fee.f64(), token));
            }
         }
         Self::Unshield(p) => {
            if let (Some(amount), Some(token)) = (&p.amount, &p.erc20) {
               p.amount_usd = Some(ctx.get_token_value_for_amount(amount.f64(), token));
            }
            if let (Some(fee), Some(token)) = (&p.fee, &p.erc20) {
               p.fee_usd = Some(ctx.get_token_value_for_amount(fee.f64(), token));
            }
            if let (Some(fee), Some(token)) = (&p.broadcaster_fee, &p.fee_token) {
               p.broadcaster_fee_usd = Some(ctx.get_token_value_for_amount(fee.f64(), token));
            }
         }
         Self::PrivateTransfer(p) => {
            if let (Some(amount), Some(token)) = (&p.amount, &p.erc20) {
               p.amount_usd = Some(ctx.get_token_value_for_amount(amount.f64(), token));
            }
         }
         // Nothing to price and nothing to refresh: an NFT has no pool, and this app keeps no USD
         // value for one. That goes for an approval of one too.
         Self::NftTransfer(_) | Self::NftApprove(_) => {}
         Self::EOADelegate(_) | Self::Other => {}
      }
   }
}
