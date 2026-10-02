use crate::core::ZeusCtx;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::{erc165, erc721, erc1155},
   alloy_primitives::{Address, Log, U256},
   nft::NftStandard,
};

/// An NFT transfer, mint, burn or approval, decoded from one log.
///
/// One params per moved token: an ERC-1155 `TransferBatch` is a single log carrying N transfers, and
/// the decode pipeline's `DecodeOutcome::Many` already carries what one log cannot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftTransferParams {
   pub chain: u64,
   pub standard: NftStandard,
   /// The collection that emitted the log — for an NFT there is no other contract involved.
   pub collection: Address,
   /// The token this concerns. `None` for a collection-wide approval (`ApprovalForAll`), which has no
   /// token id: a zero id would read as a real token in the UI.
   pub token_id: Option<U256>,
   /// How many units moved: `1` for every ERC-721 transfer (the id *is* the thing), the event's value
   /// for ERC-1155, and zero for an approval.
   pub amount: U256,
   pub from: Address,
   /// The recipient, or the operator for an approval.
   pub to: Address,
   pub is_mint: bool,
   pub is_burn: bool,
   /// `None` for a transfer, `Some(approved)` for an approval — a **revoked** approval
   /// (`approved == false`) is still an approval, which a plain `bool` could not express.
   pub approval: Option<bool>,
}

impl NftTransferParams {
   pub fn name(&self) -> String {
      match self {
         Self {
            approval: Some(_), ..
         } => "NFT Approval".to_string(),
         Self { is_mint: true, .. } => "NFT Mint".to_string(),
         Self { is_burn: true, .. } => "NFT Burn".to_string(),
         _ => "NFT Transfer".to_string(),
      }
   }

   /// Decode any NFT log, filling in whatever the log alone cannot say.
   ///
   /// The per-standard halves are separate and pure so they can be tested without a context. Only
   /// `ApprovalForAll` needs one: ERC-721 and ERC-1155 emit a **byte-identical** log for it, so the
   /// standard has to be asked of the contract itself.
   pub async fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Vec<Self>, anyhow::Error> {
      if let Some(params) = Self::from_erc721_transfer(chain, log) {
         return Ok(vec![params]);
      }

      if let Some(params) = Self::from_erc1155_transfer(chain, log) {
         return Ok(params);
      }

      if let Some(mut params) = Self::from_approval_for_all(chain, log) {
         params.standard = Self::standard_of(ctx, chain, log.address).await;
         return Ok(vec![params]);
      }

      Err(anyhow!("Not an NFT log"))
   }

   /// ERC-721 `Transfer(address indexed from, address indexed to, uint256 indexed tokenId)`.
   ///
   /// Its topic0 is the same as the ERC-20 `Transfer`'s, and the two are told apart by **topic count**:
   /// ERC-721 indexes the token id (4 topics, empty data) while ERC-20 leaves the value in data (3
   /// topics). The decode fails on the other shape, which is why the decode ladder can try the
   /// fungible decoder first and this one right after without either shadowing the other.
   fn from_erc721_transfer(chain: u64, log: &Log) -> Option<Self> {
      let decoded = erc721::decode_transfer_log(log).ok()?;

      Some(Self {
         chain,
         standard: NftStandard::Erc721,
         collection: log.address,
         token_id: Some(decoded.tokenId),
         amount: U256::from(1),
         from: decoded.from,
         to: decoded.to,
         is_mint: decoded.from.is_zero(),
         is_burn: decoded.to.is_zero(),
         approval: None,
      })
   }

   /// ERC-1155 `TransferSingle` and `TransferBatch` — their own topic0s, so nothing else in the ladder
   /// can be confused for them.
   fn from_erc1155_transfer(chain: u64, log: &Log) -> Option<Vec<Self>> {
      if let Ok(decoded) = erc1155::decode_transfer_single_log(log) {
         return Some(vec![Self::erc1155(
            chain,
            log.address,
            decoded.from,
            decoded.to,
            decoded.id,
            decoded.value,
         )]);
      }

      let batch = erc1155::decode_transfer_batch_log(log).ok()?;
      let (from, to) = (batch.from, batch.to);

      // One event per `(id, amount)` pair. `zip` stops at the shorter array, so a malformed batch
      // yields fewer events rather than inventing a token with a missing amount.
      let events = batch
         .ids
         .into_iter()
         .zip(batch.values)
         .map(|(id, amount)| Self::erc1155(chain, log.address, from, to, id, amount))
         .collect();

      Some(events)
   }

   fn erc1155(
      chain: u64,
      collection: Address,
      from: Address,
      to: Address,
      id: U256,
      amount: U256,
   ) -> Self {
      Self {
         chain,
         standard: NftStandard::Erc1155,
         collection,
         token_id: Some(id),
         amount,
         from,
         to,
         is_mint: from.is_zero(),
         is_burn: to.is_zero(),
         approval: None,
      }
   }

   /// `ApprovalForAll(address indexed owner, address indexed operator, bool approved)`.
   ///
   /// The struct is built with ERC-721 and corrected by the caller — the log carries no standard.
   fn from_approval_for_all(chain: u64, log: &Log) -> Option<Self> {
      let decoded = erc721::decode_approval_for_all_log(log).ok()?;

      Some(Self {
         chain,
         standard: NftStandard::Erc721,
         collection: log.address,
         token_id: None,
         amount: U256::ZERO,
         from: decoded.owner,
         to: decoded.operator,
         is_mint: false,
         is_burn: false,
         approval: Some(decoded.approved),
      })
   }

   /// Which standard emitted an `ApprovalForAll`: the cached collection first (no call), then the
   /// contract's own ERC-165 answer.
   ///
   /// A contract that answers neither leaves the label as ERC-721 — that costs a label, not the event,
   /// and losing an approval from the history would cost the user the one thing approvals are worth
   /// showing for.
   async fn standard_of(ctx: ZeusCtx, chain: u64, collection: Address) -> NftStandard {
      if let Some(collection) = ctx.read(|ctx| ctx.nft_db.get_collection(chain, collection)) {
         return collection.standard;
      }

      let Ok(client) = ctx.get_client(chain).await else {
         return NftStandard::Erc721;
      };

      match erc165::probe(client, collection).await.is_erc1155() {
         true => NftStandard::Erc1155,
         false => NftStandard::Erc721,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_sol_types::SolEvent;
   use zeus_eth::{
      abi::{erc20::IERC20, erc721::IERC721, erc1155::IERC1155},
      alloy_primitives::Log,
   };

   const COLLECTION: &str = "0xbc4ca0eda7647a8ab7c2061c2e118a18a936f13d";
   const OPERATOR: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
   const FROM: &str = "0x1111111111111111111111111111111111111111";
   const TO: &str = "0x2222222222222222222222222222222222222222";
   const ZERO: &str = "0x0000000000000000000000000000000000000000";

   fn addr(a: &str) -> Address {
      a.parse().unwrap()
   }

   fn collection() -> Address {
      addr(COLLECTION)
   }

   /// A real ERC-721 `Transfer`: the token id is **indexed**, so it lands in the topics and the data
   /// stays empty — which is exactly what separates it from the ERC-20 shape below. The raw layout of
   /// both is pinned in zeus-eth against logs captured from a real EVM; these only have to be the same
   /// events.
   fn erc721_transfer_log(sender: &str, recipient: &str, token_id: u64) -> Log {
      Log {
         address: collection(),
         data: IERC721::Transfer {
            from: addr(sender),
            to: addr(recipient),
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      }
   }

   /// An ERC-20 `Transfer` — the **same** topic0, with the value in the data instead of an indexed id.
   fn erc20_transfer_log() -> Log {
      Log {
         address: collection(),
         data: IERC20::Transfer {
            from: addr(FROM),
            to: addr(TO),
            value: U256::from(1000),
         }
         .encode_log_data(),
      }
   }

   fn erc1155_single_log(id: u64, amount: u64) -> Log {
      Log {
         address: collection(),
         data: IERC1155::TransferSingle {
            operator: addr(OPERATOR),
            from: addr(FROM),
            to: addr(TO),
            id: U256::from(id),
            value: U256::from(amount),
         }
         .encode_log_data(),
      }
   }

   fn erc1155_batch_log(ids: Vec<u64>, amounts: Vec<u64>) -> Log {
      Log {
         address: collection(),
         data: IERC1155::TransferBatch {
            operator: addr(OPERATOR),
            from: addr(FROM),
            to: addr(TO),
            ids: ids.into_iter().map(U256::from).collect(),
            values: amounts.into_iter().map(U256::from).collect(),
         }
         .encode_log_data(),
      }
   }

   /// The load-bearing one. ERC-721's `Transfer` is ERC-20's topic0 with the token id indexed instead
   /// of the value: if the fungible shape ever decoded as an NFT, every ERC-20 transfer in the app
   /// would be reported as one. Asserted from both sides, because the decode ladder depends on the
   /// fungible attempt failing here.
   #[test]
   fn an_erc20_transfer_is_not_an_nft_transfer() {
      assert!(
         NftTransferParams::from_erc721_transfer(1, &erc20_transfer_log()).is_none(),
         "three topics is the fungible shape"
      );

      let nft = NftTransferParams::from_erc721_transfer(1, &erc721_transfer_log(FROM, TO, 1));
      assert!(nft.is_some(), "four topics is the NFT shape");
   }

   #[test]
   fn an_erc721_transfer_carries_one_token_and_its_owner_pair() {
      let params =
         NftTransferParams::from_erc721_transfer(1, &erc721_transfer_log(FROM, TO, 7)).unwrap();

      assert_eq!(params.standard, NftStandard::Erc721);
      assert_eq!(params.collection, addr(COLLECTION));
      assert_eq!(params.token_id, Some(U256::from(7)));
      assert_eq!(
         params.amount,
         U256::from(1),
         "an ERC-721 transfer moves exactly one"
      );
      assert_eq!((params.from, params.to), (addr(FROM), addr(TO)));
      assert_eq!(params.approval, None);
      assert_eq!(params.name(), "NFT Transfer");
   }

   /// Mint and burn are the zero address on either side, and they read differently to the user.
   #[test]
   fn mint_and_burn_are_named_from_the_zero_address() {
      let minted =
         NftTransferParams::from_erc721_transfer(1, &erc721_transfer_log(ZERO, TO, 7)).unwrap();
      assert!(minted.is_mint && !minted.is_burn);
      assert_eq!(minted.name(), "NFT Mint");

      let burned =
         NftTransferParams::from_erc721_transfer(1, &erc721_transfer_log(TO, ZERO, 7)).unwrap();
      assert!(burned.is_burn && !burned.is_mint);
      assert_eq!(burned.name(), "NFT Burn");
   }

   /// An ERC-1155 `TransferSingle` captured from a real EVM — the same anvil capture the zeus-eth ABI
   /// tests pin — with `(operator, from, to)` in the topics and `(id, value)` in the data.
   #[test]
   fn an_erc1155_single_decodes_its_amount() {
      let log = erc1155_single_log(42, 7);

      let params = NftTransferParams::from_erc1155_transfer(1, &log).unwrap();

      assert_eq!(params.len(), 1);
      assert_eq!(params[0].standard, NftStandard::Erc1155);
      assert_eq!(params[0].token_id, Some(U256::from(42)));
      assert_eq!(params[0].amount, U256::from(7));
      assert_eq!(params[0].name(), "NFT Transfer");
   }

   /// A batch is one log carrying N transfers: every id has to come out, not just the first, and each
   /// one pairs with its own amount.
   #[test]
   fn an_erc1155_batch_expands_to_one_event_per_id() {
      let log = erc1155_batch_log(vec![1, 2], vec![3, 4]);

      let params = NftTransferParams::from_erc1155_transfer(1, &log).unwrap();

      assert_eq!(params.len(), 2, "one event per id");
      assert_eq!(params[0].token_id, Some(U256::from(1)));
      assert_eq!(params[0].amount, U256::from(3));
      assert_eq!(params[1].token_id, Some(U256::from(2)));
      assert_eq!(params[1].amount, U256::from(4));
      assert!(params.iter().all(|p| p.from == addr(FROM) && p.to == addr(TO)));
   }

   fn approval_log(approved: bool) -> Log {
      Log {
         address: collection(),
         data: IERC721::ApprovalForAll {
            owner: addr(OPERATOR),
            operator: addr(TO),
            approved,
         }
         .encode_log_data(),
      }
   }

   /// An approval has no token and keeps the flag: a **revoked** approval is an approval too, which is
   /// exactly why the flag is an `Option<bool>` and not a `bool`.
   #[test]
   fn an_approval_has_no_token_and_keeps_its_flag() {
      let granted = NftTransferParams::from_approval_for_all(1, &approval_log(true)).unwrap();
      assert_eq!(granted.approval, Some(true));
      assert_eq!(
         granted.token_id, None,
         "an ApprovalForAll has no token id"
      );
      assert_eq!(granted.amount, U256::ZERO);
      assert_eq!(
         (granted.from, granted.to),
         (addr(OPERATOR), addr(TO))
      );
      assert_eq!(granted.name(), "NFT Approval");

      let revoked = NftTransferParams::from_approval_for_all(1, &approval_log(false)).unwrap();
      assert_eq!(revoked.approval, Some(false));
      assert_eq!(
         revoked.name(),
         "NFT Approval",
         "revoking is still an approval"
      );
   }
}
