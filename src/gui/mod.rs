pub mod ambient;
pub mod app;
pub mod appearance;
pub mod tray;
pub mod ui;
pub mod ui_host;

use egui::{Context, Ui};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use ui::settings;

use crate::assets::icons::Icons;
use crate::core::context::{ZeusContext, ZeusCtx};
use crate::gui::appearance::{AppearanceSettings, load_theme_kind};
use egui_elements::{Announcer, editor::ThemeEditor, theme::*};
use lazy_static::lazy_static;

pub use crate::gui::ui::{
   AccountPanel, ApprovalsUi, ConfirmWindow, LoadingWindow, LockScreen, MsgWindow, Notification,
   PortfolioUi, RecipientSelectionWindow, RecoverHDWallet, SendCryptoUi, SettingsUi,
   TokenSelectionWindow, TxConfirmationWindow, TxWindow, UnlockVault, UpdateWindow, WalletUi,
   common::dots_button,
   dapps::{
      across::AcrossBridge,
      connect::{ConnectDappWindow, DappAccountChoice, DappConnectResult},
      railgun::{MergeNotesWindow, ShieldUi},
      uniswap::UniswapUi,
   },
   panels::{central_panel::FPSMetrics, left_panel::ConnectedDappsUi},
   settings::SettingsPage,
   sign_msg_window::SignMsgWindow,
   tx::SpentNoteWindow,
   tx_history::TxHistory,
};

#[cfg(feature = "dev")]
pub use crate::gui::ui::dev::DevUi;

lazy_static! {
   pub static ref SHARED_GUI: SharedGUI = SharedGUI::default();
}

#[derive(Clone)]
pub struct SharedGUI(Arc<Mutex<GUI>>);

impl SharedGUI {
   /// Shared access to the [GUI]
   pub fn read<R>(&self, reader: impl FnOnce(&GUI) -> R) -> R {
      reader(&self.0.lock().expect("SharedGUI poisoned"))
   }

   /// Exclusive mutable access to the [GUI]
   pub fn write<R>(&self, writer: impl FnOnce(&mut GUI) -> R) -> R {
      writer(&mut self.0.lock().expect("SharedGUI poisoned"))
   }

   pub fn request_repaint(&self) {
      self.read(|gui| gui.request_repaint());
   }

   pub fn open_loading(&self, msg: impl Into<String>) {
      self.write(|gui| gui.loading_window.open(msg));
   }

   pub fn reset_loading(&self) {
      self.write(|gui| gui.loading_window.reset());
   }
}

impl Default for SharedGUI {
   fn default() -> Self {
      Self(Arc::new(Mutex::new(GUI::default())))
   }
}

/// The page the main window is showing.
///
/// One variant per nav entry. The names are the words the nav draws, so a heading built from
/// [`Page::name`] says exactly what the plate that opened it said. Shield and Unshield share
/// [`Page::Shield`]: privacy mode flips the nav word, not the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
   Home,
   Send,
   Shield,
   Swap,
   Bridge,
   Wallets,
   Transactions,
   Approvals,
   Settings,
   ConnectedDapps,
   Dev,
}

impl Page {
   pub fn name(&self) -> &'static str {
      match self {
         Self::Home => "Home",
         Self::Send => "Send",
         Self::Shield => "Shield",
         Self::Swap => "Swap",
         Self::Bridge => "Bridge",
         Self::Wallets => "Wallets",
         Self::Transactions => "Transactions",
         Self::Approvals => "Approvals",
         Self::Settings => "Settings",
         Self::ConnectedDapps => "Connected Dapps",
         Self::Dev => "Dev UI",
      }
   }
}

pub struct GUI {
   pub egui_ctx: Context,
   /// Set by `bring_to_front` (a worker thread) and consumed once by `ZeusApp::logic`,
   /// which owns the hidden-in-tray bookkeeping.
   pub show_requested: AtomicBool,
   pub ctx: ZeusCtx,
   pub icons: Arc<Icons>,
   pub theme: Theme,
   pub appearance_settings: AppearanceSettings,
   pub editor: ThemeEditor,
   pub shield_ui: ShieldUi,
   pub uniswap: UniswapUi,
   pub across_bridge: AcrossBridge,
   pub approvals: ApprovalsUi,
   pub account_panel: AccountPanel,
   pub token_selection: TokenSelectionWindow,
   pub recipient_selection: RecipientSelectionWindow,
   pub wallet_ui: WalletUi,
   pub unlock_vault_ui: UnlockVault,
   pub recover_wallet_ui: RecoverHDWallet,
   pub lock_screen: LockScreen,
   pub portofolio: PortfolioUi,
   pub send_crypto: SendCryptoUi,
   pub msg_window: MsgWindow,
   pub loading_window: LoadingWindow,
   pub settings: SettingsUi,
   pub tx_history: TxHistory,
   pub data_inspection: bool,
   pub confirm_window: ConfirmWindow,
   pub connect_dapp_window: ConnectDappWindow,
   pub tx_confirmation_window: TxConfirmationWindow,
   pub tx_window: TxWindow,
   pub spent_note_window: SpentNoteWindow,
   pub sign_msg_window: SignMsgWindow,
   pub fps_metrics: FPSMetrics,
   pub connected_dapps: ConnectedDappsUi,
   pub notification: Notification,
   pub update_window: UpdateWindow,
   /// Whether an assistive technology is attached.
   ///
   /// Sticky: once one has been seen the flag stays set, so prose stays reachable even
   /// across a frame in which the platform builds no tree.
   pub at_active: bool,
   /// Whether the attempt-cap notice is waiting for its OK before Zeus closes.
   ///
   /// Set only when [`Self::at_active`]: a sighted user gets today's immediate close, while a
   /// reader is owed the frame that lets the notice speak. [`Self::finish_lockout_notice`]
   /// shuts down once the window is gone.
   pub lockout_notice_held: bool,
   /// The page whose heading last took focus, if any.
   ///
   /// `None` means the next pass's heading takes focus, which is what unlocking asks for: the
   /// lock clears it, so the Home heading is focused when the page appears again.
   last_page: Option<Page>,
   /// The pending announcement; the last `announce` of a frame wins.
   announcement: String,
   /// Keeps the live `Status` node in the tree between frames: a node created together
   /// with its message never speaks (`egui-elements/src/announce.rs`).
   announcer: Announcer,
   /// The status pill's message last frame, so its transitions are announced instead of the same
   /// sentence every frame while work runs.
   last_status_pill: Option<String>,
   #[cfg(feature = "dev")]
   pub dev: DevUi,
   pub merge_notes_window: MergeNotesWindow,
}

impl GUI {
   pub fn new(icons: Arc<Icons>, theme: Theme, egui_ctx: Context) -> Self {
      let ctx = ZeusCtx::new(crate::runtime::rt().clone());

      let mut appearance_settings = match AppearanceSettings::load_from_file() {
         Ok(config) => config,
         Err(e) => {
            tracing::error!("Failed to load appearance settings: {:?}", e);
            AppearanceSettings::default()
         }
      };

      let token_selection = ui::TokenSelectionWindow::new();
      let recipient_selection = ui::RecipientSelectionWindow::new();
      let send_crypto = ui::SendCryptoUi::new();
      let across_bridge = ui::dapps::across::AcrossBridge::new();
      let account_panel = AccountPanel::new();

      let msg_window = ui::MsgWindow::new();
      let loading_window = ui::LoadingWindow::new();
      let confirm_window = ui::common::ConfirmWindow::new();
      let connect_dapp_window = ConnectDappWindow::new();
      let tx_confirmation_window = TxConfirmationWindow::new();
      let tx_window = TxWindow::new();
      let spent_note_window = SpentNoteWindow::new();
      let wallet_ui = ui::WalletUi::new();
      let approvals = ApprovalsUi::new();

      let settings = ctx.write(|ctx| settings::SettingsUi::new(ctx, &mut appearance_settings));

      let tx_history = ui::tx_history::TxHistory::new();
      let sign_msg_window = SignMsgWindow::new();
      let connected_dapps = ConnectedDappsUi::new();
      let notification = Notification::new();
      let update_window = UpdateWindow::new();
      let fps_metrics = FPSMetrics::new();
      let uniswap = UniswapUi::new();
      let shield_ui = ShieldUi::new();
      let merge_notes_window = MergeNotesWindow::new();
      let unlock_vault_ui = UnlockVault::new();
      let recover_wallet_ui = RecoverHDWallet::new();
      let lock_screen = LockScreen::new();

      Self {
         egui_ctx,
         show_requested: AtomicBool::new(false),
         ctx: ctx.clone(),
         theme,
         appearance_settings,
         editor: ThemeEditor::new(),
         icons,
         account_panel,
         token_selection,
         recipient_selection,
         wallet_ui,
         approvals,
         shield_ui,
         uniswap,
         across_bridge,
         unlock_vault_ui,
         recover_wallet_ui,
         lock_screen,
         portofolio: PortfolioUi::new(),
         send_crypto,
         msg_window,
         loading_window,
         settings,
         tx_history,
         data_inspection: false,
         confirm_window,
         connect_dapp_window,
         tx_confirmation_window,
         tx_window,
         spent_note_window,
         sign_msg_window,
         fps_metrics,
         connected_dapps,
         notification,
         update_window,
         at_active: false,
         lockout_notice_held: false,
         last_page: None,
         announcement: String::new(),
         announcer: Announcer::new("zeus"),
         last_status_pill: None,
         #[cfg(feature = "dev")]
         dev: DevUi::new(),
         merge_notes_window,
      }
   }

   pub fn show_top_panel(&mut self, ctx: &mut ZeusContext, ui: &mut Ui) {
      ui::panels::top_panel::show(self, ctx, ui);
   }

   pub fn show_bottom_panel(&mut self, ctx: &mut ZeusContext, ui: &mut Ui) {
      ui::panels::bottom_panel::_show(self, ctx, ui);
   }

   pub fn show_left_panel(&mut self, ctx: &mut ZeusContext, ui: &mut Ui) {
      ui::panels::left_panel::show(self, ctx, ui);
   }

   pub fn show_right_panel(&mut self, ui: &mut Ui) {
      ui::panels::right_panel::show(ui, self);
   }

   pub fn show_central_panel(&mut self, ctx: &mut ZeusContext, ui: &mut Ui) {
      ui::panels::central_panel::show(self, ctx, ui);
   }

   pub fn open_msg_window(&mut self, msg: impl Into<String>) {
      let msg = msg.into();
      // The dialog is drawn in the same pass the caller asked for it, and a reader is told what it says
      // even before it is reached.
      self.announce(msg.clone());
      self.msg_window.open(msg);
   }

   /// App-wide overlay dialogs.
   ///
   /// Areas bind to the *current* viewport, and the app has one: the central panel calls this,
   /// and the Settings window over it is drawn in the same pass.
   pub fn show_overlay_modals(&mut self, ui: &mut Ui) {
      let theme = &self.theme;
      self.msg_window.show(theme, ui);
      self.loading_window.show(theme, ui);
      self.confirm_window.show(theme, ui);
      self.connect_dapp_window.show(theme, ui);
      self.update_window.show(theme, ui);
   }

   /// Notice an attached screen reader and give egui its focus model.
   ///
   /// Runs once per frame on the root viewport pass. Nothing native sets
   /// [`egui::Options::screen_reader`], so once an AT has been seen Zeus sets it itself:
   /// with it on, egui makes every `Label` focusable, which is what puts prose on the
   /// keyboard path.
   ///
   /// Two signals, either one enough: the tree is being built for this pass (the root
   /// node exists only then), and an AccessKit action request in the frame's input (only
   /// a reader can send one).
   pub fn track_assistive_tech(&mut self, ui: &Ui) {
      if self.at_active {
         return;
      }

      let tree_built = ui.ctx().accesskit_node_builder(egui::accesskit_root_id(), |_| ()).is_some();

      let at_interacted = ui
         .ctx()
         .input(|i| i.events.iter().any(|e| matches!(e, egui::Event::AccessKitActionRequest(_))));

      if !(tree_built || at_interacted) {
         return;
      }

      self.at_active = true;
      ui.ctx().options_mut(|o| o.screen_reader = true);
      tracing::info!("Screen reader attached: prose is reachable with Tab from now on");
   }

   /// Queue `text` for the screen reader.
   ///
   /// One channel for the whole app, and the last call of a frame wins. It is spoken once,
   /// when the text changes, so pass a transition ("Transaction confirmed") rather than a
   /// state, and never a secret: any AT client can read the announcement node.
   pub fn announce(&mut self, text: impl Into<String>) {
      self.announcement = text.into();
   }

   /// Put this frame's announcement in the tree, once per viewport pass, after the panels.
   ///
   /// The text is set on every pass so the node that carries it outlives it — a live
   /// region created together with its message never speaks — and cleared once drawn, so
   /// the same words twice are two announcements.
   pub fn show_announcer(&mut self, ui: &Ui) {
      self.announcer.announce(ui, self.announcement.as_str());
      self.announcement.clear();
   }

   /// Close Zeus once the held attempt-cap notice has been acknowledged.
   ///
   /// Runs once per frame on the root pass, after the panels that draw the notice. The cap
   /// asks for a shutdown; holding it until the window is closed is what gives the notice a
   /// frame in which a reader can speak it.
   pub fn finish_lockout_notice(&mut self) {
      if !self.lockout_notice_held || self.msg_window.is_open() {
         return;
      }

      self.lockout_notice_held = false;
      self.shutdown();
   }

   /// Cancel every prompt that authorizes something, answering its awaiting
   /// flow with a rejection.
   ///
   /// Called while auto-locked: a prompt that was open when the lock fired must
   /// not be approvable, and must not resurface after unlock. The flow waiting
   /// on it (`sign_message`, `confirm_tx`, the connect server loop) then errors
   /// out instead of hanging.
   ///
   /// `is_busy` rather than `is_open`: a flow that claimed a window but has not
   /// drawn its prompt yet is waiting just the same, and rejecting only the drawn
   /// ones would leave it hanging for the whole locked period.
   pub fn cancel_pending_actions(&mut self) {
      if self.sign_msg_window.is_busy() {
         self.sign_msg_window.reject();
      }
      if self.tx_confirmation_window.is_busy() {
         self.tx_confirmation_window.reject();
      }
      if self.confirm_window.is_busy() {
         self.confirm_window.reject();
      }
      if self.connect_dapp_window.is_busy() {
         self.connect_dapp_window.reject();
      }
   }

   pub fn request_repaint(&self) {
      self.egui_ctx.request_repaint();
   }

   /// Close Zeus through the normal shutdown path (`ZeusApp::on_shutdown`),
   /// which saves the vault and erases in-memory secrets before the window
   /// closes. Used to refuse further credential tries after too many failures.
   pub fn shutdown(&self) {
      self.egui_ctx.send_viewport_cmd_to(
         egui::ViewportId::ROOT,
         egui::ViewportCommand::Close,
      );
   }

   /// Raise the main window so a dapp prompt is not hidden behind the browser.
   ///
   /// Also undoes a minimize-to-tray hide: the window is hidden with
   /// `ViewportCommand::Visible(false)` (see [`crate::gui::tray::hide_window`]), so
   /// without a `Visible(true)` here every other command is a no-op and the prompt
   /// never surfaces. `ViewportCommand::Focus` works on Windows, macOS, and X11 but is
   /// a no-op on Wayland, so taskbar/dock attention (flash / bounce) is asked for too.
   ///
   /// Called from a worker thread (the local server), so the app's hidden-in-tray flag
   /// is not touched here: [`Self::take_show_request`] is drained by
   /// [`ZeusApp::logic`](crate::gui::app::ZeusApp::logic), which also runs while the
   /// window is hidden.
   pub fn bring_to_front(&self) {
      let ctx = &self.egui_ctx;

      crate::gui::tray::show_window(ctx);
      ctx.send_viewport_cmd_to(
         egui::ViewportId::ROOT,
         egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical),
      );

      self.show_requested.store(true, Ordering::SeqCst);
      self.request_repaint();
   }

   /// Whether a [`Self::bring_to_front`] asked to raise the window since the last call.
   ///
   /// The frame path consumes it in
   /// [`ZeusApp::logic`](crate::gui::app::ZeusApp::logic) to clear the hidden-in-tray
   /// flag, so the next tray click hides the window again instead of no-oping on a
   /// window a dapp prompt already brought up.
   pub fn take_show_request(&self) -> bool {
      self.show_requested.swap(false, Ordering::SeqCst)
   }

   /// The page the central panel is showing.
   ///
   /// Pages are checked in the order the central panel draws them — the first open one is the one
   /// whose body a reader lands in — and a wallet with no page open is on Home. Settings is a page
   /// like the others now, but drawn as a window over the main window rather than by the central
   /// panel, so it is only named when no page at all is open; otherwise the heading keeps naming
   /// the page the main window still shows.
   pub fn current_page(&self) -> Page {
      if self.across_bridge.is_open() {
         return Page::Bridge;
      }
      if self.send_crypto.is_open() {
         return Page::Send;
      }
      if self.portofolio.is_open() {
         return Page::Home;
      }
      if self.uniswap.is_open() {
         return Page::Swap;
      }
      if self.shield_ui.is_open() {
         return Page::Shield;
      }
      if self.connected_dapps.is_open() {
         return Page::ConnectedDapps;
      }
      if self.wallet_ui.is_open() {
         return Page::Wallets;
      }
      if self.tx_history.is_open() {
         return Page::Transactions;
      }
      if self.approvals.is_open() {
         return Page::Approvals;
      }
      if self.settings.is_open() {
         return Page::Settings;
      }
      #[cfg(feature = "dev")]
      if self.dev.is_open() {
         return Page::Dev;
      }
      Page::Home
   }

   pub fn should_show_right_panel(&self) -> bool {
      self.uniswap.is_open()
   }
}

impl Default for GUI {
   fn default() -> Self {
      let icons = Arc::new(Icons::default());

      let theme_kind = if let Ok(kind) = load_theme_kind() {
         kind
      } else {
         ThemeKind::TokyoNight
      };

      let theme = Theme::new(theme_kind);

      GUI::new(icons, theme, Context::default())
   }
}
