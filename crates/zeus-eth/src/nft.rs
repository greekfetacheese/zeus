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

use crate::abi::{erc165, erc721, erc1155};
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, U256};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
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
      let support = erc165::probe(client.clone(), address).await;

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
}
