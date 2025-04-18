use alloy_primitives::{Address, U256, address};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use anyhow::bail;
use types::ChainId;

#[derive(Debug, Default, Clone)]
pub struct ClientResponse {
   /// The Origin Chain used for the request
   pub origin_chain: u64,
   /// The Destination Chain used for the request
   pub destination_chain: u64,
   /// The input token used for the request
   pub input_token: Address,
   /// The output token used for the request
   pub output_token: Address,
   /// The amount used for the request
   pub amount: U256,
   /// The suggested fees for the request
   pub suggested_fees: SuggestedFeesResponse,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FeeDetail {
   pub pct: String,   // Percentage as a string (e.g., "78930919924823")
   pub total: String, // Total fee in wei as a string (e.g., "78930919924823")
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Limits {
   #[serde(rename = "minDeposit")]
   pub min_deposit: String,
   #[serde(rename = "maxDeposit")]
   pub max_deposit: String,
   #[serde(rename = "maxDepositInstant")]
   pub max_deposit_instant: String,
   #[serde(rename = "maxDepositShortDelay")]
   pub max_deposit_short_delay: String,
   #[serde(rename = "recommendedDepositInstant")]
   pub recommended_deposit_instant: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SuggestedFeesResponse {
   #[serde(rename = "estimatedFillTimeSec")]
   pub estimated_fill_time_sec: u32,
   #[serde(rename = "capitalFeePct")]
   pub capital_fee_pct: String,
   #[serde(rename = "capitalFeeTotal")]
   pub capital_fee_total: String,
   #[serde(rename = "relayGasFeePct")]
   pub relay_gas_fee_pct: String,
   #[serde(rename = "relayGasFeeTotal")]
   pub relay_gas_fee_total: String,
   #[serde(rename = "relayFeePct")]
   pub relay_fee_pct: String,
   #[serde(rename = "relayFeeTotal")]
   pub relay_fee_total: String,
   #[serde(rename = "lpFeePct")]
   pub lp_fee_pct: String,
   pub timestamp: String,
   #[serde(rename = "isAmountTooLow")]
   pub is_amount_too_low: bool,
   #[serde(rename = "quoteBlock")]
   pub quote_block: String,
   #[serde(rename = "exclusiveRelayer")]
   pub exclusive_relayer: Address,
   #[serde(rename = "exclusivityDeadline")]
   pub exclusivity_deadline: u32,
   #[serde(rename = "spokePoolAddress")]
   pub spoke_pool_address: Address,
   #[serde(rename = "destinationSpokePoolAddress")]
   pub destination_spoke_pool_address: Address,
   #[serde(rename = "totalRelayFee")]
   pub total_relay_fee: FeeDetail,
   #[serde(rename = "relayerCapitalFee")]
   pub relayer_capital_fee: FeeDetail,
   #[serde(rename = "relayerGasFee")]
   pub relayer_gas_fee: FeeDetail,
   #[serde(rename = "lpFee")]
   pub lp_fee: FeeDetail,
   pub limits: Limits,
   #[serde(rename = "fillDeadline")]
   pub fill_deadline: String,
}

pub async fn get_suggested_fees(
   input_token: Address,
   output_token: Address,
   origin_chain_id: u64,
   destination_chain_id: u64,
   amount: U256,
) -> Result<ClientResponse, anyhow::Error> {
   let client = Client::new();
   let url = "https://app.across.to/api/suggested-fees";

   let params = [
      ("inputToken", input_token.to_string()),
      ("outputToken", output_token.to_string()),
      ("originChainId", origin_chain_id.to_string()),
      ("destinationChainId", destination_chain_id.to_string()),
      ("amount", amount.to_string()),
   ];

   let raw_response = client.get(url).query(&params).send().await?.text().await?;

   // println!("Raw JSON response: {}", raw_response);

   let response = serde_json::from_str::<SuggestedFeesResponse>(&raw_response)?;

   let res = ClientResponse {
      origin_chain: origin_chain_id,
      destination_chain: destination_chain_id,
      input_token,
      output_token,
      amount,
      suggested_fees: response,
   };

   Ok(res)
}

/// Does Across support the specified chain?
pub fn supports_chain(chain_id: u64) -> Result<bool, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum(_) => Ok(true),
      ChainId::Optimism(_) => Ok(true),
      ChainId::Base(_) => Ok(true),
      ChainId::Arbitrum(_) => Ok(true),
      ChainId::BinanceSmartChain(_) => Ok(false),
   }
}

/// Return the address of the SpokePool contract on the specified chain
pub fn spoke_pool_address(chain_id: u64) -> Result<Address, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum(_) => Ok(address!("5c7BCd6E7De5423a257D81B442095A1a6ced35C5")),
      ChainId::Optimism(_) => Ok(address!("6f26Bf09B1C792e3228e5467807a900A503c0281")),
      ChainId::Base(_) => Ok(address!("09aea4b2242abC8bb4BB78D537A67a245A7bEC64")),
      ChainId::Arbitrum(_) => Ok(address!("e35e9842fceaca96570b734083f4a58e8f7c5f2a")),
      ChainId::BinanceSmartChain(_) => bail!("Across Protocol does not support BSC"),
   }
}
