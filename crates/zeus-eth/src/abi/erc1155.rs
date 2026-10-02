//! ABI for the ERC1155 (multi-token) interfaces
//!
//! [`IERC1155`] is the core interface (balances, approvals, both transfer flavours and the
//! `TransferSingle` / `TransferBatch` / `ApprovalForAll` / `URI` events).
//! [`IERC1155Metadata`] holds `uri(uint256)`, which is a *separate* interface id
//! (`0x0e89341c`) and must be probed via `abi::erc165` — not every collection implements it.
//!
//! Two encoding traps worth knowing before decoding logs from an unknown address:
//!
//! 1. `ApprovalForAll(address,address,bool)` is **byte-for-byte identical** to ERC-721's
//!    `ApprovalForAll` — same topic0, same 2 indexed + `bool` layout. The log alone cannot
//!    tell you which standard emitted it; only the emitting contract's ERC-165 can.
//! 2. `URI(string,uint256)` indexes the id and puts the string in data, so the id is a topic
//!    while the URI needs full ABI string decoding.

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
