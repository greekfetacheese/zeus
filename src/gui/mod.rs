pub mod app;
pub mod ui;
pub mod window;

use eframe::egui::{Context, Ui};
use std::sync::{Arc, RwLock};
use ui::settings;

use crate::assets::icons::Icons;
use crate::core::context::ZeusCtx;
use egui_theme::{Theme, ThemeEditor, ThemeKind};
use lazy_static::lazy_static;

lazy_static! {
   pub static ref SHARED_GUI: SharedGUI = SharedGUI::default();
}

#[derive(Clone)]
pub struct SharedGUI(Arc<RwLock<GUI>>);

impl SharedGUI {
   /// Shared access to the [GUI]
   pub fn read<R>(&self, reader: impl FnOnce(&GUI) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   /// Exclusive mutable access to the [GUI]
   pub fn write<R>(&self, writer: impl FnOnce(&mut GUI) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   pub fn request_repaint(&self) {
      self.read(|gui| gui.request_repaint());
   }
}

impl Default for SharedGUI {
   fn default() -> Self {
      Self(Arc::new(RwLock::new(GUI::default())))
   }
}

pub struct GUI {
   pub egui_ctx: Context,

   pub ctx: ZeusCtx,

   pub theme: Theme,

   pub chain_selection: ui::panels::top_panel::ChainSelection,

   pub wallet_selection: ui::panels::top_panel::WalletSelection,

   /// True if there is any [egui::Window] open
   pub show_overlay: bool,

   pub editor: ThemeEditor,

   pub icons: Arc<Icons>,

   pub swap_ui: ui::SwapUi,

   pub across_bridge: ui::dapps::across::AcrossBridge,

   pub token_selection: ui::TokenSelectionWindow,

   pub recipient_selection: ui::RecipientSelectionWindow,

   pub wallet_ui: ui::WalletUi,

   pub login: ui::LoginUi,

   pub register: ui::RegisterUi,

   pub portofolio: ui::PortfolioUi,

   pub send_crypto: ui::SendCryptoUi,

   pub msg_window: ui::MsgWindow,

   pub loading_window: ui::LoadingWindow,

   pub settings: ui::settings::SettingsUi,

   pub tx_history: ui::tx_history::TxHistory,

   pub data_inspection: bool,

   pub testing_window: ui::misc::TestingWindow,

   pub ui_testing: ui::panels::central_panel::UiTesting,

   pub progress_window: ui::misc::ProgressWindow,

   pub confirm_window: ui::misc::ConfirmWindow,

   pub tx_confirm_window: ui::TxConfirmWindow,

   pub sign_msg_window: ui::misc::SignMsgWindow,
}

impl GUI {
   pub fn new(icons: Arc<Icons>, theme: Theme, egui_ctx: Context) -> Self {
      let token_selection = ui::TokenSelectionWindow::new();
      let recipient_selection = ui::RecipientSelectionWindow::new();
      let send_crypto = ui::SendCryptoUi::new();
      let across_bridge = ui::dapps::across::AcrossBridge::new();
      let chain_selection = ui::panels::top_panel::ChainSelection::new();
      let wallet_selection = ui::panels::top_panel::WalletSelection::new();

      let msg_window = ui::MsgWindow::new();
      let loading_window = ui::LoadingWindow::new();
      let confirm_window = ui::misc::ConfirmWindow::new();
      let tx_confirm_window = ui::TxConfirmWindow::new();
      let wallet_ui = ui::WalletUi::new();
      let settings = settings::SettingsUi::new();
      let tx_history = ui::tx_history::TxHistory::new();
      let ui_testing = ui::panels::central_panel::UiTesting::new();
      let progress_window = ui::misc::ProgressWindow::new();
      let sign_msg_window = ui::misc::SignMsgWindow::new();

      Self {
         egui_ctx,
         ctx: ZeusCtx::new(),
         theme,
         chain_selection,
         wallet_selection,
         show_overlay: false,
         editor: ThemeEditor::new(),
         icons,
         token_selection,
         recipient_selection,
         wallet_ui,
         swap_ui: ui::SwapUi::new(),
         across_bridge,
         login: ui::LoginUi::new(),
         register: ui::RegisterUi::new(),
         portofolio: ui::PortfolioUi::new(),
         send_crypto,
         msg_window,
         loading_window,
         settings,
         tx_history,
         data_inspection: false,
         testing_window: ui::misc::TestingWindow::new(),
         ui_testing,
         confirm_window,
         tx_confirm_window,
         progress_window,
         sign_msg_window,
      }
   }

   pub fn show_top_panel(&mut self, ui: &mut Ui) {
      ui::panels::top_panel::show(self, ui);
   }

   pub fn show_left_panel(&mut self, ui: &mut Ui) {
      ui::panels::left_panel::show(ui, self);
   }

   pub fn show_central_panel(&mut self, ui: &mut Ui) {
      ui::panels::central_panel::show(ui, self);
   }

   pub fn open_msg_window(&mut self, title: impl Into<String>, msg: impl Into<String>) {
      self.msg_window.open(title, msg);
   }

   pub fn request_repaint(&self) {
      self.egui_ctx.request_repaint();
   }
}

impl Default for GUI {
   fn default() -> Self {
      let icons = Arc::new(Icons::new(&Context::default()).unwrap());
      GUI::new(
         icons,
         Theme::new(ThemeKind::Mocha),
         Context::default(),
      )
   }
}
