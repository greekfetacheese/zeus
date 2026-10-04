//! NFT (ERC-721 / ERC-1155) domain types.
//!
//! Deliberately separate from [`crate::currency`]: an NFT has no `decimals`, no fungible amount
//! and no pool price, so it must not enter the `Currency` enum — that enum stays fungible-only
//! and every existing match arm (price manager, AMM, swap, portfolio) is left alone.
//!
//! `name` / `symbol` and the standard live on [`NftCollection`] rather than on [`NftToken`]:
//! they are collection-level facts, so storing them per token would repeat the same strings for
//! every token a wallet holds.
//!
//! There is no `image` field here either — images are cached on disk and in
//! `assets::icons`, exactly like ERC-20 token icons. See the NFT icon store (task 4.2).

use crate::abi::erc165::Erc165Support;
use crate::abi::{erc165, erc721, erc1155};
use crate::utils::batch::{NftRef, get_erc721_owners, get_erc1155_balances};
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, U256};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Which NFT standard a collection implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NftStandard {
   Erc721,
   Erc1155,
}

impl NftStandard {
   pub fn is_erc721(&self) -> bool {
      matches!(self, Self::Erc721)
   }

   pub fn is_erc1155(&self) -> bool {
      matches!(self, Self::Erc1155)
   }
}

impl fmt::Display for NftStandard {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.write_str(match self {
         Self::Erc721 => "ERC-721",
         Self::Erc1155 => "ERC-1155",
      })
   }
}

/// An NFT collection (a contract) on one chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftCollection {
   pub chain_id: u64,
   pub address: Address,
   pub standard: NftStandard,
   /// From `name()`. `None` when the contract does not implement it.
   pub name: Option<String>,
   /// From `symbol()`. `None` when the contract does not implement it.
   pub symbol: Option<String>,
}

impl NftCollection {
   /// Resolve a collection by address: standard via ERC-165, then `name()` / `symbol()`.
   ///
   /// Errors when the address is not an ERC-721 or ERC-1155 contract, so a pasted ERC-20 address
   /// is rejected here rather than producing a nonsense collection.
   ///
   /// `name()` / `symbol()` are attempted rather than gated on ERC-165: ERC-1155 does not
   /// standardize them at all, and plenty of contracts implement a call without advertising the
   /// interface (the same trap as `uri()`). A failure just yields `None`.
   pub async fn fetch<P, N>(
      client: P,
      chain_id: u64,
      address: Address,
   ) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let support = erc165::probe(client.clone(), address).await?;
      Self::with_support(client, chain_id, address, support).await
   }

   /// The body of [`fetch`], taking an ERC-165 sweep the caller has already paid for.
   ///
   /// [`collections_of`] has to probe a contract to learn it is enumerable; without this it would
   /// sweep every discovered collection a second time.
   async fn with_support<P, N>(
      client: P,
      chain_id: u64,
      address: Address,
      support: Erc165Support,
   ) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let standard = if support.is_erc721() {
         NftStandard::Erc721
      } else if support.is_erc1155() {
         NftStandard::Erc1155
      } else {
         anyhow::bail!("{address} is not an ERC-721 or ERC-1155 contract");
      };

      let name = erc721::collection_name(address, client.clone()).await.ok();
      let symbol = erc721::collection_symbol(address, client).await.ok();

      Ok(Self {
         chain_id,
         address,
         standard,
         name,
         symbol,
      })
   }

   pub fn is_erc721(&self) -> bool {
      self.standard.is_erc721()
   }

   pub fn is_erc1155(&self) -> bool {
      self.standard.is_erc1155()
   }
}

/// One concrete NFT: a collection plus a token id.
///
/// Identity is `(chain_id, collection, token_id)` — metadata is deliberately excluded so a token
/// whose metadata was refreshed or never fetched is still the same token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftToken {
   pub chain_id: u64,
   pub collection: Address,
   pub token_id: U256,
   pub standard: NftStandard,
   /// `tokenURI` (ERC-721) or `uri` (ERC-1155) for **this** token, with any `{id}` placeholder
   /// already expanded. `None` when the contract exposes no metadata.
   pub metadata_uri: Option<String>,
}

impl PartialEq for NftToken {
   fn eq(&self, other: &Self) -> bool {
      self.chain_id == other.chain_id
         && self.collection == other.collection
         && self.token_id == other.token_id
   }
}

impl Eq for NftToken {}

/// Consistent with [`NftToken::eq`] (unlike `ERC20Token`, whose `Ord` only compares the address
/// while `Eq` also compares the chain — keep these in sync here).
impl Ord for NftToken {
   fn cmp(&self, other: &Self) -> Ordering {
      (self.chain_id, self.collection, self.token_id).cmp(&(
         other.chain_id,
         other.collection,
         other.token_id,
      ))
   }
}

impl PartialOrd for NftToken {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
   }
}

impl Hash for NftToken {
   fn hash<H: Hasher>(&self, state: &mut H) {
      self.chain_id.hash(state);
      self.collection.hash(state);
      self.token_id.hash(state);
   }
}

impl NftToken {
   /// Resolve one token of an already-resolved [`NftCollection`].
   ///
   /// Takes the collection rather than a bare address because resolving the standard costs an
   /// ERC-165 sweep, and the standard cannot change per token.
   ///
   /// The metadata call is attempted and tolerated: contracts implement `tokenURI` / `uri`
   /// without advertising the ERC-165 metadata interface, so a gate would drop working metadata.
   pub async fn fetch<P, N>(
      client: P,
      collection: &NftCollection,
      token_id: U256,
   ) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let metadata_uri = match collection.standard {
         NftStandard::Erc721 => erc721::token_uri(collection.address, token_id, client).await.ok(),
         NftStandard::Erc1155 => erc1155::uri(collection.address, token_id, client).await.ok(),
      }
      .filter(|uri| !uri.trim().is_empty())
      .map(|uri| expand_id_placeholder(&uri, token_id));

      Ok(Self {
         chain_id: collection.chain_id,
         collection: collection.address,
         token_id,
         standard: collection.standard,
         metadata_uri,
      })
   }

   pub fn is_erc721(&self) -> bool {
      self.standard.is_erc721()
   }

   pub fn is_erc1155(&self) -> bool {
      self.standard.is_erc1155()
   }

   /// `safeTransferFrom` calldata for this token.
   ///
   /// ERC-721 always moves exactly one token, so `amount` is ignored there; ERC-1155 requires it.
   pub fn encode_transfer(&self, from: Address, to: Address, amount: U256) -> Bytes {
      match self.standard {
         NftStandard::Erc721 => erc721::encode_safe_transfer_from(from, to, self.token_id),
         NftStandard::Erc1155 => {
            erc1155::encode_safe_transfer_from(from, to, self.token_id, amount, Bytes::new())
         }
      }
   }
}

/// Expand an EIP-1155 `{id}` placeholder in a metadata URI.
///
/// The id is substituted as lowercase hex, zero-padded to 64 characters. Matching is
/// case-insensitive because contracts emit `{id}` and `{ID}` both, and clients are expected to
/// handle either.
pub fn expand_id_placeholder(uri: &str, token_id: U256) -> String {
   if !uri.contains('{') {
      return uri.to_string();
   }
   let hex = format!("{token_id:064x}");
   uri.replace("{id}", &hex).replace("{ID}", &hex)
}

/// One collection the owner holds tokens in, as found by enumeration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionHolding {
   pub collection: NftCollection,
   /// Owned token ids, from `tokenOfOwnerByIndex`.
   pub token_ids: Vec<U256>,
}

/// Ceiling on how many ids we will enumerate for one collection.
///
/// `balanceOf` is contract-controlled: a hostile or buggy contract can answer with an enormous
/// number, and sizing an allocation from it would abort the process. Real wallets hold tens.
const MAX_ENUMERATED_TOKENS: u64 = 1_000;

/// What `owner` holds in `candidates`, via the ERC-721 Enumerable path.
///
/// `candidates` is required because Zeus has no indexer: nothing on-chain answers "which
/// collections has this address ever touched". Only collections implementing ERC-721
/// **Enumerable** can turn an owner into a token list, so non-enumerable collections and all
/// ERC-1155s are skipped here — for those, ask about a specific token id with
/// [`verify_ownership`]. Collections the owner holds nothing in are omitted.
///
/// Two steps on purpose: one `balanceOf` filters candidates the owner holds nothing in (the common
/// case) for a single round trip, and only a collection with a non-zero balance pays for the
/// ERC-165 sweep.
pub async fn collections_of<P, N>(
   client: P,
   chain_id: u64,
   owner: Address,
   candidates: &[Address],
) -> Result<Vec<CollectionHolding>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let mut holdings = Vec::new();

   for &candidate in candidates {
      // The core ERC-721 selector. ERC-1155 and ERC-20 contracts do not answer it, so they fall
      // out here for the price of one call instead of a full sweep.
      let Ok(balance) = erc721::balance_of(candidate, owner, client.clone(), None).await else {
         continue;
      };

      if balance.is_zero() {
         continue;
      }

      // Without Enumerable there is no way to get from a count to ids. A collection the node could not
      // be asked about fails the call instead of being skipped: an outage must not read as "this one
      // has no enumerable tokens", or a whole tab of holdings would quietly empty itself.
      let support = erc165::probe(client.clone(), candidate).await?;
      if !support.is_erc721_enumerable() {
         continue;
      }

      let collection =
         NftCollection::with_support(client.clone(), chain_id, candidate, support).await?;

      let wanted = balance.min(U256::from(MAX_ENUMERATED_TOKENS)).to::<u64>() as usize;
      let mut token_ids = Vec::with_capacity(wanted);

      for index in 0..wanted {
         // A revert mid-scan (the collection mutated under us, or an index past the end) ends this
         // collection's enumeration rather than failing the whole call.
         match erc721::token_of_owner_by_index(
            candidate,
            owner,
            U256::from(index),
            client.clone(),
         )
         .await
         {
            Ok(token_id) => token_ids.push(token_id),
            Err(_) => break,
         }
      }

      if !token_ids.is_empty() {
         holdings.push(CollectionHolding {
            collection,
            token_ids,
         });
      }
   }

   Ok(holdings)
}

/// Whether `owner` currently holds `token_id`.
///
/// Works for both standards and needs no ERC-165 answer and no Enumerable support — this is the
/// path for an id the user pasted by hand. A **revert** reads as `false`: for ERC-721 the token
/// does not exist, for ERC-1155 the address is not that standard. A transport failure stays an
/// error, so an RPC outage can never be reported to the user as "you do not own this".
pub async fn verify_ownership<P, N>(
   client: P,
   collection: &NftCollection,
   token_id: U256,
   owner: Address,
) -> Result<bool, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   match collection.standard {
      NftStandard::Erc721 => {
         let contract = erc721::IERC721::new(collection.address, client);
         match contract.ownerOf(token_id).call().await {
            Ok(current) => Ok(current == owner),
            Err(err) if is_revert(&err) => Ok(false),
            Err(err) => Err(err.into()),
         }
      }
      NftStandard::Erc1155 => {
         let contract = erc1155::IERC1155::new(collection.address, client);
         match contract.balanceOf(owner, token_id).call().await {
            Ok(balance) => Ok(!balance.is_zero()),
            Err(err) if is_revert(&err) => Ok(false),
            Err(err) => Err(err.into()),
         }
      }
   }
}

/// Whether a contract call failed by *reverting* rather than by failing to reach the node.
///
/// A revert carries data; a transport failure does not. That is the whole distinction behind
/// [`verify_ownership`]'s "not owned" versus "cannot tell".
fn is_revert(err: &alloy_contract::Error) -> bool {
   err.as_revert_data().is_some()
}

/// What this wallet holds of each of `tokens`, as `(collection, id) -> amount`.
///
/// A `0` (or a missing entry) means the wallet does not hold that token; `None` means the chain
/// could not be asked at all. The two are deliberately different: callers list tokens the wallet may
/// or may not hold, and an RPC hiccup must not be rendered as "not owned".
///
/// One Multicall3 aggregate per standard, because a multicall decodes to a single type — `ownerOf`
/// for the ERC-721 ids and `balanceOf(owner, id)` for the ERC-1155 ones. This is what lets a row
/// show ownership without paying a chain call per row.
pub async fn verify_ownership_batch<P, N>(
   client: P,
   owner: Address,
   tokens: &[NftToken],
) -> Option<HashMap<(Address, U256), u64>>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let refs = |standard: NftStandard| -> Vec<NftRef> {
      tokens
         .iter()
         .filter(|token| token.standard == standard)
         .map(|token| (token.collection, token.token_id))
         .collect()
   };

   let owners = match get_erc721_owners(client.clone(), refs(NftStandard::Erc721), None).await {
      Ok(owners) => owners,
      Err(e) => {
         tracing::error!("Failed to read ERC-721 owners: {e:?}");
         return None;
      }
   };

   let balances = match get_erc1155_balances(client, owner, refs(NftStandard::Erc1155), None).await
   {
      Ok(balances) => balances,
      Err(e) => {
         tracing::error!("Failed to read ERC-1155 balances: {e:?}");
         return None;
      }
   };

   Some(holdings_from(owners, balances, owner))
}

/// Fold the two multicall answers into one `(collection, id) -> amount` map.
///
/// A reverted `ownerOf` — a burned or never-minted id — is a real zero: the contract answered.
/// An ERC-1155 ref with no entry is also a zero, since `get_erc1155_balances` drops only calls that
/// failed, which for a genuine ERC-1155 means the id is simply not held.
fn holdings_from(
   owners: Vec<(Address, U256, Option<Address>)>,
   balances: Vec<(Address, U256, U256)>,
   owner: Address,
) -> HashMap<(Address, U256), u64> {
   let mut holdings = HashMap::new();

   for (collection, token_id, current_owner) in owners {
      holdings.insert(
         (collection, token_id),
         u64::from(current_owner == Some(owner)),
      );
   }

   for (collection, token_id, amount) in balances {
      holdings.insert((collection, token_id), to_u64(amount));
   }

   holdings
}

/// `balanceOf` returns whatever `uint256` it likes, so saturate instead of wrapping.
fn to_u64(amount: U256) -> u64 {
   amount.try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::{address, hex};

   fn token(standard: NftStandard, token_id: u64, metadata_uri: Option<&str>) -> NftToken {
      NftToken {
         chain_id: 1,
         collection: address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D"),
         token_id: U256::from(token_id),
         standard,
         metadata_uri: metadata_uri.map(|s| s.to_string()),
      }
   }

   /// Ownership is "the chain says this wallet", and a reverted `ownerOf` is a real no rather than an
   /// unknown. Amounts survive for ERC-1155, where owning is a quantity, and collapse to 1 for
   /// ERC-721, where it is a flag.
   #[test]
   fn holdings_fold_both_standards() {
      let me = address!("8054f96990662150be89d559Ef249A15809567a9");
      let someone_else = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
      let erc721 = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
      let erc1155 = address!("3E6F909dDBD068c6299ee2A47AD9FE44760D61E0");

      let owners = vec![
         (erc721, U256::from(1), Some(me)),
         (erc721, U256::from(2), Some(someone_else)),
         // Reverted: burned or never minted. The contract answered, so this is a no, not an unknown.
         (erc721, U256::from(3), None),
      ];
      let balances = vec![
         (erc1155, U256::from(1), U256::from(3)),
         (erc1155, U256::from(2), U256::ZERO),
      ];

      let holdings = holdings_from(owners, balances, me);

      assert_eq!(holdings[&(erc721, U256::from(1))], 1);
      assert_eq!(holdings[&(erc721, U256::from(2))], 0);
      assert_eq!(holdings[&(erc721, U256::from(3))], 0);
      assert_eq!(holdings[&(erc1155, U256::from(1))], 3);
      assert_eq!(holdings[&(erc1155, U256::from(2))], 0);
   }

   /// The exact placeholder form the OpenSea shared storefront returns for `uri(1)`.
   #[test]
   fn expands_the_real_storefront_placeholder() {
      let uri =
         "https://api.opensea.io/api/v1/metadata/0x495f947276749Ce646f68AC8c248420045cb7b5e/0x{id}";
      let expanded = expand_id_placeholder(uri, U256::from(1));
      assert_eq!(
         expanded,
         "https://api.opensea.io/api/v1/metadata/0x495f947276749Ce646f68AC8c248420045cb7b5e/0x0000000000000000000000000000000000000000000000000000000000000001"
      );
   }

   #[test]
   fn expands_uppercase_and_leaves_plain_uris_alone() {
      assert_eq!(
         expand_id_placeholder("ipfs://cid/{ID}.json", U256::from(0x2a)),
         format!("ipfs://cid/{:064x}.json", 0x2a)
      );

      let plain = "ipfs://QmeSjSinHpPnmXmspMjwiXyN6zS4E9zccariGR3jxcaWtq/1";
      assert_eq!(expand_id_placeholder(plain, U256::from(1)), plain);
   }

   /// Identity is the token, not its metadata: a refreshed (or not-yet-fetched) URI must not
   /// make the same token compare unequal, or the portfolio would accumulate duplicates.
   #[test]
   fn identity_ignores_metadata() {
      let with_uri = token(NftStandard::Erc721, 1, Some("ipfs://a/1"));
      let without_uri = token(NftStandard::Erc721, 1, None);
      assert_eq!(with_uri, without_uri);

      let different_id = token(NftStandard::Erc721, 2, Some("ipfs://a/1"));
      assert_ne!(with_uri, different_id);

      let mut other_chain = token(NftStandard::Erc721, 1, None);
      other_chain.chain_id = 10;
      assert_ne!(with_uri, other_chain);
      // Ord must agree with Eq, or sorting and dedup disagree.
      assert!(
         with_uri.cmp(&different_id) != Ordering::Equal,
         "Ord must distinguish what Eq distinguishes"
      );
      assert_eq!(with_uri.cmp(&without_uri), Ordering::Equal);
   }

   /// The types are persisted in the sealed wallet state, so they must round-trip.
   #[test]
   fn token_roundtrips_through_json() {
      let original = token(
         NftStandard::Erc1155,
         42,
         Some("https://example.invalid/42.json"),
      );
      let json = serde_json::to_string(&original).unwrap();
      let restored: NftToken = serde_json::from_str(&json).unwrap();
      assert_eq!(original, restored);
      assert_eq!(original.standard, restored.standard);
      assert_eq!(original.metadata_uri, restored.metadata_uri);
   }

   /// Transfer calldata must go to the right standard, and carry the id (and amount) correctly.
   #[test]
   fn encode_transfer_targets_the_right_standard() {
      let from = address!("00000000000000000000000000000000000000aa");
      let to = address!("00000000000000000000000000000000000000bb");

      let erc721 = token(NftStandard::Erc721, 7, None);
      let data = erc721.encode_transfer(from, to, U256::from(1));
      assert_eq!(&data[..4], &hex!("42842e0e"));
      let (d_from, d_to, id) = erc721::decode_safe_transfer_from_call(&data).unwrap();
      assert_eq!((d_from, d_to, id), (from, to, U256::from(7)));

      let erc1155 = token(NftStandard::Erc1155, 7, None);
      let data = erc1155.encode_transfer(from, to, U256::from(3));
      assert_eq!(&data[..4], &hex!("f242432a"));
      let (d_from, d_to, id, amount, payload) =
         erc1155::decode_safe_transfer_from_call(&data).unwrap();
      assert_eq!(
         (d_from, d_to, id, amount),
         (from, to, U256::from(7), U256::from(3))
      );
      assert!(payload.is_empty());
   }

   #[test]
   fn standard_display_is_ui_ready() {
      assert_eq!(NftStandard::Erc721.to_string(), "ERC-721");
      assert_eq!(NftStandard::Erc1155.to_string(), "ERC-1155");
   }

   /// Live check against mainnet. Ignored by default — see `crate::test_utils`.
   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn fetches_a_real_collection_and_token() {
      use alloy_provider::ProviderBuilder;

      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      // BAYC. Its on-chain name really is the unspaced "BoredApeYachtClub".
      let bayc = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
      let collection = NftCollection::fetch(client.clone(), 1, bayc).await.unwrap();
      assert_eq!(collection.standard, NftStandard::Erc721);
      assert_eq!(collection.symbol.as_deref(), Some("BAYC"));
      assert_eq!(
         collection.name.as_deref(),
         Some("BoredApeYachtClub")
      );

      let token = NftToken::fetch(client.clone(), &collection, U256::from(1)).await.unwrap();
      assert_eq!(
         token.metadata_uri.as_deref(),
         Some("ipfs://QmeSjSinHpPnmXmspMjwiXyN6zS4E9zccariGR3jxcaWtq/1")
      );

      // A pasted ERC-20 address must not become a collection.
      let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
      assert!(
         NftCollection::fetch(client, 1, weth).await.is_err(),
         "an ERC-20 must be rejected as a collection"
      );
   }

   /// Live check of the discovery helpers. Ignored by default — see `crate::test_utils`.
   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn discovers_and_verifies_against_mainnet() {
      use alloy_provider::ProviderBuilder;

      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      let bayc = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
      let storefront = address!("495f947276749Ce646f68AC8c248420045cb7b5e");
      let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
      // Holds exactly one BAYC (token #1). Also offered WETH — an ERC-20 that answers
      // balanceOf(address), so it must be rejected — and the ERC-1155 storefront, which does not
      // answer that selector at all. This owner has no WETH, so both exit at the cheap balance
      // filter; the non-zero-balance path is covered at the end.
      let holder = address!("46efbaedc92067e6d60e84ed6395099723252496");

      let started = std::time::Instant::now();
      let holdings = collections_of(
         client.clone(),
         1,
         holder,
         &[bayc, storefront, weth],
      )
      .await
      .unwrap();
      eprintln!(
         "collections_of over 3 candidates took {:?}",
         started.elapsed()
      );

      assert_eq!(holdings.len(), 1, "only BAYC should enumerate");
      assert_eq!(holdings[0].collection.address, bayc);
      assert_eq!(
         holdings[0].collection.standard,
         NftStandard::Erc721
      );
      assert_eq!(
         holdings[0].collection.symbol.as_deref(),
         Some("BAYC")
      );
      assert_eq!(holdings[0].token_ids, vec![U256::from(1)]);

      let collection = &holdings[0].collection;

      // Owner of #1 holds it.
      assert!(
         verify_ownership(client.clone(), collection, U256::from(1), holder)
            .await
            .unwrap()
      );
      // The owner of #9999 does not hold #1 — a plain false, no revert involved.
      let someone_else = address!("37f11f9d0749a053dfe6243a4c1d294ea293ec12");
      assert!(
         !verify_ownership(
            client.clone(),
            collection,
            U256::from(1),
            someone_else
         )
         .await
         .unwrap()
      );
      // A nonexistent token reverts ("owner query for nonexistent token") and must read as false
      // rather than surfacing an error the UI would have to explain.
      assert!(
         !verify_ownership(
            client.clone(),
            collection,
            U256::from(1_000_000_000),
            holder
         )
         .await
         .unwrap()
      );

      // ERC-1155 arm, cross-checked against the same balance read through the batch helper: two
      // independent paths to the same chain state must agree.
      let vitalik = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
      let storefront_collection = NftCollection {
         chain_id: 1,
         address: storefront,
         standard: NftStandard::Erc1155,
         name: None,
         symbol: None,
      };
      let via_batch = crate::utils::batch::get_erc1155_balances(
         client.clone(),
         vitalik,
         vec![(storefront, U256::from(1))],
         None,
      )
      .await
      .unwrap();
      let via_verify = verify_ownership(
         client.clone(),
         &storefront_collection,
         U256::from(1),
         vitalik,
      )
      .await
      .unwrap();

      assert_eq!(
         via_verify,
         !via_batch[0].2.is_zero(),
         "verify_ownership must agree with balanceOf"
      );

      // An ERC-20 the owner *does* hold must still not be discovered as a collection: a non-zero
      // balance carries the candidate past the cheap filter and into the ERC-165 sweep, which is
      // what has to reject it. Vitalik holds WETH, unlike the `holder` above.
      let erc20_only = collections_of(client, 1, vitalik, &[weth]).await.unwrap();
      assert!(
         erc20_only.is_empty(),
         "an ERC-20 must never be discovered as a collection"
      );
   }
}
