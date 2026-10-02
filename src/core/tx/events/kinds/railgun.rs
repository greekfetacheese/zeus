use crate::core::ZeusCtx;
use alloy_sol_types::SolEvent;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   alloy_primitives::{Address, Log, U256},
   currency::ERC20Token,
   nft::NftToken,
   utils::NumericValue,
};
use zeus_railgun::abi::railgun::TokenType;
use zeus_railgun::{
   abi::railgun::{RailgunSmartWallet, TokenData},
   caip::AssetId,
};

fn default_fee_token() -> Option<ERC20Token> {
   Some(ERC20Token::wrapped_native_token(1))
}

/// Decoded Railgun shield event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldParams {
   pub chain: u64,
   pub recipient: Option<String>,
   pub asset: AssetId,
   pub amount_wei: U256,
   pub erc20: Option<ERC20Token>,
   /// The token itself, when the shielded asset is one. `None` for ERC-20s, and also for an ERC-721
   /// whose metadata could not be read — the `asset` still carries the collection and the token id.
   #[serde(default)]
   pub nft: Option<NftToken>,
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
            let mut nft = None;
            let mut amount_fmt_opt = None;
            let mut amount_usd_opt = None;
            let mut fee_fmt_opt = None;
            let mut fee_usd_opt = None;

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

            // ERC-721. An ERC-1155 is a *quantity* of an id, so it needs its own amount handling and a
            // different note model — still out of scope (D6).
            if let AssetId::Erc721(collection, token_id) = &asset {
               // Shield fees are taken in the shielded asset, and an indivisible token cannot pay one:
               // a non-zero fee here would mean the event was misread, not that a fee was charged.
               if !fee_wei.is_zero() {
                  tracing::warn!("Non-zero fee {} on an ERC-721 shield", fee_wei);
               }

               // A failed lookup costs the name and the art, never the event: the asset already carries
               // the collection and the token id, so the UI can always name it.
               match ctx.get_nft(chain, *collection, *token_id).await {
                  Ok(token) => nft = Some(token),
                  Err(e) => tracing::warn!(
                     "Could not resolve shielded NFT {} #{}: {}",
                     collection,
                     token_id,
                     e
                  ),
               }
            }

            let event = ShieldParams {
               chain,
               recipient: None,
               asset,
               amount_wei,
               erc20,
               nft,
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
   /// The token itself for an ERC-721 unshield, resolved on the shield side's terms. `None` for an
   /// ERC-20, and for an ERC-721 whose metadata could not be read — `token_data` still names it.
   #[serde(default)]
   pub nft: Option<NftToken>,
   pub amount_wei: U256,
   pub amount: Option<NumericValue>,
   pub amount_usd: Option<NumericValue>,
   pub fee: Option<NumericValue>,
   pub fee_usd: Option<NumericValue>,
   pub is_self_broadcast: bool,
   #[serde(default = "default_fee_token")]
   pub fee_token: Option<ERC20Token>,
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
               nft: None,
               amount: Some(amount),
               amount_usd: Some(amount_usd),
               fee: Some(fee),
               fee_usd: Some(fee_usd),
               is_self_broadcast: false,
               fee_token: None,
               broadcaster_fee: None,
               broadcaster_fee_usd: None,
            });
         }

         // ERC-721. ERC-1155 stays out of scope (D6) and falls through to the raw params below.
         if decoded.token.tokenType == TokenType::ERC721 {
            let asset: AssetId = decoded.token.clone().into();
            let nft = match asset {
               AssetId::Erc721(collection, token_id) => {
                  match ctx.get_nft(chain, collection, token_id).await {
                     Ok(token) => Some(token),
                     Err(e) => {
                        tracing::warn!(
                           "Could not resolve unshielded NFT {} #{}: {}",
                           collection,
                           token_id,
                           e
                        );
                        None
                     }
                  }
               }
               _ => None,
            };

            return Ok(Self {
               chain,
               recipient: decoded.to,
               token_data: decoded.token.clone(),
               amount_wei: decoded.amount,
               erc20: None,
               nft,
               amount: None,
               amount_usd: None,
               // Unshield fees are charged in the asset's own units and an indivisible token cannot
               // pay one, so the protocol takes none.
               fee: None,
               fee_usd: None,
               is_self_broadcast: false,
               fee_token: None,
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
            nft: None,
            amount: None,
            amount_usd: None,
            fee: None,
            fee_usd: None,
            is_self_broadcast: false,
            fee_token: None,
            broadcaster_fee: None,
            broadcaster_fee_usd: None,
         });
      } else {
         Err(anyhow!("Log decoding failed"))
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::{ZeusCtx, context::client::Rpc, tx::events::DecodedEvent};
   use zeus_eth::{
      alloy_primitives::{B256, Uint, address},
      nft::NftStandard,
   };
   use zeus_railgun::abi::railgun::{CommitmentPreimage, RailgunSmartWallet, ShieldCiphertext};

   const BAYC: Address = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
   const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
   const RAILGUN: Address = address!("FA7093CDD9EE6932B4eb2c9e1cde7CE00B1FA4b9");

   /// A `Shield` log carrying one commitment. `fees` is parallel to `commitments` in the real event, so
   /// both are single-element.
   fn shield_log(token: TokenData, value: u64) -> Log {
      Log {
         address: RAILGUN,
         data: RailgunSmartWallet::Shield {
            treeNumber: U256::ZERO,
            startPosition: U256::ZERO,
            commitments: vec![CommitmentPreimage {
               npk: B256::ZERO,
               token,
               value: Uint::<120, 2>::from(value),
            }],
            shieldCiphertext: vec![ShieldCiphertext {
               encryptedBundle: [B256::ZERO; 3],
               shieldKey: B256::ZERO,
            }],
            fees: vec![U256::ZERO],
         }
         .encode_log_data(),
      }
   }

   fn erc721_token_data() -> TokenData {
      TokenData {
         tokenType: TokenType::ERC721,
         tokenAddress: BAYC,
         tokenSubID: U256::from(1),
      }
   }

   /// These params ride in the sealed tx history, so an old payload — written before `nft` existed — has
   /// to keep loading. `#[serde(default)]` is the whole reason it does.
   #[test]
   fn a_stored_shield_without_an_nft_field_still_loads() {
      let event = DecodedEvent::dummy_shield();
      let params = event.shield_params();

      let mut stored = serde_json::to_value(params).unwrap();
      stored.as_object_mut().unwrap().remove("nft");
      assert!(
         stored.get("nft").is_none(),
         "the fixture must look old"
      );

      let restored: ShieldParams = serde_json::from_value(stored).unwrap();

      assert!(restored.nft.is_none());
      assert_eq!(restored.chain, params.chain);
      assert_eq!(restored.asset, params.asset);
      assert_eq!(restored.amount_wei, params.amount_wei);
   }

   #[test]
   fn an_unshield_payload_without_an_nft_field_still_loads() {
      let event = DecodedEvent::dummy_unshield();
      let params = event.unshield_params();

      let mut stored = serde_json::to_value(params).unwrap();
      stored.as_object_mut().unwrap().remove("nft");

      let restored: UnshieldParams = serde_json::from_value(stored).unwrap();

      assert!(restored.nft.is_none());
      assert_eq!(restored.token_data.tokenType, TokenType::ERC20);
   }

   /// The NFT the params carry has to survive the round trip too — an `NftToken` is a collection, an id
   /// and a standard, and losing any of them costs the name or the art.
   #[test]
   fn an_nft_shield_round_trips() {
      let nft = NftToken {
         chain_id: 1,
         collection: BAYC,
         token_id: U256::from(1),
         standard: NftStandard::Erc721,
         metadata_uri: Some("ipfs://QmeSjSinHpPnmXmspMjwiXyN6zS4E9zccariGR3jxcaWtq/1".to_string()),
      };

      let mut params = DecodedEvent::dummy_shield().shield_params().clone();
      params.asset = AssetId::Erc721(BAYC, U256::from(1));
      params.erc20 = None;
      params.nft = Some(nft.clone());
      params.amount = None;
      params.amount_usd = None;
      params.fee = None;
      params.fee_usd = None;

      let restored: ShieldParams =
         serde_json::from_str(&serde_json::to_string(&params).unwrap()).unwrap();

      assert_eq!(restored.nft.unwrap(), nft);
      assert_eq!(
         restored.asset,
         AssetId::Erc721(BAYC, U256::from(1))
      );
   }

   /// A context whose mainnet RPC is the keyed endpoint in `ZEUS_ETH_RPC`, marked measured the way the
   /// app does at startup: builtin endpoints are seeded **disabled**, and `get_best_rpc` only hands out
   /// enabled, working ones.
   fn ctx_with_keyed_mainnet_rpc() -> ZeusCtx {
      let url = std::env::var("ZEUS_ETH_RPC")
         .expect("ZEUS_ETH_RPC must point at an RPC that serves eth_call");

      let ctx = ZeusCtx::new();
      let mut rpc = Rpc::builder(url, 1).build();
      rpc.enabled = true;
      rpc.check.working = true;
      rpc.check.fully_functional = true;
      ctx.get_zeus_client().add_rpc(1, rpc);

      ctx
   }

   /// The ERC-721 arm against real state: a shield of BAYC #1 has to resolve into a token with the right
   /// collection, id and standard, and must **not** be mistaken for a fungible asset.
   ///
   /// It also moves the process working directory: the context caches what it resolves into `data/`,
   /// which is cwd-relative, and a test must not write over the real one. Run it alone.
   ///
   /// Deliberately not a `#[tokio::test]`: the working directory has to be moved around the runtime, and
   /// a runtime cannot be started from inside another one.
   #[test]
   #[ignore = "needs ZEUS_ETH_RPC; moves the process working directory, run alone"]
   fn a_shield_of_an_erc721_resolves_its_token_and_one_of_an_erc20_is_unchanged() {
      let previous = std::env::current_dir().unwrap();
      let dir = tempfile::tempdir().unwrap();
      std::env::set_current_dir(dir.path()).unwrap();

      let result = std::panic::catch_unwind(|| {
         tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
               let ctx = ctx_with_keyed_mainnet_rpc();

               // ERC-721.
               let logs = ShieldParams::from_log(
                  ctx.clone(),
                  1,
                  &shield_log(erc721_token_data(), 1),
               )
               .await
               .unwrap();

               assert_eq!(logs.len(), 1);
               let params = &logs[0];
               assert_eq!(params.asset, AssetId::Erc721(BAYC, U256::from(1)));
               assert!(params.erc20.is_none(), "an NFT is not an ERC-20");

               let nft = params.nft.as_ref().expect("BAYC #1 must resolve");
               assert_eq!(nft.collection, BAYC);
               assert_eq!(nft.token_id, U256::from(1));
               assert_eq!(nft.standard, NftStandard::Erc721);
               assert!(
                  nft.metadata_uri.is_some(),
                  "BAYC publishes a tokenURI"
               );

               // An indivisible token has no decimals to format against, so nothing is formatted.
               assert!(params.amount.is_none() && params.fee.is_none());

               // ERC-20, on the same code path, must be untouched by the new arm.
               let logs = ShieldParams::from_log(
                  ctx,
                  1,
                  &shield_log(
                     TokenData {
                        tokenType: TokenType::ERC20,
                        tokenAddress: WETH,
                        tokenSubID: U256::ZERO,
                     },
                     1_000_000_000_000_000_000,
                  ),
               )
               .await
               .unwrap();

               let params = &logs[0];
               assert!(params.nft.is_none(), "a WETH shield has no token");
               let erc20 = params.erc20.as_ref().expect("WETH must resolve");
               assert_eq!(&*erc20.symbol, "WETH");
               assert_eq!(erc20.decimals, 18);
               assert_eq!(params.amount.as_ref().unwrap().f64(), 1.0);
            });
      });

      std::env::set_current_dir(previous).unwrap();
      assert!(result.is_ok(), "the shield decode failed");
   }
}
