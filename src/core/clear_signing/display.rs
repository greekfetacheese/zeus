use crate::utils::TimeStamp;
use serde::{Deserialize, Serialize};
use zeus_eth::{alloy_primitives::Address, currency::ERC20Token, utils::NumericValue};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearDisplay {
   /// Short heading shown as the sign-window title.
   pub heading: String,
   pub intent: Intent,
   pub interpolated_intent: Option<String>,
   pub owner: Option<String>,
   pub contract_name: Option<String>,
   pub info_url: Option<String>,
   pub fields: Vec<DisplayField>,
   pub source: ClearSource,
   pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Intent {
   Text(String),
   Pairs(Vec<(String, String)>),
}

impl Intent {
   pub fn heading(&self) -> String {
      match self {
         Self::Text(s) => s.clone(),
         Self::Pairs(pairs) => {
            pairs.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(" · ")
         }
      }
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClearSource {
   Registry { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayField {
   pub label: String,
   pub value: FormattedValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FormattedValue {
   Text(String),
   Address(Address),
   TokenAmount {
      amount: NumericValue,
      token: ERC20Token,
      unlimited: bool,
   },
   Date(TimeStamp),
   Bytes(String),
}

impl FormattedValue {
   pub fn as_text(&self) -> String {
      match self {
         Self::Text(s) => s.clone(),
         Self::Address(addr) => addr.to_string(),
         Self::TokenAmount {
            amount,
            token,
            unlimited,
         } => {
            if *unlimited {
               format!("Unlimited {}", token.symbol)
            } else {
               format!("{} {}", amount.abbreviated(), token.symbol)
            }
         }
         Self::Date(ts) => ts.to_relative(),
         Self::Bytes(s) => s.clone(),
      }
   }
}

impl ClearDisplay {
   /// Offline fixture for the confirm-window calldata UI (dev panel).
   pub fn dummy_calldata() -> Self {
      use std::str::FromStr;
      use zeus_eth::alloy_primitives::U256;

      let usdc = Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap();
      let recipient = Address::from_str("0x2222222222222222222222222222222222222222").unwrap();
      let token = ERC20Token {
         chain_id: 8453,
         address: usdc,
         symbol: "USDC".into(),
         name: "USD Coin".into(),
         decimals: 6,
         total_supply: U256::ZERO,
      };
      Self {
         heading: "Supply".to_string(),
         intent: Intent::Text("Supply".to_string()),
         interpolated_intent: Some("Supply 1 USDC".to_string()),
         owner: Some("Aave DAO".to_string()),
         contract_name: Some("PoolInstance".to_string()),
         info_url: Some("https://aave.com".to_string()),
         fields: vec![
            DisplayField {
               label: "Amount to supply".to_string(),
               value: FormattedValue::TokenAmount {
                  amount: NumericValue::format_wei(U256::from(1_000_000u64), 6),
                  token,
                  unlimited: false,
               },
            },
            DisplayField {
               label: "Collateral recipient".to_string(),
               value: FormattedValue::Address(recipient),
            },
            DisplayField {
               label: "Deadline".to_string(),
               value: FormattedValue::Date(TimeStamp::Seconds(1_745_151_870)),
            },
         ],
         source: ClearSource::Registry {
            path: "dummy".to_string(),
         },
         warnings: Vec::new(),
      }
   }
}
