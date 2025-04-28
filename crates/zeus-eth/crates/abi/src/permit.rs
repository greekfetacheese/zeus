use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, Bytes, Signature, U256,
   aliases::{U48, U160},
};
use alloy_sol_types::{SolValue, sol};

sol! {

    #[sol(rpc)]
    contract Permit2 {

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

        function allowance(address user, address token, address spender)
        external
        view
        returns (uint160 amount, uint48 expiration, uint48 nonce);
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