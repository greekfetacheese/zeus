//! ABI for the ERC1155 (multi-token) interfaces
//!
//! [`IERC1155`] is the core interface (balances, approvals, both transfer flavours and the
//! `TransferSingle` / `TransferBatch` / `ApprovalForAll` / `URI` events).
//! [`IERC1155Metadata`] holds `uri(uint256)`, which is a *separate* interface id
//! (`0x0e89341c`) and must be probed via `abi::erc165` — not every collection implements it.
//!
//! Two optional extensions live here too, because each *extends* ERC-1155 rather than
//! replacing it:
//!
//! - [`IERC5216`] — the allowance extension: per-`id` approvals by amount, the ERC-1155
//!   answer to ERC-20 `approve`. Its `Approval(address,address,uint256,uint256)` event has
//!   its **own** topic0, so unlike `ApprovalForAll` it is unambiguous from the log alone.
//! - [`IERC1155Permit`] — ERC-7604, the `permit` extension (draft, not live as of 2026-10).
//!   It emits the ERC-5216 `Approval` event, so a permit transaction is already fully
//!   observable through [`IERC5216`] — what the interface adds is only the ability to
//!   *create* an approval from a signature, which is a sending path.
//!
//! Three encoding traps worth knowing before decoding logs from an unknown address:
//!
//! 1. `ApprovalForAll(address,address,bool)` is **byte-for-byte identical** to ERC-721's
//!    `ApprovalForAll` — same topic0, same 2 indexed + `bool` layout. The log alone cannot
//!    tell you which standard emitted it; only the emitting contract's ERC-165 can.
//! 2. `URI(string,uint256)` indexes the id and puts the string in data, so the id is a topic
//!    while the URI needs full ABI string decoding.
//! 3. The ERC-5216 `Approval` does **not** index its `id`: the third topic slot that ERC-721
//!    `Approval(owner,approved,tokenId)` uses for the id is absent here, so these two are
//!    told apart by topic count (3 vs 4) *and* by topic0.

use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, LogData, U256};
use alloy_rpc_types::BlockId;
use alloy_sol_types::{SolCall, SolEvent, sol};

sol! {
    #[sol(rpc)]
    contract IERC1155 {
        event TransferSingle(
            address indexed operator,
            address indexed from,
            address indexed to,
            uint256 id,
            uint256 value
        );
        event TransferBatch(
            address indexed operator,
            address indexed from,
            address indexed to,
            uint256[] ids,
            uint256[] values
        );
        event ApprovalForAll(address indexed account, address indexed operator, bool approved);
        event URI(string value, uint256 indexed id);

        function balanceOf(address account, uint256 id) external view returns (uint256);
        function balanceOfBatch(
            address[] calldata accounts,
            uint256[] calldata ids
        ) external view returns (uint256[] memory);
        function setApprovalForAll(address operator, bool approved) external;
        function isApprovedForAll(address account, address operator) external view returns (bool);
        function safeTransferFrom(
            address from,
            address to,
            uint256 id,
            uint256 amount,
            bytes calldata data
        ) external;
        function safeBatchTransferFrom(
            address from,
            address to,
            uint256[] calldata ids,
            uint256[] calldata amounts,
            bytes calldata data
        ) external;
    }
}

sol! {
    #[sol(rpc)]
    contract IERC1155Metadata {
        function uri(uint256 id) external view returns (string memory);
    }
}

// ERC-5216, the ERC-1155 allowance extension.
//
// Declared standalone rather than `is IERC1155` (the `sol!` macro has no inheritance), which
// is all a decoder needs: the members below are the ones ERC-1155 does not already have.
//
// The interface id is `0x1be07d74` — the XOR of `approve(address,uint256,uint256)` and
// `allowance(address,address,uint256)`, which a test derives rather than trusting this note.
//
// (`//` rather than `///`: `sol!` expands to items, so a doc comment here attaches to nothing
// and the compiler warns.)
sol! {
    #[sol(rpc)]
    contract IERC5216 {
        // `id` is deliberately **not** indexed, unlike ERC-721's per-token `Approval`.
        event Approval(address indexed account, address indexed operator, uint256 id, uint256 amount);

        function approve(address operator, uint256 id, uint256 amount) external;
        function allowance(
            address account,
            address operator,
            uint256 id
        ) external view returns (uint256);
    }
}

// ERC-7604, the ERC-1155 `permit` extension (**draft** — not live as of 2026-10).
//
// Kept as a placeholder so the day it ships the shape is already here: the permit itself
// emits the ERC-5216 `Approval` event, so reading approvals needs nothing from this
// interface. Only *creating* an approval from a signature would call `permit`.
//
// `nonces` is keyed by `(owner, tokenId)` — per token id, not per owner, which is where this
// parts company with both ERC-2612 and ERC-4494.
sol! {
    #[sol(rpc)]
    contract IERC1155Permit {
        function permit(
            address owner,
            address operator,
            uint256 tokenId,
            uint256 value,
            uint256 deadline,
            bytes calldata sig
        ) external;
        function nonces(address owner, uint256 tokenId) external view returns (uint256);
        function DOMAIN_SEPARATOR() external view returns (bytes32);
    }
}

// ** ABI Query Functions

pub async fn balance_of<P, N>(
   token: Address,
   account: Address,
   id: U256,
   client: P,
   block: Option<BlockId>,
) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = IERC1155::new(token, client);
   let b = contract.balanceOf(account, id).block(block).call().await?;
   Ok(b)
}

pub async fn balance_of_batch<P, N>(
   token: Address,
   accounts: Vec<Address>,
   ids: Vec<U256>,
   client: P,
   block: Option<BlockId>,
) -> Result<Vec<U256>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = IERC1155::new(token, client);
   let b = contract.balanceOfBatch(accounts, ids).block(block).call().await?;
   Ok(b)
}

pub async fn is_approved_for_all<P, N>(
   token: Address,
   account: Address,
   operator: Address,
   client: P,
) -> Result<bool, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC1155::new(token, client);
   let a = contract.isApprovedForAll(account, operator).call().await?;
   Ok(a)
}

/// Only available on contracts advertising `IERC1155Metadata` (`0x0e89341c`) via ERC-165.
///
/// The returned string may contain the `{id}` placeholder, which the caller must substitute
/// with the id as lowercase hex padded to 64 characters (per the EIP-1155 metadata rules).
pub async fn uri<P, N>(token: Address, id: U256, client: P) -> Result<String, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC1155Metadata::new(token, client);
   let u = contract.uri(id).call().await?;
   Ok(u)
}

/// ERC-5216 `allowance(address,address,uint256)` — how many units of `id` `operator` may move
/// for `account`.
///
/// A contract that does not implement ERC-5216 reverts or returns garbage here; gate on
/// [`crate::abi::erc165::Erc165Support::is_erc5216`] first.
pub async fn allowance<P, N>(
   token: Address,
   account: Address,
   operator: Address,
   id: U256,
   client: P,
) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC5216::new(token, client);
   let a = contract.allowance(account, operator, id).call().await?;
   Ok(a)
}

/// ERC-7604 `nonces(address,uint256)` — the signed-permit counter for one token id.
pub async fn nonces<P, N>(
   token: Address,
   owner: Address,
   token_id: U256,
   client: P,
) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC1155Permit::new(token, client);
   let n = contract.nonces(owner, token_id).call().await?;
   Ok(n)
}

/// ERC-7604 `DOMAIN_SEPARATOR()`.
pub async fn domain_separator<P, N>(
   token: Address,
   client: P,
) -> Result<alloy_primitives::B256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC1155Permit::new(token, client);
   let d = contract.DOMAIN_SEPARATOR().call().await?;
   Ok(d)
}

// ** ABI Encode Functions

pub fn encode_balance_of(account: Address, id: U256) -> Bytes {
   let c = IERC1155::balanceOfCall { account, id };
   Bytes::from(c.abi_encode())
}

pub fn encode_balance_of_batch(accounts: Vec<Address>, ids: Vec<U256>) -> Bytes {
   let c = IERC1155::balanceOfBatchCall { accounts, ids };
   Bytes::from(c.abi_encode())
}

pub fn encode_is_approved_for_all(account: Address, operator: Address) -> Bytes {
   let c = IERC1155::isApprovedForAllCall { account, operator };
   Bytes::from(c.abi_encode())
}

pub fn encode_set_approval_for_all(operator: Address, approved: bool) -> Bytes {
   let c = IERC1155::setApprovalForAllCall { operator, approved };
   Bytes::from(c.abi_encode())
}

pub fn encode_safe_transfer_from(
   from: Address,
   to: Address,
   id: U256,
   amount: U256,
   data: Bytes,
) -> Bytes {
   let c = IERC1155::safeTransferFromCall {
      from,
      to,
      id,
      amount,
      data,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_safe_batch_transfer_from(
   from: Address,
   to: Address,
   ids: Vec<U256>,
   amounts: Vec<U256>,
   data: Bytes,
) -> Bytes {
   let c = IERC1155::safeBatchTransferFromCall {
      from,
      to,
      ids,
      amounts,
      data,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_uri(id: U256) -> Bytes {
   let c = IERC1155Metadata::uriCall { id };
   Bytes::from(c.abi_encode())
}

/// ERC-5216 `approve(address,uint256,uint256)` — the per-`id` allowance grant. `amount == 0`
/// revokes.
pub fn encode_approve(operator: Address, id: U256, amount: U256) -> Bytes {
   let c = IERC5216::approveCall {
      operator,
      id,
      amount,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_allowance(account: Address, operator: Address, id: U256) -> Bytes {
   let c = IERC5216::allowanceCall {
      account,
      operator,
      id,
   };
   Bytes::from(c.abi_encode())
}

/// ERC-7604 `permit(...)`. Signature (`sig`) is the raw 65-byte `r||s||v` (or an EIP-2098
/// compact form) — this ERC takes a `bytes` array rather than splitting into `v,r,s`.
pub fn encode_permit(
   owner: Address,
   operator: Address,
   token_id: U256,
   value: U256,
   deadline: U256,
   sig: Bytes,
) -> Bytes {
   let c = IERC1155Permit::permitCall {
      owner,
      operator,
      tokenId: token_id,
      value,
      deadline,
      sig,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_nonces(owner: Address, token_id: U256) -> Bytes {
   let c = IERC1155Permit::noncesCall {
      owner,
      tokenId: token_id,
   };
   Bytes::from(c.abi_encode())
}

pub fn encode_domain_separator() -> Bytes {
   Bytes::from(IERC1155Permit::DOMAIN_SEPARATORCall {}.abi_encode())
}

// ** ABI Decode Functions

pub fn decode_transfer_single_log(
   log: &LogData,
) -> Result<IERC1155::TransferSingle, anyhow::Error> {
   let b = IERC1155::TransferSingle::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_transfer_batch_log(log: &LogData) -> Result<IERC1155::TransferBatch, anyhow::Error> {
   let b = IERC1155::TransferBatch::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_approval_for_all_log(
   log: &LogData,
) -> Result<IERC1155::ApprovalForAll, anyhow::Error> {
   let b = IERC1155::ApprovalForAll::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_uri_log(log: &LogData) -> Result<IERC1155::URI, anyhow::Error> {
   let b = IERC1155::URI::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_balance_of(bytes: &Bytes) -> Result<U256, anyhow::Error> {
   let b = IERC1155::balanceOfCall::abi_decode_returns(bytes)?;
   Ok(b)
}

pub fn decode_balance_of_batch(bytes: &Bytes) -> Result<Vec<U256>, anyhow::Error> {
   let b = IERC1155::balanceOfBatchCall::abi_decode_returns(bytes)?;
   Ok(b)
}

pub fn decode_is_approved_for_all(bytes: &Bytes) -> Result<bool, anyhow::Error> {
   let a = IERC1155::isApprovedForAllCall::abi_decode_returns(bytes)?;
   Ok(a)
}

pub fn decode_uri(bytes: &Bytes) -> Result<String, anyhow::Error> {
   let u = IERC1155Metadata::uriCall::abi_decode_returns(bytes)?;
   Ok(u)
}

/// ERC-5216 `Approval(address indexed account, address indexed operator, uint256 id, uint256 amount)`.
///
/// `id` is **not** indexed here (unlike ERC-721's per-token `Approval`), so it arrives in the
/// data as the first word with `amount` right after it.
pub fn decode_approval_log(log: &LogData) -> Result<IERC5216::Approval, anyhow::Error> {
   let b = IERC5216::Approval::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_allowance(bytes: &Bytes) -> Result<U256, anyhow::Error> {
   let a = IERC5216::allowanceCall::abi_decode_returns(bytes)?;
   Ok(a)
}

/// Decode an ERC-5216 `approve(address, uint256, uint256)` **calldata** payload into
/// `(operator, id, amount)`. The three-argument `approve` is what separates it from an ERC-20 one.
pub fn decode_approve_call(bytes: &Bytes) -> Result<(Address, U256, U256), anyhow::Error> {
   let c = IERC5216::approveCall::abi_decode(bytes)?;
   Ok((c.operator, c.id, c.amount))
}

pub fn decode_nonces(bytes: &Bytes) -> Result<U256, anyhow::Error> {
   let n = IERC1155Permit::noncesCall::abi_decode_returns(bytes)?;
   Ok(n)
}

pub fn decode_domain_separator(bytes: &Bytes) -> Result<alloy_primitives::B256, anyhow::Error> {
   let d = IERC1155Permit::DOMAIN_SEPARATORCall::abi_decode_returns(bytes)?;
   Ok(d)
}

/// Decode a `safeTransferFrom` **calldata** payload into
/// `(from, to, id, amount, data)`.
pub fn decode_safe_transfer_from_call(
   bytes: &Bytes,
) -> Result<(Address, Address, U256, U256, Bytes), anyhow::Error> {
   let c = IERC1155::safeTransferFromCall::abi_decode(bytes)?;
   Ok((c.from, c.to, c.id, c.amount, c.data))
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::{B256, hex};

   /// Selector constants are computed from the canonical signatures
   /// (`cast sig "<signature>"`), cross-checked against `uri(uint256)` = `0x0e89341c`,
   /// which was confirmed against a live mainnet contract.
   #[test]
   fn selectors_match_the_erc1155_standard() {
      assert_eq!(
         IERC1155::balanceOfCall::SELECTOR,
         [0x00, 0xfd, 0xd5, 0x8e]
      );
      assert_eq!(
         IERC1155::balanceOfBatchCall::SELECTOR,
         [0x4e, 0x12, 0x73, 0xf4]
      );
      assert_eq!(
         IERC1155::setApprovalForAllCall::SELECTOR,
         [0xa2, 0x2c, 0xb4, 0x65]
      );
      assert_eq!(
         IERC1155::isApprovedForAllCall::SELECTOR,
         [0xe9, 0x85, 0xe9, 0xc5]
      );
      assert_eq!(
         IERC1155::safeTransferFromCall::SELECTOR,
         [0xf2, 0x42, 0x43, 0x2a]
      );
      assert_eq!(
         IERC1155::safeBatchTransferFromCall::SELECTOR,
         [0x2e, 0xb2, 0xc2, 0xd6]
      );
      assert_eq!(
         IERC1155Metadata::uriCall::SELECTOR,
         [0x0e, 0x89, 0x34, 0x1c]
      );
   }

   fn log_data(topics: Vec<B256>, data: Bytes) -> LogData {
      LogData::new_unchecked(topics, data)
   }

   const OPERATOR: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
   const FROM: &str = "0x1111111111111111111111111111111111111111";
   const TO: &str = "0x2222222222222222222222222222222222222222";

   /// Indexed address topic: the 20 address bytes right-aligned in a 32-byte word.
   fn topic_addr(a: &str) -> B256 {
      let bytes = alloy_primitives::hex::decode(a.trim_start_matches("0x")).unwrap();
      let mut buf = [0u8; 32];
      buf[12..].copy_from_slice(&bytes);
      B256::from(buf)
   }

   fn addr(a: &str) -> Address {
      a.parse().unwrap()
   }

   fn transfer_single_log() -> LogData {
      log_data(
         vec![
            // Real topic0 emitted by a real EVM (local anvil, solc 0.8.30).
            IERC1155::TransferSingle::SIGNATURE_HASH,
            topic_addr(OPERATOR),
            topic_addr(FROM),
            topic_addr(TO),
         ],
         Bytes::from(hex!(
            "000000000000000000000000000000000000000000000000000000000000002a\
                           0000000000000000000000000000000000000000000000000000000000000007"
         )),
      )
   }

   /// Decodes a log captured from a real EVM (anvil), not a hand-rolled encoding — this is what
   /// proves the generated struct's field order matches what the chain actually emits.
   #[test]
   fn decodes_real_transfer_single_log() {
      let decoded = decode_transfer_single_log(&transfer_single_log()).unwrap();
      assert_eq!(decoded.operator, addr(OPERATOR));
      assert_eq!(decoded.from, addr(FROM));
      assert_eq!(decoded.to, addr(TO));
      assert_eq!(decoded.id, U256::from(42));
      assert_eq!(decoded.value, U256::from(7));
   }

   #[test]
   fn decodes_real_transfer_batch_log() {
      // Captured from anvil: emitBatch() with ids = [1, 2], values = [3, 4].
      let data = hex!(
         "0000000000000000000000000000000000000000000000000000000000000040\
          00000000000000000000000000000000000000000000000000000000000000a0\
          0000000000000000000000000000000000000000000000000000000000000002\
          0000000000000000000000000000000000000000000000000000000000000001\
          0000000000000000000000000000000000000000000000000000000000000002\
          0000000000000000000000000000000000000000000000000000000000000002\
          0000000000000000000000000000000000000000000000000000000000000003\
          0000000000000000000000000000000000000000000000000000000000000004"
      );
      let log = log_data(
         vec![
            IERC1155::TransferBatch::SIGNATURE_HASH,
            topic_addr(OPERATOR),
            topic_addr(FROM),
            topic_addr(TO),
         ],
         Bytes::from(data),
      );

      let decoded = decode_transfer_batch_log(&log).unwrap();
      assert_eq!(decoded.ids, vec![U256::from(1), U256::from(2)]);
      assert_eq!(decoded.values, vec![U256::from(3), U256::from(4)]);
   }

   /// `URI` indexes the id and ABI-encodes the string in data — the capture had
   /// `id = 42` and the value `https://example.invalid/{id}.json`.
   #[test]
   fn decodes_real_uri_log() {
      let log = log_data(
         vec![
            IERC1155::URI::SIGNATURE_HASH,
            B256::from(U256::from(42).to_be_bytes::<32>()),
         ],
         Bytes::from(hex!(
            "0000000000000000000000000000000000000000000000000000000000000020\
             0000000000000000000000000000000000000000000000000000000000000021\
             68747470733a2f2f6578616d706c652e696e76616c69642f7b69647d2e6a736f6e\
             00000000000000000000000000000000000000000000000000000000000000"
         )),
      );

      let decoded = decode_uri_log(&log).unwrap();
      assert_eq!(decoded.id, U256::from(42));
      assert_eq!(decoded.value, "https://example.invalid/{id}.json");
   }

   #[test]
   fn decodes_real_approval_for_all_log() {
      let log = log_data(
         vec![
            IERC1155::ApprovalForAll::SIGNATURE_HASH,
            topic_addr(OPERATOR),
            topic_addr(TO),
         ],
         Bytes::from(hex!(
            "0000000000000000000000000000000000000000000000000000000000000001"
         )),
      );

      let decoded = decode_approval_for_all_log(&log).unwrap();
      assert_eq!(decoded.account, addr(OPERATOR));
      assert_eq!(decoded.operator, addr(TO));
      assert!(decoded.approved);
   }

   /// Pins the trap described in the module docs: an `ApprovalForAll` log is indistinguishable
   /// between ERC-721 and ERC-1155, so a decoder must disambiguate by the emitting contract.
   #[test]
   fn approval_for_all_is_indistinguishable_from_erc721() {
      use crate::abi::erc721::IERC721;
      assert_eq!(
         IERC1155::ApprovalForAll::SIGNATURE_HASH,
         IERC721::ApprovalForAll::SIGNATURE_HASH
      );
   }

   /// Pins the topic0s against values observed from a real EVM emission.
   #[test]
   fn topic0s_match_observed_chain_output() {
      assert_eq!(
         IERC1155::TransferSingle::SIGNATURE_HASH,
         hex!("c3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62")
      );
      assert_eq!(
         IERC1155::TransferBatch::SIGNATURE_HASH,
         hex!("4a39dc06d4c0dbc64b70af90fd698a233a518aa5d07e595d983b8c0526c8f7fb")
      );
      assert_eq!(
         IERC1155::ApprovalForAll::SIGNATURE_HASH,
         hex!("17307eab39ab6107e8899845ad3d59bd9653f200f220920489ca2b5937696c31")
      );
      assert_eq!(
         IERC1155::URI::SIGNATURE_HASH,
         hex!("6bb7ff708619ba0610cba295a58592e0451dee2622938c8755667688daf3529b")
      );
   }

   /// The ERC-5216 selectors, from `cast sig`. `approve` and `allowance` are names ERC-1155
   /// itself does not have, so these are the extension's own.
   #[test]
   fn selectors_match_erc5216_and_erc7604() {
      assert_eq!(
         IERC5216::approveCall::SELECTOR,
         [0x42, 0x6a, 0x84, 0x93]
      );
      assert_eq!(
         IERC5216::allowanceCall::SELECTOR,
         [0x59, 0x8a, 0xf9, 0xe7]
      );

      assert_eq!(
         IERC1155Permit::permitCall::SELECTOR,
         [0x4f, 0x6b, 0xe2, 0xb7]
      );
      assert_eq!(
         IERC1155Permit::noncesCall::SELECTOR,
         [0x50, 0x2e, 0x1a, 0x16]
      );
      assert_eq!(
         IERC1155Permit::DOMAIN_SEPARATORCall::SELECTOR,
         [0x36, 0x44, 0xe5, 0x15]
      );
   }

   /// The ERC-5216 `Approval` has its **own** topic0 — the whole reason it can be decoded from
   /// the log alone, without asking the contract which standard it is. Pinned as a literal so a
   /// future rename or signature edit cannot quietly make it collide with the ERC-721
   /// per-token `Approval` (which shares only the *name*, not the signature).
   #[test]
   fn erc5216_approval_topic0_is_its_own() {
      assert_eq!(
         IERC5216::Approval::SIGNATURE_HASH,
         hex!("b3fd5071835887567a0671151121894ddccc2842f1d10bedad13e0d17cace9a7")
      );

      use crate::abi::erc721::IERC721;
      assert_ne!(
         IERC5216::Approval::SIGNATURE_HASH,
         IERC721::Approval::SIGNATURE_HASH
      );
      assert_ne!(
         IERC5216::Approval::SIGNATURE_HASH,
         IERC1155::ApprovalForAll::SIGNATURE_HASH
      );
   }

   /// Decodes the ERC-5216 `Approval` from its real layout: **3** topics (`account`, `operator`)
   /// with `(id, amount)` in the data — the id is not indexed.
   #[test]
   fn decodes_erc5216_approval_log() {
      let log = log_data(
         vec![
            IERC5216::Approval::SIGNATURE_HASH,
            topic_addr(FROM),
            topic_addr(OPERATOR),
         ],
         Bytes::from(hex!(
            "000000000000000000000000000000000000000000000000000000000000002a\
             0000000000000000000000000000000000000000000000000000000000000007"
         )),
      );

      let decoded = decode_approval_log(&log).unwrap();
      assert_eq!(decoded.account, addr(FROM));
      assert_eq!(decoded.operator, addr(OPERATOR));
      assert_eq!(decoded.id, U256::from(42));
      assert_eq!(decoded.amount, U256::from(7));
   }

   /// The disambiguation that the whole NFT-approval decode ladder rests on: an ERC-5216 log
   /// (3 topics) and an ERC-721 per-token `Approval` (4 topics, empty data) must each be
   /// refused by the other's decoder. Without this, one silently decodes as the other and a
   /// per-token approval turns into an allowance of whatever the words happen to say.
   #[test]
   fn erc5216_and_erc721_approvals_are_not_interchangeable() {
      use crate::abi::erc721::IERC721;

      let erc5216 = log_data(
         vec![
            IERC5216::Approval::SIGNATURE_HASH,
            topic_addr(FROM),
            topic_addr(OPERATOR),
         ],
         Bytes::from(hex!(
            "000000000000000000000000000000000000000000000000000000000000002a\
             0000000000000000000000000000000000000000000000000000000000000007"
         )),
      );

      let erc721 = log_data(
         vec![
            IERC721::Approval::SIGNATURE_HASH,
            topic_addr(FROM),
            topic_addr(OPERATOR),
            B256::from(U256::from(42).to_be_bytes::<32>()),
         ],
         Bytes::new(),
      );

      assert!(decode_approval_log(&erc5216).is_ok());
      assert!(crate::abi::erc721::decode_approval_log(&erc721).is_ok());

      // Each must fail on the other's shape.
      assert!(decode_approval_log(&erc721).is_err());
      assert!(crate::abi::erc721::decode_approval_log(&erc5216).is_err());
   }

   /// The ERC-5216 interface id is derivable — XOR of the two function selectors — which is a
   /// stronger check than trusting the constant copied from the ERC text.
   #[test]
   fn erc5216_interface_id_is_the_selector_xor() {
      let a = u32::from_be_bytes(IERC5216::approveCall::SELECTOR);
      let b = u32::from_be_bytes(IERC5216::allowanceCall::SELECTOR);
      assert_eq!(a ^ b, 0x1be0_7d74);
   }

   #[test]
   fn erc5216_approve_encodes_operator_id_then_amount() {
      let operator = Address::repeat_byte(0x11);
      let call = encode_approve(operator, U256::from(7), U256::from(3));

      assert_eq!(&call[..4], &IERC5216::approveCall::SELECTOR);
      assert_eq!(call.len(), 4 + 32 * 3);

      let mut word = [0u8; 32];
      word[12..].copy_from_slice(operator.as_slice());
      assert_eq!(
         &call[4..36],
         &word,
         "the operator is the first word"
      );
      assert_eq!(U256::from_be_slice(&call[36..68]), U256::from(7));
      assert_eq!(U256::from_be_slice(&call[68..100]), U256::from(3));
   }

   /// `nonces` is keyed by `(owner, tokenId)` in ERC-7604 — per token id, unlike ERC-2612 and
   /// ERC-4494 where a nonce belongs to the owner alone.
   #[test]
   fn erc7604_nonces_takes_the_token_id() {
      let owner = Address::repeat_byte(0x22);
      let call = encode_nonces(owner, U256::from(9));

      assert_eq!(&call[..4], &IERC1155Permit::noncesCall::SELECTOR);
      assert_eq!(call.len(), 4 + 32 * 2);

      let mut word = [0u8; 32];
      word[12..].copy_from_slice(owner.as_slice());
      assert_eq!(&call[4..36], &word);
      assert_eq!(U256::from_be_slice(&call[36..68]), U256::from(9));
   }

   #[test]
   fn safe_transfer_from_roundtrips() {
      let from = Address::repeat_byte(0x11);
      let to = Address::repeat_byte(0x22);

      let data = encode_safe_transfer_from(
         from,
         to,
         U256::from(7),
         U256::from(3),
         Bytes::new(),
      );
      let (d_from, d_to, id, amount, payload) = decode_safe_transfer_from_call(&data).unwrap();
      assert_eq!((d_from, d_to), (from, to));
      assert_eq!((id, amount), (U256::from(7), U256::from(3)));
      assert!(payload.is_empty());
   }

   /// The read the NFT send path checks a received amount with. Argument order is the bug this
   /// guards: `balanceOf(address account, uint256 id)` puts the account in the first word and the id
   /// in the second, and swapping them asks about a garbage account.
   #[test]
   fn balance_of_encodes_the_account_then_the_id() {
      let account = Address::repeat_byte(0x11);
      let call = encode_balance_of(account, U256::from(7));

      assert_eq!(&call[..4], &IERC1155::balanceOfCall::SELECTOR);
      assert_eq!(call.len(), 4 + 32 * 2);

      let mut word = [0u8; 32];
      word[12..].copy_from_slice(account.as_slice());

      assert_eq!(
         &call[4..36],
         &word,
         "the account is the first word"
      );
      assert_eq!(
         U256::from_be_slice(&call[36..68]),
         U256::from(7),
         "the id is the second"
      );
   }
}
