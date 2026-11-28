//! The money flows, as the agent pipeline calls them.
//!
//! The pipeline in `zeus_core::agent::exec` decides *whether* an action runs — the locks, the grant, the
//! allowlist, the screen, the record. This is what runs it: the app's own send and revoke flows, with the
//! grant standing in for the click. Nothing here decides anything. It resolves what a tool named into the
//! types the flows take, and turns their sentences into the codes a caller branches on.

use crate::core::agent::exec::{
   DelegateRequest, ExecError, MergeNotesRequest, NftMovement, NftSendRequest, RevokeRequest,
   RevokeTarget, SendRequest, ShieldRequest, UnshieldRequest,
};
use crate::core::tx::events::NftApproveParams;
use crate::core::tx::{TransactionRich, TxRefusal, delegate_to};
use crate::core::types::BundlerUrl;
use crate::core::{AgentEnv, ClientKind, ZeusCtx};
use crate::gui::ui::approvals::{revoke_erc20_approval, revoke_nft_approval};
use crate::gui::ui::dapps::railgun::unshield::unshield as unshield_flow;
use crate::gui::ui::dapps::railgun::{
   RailgunAsset, default_bundler_url, private_merge_notes, shield::shield as shield_flow,
};
use crate::gui::ui::send_crypto::{send_eth, send_nft as send_nft_flow, send_token};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::currency::{Currency, ERC20Token, NativeCurrency};
use zeus_eth::nft::{NftStandard, NftToken};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

/// Send the chain's own coin, or the token a call named.
pub async fn send(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: SendRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = chain(request.chain)?;
   let amount = NumericValue::format_wei(request.amount, request.decimals);

   let currency = match request.token {
      None => Currency::from(NativeCurrency::from(chain.id())),
      Some(address) => {
         let token = erc20(&ctx, chain, request.from, address).await?;

         // The one thing that cannot be guessed: a decimals count is the scale the amount is read in, so a
         // request that states the wrong one would send an amount three orders of magnitude out.
         if token.decimals != request.decimals {
            return Err(ExecError::Invalid(format!(
               "{address:#x} has {} decimals, not {}: the amount would be scaled wrong",
               token.decimals, request.decimals
            )));
         }

         Currency::from(token)
      }
   };

   let sent = if currency.is_native() {
      send_eth(
         ctx,
         env,
         chain,
         request.from,
         request.to,
         amount,
         currency,
         None,
         None,
      )
      .await
   } else {
      send_token(
         ctx,
         env,
         chain,
         request.from,
         request.to,
         currency,
         amount,
         None,
         None,
      )
      .await
   };

   sent.map_err(classify)
}

/// Send one NFT, as the call named it.
pub async fn send_nft(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: NftSendRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = chain(request.chain)?;

   let nft = NftToken {
      chain_id: request.chain,
      collection: request.collection,
      token_id: request.token_id,
      standard: request.standard,
      // No metadata is read here: what moves is the id, and the flow does not need its art to move it.
      metadata_uri: None,
   };

   send_nft_flow(
      ctx,
      env,
      chain,
      request.from,
      request.to,
      nft,
      request.amount,
      None,
      None,
   )
   .await
   .map_err(classify)
}

/// Clear one approval, in the shape the call named.
pub async fn revoke(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: RevokeRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = chain(request.chain)?;

   match request.target {
      RevokeTarget::Token { token, spender } => {
         let token = erc20(&ctx, chain, request.owner, token).await?;

         revoke_erc20_approval(env, request.chain, token, request.owner, spender)
            .await
            .map_err(classify)
      }
      RevokeTarget::Nft {
         collection,
         operator,
         token_id,
         standard,
      } => {
         // The three shapes the store and the chains both model: no id is the collection-wide flag, one id
         // is an ERC-721 operator or an ERC-1155 allowance, and each is cleared its own way.
         let params = NftApproveParams {
            chain: request.chain,
            standard,
            collection,
            token_id,
            owner: request.owner,
            operator,
            approved: token_id.is_none().then_some(true),
            amount: (token_id.is_some() && matches!(standard, Some(NftStandard::Erc1155)))
               .then_some(U256::ZERO),
            revoked: false,
         };

         revoke_nft_approval(env, request.chain, params).await.map_err(classify)
      }
   }
}

/// Set or remove one wallet's EIP-7702 delegation.
pub async fn delegate(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: DelegateRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = chain(request.chain)?;

   delegate_to(
      ctx,
      env,
      true,
      chain,
      request.from,
      request.delegate_to,
   )
   .await
   .map_err(classify)
}

/// Shield a public asset into the private pool.
pub async fn shield(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: ShieldRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = chain(request.chain)?;
   let asset = private_asset(
      &ctx,
      chain,
      request.from,
      request.token,
      request.nft,
   )
   .await?;
   let amount = NumericValue::format_wei(request.amount, request.decimals);

   offloaded(move || {
      crate::runtime::rt().block_on(shield_flow(
         ctx,
         env,
         chain,
         asset,
         amount,
         request.from,
         request.recipient,
         None,
      ))
   })
   .await
}

/// Unshield private notes to a public address.
pub async fn unshield(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: UnshieldRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = chain(request.chain)?;
   let asset = private_asset(
      &ctx,
      chain,
      request.from,
      request.token,
      request.nft,
   )
   .await?;
   let amount = NumericValue::format_wei(request.amount, request.decimals);

   // The URL the app holds is what the page would have sent through; a call that names another one has
   // already been checked against the grant that may change it.
   let bundler_url = match &request.broadcast.bundler_url {
      Some(url) => url.clone(),
      None => stored_bundler_url(&ctx).unwrap_or_else(|| default_bundler_url(chain.id())),
   };

   let to = format!("{:#x}", request.to);
   let memo = request.memo.unwrap_or_default();
   let self_broadcast = request.broadcast.self_broadcast;
   // An NFT has nothing to unwrap, so the flag is a fungible one whether or not the call set it.
   let unwrap_to_eth = request.unwrap_to_eth && request.nft.is_none();
   let from = request.from;

   offloaded(move || {
      crate::runtime::rt().block_on(unshield_flow(
         ctx,
         env,
         chain,
         asset,
         amount,
         from,
         to,
         self_broadcast,
         unwrap_to_eth,
         bundler_url,
         memo,
         None,
      ))
   })
   .await
}

/// Merge the wallet's own notes: a private self-transfer, so no value leaves.
pub async fn merge_notes(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: MergeNotesRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = chain(request.chain)?;
   let currency = Currency::from(erc20(&ctx, chain, request.from, request.currency).await?);
   let amount = NumericValue::format_wei(request.amount, request.decimals);
   let from = request.from;

   offloaded(move || {
      crate::runtime::rt().block_on(private_merge_notes(
         ctx, env, chain, currency, amount, from,
      ))
   })
   .await
}

/// Whether an address can receive funds on one chain, in the app's own words.
pub async fn screen(ctx: ZeusCtx, chain: ChainId, recipient: Address) -> Result<(), ExecError> {
   crate::core::tx::ensure_recipient_is_not_contract(ctx, chain, recipient)
      .await
      .map_err(classify)
}

/// The sealed bundler URL the app holds for its own unshields, when it holds one.
pub fn stored_bundler_url(ctx: &ZeusCtx) -> Option<String> {
   let key = ctx.read_vault(|vault| vault.wallet_state_key()).ok()?;

   BundlerUrl::load(&key).ok().map(|stored| stored.url)
}

/// What a private call is about, in the shape the flows take.
async fn private_asset(
   ctx: &ZeusCtx,
   chain: ChainId,
   owner: Address,
   token: Address,
   nft: Option<NftMovement>,
) -> Result<RailgunAsset, ExecError> {
   let Some(nft) = nft else {
      let currency = if token.is_zero() {
         Currency::from(NativeCurrency::from(chain.id()))
      } else {
         Currency::from(erc20(ctx, chain, owner, token).await?)
      };

      return Ok(RailgunAsset::Fungible(currency));
   };

   Ok(RailgunAsset::Nft(NftToken {
      chain_id: chain.id(),
      collection: token,
      token_id: nft.token_id,
      standard: nft.standard,
      metadata_uri: None,
   }))
}

/// Run one private flow on a blocking thread of the process runtime.
///
/// Proving is CPU work and the bundler client is not Send, so neither the flow nor its future can run on
/// the request's own task. The GUI does the same thing, for the same reason.
async fn offloaded<T, F>(work: F) -> Result<T, ExecError>
where
   F: FnOnce() -> Result<T, anyhow::Error> + Send + 'static,
   T: Send + 'static,
{
   crate::runtime::rt()
      .spawn_blocking(work)
      .await
      .map_err(|error| ExecError::Failed(format!("the flow did not run: {error}")))?
      .map_err(classify)
}

/// The chain a call named, as the flows take it.
pub(crate) fn chain(chain: u64) -> Result<ChainId, ExecError> {
   ChainId::new(chain)
      .map_err(|_| ExecError::Invalid(format!("chain {chain} is not one Zeus supports")))
}

/// One token, with the metadata a flow needs to describe it.
///
/// The app's portfolio answers when it holds the token, which is the common case and costs no node call; a
/// token the wallet has never seen is read from its own contract, which is also how it is told from an
/// address that is not an ERC-20 at all.
pub(crate) async fn erc20(
   ctx: &ZeusCtx,
   chain: ChainId,
   owner: Address,
   address: Address,
) -> Result<ERC20Token, ExecError> {
   if let Some(token) = ctx
      .get_portfolio(chain.id(), owner)
      .tokens()
      .iter()
      .find(|token| token.address == address)
      .cloned()
   {
      return Ok(token);
   }

   let manager = ctx.get_client_manager();
   let rpc = manager
      .get_best_rpc(chain.id())
      .ok_or_else(|| ExecError::Failed("no endpoint for this chain answered".to_owned()))?;
   let client = manager
      .client_for(&rpc, ClientKind::Short)
      .await
      .map_err(|error| ExecError::Failed(error.to_string()))?;

   ERC20Token::new(client, address, chain.id()).await.map_err(|error| {
      ExecError::Invalid(format!(
         "{address:#x} is not an ERC-20 on this chain: {error}"
      ))
   })
}

/// The pipeline's typed refusals, as the pipeline reports them, and everything else as itself.
pub(crate) fn classify(error: anyhow::Error) -> ExecError {
   // A refusal the grant itself made keeps its variant, so the tool boundary answers the code that names
   // it — PRICE_UNAVAILABLE, OUTPUT_SHORTFALL, TOKEN_NOT_ALLOWED — rather than "the app's own words"
   // under a code whose meaning is "report this".
   if let Some(denial) = error.downcast_ref::<crate::core::agent::AgentDenial>() {
      return ExecError::Denied(denial.0.clone());
   }

   // A cancelled prompt is the user saying no, not the app failing. A swap signs its permit through the
   // same window a dapp request does, so this is the answer an agent sees when nobody answered it.
   if crate::server::is_user_rejected(&error) {
      return ExecError::Rejected;
   }

   match error.downcast_ref::<TxRefusal>() {
      Some(TxRefusal::Locked) => ExecError::Locked,
      Some(TxRefusal::Busy) => ExecError::Busy,
      Some(TxRefusal::NoUi) => ExecError::NoUi,
      Some(TxRefusal::Rejected) => ExecError::Rejected,
      None => ExecError::Failed(error.to_string()),
   }
}
