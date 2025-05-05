use V3SpokePoolInterface::{V3SpokePoolInterfaceCalls, depositV3Call};
use alloy_primitives::{Address, Bytes, LogData, U256};
use alloy_sol_types::{SolCall, SolEvent, SolInterface, sol};

sol! {
    #[sol(rpc)]
    interface V3SpokePoolInterface {

      #[derive(Debug)]
      enum FillType {
         FastFill,
         ReplacedSlowFill,
         SlowFill
     }

      #[derive(Debug)]
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

/// Emitted on the origin chain which the deposit was made on.
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

/// Emitted on the destination chain when a deposit is filled.
#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct V3RelayExecutionEventInfo {
   pub updated_recipient: Bytes,
   pub updated_message_hash: Bytes,
   pub updated_output_amount: U256,
   pub fill_type: V3SpokePoolInterface::FillType,
}

pub fn funds_deposited_signature() -> &'static str {
   V3SpokePoolInterface::FundsDeposited::SIGNATURE
}

pub fn filled_relay_signature() -> &'static str {
   V3SpokePoolInterface::FilledRelay::SIGNATURE
}

pub fn deposit_v3_signature() -> &'static str {
   V3SpokePoolInterface::depositV3Call::SIGNATURE
}

pub fn deposit_v3_selector() -> [u8; 4] {
   V3SpokePoolInterface::depositV3Call::SELECTOR
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

pub fn decode_deposit_v3_call(data: &Bytes) -> Result<depositV3Call, anyhow::Error> {
   let args = depositV3Call::abi_decode(data)?;
   Ok(args)
}

pub fn decode_funds_deposited_log(log: &LogData) -> Result<FundsDeposited, anyhow::Error> {
   let decoded = V3SpokePoolInterface::FundsDeposited::decode_raw_log(log.topics(), &log.data)?;

   let input_token = Address::from_slice(&decoded.inputToken[12..]);
   let output_token = Address::from_slice(&decoded.outputToken[12..]);
   let depositor = Address::from_slice(&decoded.depositor[12..]);
   let recipient = Address::from_slice(&decoded.recipient[12..]);
   let exclusive_relayer = Address::from_slice(&decoded.exclusiveRelayer[12..]);

   Ok(FundsDeposited {
      input_token,
      output_token,
      input_amount: decoded.inputAmount,
      output_amount: decoded.outputAmount,
      destination_chain_id: decoded.destinationChainId,
      deposit_id: decoded.depositId,
      quote_timestamp: decoded.quoteTimestamp,
      fill_deadline: decoded.fillDeadline,
      exclusivity_deadline: decoded.exclusivityDeadline,
      depositor,
      recipient,
      exclusive_relayer,
      message: decoded.message,
   })
}

pub fn decode_filled_relay_log(log: &LogData) -> Result<FilledRelay, anyhow::Error> {
   let decoded = V3SpokePoolInterface::FilledRelay::decode_raw_log(log.topics(), &log.data)?;

   let input_token = Address::from_slice(&decoded.inputToken[12..]);
   let output_token = Address::from_slice(&decoded.outputToken[12..]);
   let depositor = Address::from_slice(&decoded.depositor[12..]);
   let recipient = Address::from_slice(&decoded.recipient[12..]);
   let exclusive_relayer = Address::from_slice(&decoded.exclusiveRelayer[12..]);
   let relayer = Address::from_slice(&decoded.relayer[12..]);

   let relay_execution_info = V3RelayExecutionEventInfo {
      updated_recipient: decoded.relayExecutionInfo.updatedRecipient.into(),
      updated_message_hash: decoded.relayExecutionInfo.updatedMessageHash.into(),
      updated_output_amount: decoded.relayExecutionInfo.updatedOutputAmount,
      fill_type: decoded.relayExecutionInfo.fillType,
   };

   Ok(FilledRelay {
      input_token,
      output_token,
      input_amount: decoded.inputAmount,
      output_amount: decoded.outputAmount,
      repayment_chain_id: decoded.repaymentChainId,
      origin_chain_id: decoded.originChainId,
      deposit_id: decoded.depositId,
      fill_deadline: decoded.fillDeadline,
      exclusivity_deadline: decoded.exclusivityDeadline,
      exclusive_relayer,
      relayer,
      depositor,
      recipient,
      message_hash: decoded.messageHash.into(),
      relay_execution_info,
   })
}
