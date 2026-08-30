//! ABI for the Permit2 contract
//! https://github.com/Uniswap/permit2

use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, Bytes, LogData, Signature, U256,
   aliases::{U48, U160},
};
use alloy_sol_types::{SolCall, SolError, SolEvent, sol};

sol! {

    #[sol(rpc)]
    contract Permit2 {

      event Approval(address indexed owner, address indexed token, address indexed spender, uint160 amount, uint48 expiration);

      struct AllowanceTransferDetails {
        // the owner of the token
        address from;
        // the recipient of the token
        address to;
        // the amount of the token
        uint160 amount;
        // the token to be transferred
        address token;
    }

       /// @notice The permit data for a token
      #[derive(Debug, Default, PartialEq, Eq)]
      struct PermitDetails {
          // ERC20 token address
          address token;
          // the maximum amount allowed to spend
          uint160 amount;
          // timestamp at which a spender's token allowances become invalid
          uint48 expiration;
          // an incrementing value indexed per owner,token,and spender for each signature
          uint48 nonce;
      }

      /// @notice The permit message signed for a single token allowance
      #[derive(Debug, Default, PartialEq, Eq)]
      struct PermitSingle {
          // the permit data for a single token allowance
          PermitDetails details;
          // address permissioned on the allowed tokens
          address spender;
          // deadline on the permit signature
          uint256 sigDeadline;
      }

      /// @notice The permit message signed for multiple token allowances
      #[derive(Debug, Default, PartialEq, Eq)]
      struct PermitBatch {
          // the permit data for multiple token allowances
          PermitDetails[] details;
          // address permissioned on the allowed tokens
          address spender;
          // deadline on the permit signature
          uint256 sigDeadline;
      }

        #[derive(Debug)]
        function allowance(address user, address token, address spender)
        external
        view
        returns (uint160 amount, uint48 expiration, uint48 nonce);

        function permit(address owner, PermitSingle memory permitSingle, bytes calldata signature) external;

        function permit(address owner, PermitBatch memory permitBatch, bytes calldata signature) external;

        function approve(address token, address spender, uint160 amount, uint48 expiration) external;

        event Permit(address indexed owner, address indexed token, address indexed spender, uint160 amount, uint48 expiration, uint48 nonce);

        error InvalidSigner();
        error InvalidSignature();
        error InvalidNonce();
        error InvalidSignatureLength();
        error LengthMismatch();
        error SignatureExpired(uint256 signatureDeadline);
        error InvalidAmount(uint256 maxAmount);
        error AllowanceExpired(uint256 deadline);
        error InsufficientAllowance(uint256 amount);
    }

}

pub async fn allowance<P, N>(
   client: P,
   permit2: Address,
   owner: Address,
   token: Address,
   spender: Address,
) -> Result<Permit2::allowanceReturn, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let permit2 = Permit2::new(permit2, client);
   let allowance = permit2.allowance(owner, token, spender).call().await?;
   Ok(allowance)
}

/// Encode `Permit2.permit(owner, PermitSingle, signature)` calldata.
pub fn encode_permit_single_call(
   owner: Address,
   token: Address,
   amount: U256,
   expiration: U256,
   nonce: U48,
   spender: Address,
   sig_deadline: U256,
   signature: Signature,
) -> Bytes {
   let amount = U160::from(amount);
   let expiration = U48::from(expiration);

   let permit_details = Permit2::PermitDetails {
      token,
      amount,
      expiration,
      nonce,
   };

   let permit_single = Permit2::PermitSingle {
      details: permit_details,
      spender,
      sigDeadline: sig_deadline,
   };

   let sig_bytes = Bytes::from(signature.as_bytes());
   // Overload: (owner, PermitSingle, signature)
   let encoded = Permit2::permit_0Call {
      owner,
      permitSingle: permit_single,
      signature: sig_bytes,
   };
   encoded.abi_encode().into()
}

/// Decode Permit2 custom errors. `Error(string)` / Panic still go through `decode_revert_reason`.
pub fn decode_permit2_revert(data: &[u8]) -> Option<String> {
   if data.len() < 4 {
      return None;
   }

   if Permit2::InvalidSigner::abi_decode(data).is_ok() {
      return Some("InvalidSigner()".to_string());
   }
   if Permit2::InvalidSignature::abi_decode(data).is_ok() {
      return Some("InvalidSignature()".to_string());
   }
   if Permit2::InvalidNonce::abi_decode(data).is_ok() {
      return Some("InvalidNonce()".to_string());
   }
   if Permit2::InvalidSignatureLength::abi_decode(data).is_ok() {
      return Some("InvalidSignatureLength()".to_string());
   }
   if Permit2::LengthMismatch::abi_decode(data).is_ok() {
      return Some("LengthMismatch()".to_string());
   }
   if let Ok(err) = Permit2::SignatureExpired::abi_decode(data) {
      return Some(format!("SignatureExpired({})", err.signatureDeadline));
   }
   if let Ok(err) = Permit2::InvalidAmount::abi_decode(data) {
      return Some(format!("InvalidAmount({})", err.maxAmount));
   }
   if let Ok(err) = Permit2::AllowanceExpired::abi_decode(data) {
      return Some(format!("AllowanceExpired({})", err.deadline));
   }
   if let Ok(err) = Permit2::InsufficientAllowance::abi_decode(data) {
      return Some(format!("InsufficientAllowance({})", err.amount));
   }

   None
}

pub fn decode_permit_log(log: &LogData) -> Result<Permit2::Permit, anyhow::Error> {
   let decoded = Permit2::Permit::decode_raw_log(log.topics(), &log.data)?;
   Ok(decoded)
}

pub fn decode_approval_log(log: &LogData) -> Result<Permit2::Approval, anyhow::Error> {
   let decoded = Permit2::Approval::decode_raw_log(log.topics(), &log.data)?;
   Ok(decoded)
}
