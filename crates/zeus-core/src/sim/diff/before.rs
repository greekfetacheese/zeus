//! Fetch the "before" side over RPC at the transaction's pinned block.

use super::probe::{ALLOWANCE_PAIR_BATCH, TOKEN_BALANCE_BATCH};
use super::state::{BeforeState, NftRequest, NftState};
use crate::ZeusCtx;
use crate::sim::approval_diff::{NftApprovalCandidate, NftApprovalTarget, NftApprovalValue};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::alloy_rpc_types::BlockId;
use zeus_eth::nft::NftStandard;
use zeus_eth::utils::address_book;
use zeus_eth::utils::batch;
use zeus_eth::utils::batch::NftRef;

// ! This may return an empty or partial map if any requests fail
async fn fetch_token_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   tokens: Vec<Address>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<Address, U256> {
   if tokens.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in tokens.chunks(TOKEN_BALANCE_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_erc20_balances(client, chain, Some(block_id), from, chunk).await
            })
            .await
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok(rows)) => {
            for row in rows {
               out.insert(row.token, row.balance);
            }
         }
         Ok(Err(e)) => tracing::warn!("ERC-20 balances at block failed: {:?}", e),
         Err(e) => tracing::warn!("ERC-20 balances at block failed: {e:?}"),
      }
   }

   out
}

// ! This may return an empty or partial map if any requests fail
async fn fetch_erc20_allowance_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   pairs: Vec<(Address, Address)>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<(Address, Address), U256> {
   if pairs.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in pairs.chunks(ALLOWANCE_PAIR_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_erc20_allowances(client, from, chunk, Some(block_id)).await
            })
            .await
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok(rows)) => {
            for (token, spender, amount) in rows {
               out.insert((token, spender), amount);
            }
         }
         Ok(Err(e)) => tracing::warn!("ERC-20 allowances at block failed: {:?}", e),
         Err(e) => tracing::warn!("ERC-20 allowances at block failed: {e:?}"),
      }
   }

   out
}

// ! This may return an empty or partial map if any requests fail
async fn fetch_permit2_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   pairs: Vec<(Address, Address)>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<(Address, Address), (U256, u64)> {
   if pairs.is_empty() {
      return HashMap::new();
   }

   let Some(permit2) = address_book::permit2_contract(chain).ok() else {
      return HashMap::new();
   };

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in pairs.chunks(ALLOWANCE_PAIR_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_permit2_allowances(client, permit2, from, chunk, Some(block_id)).await
            })
            .await
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok(rows)) => {
            for (token, spender, amount, expiration) in rows {
               out.insert((token, spender), (amount, expiration));
            }
         }
         Ok(Err(e)) => tracing::warn!("Multicall3 Permit2 allowances failed: {:?}", e),
         Err(e) => tracing::warn!("Multicall3 Permit2 allowances failed: {e:?}"),
      }
   }

   out
}

/// Max NFT `(collection, id)` refs per Multicall3 aggregate so the eth_call stays under gas limits.
const NFT_REF_BATCH: usize = 20;

// ! Each of these may return an empty or partial map if any requests fail. A reverted *call*, on the
// ! other hand, is the contract answering — and each answers with the shape's own "nothing", so a
// ! candidate whose call reverted still gets an entry rather than silently dropping out of the diff.

/// ERC-721 ownership at `block_id`, as a count of what the signer holds (`0` or `1`).
async fn fetch_nft_ownership_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   refs: Vec<NftRef>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<(Address, U256), U256> {
   if refs.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in refs.chunks(NFT_REF_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_erc721_owners(client, chunk, Some(block_id)).await
            })
            .await
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok(rows)) => {
            for (collection, token_id, owner) in rows {
               // A reverted `ownerOf` says the token has no owner — "not the signer's" is the answer
               // the row needs, not a hole in the map.
               out.insert(
                  (collection, token_id),
                  U256::from(u8::from(owner == Some(from))),
               );
            }
         }
         Ok(Err(e)) => tracing::warn!("ERC-721 owners at block failed: {:?}", e),
         Err(e) => tracing::warn!("ERC-721 owners at block failed: {e:?}"),
      }
   }

   out
}

/// ERC-1155 balances at `block_id`.
async fn fetch_nft_balances_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   refs: Vec<NftRef>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<(Address, U256), U256> {
   if refs.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in refs.chunks(NFT_REF_BATCH) {
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         client
            .request_with(chain, chunk.clone(), |client, chunk| async move {
               batch::get_erc1155_balances(client, from, chunk, Some(block_id)).await
            })
            .await
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok(Ok(rows)) => {
            for (collection, token_id, balance) in rows {
               if let Some(balance) = balance {
                  out.insert((collection, token_id), balance);
               }
            }
         }
         Ok(Err(e)) => tracing::warn!("ERC-1155 balances at block failed: {:?}", e),
         Err(e) => tracing::warn!("ERC-1155 balances at block failed: {e:?}"),
      }
   }

   out
}

/// ERC-721 per-token approvals at `block_id`, for the `Token(id)` candidates.
async fn fetch_nft_token_approvals_before(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   candidates: Vec<NftApprovalCandidate>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<NftApprovalCandidate, NftApprovalValue> {
   if candidates.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in candidates.chunks(NFT_REF_BATCH) {
      debug_assert!(
         chunk
            .iter()
            .all(|candidate| matches!(candidate.target, NftApprovalTarget::Token(_))),
         "this fetch is only handed per-token candidates"
      );

      let refs: Vec<NftRef> = chunk
         .iter()
         .filter_map(|candidate| match candidate.target {
            NftApprovalTarget::Token(id) => Some((candidate.collection, id)),
            _ => None,
         })
         .collect();
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         let rows = client
            .request_with(chain, refs.clone(), |client, refs| async move {
               batch::get_erc721_approved(client, refs, Some(block_id)).await
            })
            .await;

         (rows, chunk)
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         // The helper returns one row per ref, in order, so the zip keeps each answer with its own
         // candidate.
         Ok((Ok(rows), chunk)) => {
            for (row, candidate) in rows.into_iter().zip(chunk) {
               // A reverted `getApproved` is the zero address: no approval, which a mint that also
               // approves someone still shows up as a change from.
               out.insert(
                  candidate,
                  NftApprovalValue::Approved(row.2.unwrap_or_default()),
               );
            }
         }
         Ok((Err(e), _)) => tracing::warn!("ERC-721 approvals at block failed: {:?}", e),
         Err(e) => tracing::warn!("ERC-721 approvals at block failed: {e:?}"),
      }
   }

   out
}

/// `isApprovedForAll` at `block_id`, for the `ForAll` candidates.
async fn fetch_nft_for_all_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   candidates: Vec<NftApprovalCandidate>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<NftApprovalCandidate, NftApprovalValue> {
   if candidates.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in candidates.chunks(NFT_REF_BATCH) {
      let targets: Vec<(Address, Address)> = chunk
         .iter()
         .map(|candidate| (candidate.collection, candidate.operator))
         .collect();
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         let rows = client
            .request_with(
               chain,
               targets.clone(),
               |client, targets| async move {
                  batch::get_erc721_is_approved_for_all(client, from, targets, Some(block_id)).await
               },
            )
            .await;

         (rows, chunk)
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok((Ok(rows), chunk)) => {
            for (row, candidate) in rows.into_iter().zip(chunk) {
               out.insert(
                  candidate,
                  NftApprovalValue::ForAll(row.2.unwrap_or(false)),
               );
            }
         }
         Ok((Err(e), _)) => tracing::warn!("isApprovedForAll at block failed: {:?}", e),
         Err(e) => tracing::warn!("isApprovedForAll at block failed: {e:?}"),
      }
   }

   out
}

/// ERC-5216 allowances at `block_id`, for the `Allowance(id)` candidates.
async fn fetch_nft_allowances_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   candidates: Vec<NftApprovalCandidate>,
   semaphore: &Arc<Semaphore>,
) -> HashMap<NftApprovalCandidate, NftApprovalValue> {
   if candidates.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut tasks = Vec::new();

   for chunk in candidates.chunks(NFT_REF_BATCH) {
      let refs: Vec<(Address, Address, U256)> = chunk
         .iter()
         .filter_map(|candidate| match candidate.target {
            NftApprovalTarget::Allowance(id) => {
               Some((candidate.collection, candidate.operator, id))
            }
            _ => None,
         })
         .collect();
      let chunk = chunk.to_vec();
      let client = client.clone();
      let semaphore = Arc::clone(semaphore);

      tasks.push(tokio::spawn(async move {
         let _permit = semaphore.acquire().await.expect("the fetch semaphore is never closed");

         let rows = client
            .request_with(chain, refs.clone(), |client, refs| async move {
               batch::get_erc1155_allowances(client, from, refs, Some(block_id)).await
            })
            .await;

         (rows, chunk)
      }));
   }

   let mut out = HashMap::new();
   for task in tasks {
      match task.await {
         Ok((Ok(rows), chunk)) => {
            for (row, candidate) in rows.into_iter().zip(chunk) {
               out.insert(
                  candidate,
                  NftApprovalValue::Allowance(row.3.unwrap_or_default()),
               );
            }
         }
         Ok((Err(e), _)) => tracing::warn!("ERC-5216 allowances at block failed: {:?}", e),
         Err(e) => tracing::warn!("ERC-5216 allowances at block failed: {e:?}"),
      }
   }

   out
}

/// The NFT half of the "before" state.
async fn fetch_nft_state(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   nft: &NftRequest,
   semaphore: &Arc<Semaphore>,
) -> NftState {
   if nft.is_empty() {
      return NftState::default();
   }

   let mut refs721 = Vec::new();
   let mut refs1155 = Vec::new();
   for candidate in &nft.balances {
      match candidate.standard {
         NftStandard::Erc721 => refs721.push((candidate.collection, candidate.token_id)),
         NftStandard::Erc1155 => refs1155.push((candidate.collection, candidate.token_id)),
      }
   }

   let mut token_approvals = Vec::new();
   let mut for_all = Vec::new();
   let mut allowances = Vec::new();
   for candidate in &nft.approvals {
      match candidate.target {
         NftApprovalTarget::Token(_) => token_approvals.push(*candidate),
         NftApprovalTarget::ForAll => for_all.push(*candidate),
         NftApprovalTarget::Allowance(_) => allowances.push(*candidate),
      }
   }

   let (ownership721, balances1155, token_apps, for_all_apps, allowance_apps) = tokio::join!(
      fetch_nft_ownership_before(
         ctx.clone(),
         chain,
         from,
         block_id,
         refs721,
         semaphore
      ),
      fetch_nft_balances_before(
         ctx.clone(),
         chain,
         from,
         block_id,
         refs1155,
         semaphore
      ),
      fetch_nft_token_approvals_before(
         ctx.clone(),
         chain,
         block_id,
         token_approvals,
         semaphore
      ),
      fetch_nft_for_all_before(
         ctx.clone(),
         chain,
         from,
         block_id,
         for_all,
         semaphore
      ),
      fetch_nft_allowances_before(ctx, chain, from, block_id, allowances, semaphore),
   );

   let mut approvals = token_apps;
   approvals.extend(for_all_apps);
   approvals.extend(allowance_apps);

   NftState {
      ownership721,
      balances1155,
      approvals,
   }
}

pub(super) async fn fetch_before_state(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   tokens: Vec<Address>,
   erc20_pairs: Vec<(Address, Address)>,
   permit2_pairs: Vec<(Address, Address)>,
   nft: NftRequest,
   semaphore: Arc<Semaphore>,
) -> BeforeState {
   let time = Instant::now();

   let tokens_fut = fetch_token_before(
      ctx.clone(),
      chain,
      from,
      block_id,
      tokens,
      &semaphore,
   );
   let erc20_fut = fetch_erc20_allowance_before(
      ctx.clone(),
      chain,
      from,
      block_id,
      erc20_pairs,
      &semaphore,
   );
   let permit2_fut = fetch_permit2_before(
      ctx.clone(),
      chain,
      from,
      block_id,
      permit2_pairs,
      &semaphore,
   );
   let nft_fut = fetch_nft_state(ctx, chain, from, block_id, &nft, &semaphore);

   let (tokens, erc20, permit2, nft) = tokio::join!(tokens_fut, erc20_fut, permit2_fut, nft_fut);

   tracing::info!(
      "fetch_before_state took {} ms",
      time.elapsed().as_millis()
   );

   BeforeState {
      tokens,
      erc20,
      permit2,
      nft,
   }
}
