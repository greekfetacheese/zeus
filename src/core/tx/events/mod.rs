//! Decoded transaction events and per-protocol param types.
//!
//! - [`DecodedEvent`]: stable serializable user-facing event kind
//! - [`kinds`]: params + log decoders per protocol/family
//! - [`decode`]: registry / topic0 dispatch used by `TransactionAnalysis::new`

pub mod decode;
pub mod kinds;

pub use kinds::*;

use crate::core::types::Dapp;
use crate::utils::TimeStamp;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};
use zeus_railgun::abi::railgun::{TokenData, TokenType};
use zeus_railgun::caip::AssetId;

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
   pub fn dummy_eoa_delegate() -> Self {
      let chain = 1;
      let eoa = Address::ZERO;
      let address = Address::ZERO;
      let nonce = 0;

      let params = EOADelegateParams {
         chain,
         eoa,
         address,
         nonce,
      };

      Self::EOADelegate(params)
   }

   pub fn dummy_permit() -> Self {
      let chain = 1;
      let owner = Address::ZERO;
      let token = Currency::from(ERC20Token::weth());
      let spender = Address::ZERO;

      let wei = U256::MAX - U256::from(1);
      let amount = NumericValue::format_wei(wei, 18);
      let amount_usd = Some(NumericValue::value(amount.f64(), 1600.0));
      let expiration = TimeStamp::now_as_secs().unwrap().saturating_add_secs(600);

      let params = PermitParams {
         event_name: "Permit".to_string(),
         chain,
         owner,
         token,
         spender,
         amount,
         amount_usd,
         expiration,
      };

      Self::Permit(params)
   }

   pub fn dummy_token_approve() -> Self {
      let token = ERC20Token::weth();
      let amount = NumericValue::parse_to_wei("1000000000", 18);
      let amount_usd = Some(NumericValue::value(amount.f64(), 1600.0));

      let owner = Address::ZERO;
      let spender = Address::ZERO;

      let params = TokenApproveParams {
         token,
         amount,
         amount_usd,
         owner,
         spender,
      };

      Self::TokenApprove(params)
   }

   pub fn dummy_wrap_eth() -> Self {
      let dst = Address::ZERO;
      let eth_wrapped = NumericValue::parse_to_wei("1", 18);
      let eth_wrapped_usd = NumericValue::value(eth_wrapped.f64(), 1600.0);

      Self::WrapETH(WrapETHParams {
         chain: 1,
         recipient: dst,
         eth_wrapped: eth_wrapped.clone(),
         eth_wrapped_usd: Some(eth_wrapped_usd.clone()),
      })
   }

   pub fn dummy_unwrap_weth() -> Self {
      let src = Address::ZERO;
      let weth_unwrapped = NumericValue::parse_to_wei("1", 18);
      let weth_unwrapped_usd = NumericValue::value(weth_unwrapped.f64(), 1600.0);

      Self::UnwrapWETH(UnwrapWETHParams {
         chain: 1,
         src,
         weth_unwrapped: weth_unwrapped.clone(),
         weth_unwrapped_usd: Some(weth_unwrapped_usd.clone()),
         eth_received: weth_unwrapped,
         eth_received_usd: Some(weth_unwrapped_usd),
      })
   }

   pub fn dummy_shield() -> Self {
      let chain = 1;
      let recipient = "0zk1qy9r469tey0ptmp7unlph80w5aw3hf8z39une75a2ewd8vlmgvs2hrv7j6fe3z53lugdcpevcmd84mghtk07gd66s4qw452llcuzap2934nyh45jxz4ry55rq67".to_string();
      let token = ERC20Token::weth();
      let amount = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount.f64(), 1600.0);
      let fee = NumericValue::parse_to_wei("0.0001", 18);
      let fee_usd = NumericValue::value(fee.f64(), 1600.0);
      let asset_id = AssetId::Erc20(token.address);

      Self::Shield(ShieldParams {
         chain,
         recipient: Some(recipient),
         asset: asset_id,
         amount_wei: amount.wei(),
         erc20: Some(token),
         amount: Some(amount),
         amount_usd: Some(amount_usd),
         fee: Some(fee),
         fee_usd: Some(fee_usd),
      })
   }

   pub fn dummy_unshield() -> Self {
      let chain = 1;
      let recipient = Address::ZERO;
      let token_data = TokenData {
         tokenType: TokenType::ERC20,
         tokenAddress: Address::ZERO,
         tokenSubID: U256::ZERO,
      };
      let erc20 = Some(ERC20Token::weth());
      let amount_wei = U256::MAX - U256::from(1);
      let amount = Some(NumericValue::parse_to_wei("1", 18));
      let amount_usd = Some(NumericValue::value(
         amount.as_ref().unwrap().f64(),
         1600.0,
      ));
      let fee = Some(NumericValue::parse_to_wei("0.0001", 18));
      let fee_usd = Some(NumericValue::value(
         fee.as_ref().unwrap().f64(),
         1600.0,
      ));
      let is_self_broadcast = false;
      let broadcaster_fee = Some(NumericValue::parse_to_wei("0.001", 18));
      let broadcaster_fee_usd = Some(NumericValue::value(
         broadcaster_fee.as_ref().unwrap().f64(),
         1600.0,
      ));

      Self::Unshield(UnshieldParams {
         chain,
         recipient,
         token_data,
         erc20: erc20.clone(),
         amount_wei,
         amount,
         amount_usd,
         fee,
         fee_usd,
         is_self_broadcast,
         fee_token: erc20,
         broadcaster_fee,
         broadcaster_fee_usd,
      })
   }

   pub fn dummy_uniswap_position_operation() -> Self {
      let currency0 = Currency::from(ERC20Token::weth());
      let currency1 = Currency::from(ERC20Token::dai());
      let amount0 = NumericValue::parse_to_wei("100", 18);
      let amount1 = NumericValue::parse_to_wei("100", 18);
      let amount0_usd = NumericValue::value(amount0.f64(), 1600.0);
      let amount1_usd = NumericValue::value(amount1.f64(), 1600.0);
      let min_amount0 = NumericValue::parse_to_wei("99", 18);
      let min_amount1 = NumericValue::parse_to_wei("99", 18);
      let min_amount0_usd = NumericValue::value(min_amount0.f64(), 1600.0);
      let min_amount1_usd = NumericValue::value(min_amount1.f64(), 1600.0);

      let params = UniswapPositionParams {
         position_operation: PositionOperation::AddLiquidity,
         currency0,
         currency1,
         amount0,
         amount1,
         amount0_usd: Some(amount0_usd),
         amount1_usd: Some(amount1_usd),
         min_amount0: Some(min_amount0),
         min_amount1: Some(min_amount1),
         min_amount0_usd: Some(min_amount0_usd),
         min_amount1_usd: Some(min_amount1_usd),
         sender: Address::ZERO,
         recipient: None,
      };

      Self::UniswapPositionOperation(params)
   }

   pub fn dummy_swap() -> Self {
      let input_token = Currency::from(ERC20Token::weth());
      let output_token = Currency::from(ERC20Token::dai());
      let amount_in = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount_in.f64(), 1600.0);
      let min_received = NumericValue::parse_to_wei("1600", 18);
      let min_received = min_received.calc_slippage(0.5, 18);
      let min_received_usd = NumericValue::value(min_received.f64(), 1.0);

      let params = SwapParams {
         dapp: Dapp::Uniswap,
         input_currency: input_token,
         output_currency: output_token,
         amount_in: amount_in.clone(),
         amount_in_usd: Some(amount_usd.clone()),
         received: amount_usd.clone(),
         received_usd: Some(amount_usd),
         min_received: Some(min_received),
         min_received_usd: Some(min_received_usd),
         sender: Address::ZERO,
         recipient: Some(Address::ZERO),
      };

      Self::SwapToken(params)
   }

   pub fn dummy_bridge() -> Self {
      let input_token = Currency::from(ERC20Token::weth());
      let output_token = Currency::from(ERC20Token::weth());
      let amount_in = NumericValue::parse_to_wei("0.000001", 18);
      let amount_usd = NumericValue::value(amount_in.f64(), 1600.0);

      let params = BridgeParams {
         dapp: Dapp::Across,
         origin_chain: 1,
         destination_chain: 10,
         input_currency: input_token,
         output_currency: output_token,
         amount: amount_in.clone(),
         amount_usd: Some(amount_usd.clone()),
         received: amount_in.clone(),
         received_usd: Some(amount_usd),
         depositor: Address::ZERO,
         recipient: Address::ZERO,
      };

      Self::Bridge(params)
   }

   pub fn dummy_transfer() -> Self {
      let currency: Currency = NativeCurrency::from(1).into();
      let amount = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount.f64(), 1600.0);

      let params = TransferParams {
         currency,
         amount,
         amount_usd: Some(amount_usd),
         real_amount_sent: None,
         real_amount_sent_usd: None,
         sender: Address::ZERO,
         recipient: Address::ZERO,
      };

      Self::Transfer(params)
   }

   pub fn dummy_erc20_transfer() -> Self {
      let currency: Currency = ERC20Token::weth().into();
      let amount = NumericValue::parse_to_wei("1", 18);
      let amount_usd = NumericValue::value(amount.f64(), 1600.0);

      let params = TransferParams {
         currency,
         amount: amount.clone(),
         amount_usd: Some(amount_usd.clone()),
         real_amount_sent: Some(amount),
         real_amount_sent_usd: Some(amount_usd),
         sender: Address::ZERO,
         recipient: Address::ZERO,
      };

      Self::Transfer(params)
   }

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
         Self::WrapETH(_) => "Wrap ETH".to_string(),
         Self::UnwrapWETH(_) => "Unwrap WETH".to_string(),
         Self::Bridge(_) => "Bridge".to_string(),
         Self::SwapToken(_) => "Swap".to_string(),
         Self::UniswapPositionOperation(p) => p.name(),
         Self::EOADelegate(_) => "Wallet Delegation".to_string(),
         Self::Permit(p) => p.event_name.clone(),
         Self::TokenApprove(p) => p.name().to_string(),
         Self::Shield(_) => "Shield".to_string(),
         Self::Unshield(_) => "Unshield".to_string(),
         Self::PrivateTransfer(_) => "Private Transfer".to_string(),
         Self::Other => "Unknown Interaction".to_string(),
      }
   }

   /// Get the bridge params
   ///
   /// Panics if the action is not a bridge
   pub fn bridge_params(&self) -> &BridgeParams {
      match self {
         Self::Bridge(params) => params,
         _ => panic!("Action is not a bridge"),
      }
   }

   /// Get the swap params
   ///
   /// Panics if the action is not a swap
   pub fn swap_params(&self) -> &SwapParams {
      match self {
         Self::SwapToken(params) => params,
         _ => panic!("Action is not a swap"),
      }
   }

   /// Get the transfer params
   ///
   /// Panics if the action is not a transfer
   pub fn transfer_params(&self) -> &TransferParams {
      match self {
         Self::Transfer(params) => params,
         _ => panic!("Action is not a transfer"),
      }
   }

   /// Get the token approval params
   ///
   /// Panics if the action is not a token approval
   pub fn token_approval_params(&self) -> &TokenApproveParams {
      match self {
         Self::TokenApprove(params) => params,
         _ => panic!("Action is not a token approval"),
      }
   }

   /// Get the wrap eth params
   ///
   /// Panics if the action is not a wrap eth
   pub fn wrap_eth_params(&self) -> &WrapETHParams {
      match self {
         Self::WrapETH(params) => params,
         _ => panic!("Action is not a wrap eth"),
      }
   }

   /// Get the unwrap weth params
   ///
   /// Panics if the action is not a unwrap eth
   pub fn unwrap_weth_params(&self) -> &UnwrapWETHParams {
      match self {
         Self::UnwrapWETH(params) => params,
         _ => panic!("Action is not a unwrap eth"),
      }
   }

   pub fn uniswap_position_params(&self) -> &UniswapPositionParams {
      match self {
         Self::UniswapPositionOperation(params) => params,
         _ => panic!("Action is not a Uniswap position operation"),
      }
   }

   pub fn eoa_delegate_params(&self) -> &EOADelegateParams {
      match self {
         Self::EOADelegate(params) => params,
         _ => panic!("Action is not a EOA Delegate"),
      }
   }

   pub fn permit_params(&self) -> &PermitParams {
      match self {
         Self::Permit(params) => params,
         _ => panic!("Action is not a Permit"),
      }
   }

   pub fn shield_params(&self) -> &ShieldParams {
      match self {
         Self::Shield(params) => params,
         _ => panic!("Action is not a Shield"),
      }
   }

   pub fn unshield_params(&self) -> &UnshieldParams {
      match self {
         Self::Unshield(params) => params,
         _ => panic!("Action is not a Unshield"),
      }
   }

   pub fn private_transfer_params(&self) -> &PrivateTransferParams {
      match self {
         Self::PrivateTransfer(params) => params,
         _ => panic!("Action is not a Private Transfer"),
      }
   }


   pub fn as_bridge(&self) -> Option<&BridgeParams> {
      match self {
         Self::Bridge(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_swap(&self) -> Option<&SwapParams> {
      match self {
         Self::SwapToken(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_transfer(&self) -> Option<&TransferParams> {
      match self {
         Self::Transfer(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_token_approve(&self) -> Option<&TokenApproveParams> {
      match self {
         Self::TokenApprove(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_wrap_eth(&self) -> Option<&WrapETHParams> {
      match self {
         Self::WrapETH(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_unwrap_weth(&self) -> Option<&UnwrapWETHParams> {
      match self {
         Self::UnwrapWETH(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_uniswap_position(&self) -> Option<&UniswapPositionParams> {
      match self {
         Self::UniswapPositionOperation(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_eoa_delegate(&self) -> Option<&EOADelegateParams> {
      match self {
         Self::EOADelegate(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_permit(&self) -> Option<&PermitParams> {
      match self {
         Self::Permit(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_shield(&self) -> Option<&ShieldParams> {
      match self {
         Self::Shield(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_unshield(&self) -> Option<&UnshieldParams> {
      match self {
         Self::Unshield(p) => Some(p),
         _ => None,
      }
   }

   pub fn as_private_transfer(&self) -> Option<&PrivateTransferParams> {
      match self {
         Self::PrivateTransfer(p) => Some(p),
         _ => None,
      }
   }

   pub fn is_bridge(&self) -> bool {
      matches!(self, Self::Bridge(_))
   }

   pub fn is_swap(&self) -> bool {
      matches!(self, Self::SwapToken(_))
   }

   pub fn is_uniswap_position_op(&self) -> bool {
      matches!(self, Self::UniswapPositionOperation(_))
   }

   pub fn is_native_transfer(&self) -> bool {
      match self {
         Self::Transfer(params) => params.is_native_transfer(),
         _ => false,
      }
   }

   pub fn is_erc20_transfer(&self) -> bool {
      match self {
         Self::Transfer(params) => params.is_erc20_transfer(),
         _ => false,
      }
   }

   pub fn is_token_approval(&self) -> bool {
      matches!(self, Self::TokenApprove(_))
   }

   pub fn is_wrap_eth(&self) -> bool {
      matches!(self, Self::WrapETH(_))
   }

   pub fn is_unwrap_weth(&self) -> bool {
      matches!(self, Self::UnwrapWETH(_))
   }

   pub fn is_eoa_delegate(&self) -> bool {
      matches!(self, Self::EOADelegate(_))
   }

   pub fn is_permit(&self) -> bool {
      matches!(self, Self::Permit(_))
   }

   pub fn is_shield(&self) -> bool {
      matches!(self, Self::Shield(_))
   }

   pub fn is_unshield(&self) -> bool {
      matches!(self, Self::Unshield(_))
   }

   pub fn is_private_transfer(&self) -> bool {
      matches!(self, Self::PrivateTransfer(_))
   }

   pub fn is_other(&self) -> bool {
      matches!(self, Self::Other)
   }

   pub fn is_known(&self) -> bool {
      !self.is_other()
   }
}
