pub mod app;
pub mod ui;

use eframe::egui::{Context, Ui};
use std::sync::{Arc, RwLock};
use ui::settings;

use crate::assets::icons::Icons;
use crate::core::context::ZeusCtx;
use egui_theme::{Theme, ThemeEditor, ThemeKind};
use lazy_static::lazy_static;

use crate::gui::ui::{
   ConfirmWindow, LoadingWindow, MsgWindow, PortfolioUi, ProgressWindow,
   RecipientSelectionWindow, UnlockVault, RecoverHDWallet, SendCryptoUi, SettingsUi, TestingWindow,
   TokenSelectionWindow, TxConfirmationWindow, TxWindow, WalletUi,
   dapps::{across::AcrossBridge, uniswap::UniswapUi},
   panels::{
      central_panel::FPSMetrics,
      top_panel::ChainSelection,
      top_panel::WalletSelection,
      left_panel::ConnectedDappsUi,
   },
   misc::dev::DevUi,
   sign_msg_window::SignMsgWindow,
   tx_history::TxHistory,
};

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

   pub fn open_loading(&self, msg: impl Into<String>) {
      self.write(|gui| gui.loading_window.open(msg));
   }

   pub fn reset_loading(&self) {
      self.write(|gui| gui.loading_window.reset());
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
   pub icons: Arc<Icons>,
   pub theme: Theme,
   pub chain_selection: ChainSelection,
   pub wallet_selection: WalletSelection,
   pub editor: ThemeEditor,
   pub uniswap: UniswapUi,
   pub across_bridge: AcrossBridge,
   pub token_selection: TokenSelectionWindow,
   pub recipient_selection: RecipientSelectionWindow,
   pub wallet_ui: WalletUi,
   pub unlock_vault_ui: UnlockVault,
   pub recover_wallet_ui: RecoverHDWallet,
   pub portofolio: PortfolioUi,
   pub send_crypto: SendCryptoUi,
   pub msg_window: MsgWindow,
   pub loading_window: LoadingWindow,
   pub settings: SettingsUi,
   pub tx_history: TxHistory,
   pub data_inspection: bool,
   pub testing_window: TestingWindow,
   pub progress_window: ProgressWindow,
   pub confirm_window: ConfirmWindow,
   pub tx_confirmation_window: TxConfirmationWindow,
   pub tx_window: TxWindow,
   pub sign_msg_window: SignMsgWindow,
   pub fps_metrics: FPSMetrics,
   pub connected_dapps: ConnectedDappsUi,
   pub dev: DevUi
}

impl GUI {
   pub fn new(icons: Arc<Icons>, theme: Theme, egui_ctx: Context) -> Self {
      let ctx = ZeusCtx::new();

      let token_selection = ui::TokenSelectionWindow::new();
      let recipient_selection = ui::RecipientSelectionWindow::new();
      let send_crypto = ui::SendCryptoUi::new();
      let across_bridge = ui::dapps::across::AcrossBridge::new();
      let chain_selection = ui::panels::top_panel::ChainSelection::new();
      let wallet_selection = ui::panels::top_panel::WalletSelection::new();

      let msg_window = ui::MsgWindow::new();
      let loading_window = ui::LoadingWindow::new();
      let confirm_window = ui::misc::ConfirmWindow::new();
      let tx_confirmation_window = TxConfirmationWindow::new();
      let tx_window = TxWindow::new();
      let wallet_ui = ui::WalletUi::new();
      let settings = settings::SettingsUi::new(ctx.clone());
      let tx_history = ui::tx_history::TxHistory::new();
      let progress_window = ui::misc::ProgressWindow::new();
      let sign_msg_window = SignMsgWindow::new();
      let connected_dapps = ConnectedDappsUi::new();

      Self {
         egui_ctx,
         ctx: ctx.clone(),
         theme,
         chain_selection,
         wallet_selection,
         editor: ThemeEditor::new(),
         icons,
         token_selection,
         recipient_selection,
         wallet_ui,
         uniswap: UniswapUi::new(),
         across_bridge,
         unlock_vault_ui: UnlockVault::new(),
         recover_wallet_ui: RecoverHDWallet::new(),
         portofolio: PortfolioUi::new(),
         send_crypto,
         msg_window,
         loading_window,
         settings,
         tx_history,
         data_inspection: false,
         testing_window: TestingWindow::new(),
         confirm_window,
         tx_confirmation_window,
         tx_window,
         progress_window,
         sign_msg_window,
         fps_metrics: FPSMetrics::new(),
         connected_dapps,
         dev: DevUi::new()
      }
   }

   pub fn show_top_panel(&mut self, ui: &mut Ui) {
      ui::panels::top_panel::show(self, ui);
   }

   pub fn show_left_panel(&mut self, ui: &mut Ui) {
      ui::panels::left_panel::show(ui, self);
   }

   pub fn show_right_panel(&mut self, ui: &mut Ui) {
      ui::panels::right_panel::show(ui, self);
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

   pub fn should_show_right_panel(&self) -> bool {
      self.uniswap.is_open()
   }
}

impl Default for GUI {
   fn default() -> Self {
      let icons = Arc::new(Icons::default());
      GUI::new(
         icons,
         Theme::new(ThemeKind::Nord),
         Context::default(),
      )
   }
}
