use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, Bytes, LogData, Signature, U256,
   aliases::{U48, U160},
};
use alloy_sol_types::{SolCall, SolEvent, SolValue, sol};

sol! {

    #[sol(rpc)]
    contract Permit2 {

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

        function permit(address owner, PermitBatch memory permitBatch, bytes calldata signature) external;

        event Permit(address indexed owner, address indexed token, address indexed spender, uint160 amount, uint48 expiration, uint48 nonce);
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

pub fn encode_permit_batch_ur_input(permit_batch: Permit2::PermitBatch, signature: Signature) -> Bytes {
   (permit_batch, Bytes::from(signature.as_bytes()))
      .abi_encode_params()
      .into()
}

pub fn encode_permit2_permit_ur_input(
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
   let encoded_args = (permit_single, sig_bytes).abi_encode_params();

   encoded_args.into()
}

pub fn encode_permit_batch_call(owner: Address, permit_batch: Permit2::PermitBatch, signature: Signature) -> Bytes {
   let sig_bytes = Bytes::from(signature.as_bytes());
   let encoded = Permit2::permitCall {
      owner,
      permitBatch: permit_batch,
      signature: sig_bytes,
   };
   encoded.abi_encode().into()
}

pub fn encode_permit2_permit_single(
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
   let encoded_args = (permit_single, sig_bytes).abi_encode();

   encoded_args.into()
}

pub fn decode_permit_log(log: &LogData) -> Result<Permit2::Permit, anyhow::Error> {
   let decoded = Permit2::Permit::decode_raw_log(log.topics(), &log.data)?;
   Ok(decoded)
}
