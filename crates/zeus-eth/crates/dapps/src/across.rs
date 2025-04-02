use alloy_primitives::{Address, Bytes, U256, address};
use alloy_rpc_types::Log;
use alloy_sol_types::{SolEvent, SolInterface, sol};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use V3SpokePoolInterface::{V3SpokePoolInterfaceCalls, depositV3Call};
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

sol! {
    #[sol(rpc)]
    interface V3SpokePoolInterface {

      enum FillType {
         FastFill,
         ReplacedSlowFill,
         SlowFill
     }

      struct V3RelayExecutionEventInfo {
         bytes32 updatedRecipient;
         bytes32 updatedMessageHash;
         uint256 updatedOutputAmount;
         FillType fillType;
     }

        function depositV3(
            address depositor,
            address recipient,
            address inputToken,
            address outputToken,
            uint256 inputAmount,
            uint256 outputAmount,
            uint256 destinationChainId,
            address exclusiveRelayer,
            uint32 quoteTimestamp,
            uint32 fillDeadline,
            uint32 exclusivityDeadline,
            bytes calldata message
        ) external payable;

        event FundsDeposited(
         bytes32 inputToken,
         bytes32 outputToken,
         uint256 inputAmount,
         uint256 outputAmount,
         uint256 indexed destinationChainId,
         uint256 indexed depositId,
         uint32 quoteTimestamp,
         uint32 fillDeadline,
         uint32 exclusivityDeadline,
         bytes32 indexed depositor,
         bytes32 recipient,
         bytes32 exclusiveRelayer,
         bytes message
     );

     event FilledRelay(
      bytes32 inputToken,
      bytes32 outputToken,
      uint256 inputAmount,
      uint256 outputAmount,
      uint256 repaymentChainId,
      uint256 indexed originChainId,
      uint256 indexed depositId,
      uint32 fillDeadline,
      uint32 exclusivityDeadline,
      bytes32 exclusiveRelayer,
      bytes32 indexed relayer,
      bytes32 depositor,
      bytes32 recipient,
      bytes32 messageHash,
      V3RelayExecutionEventInfo relayExecutionInfo
  );

    }

    #[sol(rpc)]
    contract SpokePool {
        function depositQuoteTimeBuffer() external view returns (uint32);
        function getCurrentTime() external view returns (uint256);
    }

}

#[derive(Debug, Clone)]
pub struct DepositV3Args {
   /// The address of the account which will deposit the tokens
   pub depositor: Address,

   /// The account receiving funds on the destination chain.
   ///
   ///  Can be an EOA or a contract.
   ///
   ///  If the output token is the wrapped native token for the chain, then the recipient will receive native token
   ///
   ///  If an EOA or wrapped native token if a contract.
   pub recipient: Address,

   /// The token pulled from the caller's account and locked into this contract to initiate the deposit.
   ///
   ///  The equivalent of this token on the relayer's repayment chain of choice will be sent as a refund.
   ///
   ///  If this is equal to the wrapped native token, the caller can optionally pass in native token as msg.value, provided msg.value = inputTokenAmount.
   pub input_token: Address,

   /// The token that the relayer will send to the recipient on the destination chain. Must be an ERC20.
   pub output_token: Address,

   /// The amount of input tokens pulled from the caller's account and locked into this contract.
   ///
   ///  This amount will be sent to the relayer as a refund following an optimistic challenge window in the HubPool, less a system fee.
   pub input_amount: U256,

   /// The amount of output tokens that the relayer will send to the recipient on the destination.
   pub output_amount: U256,

   /// The destination chain identifier.
   ///
   ///  Must be enabled along with the input token as a valid deposit route from this spoke pool or this transaction will revert.
   pub destination_chain_id: u64,

   /// The relayer exclusively allowed to fill this deposit before the exclusivity deadline.
   ///
   /// You probably want to set this to zero address
   pub exclusive_relayer: Address,

   /// The HubPool timestamp that determines the system fee paid by the depositor.
   ///
   ///  This must be set between [currentTime - depositQuoteTimeBuffer, currentTime] where currentTime is block.timestamp on this chain.
   pub quote_timestamp: u32,

   /// The deadline for the relayer to fill the deposit.
   ///
   ///  After this destination chain timestamp, the fill will revert on the destination chain.
   ///
   ///  Must be set before currentTime + fillDeadlineBuffer, where currentTime is block.timestamp on this chain.
   pub fill_deadline: u32,

   /// This value is used to set the exclusivity deadline timestamp in the emitted deposit event.
   ///
   ///  Before this destination chain timestamp, only the exclusiveRelayer (if set to a non-zero address), can fill this deposit.
   ///
   ///  There are three ways to use this parameter:
   ///
   ///  1. NO EXCLUSIVITY: If this value is set to 0, then a timestamp of 0 will be emitted, meaning that there is no exclusivity period.
   ///
   ///  2. OFFSET: If this value is less than MAX_EXCLUSIVITY_PERIOD_SECONDS, then add this value to the block.timestamp to derive the exclusive relayer deadline.
   ///
   ///  Note that using the parameter in this way will expose the filler of the deposit to the risk that the block.timestamp of this event gets changed due to a chain-reorg, which would also change the exclusivity timestamp.
   ///
   ///  3. TIMESTAMP: Otherwise, set this value as the exclusivity deadline timestamp. which is the deadline for the exclusiveRelayer to fill the deposit.
   pub exclusivity_deadline: u32,

   /// The message to send to the recipient on the destination chain if the recipient is a contract.
   ///
   ///  If the message is not empty, the recipient contract must implement `handleV3AcrossMessage()` or the fill will revert.
   pub message: Bytes,
}

#[derive(Debug, Clone)]
pub struct FundsDeposited {
   pub input_token: Address,
   pub output_token: Address,
   pub input_amount: U256,
   pub output_amount: U256,
   pub destination_chain_id: U256,
   pub deposit_id: U256,
   pub quote_timestamp: u32,
   pub fill_deadline: u32,
   pub exclusivity_deadline: u32,
   pub depositor: Address,
   pub recipient: Address,
   pub exclusive_relayer: Address,
   pub message: Bytes,
}

#[derive(Clone)]
pub struct FilledRelay {
   pub input_token: Address,
   pub output_token: Address,
   pub input_amount: U256,
   pub output_amount: U256,
   pub repayment_chain_id: U256,
   pub origin_chain_id: U256,
   pub deposit_id: U256,
   pub fill_deadline: u32,
   pub exclusivity_deadline: u32,
   pub exclusive_relayer: Address,
   pub relayer: Address,
   pub depositor: Address,
   pub recipient: Address,
   pub message_hash: Bytes,
   pub relay_execution_info: V3RelayExecutionEventInfo,
}

#[derive(Clone)]
pub struct V3RelayExecutionEventInfo {
   pub updated_recipient: Bytes,
   pub updated_message_hash: Bytes,
   pub updated_output_amount: U256,
   pub fill_type: V3SpokePoolInterface::FillType,
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

   // Attempt to deserialize
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

pub fn funds_deposited_signature() -> &'static str {
   V3SpokePoolInterface::FundsDeposited::SIGNATURE
}

pub fn filled_relay_signature() -> &'static str {
   V3SpokePoolInterface::FilledRelay::SIGNATURE
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
      ChainId::BinanceSmartChain(_) => bail!("SpokePool not supported on BSC"),
   }
}

pub fn encode_deposit_v3(args: DepositV3Args) -> Bytes {
   let c = V3SpokePoolInterfaceCalls::depositV3(depositV3Call {
      depositor: args.depositor,
      recipient: args.recipient,
      inputToken: args.input_token,
      outputToken: args.output_token,
      inputAmount: args.input_amount,
      outputAmount: args.output_amount,
      destinationChainId: U256::from(args.destination_chain_id),
      exclusiveRelayer: args.exclusive_relayer,
      quoteTimestamp: args.quote_timestamp,
      fillDeadline: args.fill_deadline,
      exclusivityDeadline: args.exclusivity_deadline,
      message: args.message,
   });
   Bytes::from(c.abi_encode())
}

pub fn decode_funds_deposited(log: &Log) -> Result<FundsDeposited, anyhow::Error> {
   let V3SpokePoolInterface::FundsDeposited {
      inputToken,
      outputToken,
      inputAmount,
      outputAmount,
      destinationChainId,
      depositId,
      quoteTimestamp,
      fillDeadline,
      exclusivityDeadline,
      depositor,
      recipient,
      exclusiveRelayer,
      message,
   } = log.log_decode()?.inner.data;

   let input_token = Address::from_slice(&inputToken[12..]);
   let output_token = Address::from_slice(&outputToken[12..]);
   let depositor = Address::from_slice(&depositor[12..]);
   let recipient = Address::from_slice(&recipient[12..]);
   let exclusive_relayer = Address::from_slice(&exclusiveRelayer[12..]);

   Ok(FundsDeposited {
      input_token,
      output_token,
      input_amount: inputAmount,
      output_amount: outputAmount,
      destination_chain_id: destinationChainId,
      deposit_id: depositId,
      quote_timestamp: quoteTimestamp,
      fill_deadline: fillDeadline,
      exclusivity_deadline: exclusivityDeadline,
      depositor,
      recipient,
      exclusive_relayer,
      message,
   })
}

pub fn decode_filled_relay(log: &Log) -> Result<FilledRelay, anyhow::Error> {
   let V3SpokePoolInterface::FilledRelay {
      inputToken,
      outputToken,
      inputAmount,
      outputAmount,
      repaymentChainId,
      originChainId,
      depositId,
      fillDeadline,
      exclusivityDeadline,
      exclusiveRelayer,
      relayer,
      depositor,
      recipient,
      messageHash,
      relayExecutionInfo,
   } = log.log_decode()?.inner.data;

   let input_token = Address::from_slice(&inputToken[12..]);
   let output_token = Address::from_slice(&outputToken[12..]);
   let depositor = Address::from_slice(&depositor[12..]);
   let recipient = Address::from_slice(&recipient[12..]);
   let exclusive_relayer = Address::from_slice(&exclusiveRelayer[12..]);
   let relayer = Address::from_slice(&relayer[12..]);

   let relay_execution_info = V3RelayExecutionEventInfo {
      updated_recipient: relayExecutionInfo.updatedRecipient.into(),
      updated_message_hash: relayExecutionInfo.updatedMessageHash.into(),
      updated_output_amount: relayExecutionInfo.updatedOutputAmount,
      fill_type: relayExecutionInfo.fillType,
   };

   Ok(FilledRelay {
      input_token,
      output_token,
      input_amount: inputAmount,
      output_amount: outputAmount,
      repayment_chain_id: repaymentChainId,
      origin_chain_id: originChainId,
      deposit_id: depositId,
      fill_deadline: fillDeadline,
      exclusivity_deadline: exclusivityDeadline,
      exclusive_relayer,
      relayer,
      depositor,
      recipient,
      message_hash: messageHash.into(),
      relay_execution_info,
   })
}