use crate::utils::TimeStamp;
use zeus_eth::{alloy_primitives::Address, currency::ERC20Token, utils::NumericValue};

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum ClearSource {
   Registry { path: String },
}

#[derive(Debug, Clone)]
pub struct DisplayField {
   pub label: String,
   pub value: FormattedValue,
}

#[derive(Debug, Clone)]
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
