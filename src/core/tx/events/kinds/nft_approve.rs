use crate::core::ZeusCtx;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::{erc165, erc721, erc1155},
   alloy_primitives::{Address, Log, U256},
   nft::NftStandard,
};

/// An NFT approval, decoded from one log.
///
/// Three on-chain shapes carry every ERC-721 / ERC-1155 approval, and this struct holds all
/// three without pretending they are the same shape:
///
/// | Emitted by | `token_id` | `approved` | `amount` |
/// |---|---|---|---|
/// | ERC-721 `Approval` (also EIP-4494 `permit`) | `Some(id)` | `None` | `None` |
/// | ERC-5216 `Approval` | `Some(id)` | `None` | `Some(value)` |
/// | `ApprovalForAll` | `None` | `Some(flag)` | `None` |
///
/// The `Option`s are load-bearing: `ApprovalForAll` grants a *boolean* for a whole collection
/// and ERC-5216 grants an *amount* for one id, so a single field could only express both by
/// inventing a meaning for one of them — `approved: false` silently reading as "zero
/// allowance", or a zero amount reading as "revoked operator".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftApproveParams {
   pub chain: u64,
   pub standard: NftStandard,
   /// The collection that emitted the log — an NFT has no other contract involved.
   pub collection: Address,
   /// `None` for a collection-wide `ApprovalForAll`, which has no id: a zero id would read as a
   /// real token in the UI and as a real key in the approval store.
   pub token_id: Option<U256>,
   /// Whose approval it is.
   pub owner: Address,
   /// The operator being approved.
   pub operator: Address,
   /// `ApprovalForAll`'s flag. `Some(false)` is a **revocation**, which is still an approval.
   pub approved: Option<bool>,
   /// ERC-5216's allowance for this id. `Some(0)` is a revocation.
   pub amount: Option<U256>,
}

impl NftApproveParams {
   /// Whether this approval grants nothing.
   ///
   /// Each shape says "no" its own way, and none of them is a missing value: ERC-721 clears the
   /// operator to the zero address, `ApprovalForAll` sets the flag false, ERC-5216 sets the
   /// allowance to zero.
   pub fn is_revoke(&self) -> bool {
      if let Some(approved) = self.approved {
         return !approved;
      }
      if let Some(amount) = &self.amount {
         return amount.is_zero();
      }
      self.operator.is_zero()
   }

   /// Whether this approval covers the whole collection rather than one token.
   pub fn is_collection_wide(&self) -> bool {
      self.token_id.is_none()
   }

   pub fn name(&self) -> &str {
      if self.is_revoke() {
         "Revoke NFT Approval"
      } else {
         "NFT Approval"
      }
   }

   /// Decode any NFT approval log.
   ///
   /// The three pure halves are separate so they can be tested without a context. Only
   /// `ApprovalForAll` needs one: ERC-721 and ERC-1155 emit a **byte-identical** log for it, so
   /// the standard has to be asked of the contract itself.
   ///
   /// Order matters only in that the two single-token shapes must be tried before
   /// `ApprovalForAll`; they cannot shadow each other, since ERC-5216's `Approval` has its own
   /// topic0 and ERC-721's needs a fourth topic that ERC-5216 never sends. That mutual refusal
   /// is pinned in `zeus_eth::abi::erc1155`.
   pub async fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Self, anyhow::Error> {
      if let Some(params) = Self::from_erc721_approval(chain, log) {
         return Ok(params);
      }

      if let Some(params) = Self::from_erc1155_approval(chain, log) {
         return Ok(params);
      }

      if let Some(mut params) = Self::from_approval_for_all(chain, log) {
         params.standard = Self::standard_of(ctx, chain, log.address).await;
         return Ok(params);
      }

      Err(anyhow!("Not an NFT approval log"))
   }

   /// ERC-721 `Approval(address indexed owner, address indexed approved, uint256 indexed tokenId)`.
   ///
   /// Its topic0 is the same as the ERC-20 `Approval`'s and the two are told apart by **topic
   /// count**: ERC-721 indexes the token id (4 topics, empty data) while ERC-20 leaves the value
   /// in data (3 topics). The decode fails on the other shape, which is what lets the ladder
   /// keep both without one shadowing the other.
   ///
   /// `approved` carries the operator, so the zero address here is a revocation rather than an
   /// absent value — which is why it lands in `operator` and `approved` stays `None`.
   pub fn from_erc721_approval(chain: u64, log: &Log) -> Option<Self> {
      let decoded = erc721::decode_approval_log(log).ok()?;

      Some(Self {
         chain,
         standard: NftStandard::Erc721,
         collection: log.address,
         token_id: Some(decoded.tokenId),
         owner: decoded.owner,
         operator: decoded.approved,
         approved: None,
         amount: None,
      })
   }

   /// ERC-5216 `Approval(address indexed account, address indexed operator, uint256 id, uint256 amount)`.
   ///
   /// Its own topic0, and three topics: this `id` is **not** indexed, unlike ERC-721's.
   pub fn from_erc1155_approval(chain: u64, log: &Log) -> Option<Self> {
      let decoded = erc1155::decode_approval_log(log).ok()?;

      Some(Self {
         chain,
         standard: NftStandard::Erc1155,
         collection: log.address,
         token_id: Some(decoded.id),
         owner: decoded.account,
         operator: decoded.operator,
         approved: None,
         amount: Some(decoded.amount),
      })
   }

   /// `ApprovalForAll(address indexed owner, address indexed operator, bool approved)`.
   ///
   /// The struct is built with ERC-721 and corrected by the caller — the log carries no standard.
   pub fn from_approval_for_all(chain: u64, log: &Log) -> Option<Self> {
      let decoded = erc721::decode_approval_for_all_log(log).ok()?;

      Some(Self {
         chain,
         standard: NftStandard::Erc721,
         collection: log.address,
         token_id: None,
         owner: decoded.owner,
         operator: decoded.operator,
         approved: Some(decoded.approved),
         amount: None,
      })
   }

   /// Which standard emitted an `ApprovalForAll`: the cached collection first (no call), then the
   /// contract's own ERC-165 answer.
   ///
   /// A contract that answers neither leaves the label as ERC-721 — that costs a label, not the
   /// event, and losing an approval from the history would cost the user the one thing approvals
   /// are worth showing for.
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
   use zeus_eth::abi::{erc721::IERC721, erc1155::IERC5216};

   const COLLECTION: &str = "0xbc4ca0eda7647a8ab7c2061c2e118a18a936f13d";
   const OWNER: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
   const OPERATOR: &str = "0x2222222222222222222222222222222222222222";
   const ZERO: &str = "0x0000000000000000000000000000000000000000";

   fn addr(a: &str) -> Address {
      a.parse().unwrap()
   }

   fn collection() -> Address {
      addr(COLLECTION)
   }

   /// ERC-721 per-token `Approval` — the token id is **indexed**, so it is a topic and the data
   /// is empty. Same topic0 as the ERC-20 `Approval`; the fourth topic is what separates them.
   fn erc721_approval_log(approved: &str, token_id: u64) -> Log {
      Log {
         address: collection(),
         data: IERC721::Approval {
            owner: addr(OWNER),
            approved: addr(approved),
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      }
   }

   /// ERC-5216 `Approval` — its own topic0, and the id is **not** indexed (3 topics), so it
   /// arrives in the data alongside the amount.
   fn erc5216_approval_log(operator: &str, id: u64, amount: u64) -> Log {
      Log {
         address: collection(),
         data: IERC5216::Approval {
            account: addr(OWNER),
            operator: addr(operator),
            id: U256::from(id),
            amount: U256::from(amount),
         }
         .encode_log_data(),
      }
   }

   fn approval_for_all_log(approved: bool) -> Log {
      Log {
         address: collection(),
         data: IERC721::ApprovalForAll {
            owner: addr(OWNER),
            operator: addr(OPERATOR),
            approved,
         }
         .encode_log_data(),
      }
   }

   #[test]
   fn an_erc721_approval_is_a_single_token_and_carries_no_amount() {
      let params =
         NftApproveParams::from_erc721_approval(1, &erc721_approval_log(OPERATOR, 7)).unwrap();

      assert_eq!(params.standard, NftStandard::Erc721);
      assert_eq!(params.collection, collection());
      assert_eq!(params.token_id, Some(U256::from(7)));
      assert_eq!(
         (params.owner, params.operator),
         (addr(OWNER), addr(OPERATOR))
      );
      assert_eq!(
         params.approved, None,
         "ERC-721 has no flag to report"
      );
      assert_eq!(
         params.amount, None,
         "and no amount — the id *is* the thing"
      );
      assert!(!params.is_revoke());
      assert!(!params.is_collection_wide());
      assert_eq!(params.name(), "NFT Approval");
   }

   /// Clearing an ERC-721 approval sets the operator to the zero address — a revocation has no
   /// separate event, so the zero address *is* the signal.
   #[test]
   fn clearing_the_zero_address_is_an_erc721_revoke() {
      let params =
         NftApproveParams::from_erc721_approval(1, &erc721_approval_log(ZERO, 7)).unwrap();

      assert_eq!(params.operator, Address::ZERO);
      assert!(params.is_revoke());
      assert_eq!(params.name(), "Revoke NFT Approval");
   }

   #[test]
   fn an_erc5216_approval_carries_its_amount_for_one_id() {
      let params =
         NftApproveParams::from_erc1155_approval(1, &erc5216_approval_log(OPERATOR, 42, 5))
            .unwrap();

      assert_eq!(params.standard, NftStandard::Erc1155);
      assert_eq!(params.token_id, Some(U256::from(42)));
      assert_eq!(params.amount, Some(U256::from(5)));
      assert_eq!(params.approved, None);
      assert!(!params.is_revoke());
   }

   /// A zero allowance is ERC-5216's revocation, exactly as the zero address is ERC-721's.
   #[test]
   fn a_zero_allowance_is_an_erc5216_revoke() {
      let params =
         NftApproveParams::from_erc1155_approval(1, &erc5216_approval_log(OPERATOR, 42, 0))
            .unwrap();

      assert_eq!(params.amount, Some(U256::ZERO));
      assert!(params.is_revoke());
   }

   /// `ApprovalForAll` is collection-wide and keeps the flag: a **revoked** approval is an
   /// approval too, which is why the flag is an `Option<bool>` and not a `bool`.
   #[test]
   fn an_approval_for_all_is_collection_wide_and_keeps_its_flag() {
      let granted =
         NftApproveParams::from_approval_for_all(1, &approval_for_all_log(true)).unwrap();
      assert_eq!(granted.approved, Some(true));
      assert_eq!(
         granted.token_id, None,
         "an ApprovalForAll has no token id"
      );
      assert_eq!(granted.amount, None);
      assert_eq!(
         (granted.owner, granted.operator),
         (addr(OWNER), addr(OPERATOR))
      );
      assert!(granted.is_collection_wide());
      assert!(!granted.is_revoke());

      let revoked =
         NftApproveParams::from_approval_for_all(1, &approval_for_all_log(false)).unwrap();
      assert_eq!(revoked.approved, Some(false));
      assert!(revoked.is_revoke());
      assert_eq!(
         revoked.name(),
         "Revoke NFT Approval",
         "revoking is still an approval"
      );
   }

   /// The single-token halves must not accept each other's logs. This is the same trap the
   /// zeus-eth ABI tests pin from the encoding side; asserted here against the decoders the
   /// ladder actually calls, in the order it calls them.
   #[test]
   fn the_two_single_token_shapes_do_not_decode_as_each_other() {
      let erc721 = erc721_approval_log(OPERATOR, 7);
      let erc5216 = erc5216_approval_log(OPERATOR, 42, 5);

      assert!(NftApproveParams::from_erc721_approval(1, &erc721).is_some());
      assert!(
         NftApproveParams::from_erc1155_approval(1, &erc721).is_none(),
         "a 4-topic ERC-721 log is not an ERC-5216 allowance"
      );

      assert!(NftApproveParams::from_erc1155_approval(1, &erc5216).is_some());
      assert!(
         NftApproveParams::from_erc721_approval(1, &erc5216).is_none(),
         "a 3-topic ERC-5216 log is not an ERC-721 per-token approval"
      );
   }

   /// Neither single-token shape may swallow an `ApprovalForAll`, which is a different topic0
   /// altogether — otherwise a collection-wide grant would land in the per-token branch with a
   /// made-up id.
   #[test]
   fn no_single_token_shape_claims_an_approval_for_all() {
      let log = approval_for_all_log(true);

      assert!(NftApproveParams::from_erc721_approval(1, &log).is_none());
      assert!(NftApproveParams::from_erc1155_approval(1, &log).is_none());
      assert!(NftApproveParams::from_approval_for_all(1, &log).is_some());
   }

   /// A plain ERC-20 `Approval` (3 topics, value in data) must not read as an NFT approval: it
   /// would otherwise name the collection as the token and the value as a token id.
   #[test]
   fn an_erc20_approval_is_not_an_nft_approval() {
      use zeus_eth::abi::erc20::IERC20;

      let log = Log {
         address: collection(),
         data: IERC20::Approval {
            owner: addr(OWNER),
            spender: addr(OPERATOR),
            value: U256::from(1000),
         }
         .encode_log_data(),
      };

      assert!(NftApproveParams::from_erc721_approval(1, &log).is_none());
      assert!(NftApproveParams::from_erc1155_approval(1, &log).is_none());
      assert!(NftApproveParams::from_approval_for_all(1, &log).is_none());
   }
}
