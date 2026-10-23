//! What a transaction notification describes.
//!
//! Pure data over the decoded main event, so it lives beside the pipeline that
//! produces it rather than in the window that renders it.

use super::events::{
   BridgeParams, DecodedEvent, NftApproveParams, NftTransferParams, PrivateTransferParams,
   ShieldParams, SwapParams, TokenApproveParams, TransferParams, UnshieldParams, UnwrapWETHParams,
   WrapETHParams,
};

#[derive(Clone)]
pub enum NotificationType {
   Swap(SwapParams),

   Bridge(BridgeParams),

   WrapETH(WrapETHParams),

   UnwrapWETH(UnwrapWETHParams),

   Transfer(TransferParams),

   /// NFT transfer / mint / burn
   NftTransfer(NftTransferParams),

   /// An NFT approval or revoke — a collection-wide grant or one token's operator.
   NftApproval(NftApproveParams),

   TokenApproval(TokenApproveParams),

   Shield(ShieldParams),

   Unshield(UnshieldParams),

   PrivateTransfer(PrivateTransferParams),

   Other(String),
}

impl NotificationType {
   pub fn from_main_event(main_event: DecodedEvent) -> Self {
      match main_event {
         DecodedEvent::Bridge(params) => Self::Bridge(params),
         DecodedEvent::SwapToken(params) => Self::Swap(params),
         DecodedEvent::Transfer(params) => Self::Transfer(params),
         DecodedEvent::NftTransfer(params) => Self::NftTransfer(params),
         DecodedEvent::TokenApprove(params) => Self::TokenApproval(params),
         DecodedEvent::WrapETH(params) => Self::WrapETH(params),
         DecodedEvent::UnwrapWETH(params) => Self::UnwrapWETH(params),
         DecodedEvent::UniswapPositionOperation(_params) => Self::Other(String::new()),
         DecodedEvent::EOADelegate(_params) => Self::Other(String::new()),
         DecodedEvent::Permit(_params) => Self::Other(String::new()),
         DecodedEvent::NftApprove(params) => Self::NftApproval(params),
         DecodedEvent::Shield(params) => Self::Shield(params),
         DecodedEvent::Unshield(params) => Self::Unshield(params),
         DecodedEvent::PrivateTransfer(params) => Self::PrivateTransfer(params),
         DecodedEvent::Other => Self::Other("Transaction".to_string()),
      }
   }

   pub fn is_swap(&self) -> bool {
      matches!(self, NotificationType::Swap { .. })
   }

   pub fn is_bridge(&self) -> bool {
      matches!(self, NotificationType::Bridge { .. })
   }

   pub fn is_wrap(&self) -> bool {
      matches!(self, NotificationType::WrapETH { .. })
   }

   pub fn is_unwrap(&self) -> bool {
      matches!(self, NotificationType::UnwrapWETH { .. })
   }

   pub fn is_transfer(&self) -> bool {
      matches!(self, NotificationType::Transfer { .. })
   }

   pub fn is_nft_transfer(&self) -> bool {
      matches!(self, NotificationType::NftTransfer { .. })
   }

   pub fn is_nft_approval(&self) -> bool {
      matches!(self, NotificationType::NftApproval { .. })
   }

   pub fn is_token_approval(&self) -> bool {
      matches!(self, NotificationType::TokenApproval { .. })
   }

   pub fn is_shield(&self) -> bool {
      matches!(self, NotificationType::Shield { .. })
   }

   pub fn is_unshield(&self) -> bool {
      matches!(self, NotificationType::Unshield { .. })
   }

   pub fn is_private_transfer(&self) -> bool {
      matches!(self, NotificationType::PrivateTransfer { .. })
   }

   pub fn is_other(&self) -> bool {
      matches!(self, NotificationType::Other { .. })
   }

   pub fn swap_params(&self) -> &SwapParams {
      match self {
         NotificationType::Swap(params) => params,
         _ => panic!("NotificationType is not a swap"),
      }
   }

   pub fn bridge_params(&self) -> &BridgeParams {
      match self {
         NotificationType::Bridge(params) => params,
         _ => panic!("NotificationType is not a bridge"),
      }
   }

   pub fn wrap_eth_params(&self) -> &WrapETHParams {
      match self {
         NotificationType::WrapETH(params) => params,
         _ => panic!("NotificationType is not a wrap eth"),
      }
   }

   pub fn unwrap_weth_params(&self) -> &UnwrapWETHParams {
      match self {
         NotificationType::UnwrapWETH(params) => params,
         _ => panic!("NotificationType is not a unwrap weth"),
      }
   }

   pub fn transfer_params(&self) -> &TransferParams {
      match self {
         NotificationType::Transfer(params) => params,
         _ => panic!("NotificationType is not a transfer"),
      }
   }

   pub fn token_approval_params(&self) -> &TokenApproveParams {
      match self {
         NotificationType::TokenApproval(params) => params,
         _ => panic!("NotificationType is not a token approval"),
      }
   }

   pub fn nft_transfer_params(&self) -> &NftTransferParams {
      match self {
         NotificationType::NftTransfer(params) => params,
         _ => panic!("NotificationType is not an NFT transfer"),
      }
   }

   pub fn nft_approval_params(&self) -> &NftApproveParams {
      match self {
         NotificationType::NftApproval(params) => params,
         _ => panic!("NotificationType is not an NFT approval"),
      }
   }

   pub fn shield_params(&self) -> &ShieldParams {
      match self {
         NotificationType::Shield(params) => params,
         _ => panic!("NotificationType is not a shield"),
      }
   }

   pub fn unshield_params(&self) -> &UnshieldParams {
      match self {
         NotificationType::Unshield(params) => params,
         _ => panic!("NotificationType is not a unshield"),
      }
   }

   pub fn private_transfer_params(&self) -> &PrivateTransferParams {
      match self {
         NotificationType::PrivateTransfer(params) => params,
         _ => panic!("NotificationType is not a private transfer"),
      }
   }

   pub fn other_params(&self) -> String {
      match self {
         NotificationType::Other(text) => text.clone(),
         _ => panic!("NotificationType is not Other"),
      }
   }
}
