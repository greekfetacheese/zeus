//! The one doorway from `core` into a user interface.
//!
//! The transaction and signing pipelines raise prompts — a confirmation window, a
//! signing request, a yes/no question, a loading indicator, a notification. They
//! own when a prompt is raised and what it means, but not how it is drawn. This
//! module is the port that separates the two: `core` calls [`host`], the GUI
//! installs its implementation once from `main`, and a build with no interface
//! attached refuses the prompt instead of waiting for an answer nobody can give.
//!
//! The methods are deliberately composite. The GUI's busy-check, its `open` and its
//! `claim` all happen inside one lock, so at most one authorising prompt can ever be
//! up; a trait of primitive getters would reopen that race through the port.

use crate::ZeusCtx;
use crate::clear_signing::ClearDisplay;
use crate::signature::msg::SignMsgType;
use crate::tx::{TransactionAnalysis, TransactionRich, notification::NotificationType};
use std::sync::OnceLock;
use zeus_eth::{alloy_primitives::Address, types::ChainId, utils::NumericValue};

/// The refusal used when a prompt is raised with no interface attached.
pub const NO_UI_MSG: &str = "No Zeus interface is attached to confirm this request";

/// What became of an attempt to raise a prompt.
pub enum PromptOpen {
   /// The prompt is up and claimed by this flow.
   Opened,
   /// Another authorising prompt is already up; the caller must refuse, not wait.
   Busy,
   /// There is no interface to answer the prompt.
   Unavailable,
}

/// Everything the transaction confirmation window needs to draw one prompt.
pub struct TxConfirmRequest {
   pub ctx: ZeusCtx,
   pub source_is_zeus: bool,
   pub dapp: String,
   pub chain: ChainId,
   pub analysis: TransactionAnalysis,
   pub priority_fee: String,
   pub mev_protect: bool,
   pub sponsored: bool,
   pub recipient_name: Option<(Address, String)>,
}

/// What the user left in the transaction confirmation window.
pub struct TxConfirmAnswer {
   pub priority_fee: NumericValue,
   pub gas_limit: u64,
   pub clear_display: Option<ClearDisplay>,
}

impl TxConfirmAnswer {
   /// The answer for a rejected prompt.
   ///
   /// An accepted transaction always carries a gas limit — the window floors it at
   /// the simulated gas, which is never zero — so a zero limit is the one value the
   /// answer type cannot confuse with a confirmation.
   pub fn rejected() -> Self {
      Self {
         priority_fee: NumericValue::default(),
         gas_limit: 0,
         clear_display: None,
      }
   }

   /// Whether the user rejected the prompt rather than confirming it.
   pub fn is_rejected(&self) -> bool {
      self.gas_limit == 0
   }
}

/// Everything the sign-message window needs to draw one prompt.
pub struct SignRequest {
   pub dapp: String,
   pub chain_id: u64,
   pub msg_type: SignMsgType,
   pub signer: Address,
}

/// A generic yes/no prompt: the MEV fallback, a dapp's network switch, an in-app delete.
pub struct ConfirmRequest {
   pub title: String,
   pub msg2: Option<String>,
   pub from_dapp: bool,
}

/// The prompt surface `core` drives.
///
/// Implemented once by the GUI (`GuiHost`) and once headlessly ([`NoUi`]). Every
/// method that draws a prompt also owns the window's claim, so a dropped flow can
/// release it through the matching `release_*` method and never wedge the next one.
pub trait UiHost: Send + Sync + 'static {
   fn request_repaint(&self);
   fn open_loading(&self, msg: &str);
   fn reset_loading(&self);
   fn open_tx_confirm(&self, req: TxConfirmRequest) -> PromptOpen;
   fn take_tx_confirm(&self) -> Option<TxConfirmAnswer>;
   fn release_tx_confirm(&self);
   fn open_sign(&self, req: SignRequest) -> PromptOpen;
   fn take_sign(&self) -> Option<bool>;
   fn release_sign(&self);
   fn open_confirm(&self, req: ConfirmRequest) -> PromptOpen;
   fn take_confirm(&self) -> Option<bool>;
   fn release_confirm(&self);
   fn notify_pending(&self, title: &str, kind: NotificationType);
   fn notify_mined(
      &self,
      title: &str,
      kind: NotificationType,
      tx: TransactionRich,
      now_ms: u64,
      finish_ms: u64,
   );
   fn lockout_and_shutdown(&self, msg: &str);
}

static HOST: OnceLock<Box<dyn UiHost>> = OnceLock::new();
static NO_UI: NoUi = NoUi;

/// Install the process-wide host. The first install wins; a second logs a warning.
pub fn install(host: impl UiHost + 'static) {
   if HOST.set(Box::new(host)).is_err() {
      tracing::warn!("A Zeus UI host is already installed; keeping the first one");
   }
}

/// The installed host, or the headless default when nothing is attached.
pub fn host() -> &'static dyn UiHost {
   match HOST.get() {
      Some(host) => host.as_ref(),
      None => &NO_UI,
   }
}

/// The host used when `main` never installed one: no prompt can be answered, so
/// raising one reports [`PromptOpen::Unavailable`] instead of waiting forever.
struct NoUi;

impl UiHost for NoUi {
   fn request_repaint(&self) {}

   fn open_loading(&self, _msg: &str) {}

   fn reset_loading(&self) {}

   fn open_tx_confirm(&self, _req: TxConfirmRequest) -> PromptOpen {
      PromptOpen::Unavailable
   }

   fn take_tx_confirm(&self) -> Option<TxConfirmAnswer> {
      None
   }

   fn release_tx_confirm(&self) {}

   fn open_sign(&self, _req: SignRequest) -> PromptOpen {
      PromptOpen::Unavailable
   }

   fn take_sign(&self) -> Option<bool> {
      None
   }

   fn release_sign(&self) {}

   fn open_confirm(&self, _req: ConfirmRequest) -> PromptOpen {
      PromptOpen::Unavailable
   }

   fn take_confirm(&self) -> Option<bool> {
      None
   }

   fn release_confirm(&self) {}

   fn notify_pending(&self, _title: &str, _kind: NotificationType) {}

   fn notify_mined(
      &self,
      _title: &str,
      _kind: NotificationType,
      _tx: TransactionRich,
      _now_ms: u64,
      _finish_ms: u64,
   ) {
   }

   fn lockout_and_shutdown(&self, _msg: &str) {}
}
