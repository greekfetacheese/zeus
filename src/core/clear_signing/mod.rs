pub mod descriptor;
pub mod display;
pub mod format;
pub mod path;
pub mod registry;
pub mod registry_pins;
pub mod sourcify;

pub use display::{ClearDisplay, ClearSource, DisplayField, FormattedValue, Intent};
pub use format::FormatData;
pub use path::Container;

use crate::core::ZeusCtx;
use zeus_eth::{
   alloy_dyn_abi::TypedData,
   alloy_primitives::{Address, Bytes, U256, keccak256},
   currency::ERC20Token,
};

/// Try to clear-sign an EIP-712 payload. Returns `None` on any miss/failure
/// so the caller can fall back to pretty JSON.
pub async fn try_clear_sign_typed_data(
   ctx: ZeusCtx,
   _chain: u64,
   typed: &TypedData,
) -> Option<ClearDisplay> {
   let verifying = typed.domain.verifying_contract?;
   let chain_id = typed.domain.chain_id.and_then(|id| u64::try_from(id).ok())?;
   let encode_type = typed.encode_type().ok()?;
   let type_hash = keccak256(encode_type.as_bytes());

   let mut resolved = registry::resolve_eip712_descriptor(chain_id, verifying, type_hash).await;

   if resolved.is_none() {
      if let Some(impl_addr) = sourcify::implementation_address(chain_id, verifying).await {
         if impl_addr != verifying {
            resolved = registry::resolve_eip712_descriptor(chain_id, impl_addr, type_hash).await;
         }
      }
   }

   let (path, descriptor) = resolved?;
   let spec = descriptor::bind_eip712(&descriptor, typed).ok()?;

   let signer = ctx.get_current_wallet().address();
   let container = Container {
      from: Some(signer),
      to: Some(verifying),
      chain_id,
      value: U256::ZERO,
   };

   let mut data = FormatData::default();
   let tokens = format::collect_token_addresses(
      spec,
      &typed.message,
      &descriptor.metadata,
      &container,
   );
   for (token_chain, addr) in tokens {
      if let Some(token) = resolve_token(ctx.clone(), token_chain, addr).await {
         data.tokens.insert((token_chain, addr), token);
      }
      if let Some(name) = ctx.get_address_name(token_chain, addr) {
         data.names.insert((token_chain, addr), name.to_string());
      }
   }

   Some(format::format_eip712(
      &descriptor,
      spec,
      &typed.message,
      &container,
      &data,
      path,
   ))
}

/// Try to clear-sign a contract call. Returns `None` on any miss/failure
/// so the caller can fall back to hex calldata.
pub async fn try_clear_sign_calldata(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   to: Address,
   value: U256,
   calldata: &Bytes,
) -> Option<ClearDisplay> {
   if calldata.len() < 4 {
      return None;
   }
   let mut selector = [0u8; 4];
   selector.copy_from_slice(&calldata[..4]);

   let mut resolved = registry::resolve_calldata_descriptor(chain, to).await;
   if resolved.is_none() {
      if let Some(impl_addr) = sourcify::implementation_address(chain, to).await {
         if impl_addr != to {
            resolved = registry::resolve_calldata_descriptor(chain, impl_addr).await;
         }
      }
   }

   let (path, descriptor) = resolved?;
   let mut bound = descriptor::bind_calldata(&descriptor, chain, to, selector);
   if bound.is_err() {
      if let Some(dep) = descriptor
         .context
         .contract
         .as_ref()
         .and_then(|c| c.deployments.iter().find(|d| d.chain_id == chain).map(|d| d.address))
      {
         bound = descriptor::bind_calldata(&descriptor, chain, dep, selector);
      }
   }
   let (_key, spec) = bound.ok()?;

   let parsed = descriptor::parse_call_format(_key).ok()?;
   let args = format::decode_call_args(&parsed, &calldata[4..]).ok()?;

   let container = Container {
      from: Some(from),
      to: Some(to),
      chain_id: chain,
      value,
   };

   let mut data = FormatData::default();
   let tokens = format::collect_token_addresses(spec, &args, &descriptor.metadata, &container);
   for (token_chain, addr) in tokens {
      if let Some(token) = resolve_token(ctx.clone(), token_chain, addr).await {
         data.tokens.insert((token_chain, addr), token);
      }
      if let Some(name) = ctx.get_address_name(token_chain, addr) {
         data.names.insert((token_chain, addr), name.to_string());
      }
   }
   if let Some(name) = ctx.get_address_name(chain, to) {
      data.names.insert((chain, to), name.to_string());
   }
   if let Some(name) = ctx.get_address_name(chain, from) {
      data.names.insert((chain, from), name.to_string());
   }

   Some(format::format_eip712(
      &descriptor,
      spec,
      &args,
      &container,
      &data,
      path,
   ))
}

async fn resolve_token(ctx: ZeusCtx, chain: u64, address: Address) -> Option<ERC20Token> {
   let token_res = ctx.get_token(chain, address).await;

   match token_res {
      Ok(token) => Some(token),
      Err(e) => {
         tracing::error!("Failed to resolve token: {:?}", e);
         None
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::signature::parse_typed_data;
   use serde_json::json;
   use std::str::FromStr;
   use zeus_eth::alloy_primitives::Address;

   fn permit2_json() -> serde_json::Value {
      json!({
          "types": {
              "PermitSingle": [
                  {"name": "details", "type": "PermitDetails"},
                  {"name": "spender", "type": "address"},
                  {"name": "sigDeadline", "type": "uint256"}
              ],
              "PermitDetails": [
                  {"name": "token", "type": "address"},
                  {"name": "amount", "type": "uint160"},
                  {"name": "expiration", "type": "uint48"},
                  {"name": "nonce", "type": "uint48"}
              ],
              "EIP712Domain": [
                  {"name": "name", "type": "string"},
                  {"name": "chainId", "type": "uint256"},
                  {"name": "verifyingContract", "type": "address"}
              ]
          },
          "domain": {
              "name": "Permit2",
              "chainId": "8453",
              "verifyingContract": "0x000000000022d473030f116ddee9f6b43ac78ba3"
          },
          "primaryType": "PermitSingle",
          "message": {
              "details": {
                  "token": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
                  "amount": "1000000",
                  "expiration": "1747742070",
                  "nonce": "0"
              },
              "spender": "0x6ff5693b99212da76ad316178a184ab56d299b43",
              "sigDeadline": "1745151870"
          }
      })
   }

   fn permit2_descriptor() -> serde_json::Value {
      json!({
          "$schema": "https://eips.ethereum.org/assets/eip-7730/erc7730-v2.schema.json",
          "context": {
              "eip712": {
                  "domain": { "name": "Permit2" },
                  "deployments": [
                      { "chainId": 8453, "address": "0x000000000022D473030F116dDEE9F6B43aC78BA3" }
                  ]
              }
          },
          "metadata": {
              "owner": "Uniswap Labs",
              "info": { "url": "https://uniswap.org/" }
          },
          "display": {
              "formats": {
                  "PermitSingle(PermitDetails details,address spender,uint256 sigDeadline)PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)": {
                      "intent": "Authorize spending of token",
                      "interpolatedIntent": "Approve {details.amount} for {spender}",
                      "fields": [
                          { "path": "spender", "label": "Spender", "format": "addressName" },
                          {
                              "path": "details.amount",
                              "label": "Amount",
                              "format": "tokenAmount",
                              "params": {
                                  "tokenPath": "details.token",
                                  "threshold": "0xffffffffffffffffffffffffffffffffffffffff",
                                  "message": "Unlimited"
                              }
                          },
                          {
                              "path": "details.expiration",
                              "label": "Approval expires",
                              "format": "date",
                              "params": { "encoding": "timestamp" }
                          },
                          { "path": "sigDeadline", "label": "Sig Deadline", "visible": "never" }
                      ]
                  }
              }
          }
      })
   }

   fn erc2612_json() -> serde_json::Value {
      json!({
          "types": {
              "Permit": [
                  {"name": "owner", "type": "address"},
                  {"name": "spender", "type": "address"},
                  {"name": "value", "type": "uint256"},
                  {"name": "nonce", "type": "uint256"},
                  {"name": "deadline", "type": "uint256"}
              ],
              "EIP712Domain": [
                  {"name": "name", "type": "string"},
                  {"name": "version", "type": "string"},
                  {"name": "chainId", "type": "uint256"},
                  {"name": "verifyingContract", "type": "address"}
              ]
          },
          "domain": {
              "name": "USD Coin",
              "version": "2",
              "chainId": "8453",
              "verifyingContract": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"
          },
          "primaryType": "Permit",
          "message": {
              "owner": "0x1111111111111111111111111111111111111111",
              "spender": "0x2222222222222222222222222222222222222222",
              "value": "1000000",
              "nonce": "0",
              "deadline": "1745151870"
          }
      })
   }

   fn erc2612_descriptor() -> serde_json::Value {
      json!({
          "$schema": "https://eips.ethereum.org/assets/eip-7730/erc7730-v2.schema.json",
          "context": {
              "eip712": {
                  "domain": { "name": "USD Coin" },
                  "deployments": [
                      { "chainId": 8453, "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913" }
                  ]
              }
          },
          "metadata": { "owner": "Circle", "contractName": "USDC" },
          "display": {
              "formats": {
                  "Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)": {
                      "intent": "Permit",
                      "interpolatedIntent": "Approve {value} for {spender}",
                      "fields": [
                          { "path": "spender", "label": "Spender", "format": "addressName" },
                          {
                              "path": "value",
                              "label": "Amount",
                              "format": "tokenAmount",
                              "params": { "tokenPath": "@.to" }
                          },
                          {
                              "path": "deadline",
                              "label": "Deadline",
                              "format": "date",
                              "params": { "encoding": "timestamp" }
                          }
                      ]
                  }
              }
          }
      })
   }

   #[test]
   fn bind_permit2_happy_path() {
      let typed = parse_typed_data(permit2_json()).unwrap();
      let desc = descriptor::parse_descriptor(&permit2_descriptor()).unwrap();
      let spec = descriptor::bind_eip712(&desc, &typed).unwrap();
      assert_eq!(
         spec.intent.heading(),
         "Authorize spending of token"
      );
   }

   #[test]
   fn bind_rejects_wrong_verifying_contract() {
      let mut json = permit2_json();
      json["domain"]["verifyingContract"] = json!("0x1111111111111111111111111111111111111111");
      let typed = parse_typed_data(json).unwrap();
      let desc = descriptor::parse_descriptor(&permit2_descriptor()).unwrap();
      assert!(descriptor::bind_eip712(&desc, &typed).is_err());
   }

   #[test]
   fn bind_rejects_wrong_encode_type() {
      let typed = parse_typed_data(permit2_json()).unwrap();
      let mut desc_json = permit2_descriptor();
      let formats = desc_json["display"]["formats"].as_object_mut().unwrap();
      let spec = formats.remove(
         "PermitSingle(PermitDetails details,address spender,uint256 sigDeadline)PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)",
      ).unwrap();
      formats.insert("Mail(string contents)".to_string(), spec);
      let desc = descriptor::parse_descriptor(&desc_json).unwrap();
      assert!(descriptor::bind_eip712(&desc, &typed).is_err());
   }

   #[test]
   fn extra_domain_keys_still_bind() {
      let mut json = permit2_json();
      json["domain"]["version"] = json!("1");
      let typed = parse_typed_data(json).unwrap();
      let desc = descriptor::parse_descriptor(&permit2_descriptor()).unwrap();
      assert!(descriptor::bind_eip712(&desc, &typed).is_ok());
   }

   #[test]
   fn rejects_schema_v1() {
      let mut json = permit2_descriptor();
      json["$schema"] = json!("https://eips.ethereum.org/assets/eip-7730/erc7730-v1.schema.json");
      assert!(descriptor::parse_descriptor(&json).is_err());
   }

   #[test]
   fn format_erc2612_permit() {
      let typed = parse_typed_data(erc2612_json()).unwrap();
      let desc = descriptor::parse_descriptor(&erc2612_descriptor()).unwrap();
      let spec = descriptor::bind_eip712(&desc, &typed).unwrap();

      let usdc = Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap();
      let spender = Address::from_str("0x2222222222222222222222222222222222222222").unwrap();
      let token = ERC20Token {
         chain_id: 8453,
         address: usdc,
         symbol: "USDC".into(),
         name: "USD Coin".into(),
         decimals: 6,
         total_supply: Default::default(),
      };

      let mut data = FormatData::default();
      data.tokens.insert((8453, usdc), token);
      data.names.insert((8453, spender), "Router".to_string());

      let container = Container {
         from: None,
         to: Some(usdc),
         chain_id: 8453,
         value: U256::ZERO,
      };
      let display = format::format_eip712(
         &desc,
         spec,
         &typed.message,
         &container,
         &data,
         "registry/permit/eip712-permit-base-usdc.json".to_string(),
      );

      assert_eq!(display.heading, "Permit");
      assert_eq!(display.owner.as_deref(), Some("Circle"));
      assert_eq!(display.fields.len(), 3);
      assert_eq!(display.fields[0].label, "Spender");
      assert_eq!(display.fields[0].value.as_text(), "Router");
      match &display.fields[1].value {
         FormattedValue::TokenAmount {
            token, unlimited, ..
         } => {
            assert_eq!(token.symbol.as_ref(), "USDC");
            assert!(!*unlimited);
         }
         other => panic!("expected token amount, got {other:?}"),
      }
      assert_eq!(
         display.interpolated_intent.as_deref(),
         Some("Approve 1.00 USDC for Router")
      );
   }

   #[test]
   fn index_lookup_matches_lowercase_caip10() {
      let verifying = Address::from_str("0x000000000022d473030f116ddee9f6b43ac78ba3").unwrap();
      let typed = parse_typed_data(permit2_json()).unwrap();
      let type_hash = keccak256(typed.encode_type().unwrap().as_bytes());
      let index = json!({
          "eip155:8453:0x000000000022d473030f116ddee9f6b43ac78ba3": {
              "PermitSingle": [{
                  "path": "registry/uniswap/eip712-uniswap-permit2.json",
                  "encodeTypeHashes": [format!("{type_hash:#x}")]
              }]
          }
      });
      let path = registry::index_lookup_for_tests(&index, 8453, verifying, type_hash).unwrap();
      assert_eq!(
         path,
         "registry/uniswap/eip712-uniswap-permit2.json"
      );
   }

   #[test]
   fn include_path_resolves_relative() {
      let path = registry::resolve_include_for_tests(
         "registry/uniswap/eip712-uniswap-permit2.json",
         "common-eip712-uniswap.json",
      );
      assert_eq!(
         path,
         "registry/uniswap/common-eip712-uniswap.json"
      );
   }

   #[test]
   fn include_rejects_absolute_url_and_escape() {
      use super::registry_pins::is_allowed_registry_path;
      let url = registry::resolve_include_for_tests(
         "registry/uniswap/eip712-uniswap-permit2.json",
         "https://evil.example/x.json",
      );
      assert!(!is_allowed_registry_path(&url));
      let escaped = registry::resolve_include_for_tests(
         "registry/uniswap/eip712-uniswap-permit2.json",
         "../../../../etc/passwd.json",
      );
      assert!(!is_allowed_registry_path(&escaped));
      let ercs = registry::resolve_include_for_tests(
         "registry/permit/eip712-permit-base-usdc.json",
         "../../ercs/eip712-erc2612-permit.json",
      );
      assert_eq!(ercs, "ercs/eip712-erc2612-permit.json");
      assert!(is_allowed_registry_path(&ercs));
   }

   #[test]
   fn sourcify_parses_implementation() {
      let v = json!({
          "proxyResolution": {
              "implementation": { "address": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913" }
          }
      });
      let addr = sourcify::parse_implementation(&v).unwrap();
      assert_eq!(
         addr,
         Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap()
      );
   }

   #[test]
   fn merge_includes_keeps_context_from_included() {
      let included = json!({
          "context": { "eip712": { "domain": { "name": "Permit2" } } },
          "metadata": { "owner": "Uniswap Labs" }
      });
      let including = json!({
          "includes": "common.json",
          "display": { "formats": { "X": { "intent": "Hi", "fields": [] } } }
      });
      let merged = descriptor::merge_descriptor_json(included, including);
      assert_eq!(merged["metadata"]["owner"], "Uniswap Labs");
      assert_eq!(
         merged["context"]["eip712"]["domain"]["name"],
         "Permit2"
      );
      assert_eq!(merged["display"]["formats"]["X"]["intent"], "Hi");
   }

   fn aave_supply_descriptor() -> serde_json::Value {
      json!({
          "$schema": "https://eips.ethereum.org/assets/eip-7730/erc7730-v2.schema.json",
          "context": {
              "contract": {
                  "deployments": [
                      { "chainId": 8453, "address": "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5" }
                  ]
              }
          },
          "metadata": { "owner": "Aave DAO", "contractName": "PoolInstance" },
          "display": {
              "formats": {
                  "supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode)": {
                      "intent": "Supply",
                      "fields": [
                          {
                              "path": "amount",
                              "format": "tokenAmount",
                              "label": "Amount to supply",
                              "params": { "tokenPath": "asset" }
                          },
                          { "path": "onBehalfOf", "format": "addressName", "label": "Collateral recipient" },
                          { "path": "referralCode", "label": "Referral Code", "visible": "never" }
                      ]
                  },
                  "multicall(bytes[] data)": {
                      "intent": "Multicall",
                      "fields": [
                          { "path": "data.[]", "format": "calldata", "label": "Call", "params": { "calleePath": "@.to" } }
                      ]
                  }
              }
          }
      })
   }

   #[test]
   fn supply_selector_matches_aave() {
      let sel = descriptor::selector_from_format_key(
         "supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode)",
      )
      .unwrap();
      // keccak256("supply(address,uint256,address,uint16)")[:4] = 0x617ba037
      assert_eq!(sel, [0x61, 0x7b, 0xa0, 0x37]);
   }

   #[test]
   fn bind_calldata_rejects_wrong_to() {
      let desc = descriptor::parse_descriptor(&aave_supply_descriptor()).unwrap();
      let pool = Address::from_str("0x1111111111111111111111111111111111111111").unwrap();
      let sel = [0x61, 0x7b, 0xa0, 0x37];
      assert!(descriptor::bind_calldata(&desc, 8453, pool, sel).is_err());
   }

   #[test]
   fn format_aave_supply() {
      let desc = descriptor::parse_descriptor(&aave_supply_descriptor()).unwrap();
      let pool = Address::from_str("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5").unwrap();
      let sel = descriptor::selector_from_format_key(
         "supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode)",
      )
      .unwrap();
      let (key, spec) = descriptor::bind_calldata(&desc, 8453, pool, sel).unwrap();
      let parsed = descriptor::parse_call_format(key).unwrap();

      let usdc = Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap();
      let recipient = Address::from_str("0x2222222222222222222222222222222222222222").unwrap();
      let mut encoded = Vec::new();
      encoded.extend_from_slice(&[0u8; 12]);
      encoded.extend_from_slice(usdc.as_slice());
      let amount = U256::from(1_000_000u64);
      encoded.extend_from_slice(&amount.to_be_bytes::<32>());
      encoded.extend_from_slice(&[0u8; 12]);
      encoded.extend_from_slice(recipient.as_slice());
      encoded.extend_from_slice(&U256::from(0u64).to_be_bytes::<32>());

      let args = format::decode_call_args(&parsed, &encoded).unwrap();
      let token = ERC20Token {
         chain_id: 8453,
         address: usdc,
         symbol: "USDC".into(),
         name: "USD Coin".into(),
         decimals: 6,
         total_supply: Default::default(),
      };
      let mut data = FormatData::default();
      data.tokens.insert((8453, usdc), token);
      data.names.insert((8453, recipient), "Me".to_string());
      let container = Container {
         from: None,
         to: Some(pool),
         chain_id: 8453,
         value: U256::ZERO,
      };
      let display = format::format_eip712(
         &desc,
         spec,
         &args,
         &container,
         &data,
         "registry/aave/calldata-lpv3.json".to_string(),
      );
      assert_eq!(display.heading, "Supply");
      assert_eq!(display.owner.as_deref(), Some("Aave DAO"));
      assert_eq!(display.fields.len(), 2);
      match &display.fields[0].value {
         FormattedValue::TokenAmount {
            token, unlimited, ..
         } => {
            assert_eq!(token.symbol.as_ref(), "USDC");
            assert!(!*unlimited);
         }
         other => panic!("expected token amount, got {other:?}"),
      }
      assert_eq!(display.fields[1].value.as_text(), "Me");
   }

   #[test]
   fn nested_calldata_is_skipped() {
      let desc = descriptor::parse_descriptor(&aave_supply_descriptor()).unwrap();
      let pool = Address::from_str("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5").unwrap();
      let sel = descriptor::selector_from_format_key("multicall(bytes[] data)").unwrap();
      let (_key, spec) = descriptor::bind_calldata(&desc, 8453, pool, sel).unwrap();
      let args = json!({ "data": ["0x"] });
      let container = Container {
         from: None,
         to: Some(pool),
         chain_id: 8453,
         value: U256::ZERO,
      };
      let display = format::format_eip712(
         &desc,
         spec,
         &args,
         &container,
         &FormatData::default(),
         "x".to_string(),
      );
      assert!(display.fields.is_empty());
      assert!(!display.warnings.is_empty());
   }

   #[test]
   fn calldata_index_lookup() {
      let pool = Address::from_str("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5").unwrap();
      let index = json!({
          "eip155:8453:0xa238dd80c259a72e81d7e4664a9801593f98d1c5": "registry/aave/calldata-lpv3.json"
      });
      let path = registry::calldata_index_lookup_for_tests(&index, 8453, pool).unwrap();
      assert_eq!(path, "registry/aave/calldata-lpv3.json");
   }
}
