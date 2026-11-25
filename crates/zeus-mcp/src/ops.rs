//! The port the binary implements.
//!
//! The flows an agent must be able to start — a send, a swap, a bridge, a shield, an approval revoke —
//! are private functions in the binary crate, which a library cannot call. Rather than move the money
//! path into a crate, the binary installs an implementation of this trait, the same way the GUI
//! installs a [`zeus_core::ui::UiHost`] for the opposite direction.
//!
//! Three settings screens are here for the same reason at a smaller scale. `keyboard`, `appearance` and
//! `bridge` are typed in the binary's own modules (`KeyBindings`, `AppearanceSettings`, the bridge
//! preferences), so their patch, their values and their writer cannot live in a crate either. What stays
//! in core is everything that decides *whether* a write happens: the capability, the tier and the audit.

use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use zeus_core::agent::exec::TxFlows;
use zeus_core::agent::settings::{Applied, Closed, SettingsError};
use zeus_core::context::PoolError;
use zeus_eth::alloy_primitives::Address;

/// What the edge cannot know about the process it lives in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppInfo {
   /// The build's own version, as the about screen shows it.
   pub version: String,
}

/// One settings screen whose writer lives in the binary.
///
/// The record is built when it is asked for, never cached at install: `settings_list`,
/// `settings_get` and `settings_describe` each read afresh, so a screen is not described from something
/// the app has since changed.
pub struct BinScreen {
   /// The name a tool takes, and the segment a resource URI carries.
   pub name: &'static str,
   /// The capability a read needs.
   pub read: &'static str,
   /// The capability a write needs.
   pub write: &'static str,
   /// The screen's current values, in the same shape its patch takes.
   pub values: Value,
   /// The patch's JSON Schema, built beside the struct that validates it.
   pub schema: Value,
   /// The fields an agent may never write, with the reason for each.
   pub closed: Vec<Closed>,
}

/// A flow's answer, boxed: `async fn` in a trait is not dyn-compatible, and this trait is held as
/// `Arc<dyn AgentOps>`.
pub type OpsFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One Uniswap pool, as a caller names it.
///
/// The version decides which of the fields matter: a V2 pool is a pair, a V3 pool adds a fee tier, and
/// a V4 pool adds the tick spacing and the hooks contract that are part of its identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolRequest {
   pub chain: u64,
   pub version: PoolVersion,
   pub token_a: Address,
   pub token_b: Address,
   /// The fee, in hundredths of a bip: 3000 is the 0.3% tier.
   pub fee: u32,
   /// A V4 pool's tick spacing; zero asks for the spacing its fee tier has.
   pub tick_spacing: i32,
   /// A V4 pool's hooks contract; the zero address is a pool with no hooks.
   pub hooks: Address,
}

/// Which pool standard to add.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolVersion {
   V2,
   V3,
   V4,
}

/// An operation only the app owner can finish, and the window that finishes it.
///
/// An agent opens it and the call ends there: what happens inside is typed into Zeus's own fields, never
/// into a tool argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handback {
   /// The wallet import form. `mnemonic` opens the phrase tab instead of the private-key one.
   WalletImport { mnemonic: bool },
   /// The Data page, where the archive is chosen and the vault's credentials typed.
   DataImport,
}

/// Everything the binary has to supply.
///
/// A method that starts a flow returns a boxed future: `async fn` in a trait is not dyn-compatible, and
/// this trait is held as `Arc<dyn AgentOps>`. The methods here answer from memory, so they are plain.
///
/// The fund-moving flows are [`TxFlows`] rather than methods here: the guard order in front of them lives in
/// `zeus_core::agent::exec`, which cannot name this crate, so the pipeline's port is the narrower trait and
/// this one extends it. An implementor supplies both, and there is still one implementation per build.
pub trait AgentOps: TxFlows {
   /// The running Zeus build.
   fn app_info(&self) -> AppInfo;

   /// The settings screens this binary writes, with their values read now.
   ///
   /// Empty when nothing is installed, which is the honest answer for a build with no app around it: a
   /// screen that is not here is neither listed nor writable.
   fn bin_screens(&self) -> Vec<BinScreen> {
      Vec::new()
   }

   /// Apply one patch to a screen [`Self::bin_screens`] reported, and say what changed.
   fn apply_bin_screen(&self, name: &str, patch: &Value) -> Result<Applied, SettingsError> {
      let _ = patch;

      Err(SettingsError::refused(format!(
         "the {name} screen is not written by this build"
      )))
   }

   /// Add one Uniswap pool, proving it exists on chain first, and answer the row that was added.
   ///
   /// The lookup and the validation are the flow's, because they read the chain and know what a pool
   /// of each version has to look like; the capability, the tier and the audit stay in the edge.
   fn add_pool(&self, request: PoolRequest) -> OpsFuture<'_, Result<Value, PoolError>> {
      let _ = request;

      Box::pin(async {
         Err(PoolError::Refused(
            "this build cannot add a pool".to_owned(),
         ))
      })
   }

   /// Open the window that finishes an operation only the app owner can finish.
   ///
   /// Opening is the whole call: the tool answers "the user has to do this" rather than waiting for a
   /// form whose deadline nobody can bound.
   fn open_handback(&self, handback: Handback) -> Result<(), String> {
      let _ = handback;

      Err("this build has no window to open".to_owned())
   }

   /// Install or remove the application-menu entry a `general` write asks for.
   ///
   /// It is here rather than behind the settings writer because the installer is the binary's: a write
   /// that recorded the flag without it would leave the file saying "on" with no entry beside it.
   fn set_desktop_integration(&self, on: bool) -> Result<(), String> {
      let _ = on;

      Err("this build cannot change the application menu".to_owned())
   }
}

/// A double for tests and for a binary that has not installed anything yet.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoOps;

/// The money flows refuse through [`TxFlows`]'s own defaults: this build has none to run.
impl TxFlows for NoOps {}

impl AgentOps for NoOps {
   fn app_info(&self) -> AppInfo {
      AppInfo {
         version: "0.0.0-test".to_owned(),
      }
   }
}
