//! Shared helpers for the Railgun dapp flows.

use std::time::Duration;
use tokio::time::sleep;

use anyhow::anyhow;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256},
   currency::{Currency, ERC20Token},
   nft::{NftStandard, NftToken},
   types::ChainId,
   utils::client::RpcClient,
};
use zeus_railgun::{
   RailgunProvider, caip::AssetId, rand::SeedableRng, rand_chacha::ChaCha12Rng,
   transact::TransactionBuilder,
};

use crate::core::ZeusCtx;
use crate::utils::{
   RT,
   simulate::{ForkPrefetch, ForkSim, ForkSimRequest, simulate_on_fork},
};

pub mod merge_notes;
pub mod shield;
pub mod transfer;
pub mod unshield;

pub use merge_notes::MergeNotesWindow;
pub use shield::{BundlerUrl, RailgunMode, ShieldUi};
pub use transfer::{private_merge_notes, private_transfer};
pub use unshield::default_bundler_url;

/// What the user is moving into or out of Railgun.
///
/// An enum rather than a `Currency` because an NFT never enters `Currency` (D1), and rather than two
/// optional parameters so that "neither" and "both" cannot be represented.
///
/// The amount question is answered here too: a fungible token has a quantity the user picks, while an
/// ERC-721 is exactly one — the token id *is* the asset — so the builders take what this derives rather
/// than a number each call site invents.
pub enum RailgunAsset {
   Fungible(Currency),
   Nft(NftToken),
}

impl RailgunAsset {
   /// The asset id the Railgun builders take.
   ///
   /// The standard decides it, not the fact that it is an NFT: Railgun tracks a collection's ids as two
   /// different asset types, so the same collection and id mean different things in each.
   pub fn asset_id(&self) -> AssetId {
      match self {
         Self::Fungible(currency) => AssetId::Erc20(currency.to_erc20().address),
         Self::Nft(nft) => match nft.standard {
            NftStandard::Erc721 => AssetId::Erc721(nft.collection, nft.token_id),
            NftStandard::Erc1155 => AssetId::Erc1155(nft.collection, nft.token_id),
         },
      }
   }

   /// The value to move, in the asset's own units.
   ///
   /// An ERC-721 is indivisible: the token id *is* the asset, so it moves exactly one and the amount is
   /// not consulted. An ERC-1155 is a quantity of an id — divisible, and the same kind of asset as an
   /// ERC-20 as far as Railgun is concerned — so its value **is** the amount, counted in whole units.
   pub fn value(&self, amount: U256) -> U256 {
      match self {
         Self::Fungible(_) => amount,
         Self::Nft(nft) => match nft.standard {
            NftStandard::Erc721 => U256::from(1),
            NftStandard::Erc1155 => amount,
         },
      }
   }

   /// Native ETH shields through `shield_native`, which wraps and shields in one call.
   pub fn is_native(&self) -> bool {
      matches!(self, Self::Fungible(currency) if currency.is_native())
   }
}

/// A proved Railgun transaction, reduced to the call that gets broadcast.
pub struct ProvedCall {
   pub calldata: Bytes,
   pub interact_to: Address,
   pub value: U256,
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{alloy_primitives::address, nft::NftStandard};

   const BAYC: Address = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");

   fn nft(token_id: u64, standard: NftStandard) -> NftToken {
      NftToken {
         chain_id: 1,
         collection: BAYC,
         token_id: U256::from(token_id),
         standard,
         metadata_uri: None,
      }
   }

   /// A fungible asset is the currency it wraps, and the amount is whatever the user typed.
   #[test]
   fn a_fungible_asset_keeps_its_currency_and_amount() {
      let weth = Currency::from(ERC20Token::weth());
      let asset = RailgunAsset::Fungible(weth.clone());

      assert_eq!(
         asset.asset_id(),
         AssetId::Erc20(weth.to_erc20().address)
      );
      assert_eq!(asset.value(U256::from(1500)), U256::from(1500));
      assert!(!asset.is_native());
   }

   /// An ERC-721 is its collection and its id, and it is worth exactly one — there is no quantity to ask
   /// the user for, so whatever number reaches `value` cannot change what moves.
   #[test]
   fn an_erc721_asset_is_its_id_and_always_worth_one() {
      let asset = RailgunAsset::Nft(nft(7, NftStandard::Erc721));

      assert_eq!(
         asset.asset_id(),
         AssetId::Erc721(BAYC, U256::from(7))
      );
      assert_eq!(asset.value(U256::ZERO), U256::from(1));
      assert_eq!(
         asset.value(U256::MAX),
         U256::from(1),
         "the amount is not consulted"
      );
      assert!(!asset.is_native());
   }

   /// An ERC-1155 is the same collection and id under a **different asset type**, and it is a quantity of
   /// that id: the value is the amount the user asks for, counted in whole units, with nothing
   /// substituted for it.
   #[test]
   fn an_erc1155_asset_is_its_id_under_its_own_type_and_worth_its_amount() {
      let asset = RailgunAsset::Nft(nft(7, NftStandard::Erc1155));

      assert_eq!(
         asset.asset_id(),
         AssetId::Erc1155(BAYC, U256::from(7))
      );
      assert_eq!(asset.value(U256::from(1)), U256::from(1));
      assert_eq!(asset.value(U256::from(3)), U256::from(3));
      assert!(!asset.is_native());
   }

   /// Native ETH is the one asset that shields through `shield_native`, so the flag has to be right for
   /// it — and wrong for everything else.
   #[test]
   fn only_native_currency_is_native() {
      assert!(RailgunAsset::Fungible(Currency::native(1)).is_native());
      assert!(!RailgunAsset::Fungible(Currency::from(ERC20Token::weth())).is_native());
   }
}

/// Prove `tx` and reduce it to its call.
pub async fn prove(
   provider: &mut RailgunProvider<RpcClient>,
   tx: TransactionBuilder,
) -> Result<ProvedCall, anyhow::Error> {
   let mut rng = ChaCha12Rng::from_os_rng();
   let proved = provider.build(tx, &mut rng).await?;

   Ok(ProvedCall {
      calldata: proved.tx_data.data.clone(),
      interact_to: proved.tx_data.to,
      value: proved.tx_data.value,
   })
}

/// Fork-simulate a proved Railgun transaction.
///
/// Local notes the chain already spent revert with "note already spent", which
/// means local state is behind: that schedules a resync and surfaces the failure
/// rather than pretending the transaction is simply invalid.
pub async fn simulate_proved(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   call: &ProvedCall,
   prefetch: ForkPrefetch,
   gas_limit: Option<u64>,
) -> Result<ForkSim, anyhow::Error> {
   let req = ForkSimRequest {
      from,
      interact_to: call.interact_to,
      call_data: call.calldata.clone(),
      value: call.value,
      gas_limit,
      authorization_list: vec![],
   };

   match simulate_on_fork(ctx.clone(), chain, prefetch, req).await {
      Ok(sim) => Ok(sim),
      Err(e) => {
         if e.to_string().contains("note already spent") {
            resync_railgun_later(ctx, chain);
         }

         Err(anyhow!("Simulation failed: {:?}", e))
      }
   }
}

/// Gate every Railgun operation: supported, enabled, provider ready, not syncing,
/// and synced — scheduling a resync when the local root is invalid.
pub async fn railgun_ready(
   ctx: ZeusCtx,
   chain: ChainId,
) -> Result<RailgunProvider<RpcClient>, anyhow::Error> {
   if !ctx.railgun_is_supported(chain) {
      return Err(anyhow!(
         "Railgun is not supported for the {} network",
         chain.name()
      ));
   }

   if !ctx.is_railgun_enabled(chain.id()) {
      return Err(anyhow!(
         "Railgun is disabled. Enable it in Settings → Railgun."
      ));
   }

   let provider = ctx.get_railgun_provider(chain.id(), false).await?;

   if provider.chain_id() != chain.id() {
      return Err(anyhow!(
         "Railgun provider chain id {} does not match the current chain id {}",
         provider.chain_id(),
         chain.id()
      ));
   }

   if provider.is_syncing().await {
      return Err(anyhow!("Railgun is syncing, try again later"));
   }

   if let Err(e) = ctx.sync_railgun(chain.id(), false).await {
      // If railgun cannot sync error out so we dont allow operations
      let is_invalid_root = ctx.read(|ctx| ctx.railgun_status.is_error_invalid_root(chain.id()));
      if is_invalid_root {
         resync_railgun_later(ctx.clone(), chain);

         return Err(anyhow!(
            "Railgun state is corrupted (Invalid root), resync has started"
         ));
      }

      return Err(anyhow!("Railgun is not synced: {:?}", e));
   }

   Ok(provider)
}

/// Refresh public and private state after a Railgun op.
///
/// Every op leaves the sender's public balances stale and moves private notes, so
/// all wallets' private data is refreshed: a note spent here changes what
/// another wallet of the same seed sees.
///
/// Order matters — `sync_railgun` has to land before `update_private_data`, or the
/// refresh reports the state the chain has already moved past.
pub async fn settle_railgun_op(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   token: Option<ERC20Token>,
) {
   ctx.write(|ctx| {
      ctx.railgun_status.set_op_in_progress(chain.id(), true);
   });

   let manager = ctx.balance_manager();

   if let Some(token) = token {
      if let Err(e) = manager
         .update_tokens_balance(ctx.clone(), chain.id(), from, vec![token], true)
         .await
      {
         tracing::error!("Error updating token balance: {:?}", e);
      }
   }

   // The NFT half of the same refresh. An NFT has no balance, only an owner, so what moves here is
   // ownership — which the balance manager holds for the public side, exactly like the token balances
   // above. `retry_if_unchanged`, because a shield takes the token out of the wallet and an unshield
   // puts it back: an answer that has not moved yet is the chain lagging, not a settled one. The private
   // side needs nothing — the scan below is what maintains it.
   let nfts = ctx.get_portfolio(chain.id(), from).nfts().clone();
   if !nfts.is_empty() {
      if let Err(e) = manager.update_nft_balances(ctx.clone(), chain.id(), from, nfts, true).await {
         tracing::error!("Error updating NFT balances: {:?}", e);
      }
   }

   if let Err(e) = manager.update_eth_balance(ctx.clone(), chain.id(), vec![from], true).await {
      tracing::error!("Error updating eth balance: {:?}", e);
   }

   ctx.update_public_data(chain.id(), from);

   if let Err(e) = ctx.sync_railgun(chain.id(), false).await {
      tracing::error!("Error syncing Railgun: {:?}", e);
   }

   for wallet in ctx.get_all_wallets_info() {
      ctx.update_private_data(chain.id(), wallet.address).await;
   }

   ctx.write(|ctx| {
      ctx.railgun_status.set_op_in_progress(chain.id(), false);
   });
}

/// Resync Railgun state a second from now.
///
/// Used when local state disagrees with the chain — an invalid root, or notes the
/// chain already spent. Re-reading immediately races whatever caused the
/// disagreement, so give it a beat.
pub fn resync_railgun_later(ctx: ZeusCtx, chain: ChainId) {
   RT.spawn(async move {
      sleep(Duration::from_secs(1)).await;

      match ctx.resync_railgun(chain.id()).await {
         Ok(_) => tracing::info!(
            "Railgun resynced to valid root for chain {}",
            chain.id()
         ),
         Err(e) => tracing::error!("Error syncing Railgun: {:?}", e),
      }
   });
}

/// The single event in `events`, or `too_many` / `none()`.
///
/// `none` is lazy so a caller can include diagnostics, such as how many logs it
/// scanned, without paying for them on the happy path.
pub fn expect_single_event<T>(
   events: Vec<T>,
   too_many: &str,
   none: impl FnOnce() -> String,
) -> Result<T, anyhow::Error> {
   if events.len() > 1 {
      return Err(anyhow!("{}", too_many));
   }

   events.into_iter().next().ok_or_else(|| anyhow!("{}", none()))
}
