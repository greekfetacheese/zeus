//! ERC-165 interface.

use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, FixedBytes, fixed_bytes};
use alloy_sol_types::sol;

sol! {
    #[sol(rpc)]
    contract IERC165 {
        function supportsInterface(bytes4 interfaceId) external view returns (bool);
    }
}

/// ERC-165 identifier (`supportsInterface` itself).
pub const IERC165_ID: FixedBytes<4> = fixed_bytes!("01ffc9a7");
/// ERC-721 interface id.
pub const IERC721_ID: FixedBytes<4> = fixed_bytes!("80ac58cd");
/// ERC-1155 interface id.
pub const IERC1155_ID: FixedBytes<4> = fixed_bytes!("d9b67a26");
/// Must return false on a spec-compliant ERC-165 contract.
pub const INVALID_INTERFACE_ID: FixedBytes<4> = fixed_bytes!("ffffffff");

async fn supports_interface<P, N>(client: P, token: Address, interface_id: FixedBytes<4>) -> bool
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC165::new(token, client);
   match contract.supportsInterface(interface_id).call().await {
      Ok(supported) => supported,
      Err(_) => false,
   }
}

/// Returns `true` if `token` advertises ERC-721 or ERC-1155 via ERC-165.
///
/// Contracts that do not implement ERC-165 (typical ERC-20s, EOAs) return `false`.
pub async fn is_erc721_or_erc1155<P, N>(client: P, token: Address) -> bool
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let erc165 = supports_interface(client.clone(), token, IERC165_ID).await;
   let invalid = supports_interface(client.clone(), token, INVALID_INTERFACE_ID).await;
   let erc721 = supports_interface(client.clone(), token, IERC721_ID).await;
   let erc1155 = supports_interface(client, token, IERC1155_ID).await;

   erc165 && !invalid && (erc721 || erc1155)
}
