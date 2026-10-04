use super::{ExecutionResult, revert_msg};
use alloy_primitives::{Address, Bytes, TxKind, U256};

use super::Evm2;
use anyhow::anyhow;
use revm::{DatabaseCommit, ExecuteCommitEvm, ExecuteEvm, database::Database};

use crate::abi::{
   self,
   uniswap::nft_position::{INonfungiblePositionManager, encode_decrease_liquidity},
};

/// The output of a call that **had to succeed**, or the revert it actually produced.
///
/// Reading a revert as a value is the trap this closes. `ExecutionResult::output` returns `Some`
/// for a revert as well as a success, and the generated decoders are *non-validating*, so a
/// reverted call's payload is read as an answer: OpenZeppelin's `ERC721NonexistentToken(uint256)`
/// reverts with 36 bytes — a selector and one word — and `ownerOf` on a burned token therefore
/// came back as a plausible-looking owner address instead of an error.
///
/// A caller that only wants a measurement gets none, which is the honest answer; anything that
/// needs to tell "the token does not exist" from "the read failed" has to distinguish it at the
/// call site.
fn ok_output<'a>(result: &'a ExecutionResult, call: &str) -> Result<&'a Bytes, anyhow::Error> {
   let output = result.output().ok_or_else(|| anyhow!("{call} produced no output"))?;

   if !result.is_success() {
      return Err(anyhow!("{call} reverted: {}", revert_msg(output)));
   }

   Ok(output)
}

/// Simulate ERC-721 `ownerOf(tokenId)` (does not commit).
///
/// The read half of a "did the transfer actually move it" check: called after
/// `simulate_transaction` the fork already reflects the transfer, called before it reports the
/// pre-state.
pub fn erc721_owner_of<DB>(
   evm: &mut Evm2<DB>,
   collection: Address,
   token_id: U256,
) -> Result<Address, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc721::encode_owner_of(token_id);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(collection);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = ok_output(&res.result, "ownerOf")?;
   let owner = abi::erc721::decode_owner_of(output)?;
   Ok(owner)
}

/// Simulate ERC-721 `getApproved(tokenId)` (does not commit).
///
/// The per-token approval slot — and the *single* slot that approving, switching and revoking all
/// write, so a revoke reads back as the zero address rather than as a missing value.
pub fn erc721_get_approved<DB>(
   evm: &mut Evm2<DB>,
   collection: Address,
   token_id: U256,
) -> Result<Address, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc721::encode_get_approved(token_id);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(collection);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = ok_output(&res.result, "getApproved")?;
   abi::erc721::decode_get_approved(output)
}

/// Simulate `isApprovedForAll(owner, operator)` (does not commit).
///
/// One helper for both standards on purpose: the function and its answer are byte-identical between
/// ERC-721 and ERC-1155, so which one a collection is never enters into the read.
pub fn erc721_is_approved_for_all<DB>(
   evm: &mut Evm2<DB>,
   collection: Address,
   owner: Address,
   operator: Address,
) -> Result<bool, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc721::encode_is_approved_for_all(owner, operator);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(collection);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = ok_output(&res.result, "isApprovedForAll")?;
   abi::erc721::decode_is_approved_for_all(output)
}

/// Simulate the ERC-5216 `allowance(account, operator, id)` (does not commit).
///
/// ERC-5216's allowance, not ERC-1155's `balanceOf`: it is the per-id, per-operator approval an
/// ERC-1155 can grant, and it is the third of the three shapes an NFT approval takes.
pub fn erc1155_allowance<DB>(
   evm: &mut Evm2<DB>,
   collection: Address,
   account: Address,
   operator: Address,
   id: U256,
) -> Result<U256, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc1155::encode_allowance(account, operator, id);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(collection);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = ok_output(&res.result, "ERC-5216 allowance")?;
   abi::erc1155::decode_allowance(output)
}

/// Simulate ERC-1155 `balanceOf(account, id)` (does not commit).
pub fn erc1155_balance_of<DB>(
   evm: &mut Evm2<DB>,
   collection: Address,
   account: Address,
   id: U256,
) -> Result<U256, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc1155::encode_balance_of(account, id);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(collection);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = ok_output(&res.result, "ERC-1155 balanceOf")?;
   let balance = abi::erc1155::decode_balance_of(output)?;
   Ok(balance)
}

/// Simulate the balance of function of the ERC20 contract
pub fn erc20_balance<DB>(
   evm: &mut Evm2<DB>,
   token: Address,
   owner: Address,
) -> Result<U256, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc20::encode_balance_of(owner);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = res.result.output().ok_or(anyhow!("Output not found"))?;
   let balance = abi::erc20::decode_balance_of(output)?;
   Ok(balance)
}

/// Simulate ERC-20 `allowance(owner, spender)` (does not commit).
pub fn erc20_allowance<DB>(
   evm: &mut Evm2<DB>,
   token: Address,
   owner: Address,
   spender: Address,
) -> Result<U256, anyhow::Error>
where
   DB: Database,
{
   let data = abi::erc20::encode_allowance(owner, spender);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = res.result.output().ok_or(anyhow!("Output not found"))?;
   abi::erc20::decode_allowance(output)
}

/// Simulate Permit2 `allowance(user, token, spender)` (does not commit).
///
/// Returns `(amount, expiration)`.
pub fn permit2_allowance<DB>(
   evm: &mut Evm2<DB>,
   permit2: Address,
   owner: Address,
   token: Address,
   spender: Address,
) -> Result<(U256, u64), anyhow::Error>
where
   DB: Database,
{
   let data = abi::permit::encode_allowance(owner, token, spender);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(permit2);

   let res = evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = res.result.output().ok_or(anyhow!("Output not found"))?;
   let (amount, expiration, _nonce) = abi::permit::decode_allowance(output)?;
   Ok((amount, expiration))
}

/// Simulate the transfer function in the ERC20 contract
pub fn transfer_token<DB>(
   evm: &mut Evm2<DB>,
   token: Address,
   from: Address,
   to: Address,
   amount: U256,
   commit: bool,
) -> Result<ExecutionResult, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::erc20::encode_transfer(to, amount);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = from;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = if commit {
      evm.transact_commit(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?.result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Failed to transfer token: {}", err));
   }

   Ok(res)
}

/// Simulate the approve function in the ERC20 contract
pub fn approve_token<DB>(
   evm: &mut Evm2<DB>,
   token: Address,
   owner: Address,
   spender: Address,
   amount: U256,
) -> Result<ExecutionResult, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::erc20::encode_approve(spender, amount);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = owner;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(token);

   let res = evm.transact_commit(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?;
   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Failed to approve token: {}", err));
   }

   Ok(res)
}

/// Simulate the mint function in the [INonfungiblePositionManager] contract
pub fn mint_position<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::MintParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<
   (
      ExecutionResult,
      INonfungiblePositionManager::mintReturn,
   ),
   anyhow::Error,
>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_mint(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?.result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      eprintln!(
         "Failed to mint position: {} Gas Used: {}",
         err,
         res.tx_gas_used()
      );
      return Err(anyhow!("Failed to mint position: {}", err));
   }

   let mint = abi::uniswap::nft_position::decode_mint_call(output)?;
   Ok((res, mint))
}

/// Simulate the increase liquidity function in the [INonfungiblePositionManager] contract
///
/// ## Returns
///
/// - The execution result
/// - The liquidity that was minted
/// - The amount0 that was minted
/// - The amount1 that was minted
pub fn increase_liquidity<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::IncreaseLiquidityParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<(ExecutionResult, u128, U256, U256), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_increase_liquidity(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?.result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Call Reverted: {}", err));
   }

   let (liquidity, amount0, amount1) =
      abi::uniswap::nft_position::decode_increase_liquidity_call(output)?;
   Ok((res, liquidity, amount0, amount1))
}

/// Simulate the decrease liquidity function in the [INonfungiblePositionManager] contract
pub fn decrease_liquidity<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::DecreaseLiquidityParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<(ExecutionResult, U256, U256), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = encode_decrease_liquidity(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?.result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Call Reverted: {}", err));
   }

   let (amount0, amount1) = abi::uniswap::nft_position::decode_decrease_liquidity_call(output)?;
   Ok((res, amount0, amount1))
}

/// Simulate the collect function in the [INonfungiblePositionManager] contract
///
/// Returns the amount0 and amount1 that were collected
pub fn collect_fees<DB>(
   evm: &mut Evm2<DB>,
   params: INonfungiblePositionManager::CollectParams,
   caller: Address,
   contract: Address,
   commit: bool,
) -> Result<(ExecutionResult, U256, U256), anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   let data = abi::uniswap::nft_position::encode_collect(params);

   evm.tx.chain_id = Some(evm.cfg.chain_id);
   evm.tx.caller = caller;
   evm.tx.data = data;
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Call(contract);

   let res = if commit {
      evm.transact_commit(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?
   } else {
      evm.transact(evm.tx.clone()).map_err(|e| anyhow!("{:?}", e))?.result
   };

   let output = res.output().ok_or(anyhow!("Output not found"))?;

   if !res.is_success() {
      let err = revert_msg(output);
      return Err(anyhow!("Failed to collect fees: {}", err));
   }

   let (amount0, amount1) = abi::uniswap::nft_position::decode_collect(output)?;
   Ok((res, amount0, amount1))
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{
      abi::{erc721::IERC721, erc1155::IERC1155},
      revm_utils::{ForkFactory, Output, new_evm},
      test_utils::rpc_url,
      types::ChainId,
   };
   use alloy_primitives::address;
   use alloy_provider::{Provider, ProviderBuilder};
   use alloy_rpc_types::BlockId;
   use revm::context_interface::result::{ResultGas, SuccessReason};

   /// The reads the NFT send path's transfer check makes, executed on a real fork and compared with the
   /// node's own answer **at the same block**.
   ///
   /// The two paths are independent — revm executing against forked state, and the node answering a
   /// plain `eth_call` — so agreement is what proves the helpers. They need live contracts, which is
   /// exactly why a mock would be worthless here: it could only hand my own encoding back to me.
   ///
   /// Ignored by default — see [`crate::test_utils`].
   ///
   /// The fork backend blocks on its own thread, which the current-thread runtime cannot serve — hence
   /// the multi-threaded flavour.
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   #[ignore = "needs an RPC that serves eth_call and eth_getCode"]
   async fn fork_reads_match_a_direct_call_at_the_same_block() {
      let client = ProviderBuilder::new().connect_http(rpc_url());

      let block_id = BlockId::latest();
      let block = client.get_block(block_id).await.unwrap().unwrap();
      let chain = ChainId::eth();

      // BAYC, whose #1 is owned by `holder`, and the OpenSea storefront, a live ERC-1155.
      let bayc = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
      let storefront = address!("495f947276749Ce646f68AC8c248420045cb7b5e");
      let holder = address!("46efbaedc92067e6d60e84ed6395099723252496");

      let factory =
         ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, Some(block_id));
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain, Some(&block), fork_db);

      // ERC-721 `ownerOf`.
      let from_fork = erc721_owner_of(&mut evm, bayc, U256::from(1)).unwrap();
      let from_node = IERC721::new(bayc, client.clone())
         .ownerOf(U256::from(1))
         .block(block_id)
         .call()
         .await
         .unwrap();

      assert_eq!(
         from_fork, from_node,
         "the fork and the node must agree"
      );
      assert_eq!(
         from_fork, holder,
         "BAYC #1's owner is a known fact"
      );

      // ERC-1155 `balanceOf`. Ids the holder owns none of are included on purpose: zero is the common
      // answer and must not be confusable with a failed read.
      for id in [U256::ZERO, U256::from(1), U256::from(2)] {
         let from_fork = erc1155_balance_of(&mut evm, storefront, holder, id).unwrap();
         let from_node = IERC1155::new(storefront, client.clone())
            .balanceOf(holder, id)
            .block(block_id)
            .call()
            .await
            .unwrap();

         assert_eq!(
            from_fork, from_node,
            "storefront balance of id {id}"
         );
      }

      // ERC-721 `getApproved` — the per-token approval slot, which the diff probes before and after.
      for id in [U256::from(1), U256::from(9999)] {
         let from_fork = erc721_get_approved(&mut evm, bayc, id).unwrap();
         let from_node = IERC721::new(bayc, client.clone())
            .getApproved(id)
            .block(block_id)
            .call()
            .await
            .unwrap();

         assert_eq!(
            from_fork, from_node,
            "BAYC #{id} approved address"
         );
      }

      // `isApprovedForAll`, on a real operator. Its own answer is irrelevant here — the two paths
      // agreeing is the assertion, and a `false` is as good a proof as a `true`.
      let seaport = address!("00000000000000ADc04C56Bf30aC9d3c0aAF14dC");

      for (collection, owner) in [(bayc, holder), (storefront, holder)] {
         let from_fork = erc721_is_approved_for_all(&mut evm, collection, owner, seaport).unwrap();
         let from_node = IERC721::new(collection, client.clone())
            .isApprovedForAll(owner, seaport)
            .block(block_id)
            .call()
            .await
            .unwrap();

         assert_eq!(
            from_fork, from_node,
            "isApprovedForAll({collection})"
         );
      }

      // ERC-5216 `allowance`: nothing on mainnet implements it, and this is the assertion worth
      // pinning — a collection that does not implement it **reverts**, so the probe must surface an
      // error rather than hand back a zero that would read as "not approved" in the diff.
      assert!(
         erc1155_allowance(
            &mut evm,
            storefront,
            holder,
            seaport,
            U256::from(1)
         )
         .is_err(),
         "a collection without ERC-5216 must revert, not answer zero"
      );
   }

   /// A revert must never be read as an answer, however decodable its payload looks.
   ///
   /// `ExecutionResult::output` returns `Some` for a revert too, and the generated decoders are
   /// non-validating, so before the guard a burned token's `ownerOf` — which reverts with
   /// OpenZeppelin's `ERC721NonexistentToken(uint256)`, a selector and one word — came back as a
   /// plausible owner address. That the payload *does* decode is what gives the guard its point.
   #[test]
   fn a_revert_is_never_read_as_a_value() {
      // Selector + one word: the 36 bytes an OZ custom error reverts with.
      let mut payload = vec![0x7e, 0x27, 0x30, 0xff];
      payload.extend_from_slice(&[0xab; 32]);
      let output = Bytes::from(payload);

      assert_eq!(
         abi::erc721::decode_owner_of(&output).unwrap(),
         Address::from([0xab; 20]),
         "the payload decodes as an owner — that is the trap, not a hypothetical"
      );

      let reverted = ExecutionResult::Revert {
         gas: ResultGas::default(),
         logs: Vec::new(),
         output,
      };
      assert!(
         ok_output(&reverted, "ownerOf").is_err(),
         "a revert is an error, not a decoded owner"
      );
   }

   /// A successful call still yields its output, so the guard is not a blanket refusal.
   #[test]
   fn a_successful_call_still_yields_its_output() {
      let mut word = vec![0u8; 12];
      word.extend_from_slice(&[0xcd; 20]);
      let word = Bytes::from(word);

      let success = ExecutionResult::Success {
         reason: SuccessReason::Stop,
         gas: ResultGas::default(),
         logs: Vec::new(),
         output: Output::Call(word.clone()),
      };

      assert_eq!(
         ok_output(&success, "ownerOf").unwrap(),
         &word,
         "a success must pass its output through"
      );
   }
}
