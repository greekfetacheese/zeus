use crate::core::ZeusCtx;
use alloy_sol_types::SolEvent;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   alloy_primitives::{Address, Log, U256},
   currency::ERC20Token,
   utils::NumericValue,
};
use zeus_railgun::abi::railgun::TokenType;
use zeus_railgun::{
   abi::railgun::{RailgunSmartWallet, TokenData},
   caip::AssetId,
};

/// Decoded Railgun shield event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldParams {
   pub chain: u64,
   pub recipient: Option<String>,
   pub asset: AssetId,
   pub amount_wei: U256,
   pub erc20: Option<ERC20Token>,
   pub amount: Option<NumericValue>,
   pub amount_usd: Option<NumericValue>,
   pub fee: Option<NumericValue>,
   pub fee_usd: Option<NumericValue>,
}

impl ShieldParams {
   pub async fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Vec<Self>, anyhow::Error> {
      if let Ok(decoded) = <RailgunSmartWallet::Shield as SolEvent>::decode_log(&log) {
         let mut events = Vec::new();

         if decoded.fees.len() != decoded.commitments.len() {
            tracing::warn!(
               "Shield event fees/commitments length mismatch: fees={}, commitments={}",
               decoded.fees.len(),
               decoded.commitments.len()
            );
         }

         for (idx, commitment) in decoded.commitments.iter().enumerate() {
            let fee_wei = decoded.fees.get(idx).copied().unwrap_or_else(|| {
               tracing::warn!("No fee at index {} for Shield event", idx);
               U256::ZERO
            });

            let asset: AssetId = commitment.token.clone().into();
            let amount_wei: U256 = commitment.value.saturating_to();
            let mut erc20 = None;
            let mut amount_fmt_opt = None;
            let mut amount_usd_opt = None;
            let mut fee_fmt_opt = None;
            let mut fee_usd_opt = None;

            // TODO: Add support for ERC721 and ERC1155
            if asset.is_erc20() {
               let token_addr = asset.erc20_address().unwrap();
               let token = ctx.get_token(chain, token_addr).await?;

               let amount = NumericValue::format_wei(amount_wei, token.decimals);
               let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);

               let fee = NumericValue::format_wei(fee_wei, token.decimals);
               let fee_usd = ctx.get_token_value_for_amount(fee.f64(), &token);

               amount_fmt_opt = Some(amount);
               amount_usd_opt = Some(amount_usd);
               fee_fmt_opt = Some(fee);
               fee_usd_opt = Some(fee_usd);
               erc20 = Some(token);
            }

            let event = ShieldParams {
               chain,
               recipient: None,
               asset,
               amount_wei,
               erc20,
               amount: amount_fmt_opt,
               amount_usd: amount_usd_opt,
               fee: fee_fmt_opt,
               fee_usd: fee_usd_opt,
            };

            events.push(event);
         }

         return Ok(events);
      }

      Err(anyhow!("Log decoding failed"))
   }
}

/// Railgun private (zk → zk) transfer, not decoded from a public ERC-20 log
/// built from the user intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateTransferParams {
   pub chain: u64,
   /// Recipient 0zk address
   pub recipient: String,
   pub asset: AssetId,
   pub erc20: Option<ERC20Token>,
   pub amount_wei: U256,
   pub amount: Option<NumericValue>,
   pub amount_usd: Option<NumericValue>,
}

/// Decoded unshield Railgun event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnshieldParams {
   pub chain: u64,
   pub recipient: Address,
   pub token_data: TokenData,
   pub erc20: Option<ERC20Token>,
   pub amount_wei: U256,
   pub amount: Option<NumericValue>,
   pub amount_usd: Option<NumericValue>,
   pub fee: Option<NumericValue>,
   pub fee_usd: Option<NumericValue>,
   pub is_self_broadcast: bool,
   pub broadcaster_fee: Option<NumericValue>,
   pub broadcaster_fee_usd: Option<NumericValue>,
}

impl UnshieldParams {
   pub async fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Self, anyhow::Error> {
      if let Ok(decoded) = <RailgunSmartWallet::Unshield as SolEvent>::decode_log(&log) {
         // TODO: Add support for ERC721 and ERC1155
         if decoded.token.tokenType == TokenType::ERC20 {
            let erc20 = ctx.get_token(chain, decoded.token.tokenAddress).await?;
            let amount = NumericValue::format_wei(decoded.amount, erc20.decimals);
            let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &erc20);
            let fee = NumericValue::format_wei(decoded.fee, erc20.decimals);
            let fee_usd = ctx.get_token_value_for_amount(fee.f64(), &erc20);

            return Ok(Self {
               chain,
               recipient: decoded.to,
               token_data: decoded.token.clone(),
               amount_wei: decoded.amount,
               erc20: Some(erc20),
               amount: Some(amount),
               amount_usd: Some(amount_usd),
               fee: Some(fee),
               fee_usd: Some(fee_usd),
               is_self_broadcast: false,
               broadcaster_fee: None,
               broadcaster_fee_usd: None,
            });
         }

         return Ok(Self {
            chain,
            recipient: decoded.to,
            token_data: decoded.token.clone(),
            amount_wei: decoded.amount,
            erc20: None,
            amount: None,
            amount_usd: None,
            fee: None,
            fee_usd: None,
            is_self_broadcast: false,
            broadcaster_fee: None,
            broadcaster_fee_usd: None,
         });
      } else {
         Err(anyhow!("Log decoding failed"))
      }
   }
}
