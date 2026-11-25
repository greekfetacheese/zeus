//! The binary's half of the agent edge.
//!
//! The crate that speaks MCP cannot call the wallet flows: they are private functions here, and lifting
//! them into a crate would be a refactor of the money path. So the binary installs this, the same way the
//! GUI installs a `UiHost` for the opposite direction, and the edge reaches a flow only through it.
//!
//! The settings screens whose types live here are the first thing behind it: `keyboard`, `appearance`
//! and `bridge` are read and written under the window's own lock ([`crate::gui::SHARED_GUI`]), so the
//! values an agent sees are the ones the page is showing, never a second copy.
//!
//! Two more things only the binary can do arrive the same way: adding a Uniswap pool (the lookup is the
//! app's own code, see [`crate::agent_pools`]) and opening the window that finishes an operation only the
//! owner can end — a key typed into Zeus, an archive chosen in a file dialog.

use crate::core::AgentEnv;
use crate::core::PoolError;
use crate::core::agent::exec::{
   DelegateRequest, ExecError, NftSendRequest, RevokeRequest, SendRequest, TxFlows, TxFuture,
};
use crate::core::agent::settings::{Applied, SettingsError};
use crate::core::tx::TransactionRich;
use crate::gui::ui::settings::SettingsPage;
use crate::gui::ui::wallet::import::ImportWalletType;
use serde_json::Value;
use zeus_mcp::{AgentOps, AppInfo, BinScreen, Handback, OpsFuture, PoolRequest};

/// Zeus's own answer to the edge's port.
pub struct ZeusOps;

impl AgentOps for ZeusOps {
   fn app_info(&self) -> AppInfo {
      AppInfo {
         version: env!("CARGO_PKG_VERSION").to_owned(),
      }
   }

   fn bin_screens(&self) -> Vec<BinScreen> {
      crate::agent_screens::screens()
   }

   fn apply_bin_screen(&self, name: &str, patch: &Value) -> Result<Applied, SettingsError> {
      crate::agent_screens::apply(name, patch)
   }

   fn add_pool(&self, request: PoolRequest) -> OpsFuture<'_, Result<Value, PoolError>> {
      let ctx = crate::gui::SHARED_GUI.read(|gui| gui.ctx.clone());

      Box::pin(crate::agent_pools::add(ctx, request))
   }

   fn open_handback(&self, handback: Handback) -> Result<(), String> {
      match handback {
         Handback::WalletImport { mnemonic } => {
            let kind = if mnemonic {
               ImportWalletType::MnemonicPhrase
            } else {
               ImportWalletType::PrivateKey
            };

            crate::gui::SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.import_wallet.open(kind);
               gui.request_repaint();
            });
         }
         Handback::DataImport => {
            crate::gui::SHARED_GUI.write(|gui| {
               // The page keeps its own copy of the appearance settings, which is why it is handed the
               // window's rather than a fresh one.
               gui.ctx.write(|ctx| {
                  gui.settings.open_page(
                     SettingsPage::Data,
                     ctx,
                     &mut gui.appearance_settings,
                  );
               });

               gui.request_repaint();
            });
         }
      }

      Ok(())
   }

   fn set_desktop_integration(&self, on: bool) -> Result<(), String> {
      if !crate::utils::desktop_integration::SUPPORTED {
         return Err("this platform has no application menu to add Zeus to".to_owned());
      }

      crate::utils::desktop_integration::try_apply(on).map_err(|error| error.to_string())
   }
}

/// The money flows, as the pipeline asks for them.
///
/// Each reads the live context when it is called — never a copy taken when the agent was granted — so a
/// switch, a wallet or the whole app state is whatever it is now. The flows' own guard order is inside
/// `crate::agent_tx`.
impl TxFlows for ZeusOps {
   fn send(
      &self,
      env: AgentEnv,
      request: SendRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let ctx = crate::gui::SHARED_GUI.read(|gui| gui.ctx.clone());

      Box::pin(crate::agent_tx::send(ctx, env, request))
   }

   fn send_nft(
      &self,
      env: AgentEnv,
      request: NftSendRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let ctx = crate::gui::SHARED_GUI.read(|gui| gui.ctx.clone());

      Box::pin(crate::agent_tx::send_nft(ctx, env, request))
   }

   fn revoke(
      &self,
      env: AgentEnv,
      request: RevokeRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let ctx = crate::gui::SHARED_GUI.read(|gui| gui.ctx.clone());

      Box::pin(crate::agent_tx::revoke(ctx, env, request))
   }

   fn delegate(
      &self,
      env: AgentEnv,
      request: DelegateRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let ctx = crate::gui::SHARED_GUI.read(|gui| gui.ctx.clone());

      Box::pin(crate::agent_tx::delegate(ctx, env, request))
   }
}
