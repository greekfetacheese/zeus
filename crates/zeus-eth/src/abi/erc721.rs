//! ABI for the ERC721 (NFT) interfaces
//!
//! Split the way the standard itself splits them: core [`IERC721`], [`IERC721Metadata`]
//! and [`IERC721Enumerable`]. Not every collection implements the last two, so callers must
//! gate on `abi::erc165` before relying on them (a contract that reverts on
//! `supportsInterface`, e.g. CryptoPunks, is not ERC-721 at all).
//!
//! Note: the ERC-721 `Transfer` event has the *same* topic0 as the ERC-20 `Transfer`
//! (`Transfer(address,address,uint256)`) — they are told apart by topic count, not by
//! signature (ERC-721 indexes `tokenId`, so 4 topics and no data; ERC-20 has 3 topics and
//! the value in data). A raw-log decode against the wrong interface fails on that mismatch.

use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, LogData, U256};
use alloy_rpc_types::BlockId;
use alloy_sol_types::{SolCall, SolEvent, sol};

sol! {
    #[sol(rpc)]
    contract IERC721 {
        event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
        event Approval(address indexed owner, address indexed approved, uint256 indexed tokenId);
        event ApprovalForAll(address indexed owner, address indexed operator, bool approved);

        function balanceOf(address owner) external view returns (uint256 balance);
        function ownerOf(uint256 tokenId) external view returns (address owner);
        function getApproved(uint256 tokenId) external view returns (address operator);
        function isApprovedForAll(address owner, address operator) external view returns (bool);

        function approve(address to, uint256 tokenId) external;
        function setApprovalForAll(address operator, bool approved) external;

        // Overloads: declaration order decides the generated suffix.
        // `safeTransferFrom_0Call` is the 3-arg form, `safeTransferFrom_1Call` the 4-arg one.
        function safeTransferFrom(address from, address to, uint256 tokenId) external;
        function safeTransferFrom(address from, address to, uint256 tokenId, bytes calldata data) external;
        function transferFrom(address from, address to, uint256 tokenId) external;
    }
}

sol! {
    #[sol(rpc)]
    contract IERC721Metadata {
        function name() external view returns (string memory);
        function symbol() external view returns (string memory);
        function tokenURI(uint256 tokenId) external view returns (string memory);
    }
}

sol! {
    #[sol(rpc)]
    contract IERC721Enumerable {
        function totalSupply() external view returns (uint256);
        function tokenOfOwnerByIndex(address owner, uint256 index) external view returns (uint256);
        function tokenByIndex(uint256 index) external view returns (uint256);
    }
}

// ** ABI Query Functions

pub async fn balance_of<P, N>(
   token: Address,
   owner: Address,
   client: P,
   block: Option<BlockId>,
) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = IERC721::new(token, client);
   let b = contract.balanceOf(owner).block(block).call().await?;
   Ok(b)
}

pub async fn owner_of<P, N>(
   token: Address,
   token_id: U256,
   client: P,
) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721::new(token, client);
   let o = contract.ownerOf(token_id).call().await?;
   Ok(o)
}

pub async fn get_approved<P, N>(
   token: Address,
   token_id: U256,
   client: P,
) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721::new(token, client);
   let a = contract.getApproved(token_id).call().await?;
   Ok(a)
}

pub async fn is_approved_for_all<P, N>(
   token: Address,
   owner: Address,
   operator: Address,
   client: P,
) -> Result<bool, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721::new(token, client);
   let a = contract.isApprovedForAll(owner, operator).call().await?;
   Ok(a)
}

pub async fn token_uri<P, N>(
   token: Address,
   token_id: U256,
   client: P,
) -> Result<String, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721Metadata::new(token, client);
   let u = contract.tokenURI(token_id).call().await?;
   Ok(u)
}

pub async fn collection_name<P, N>(token: Address, client: P) -> Result<String, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721Metadata::new(token, client);
   let n = contract.name().call().await?;
   Ok(n)
}

pub async fn collection_symbol<P, N>(token: Address, client: P) -> Result<String, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721Metadata::new(token, client);
   let s = contract.symbol().call().await?;
   Ok(s)
}

/// Only available when the collection advertises `IERC721Enumerable` via ERC-165.
pub async fn token_of_owner_by_index<P, N>(
   token: Address,
   owner: Address,
   index: U256,
   client: P,
) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721Enumerable::new(token, client);
   let id = contract.tokenOfOwnerByIndex(owner, index).call().await?;
   Ok(id)
}

/// Only available when the collection advertises `IERC721Enumerable` via ERC-165.
pub async fn total_supply<P, N>(token: Address, client: P) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC721Enumerable::new(token, client);
   let t = contract.totalSupply().call().await?;
   Ok(t)
}

// ** ABI Encode Functions

pub fn encode_balance_of(owner: Address) -> Bytes {
   let c = IERC721::balanceOfCall { owner };
   Bytes::from(c.abi_encode())
}

pub fn encode_owner_of(token_id: U256) -> Bytes {
   let c = IERC721::ownerOfCall { tokenId: token_id };
   Bytes::from(c.abi_encode())
}

pub fn encode_get_approved(token_id: U256) -> Bytes {
   let c = IERC721::getApprovedCall { tokenId: token_id };
   Bytes::from(c.abi_encode())
}

pub fn encode_is_approved_for_all(owner: Address, operator: Address) -> Bytes {
   let c = IERC721::isApprovedForAllCall { owner, operator };
   Bytes::from(c.abi_encode())
}

pub fn encode_approve(to: Address, token_id: U256) -> Bytes {
   let c = IERC721::approveCall {
      to,
      tokenId: token_id,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_set_approval_for_all(operator: Address, approved: bool) -> Bytes {
   let c = IERC721::setApprovalForAllCall { operator, approved };
   Bytes::from(c.abi_encode())
}

pub fn encode_safe_transfer_from(from: Address, to: Address, token_id: U256) -> Bytes {
   let c = IERC721::safeTransferFrom_0Call {
      from,
      to,
      tokenId: token_id,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_safe_transfer_from_with_data(
   from: Address,
   to: Address,
   token_id: U256,
   data: Bytes,
) -> Bytes {
   let c = IERC721::safeTransferFrom_1Call {
      from,
      to,
      tokenId: token_id,
      data,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_token_uri(token_id: U256) -> Bytes {
   let c = IERC721Metadata::tokenURICall { tokenId: token_id };
   Bytes::from(c.abi_encode())
}

pub fn encode_token_of_owner_by_index(owner: Address, index: U256) -> Bytes {
   let c = IERC721Enumerable::tokenOfOwnerByIndexCall { owner, index };
   Bytes::from(c.abi_encode())
}

// ** ABI Decode Functions

pub fn decode_transfer_log(log: &LogData) -> Result<IERC721::Transfer, anyhow::Error> {
   let b = IERC721::Transfer::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_approval_log(log: &LogData) -> Result<IERC721::Approval, anyhow::Error> {
   let b = IERC721::Approval::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_approval_for_all_log(
   log: &LogData,
) -> Result<IERC721::ApprovalForAll, anyhow::Error> {
   let b = IERC721::ApprovalForAll::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_owner_of(bytes: &Bytes) -> Result<Address, anyhow::Error> {
   let o = IERC721::ownerOfCall::abi_decode_returns(bytes)?;
   Ok(o)
}

/// `getApproved(tokenId)` returns the approved address, the zero address when there is none.
pub fn decode_get_approved(bytes: &Bytes) -> Result<Address, anyhow::Error> {
   let a = IERC721::getApprovedCall::abi_decode_returns(bytes)?;
   Ok(a)
}

pub fn decode_balance_of(bytes: &Bytes) -> Result<U256, anyhow::Error> {
   let b = IERC721::balanceOfCall::abi_decode_returns(bytes)?;
   Ok(b)
}

pub fn decode_is_approved_for_all(bytes: &Bytes) -> Result<bool, anyhow::Error> {
   let a = IERC721::isApprovedForAllCall::abi_decode_returns(bytes)?;
   Ok(a)
}

pub fn decode_token_uri(bytes: &Bytes) -> Result<String, anyhow::Error> {
   let u = IERC721Metadata::tokenURICall::abi_decode_returns(bytes)?;
   Ok(u)
}

/// Decode a 3-arg `safeTransferFrom` **calldata** payload into `(from, to, tokenId)`.
pub fn decode_safe_transfer_from_call(
   bytes: &Bytes,
) -> Result<(Address, Address, U256), anyhow::Error> {
   let c = IERC721::safeTransferFrom_0Call::abi_decode(bytes)?;
   Ok((c.from, c.to, c.tokenId))
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Guards the interface surface against the published ERC-721 selectors, and — importantly —
   /// proves the two `safeTransferFrom` overloads are not swapped (declaration order decides
   /// which one becomes `_0`).
   #[test]
   fn selectors_match_the_erc721_standard() {
      assert_eq!(
         IERC721::balanceOfCall::SELECTOR,
         [0x70, 0xa0, 0x82, 0x31]
      );
      assert_eq!(
         IERC721::ownerOfCall::SELECTOR,
         [0x63, 0x52, 0x21, 0x1e]
      );
      assert_eq!(
         IERC721::getApprovedCall::SELECTOR,
         [0x08, 0x18, 0x12, 0xfc]
      );
      assert_eq!(
         IERC721::isApprovedForAllCall::SELECTOR,
         [0xe9, 0x85, 0xe9, 0xc5]
      );
      assert_eq!(
         IERC721::approveCall::SELECTOR,
         [0x09, 0x5e, 0xa7, 0xb3]
      );
      assert_eq!(
         IERC721::setApprovalForAllCall::SELECTOR,
         [0xa2, 0x2c, 0xb4, 0x65]
      );
      assert_eq!(
         IERC721::transferFromCall::SELECTOR,
         [0x23, 0xb8, 0x72, 0xdd]
      );

      // 3-arg declared first => `_0`, 4-arg second => `_1`
      assert_eq!(
         IERC721::safeTransferFrom_0Call::SELECTOR,
         [0x42, 0x84, 0x2e, 0x0e]
      );
      assert_eq!(
         IERC721::safeTransferFrom_1Call::SELECTOR,
         [0xb8, 0x8d, 0x4f, 0xde]
      );

      assert_eq!(
         IERC721Metadata::tokenURICall::SELECTOR,
         [0xc8, 0x7b, 0x56, 0xdd]
      );
      assert_eq!(
         IERC721Enumerable::totalSupplyCall::SELECTOR,
         [0x18, 0x16, 0x0d, 0xdd]
      );
      assert_eq!(
         IERC721Enumerable::tokenOfOwnerByIndexCall::SELECTOR,
         [0x2f, 0x74, 0x5c, 0x59]
      );
   }

   /// ERC-721 `Transfer` and ERC-20 `Transfer` share a topic0; ERC-721 is distinguished by
   /// indexing `tokenId` (4 topics, empty data). This pins that assumption against the real
   /// ERC-20 ABI rather than a hardcoded hash.
   #[test]
   fn transfer_topic0_is_shared_with_erc20() {
      use crate::abi::erc20::IERC20;
      assert_eq!(
         IERC721::Transfer::SIGNATURE_HASH,
         IERC20::Transfer::SIGNATURE_HASH
      );
   }

   #[test]
   fn safe_transfer_from_roundtrips() {
      let from = Address::repeat_byte(0xaa);
      let to = Address::repeat_byte(0xbb);
      let token_id = U256::from(42);

      let data = encode_safe_transfer_from(from, to, token_id);
      assert_eq!(data.len(), 4 + 32 * 3);
      assert_eq!(
         decode_safe_transfer_from_call(&data).unwrap(),
         (from, to, token_id)
      );
   }

   /// The read the NFT send path checks ownership with, from both ends: the id is the only argument,
   /// and `ownerOf` answers a 32-byte word whose **low 20 bytes** are the address. Reading the high
   /// bytes instead would name a wrong owner and refuse a valid transfer.
   #[test]
   fn owner_of_encodes_the_id_and_decodes_the_low_twenty_bytes() {
      let call = encode_owner_of(U256::from(1));
      assert_eq!(&call[..4], &IERC721::ownerOfCall::SELECTOR);
      assert_eq!(call.len(), 4 + 32);
      assert_eq!(U256::from_be_slice(&call[4..36]), U256::from(1));

      let mut word = [0u8; 32];
      word[12..].copy_from_slice(&[0xab; 20]);

      assert_eq!(
         decode_owner_of(&Bytes::from(word.to_vec())).unwrap(),
         Address::from([0xab; 20])
      );
   }
}
