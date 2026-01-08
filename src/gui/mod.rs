pub mod app;
pub mod ui;

use eframe::egui::{Context, Ui};
use std::sync::{Arc, RwLock};
use ui::settings;

use crate::assets::icons::Icons;
use crate::core::context::{ZeusCtx, load_theme_kind};
use lazy_static::lazy_static;
use zeus_theme::{OverlayManager, Theme, ThemeEditor, ThemeKind};
use zeus_ui_components::QRScanner;

use crate::gui::ui::{
   ConfirmWindow, Header, LoadingWindow, MsgWindow, Notification, PortfolioUi,
   RecipientSelectionWindow, RecoverHDWallet, SendCryptoUi, SettingsUi, TokenSelectionWindow,
   TxConfirmationWindow, TxWindow, UnlockVault, UpdateWindow, WalletUi,
   dapps::{across::AcrossBridge, uniswap::UniswapUi},
   misc::dev::DevUi,
   panels::{central_panel::FPSMetrics, left_panel::ConnectedDappsUi},
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
   pub qr_scanner: QRScanner,
   pub egui_ctx: Context,
   pub ctx: ZeusCtx,
   pub icons: Arc<Icons>,
   pub overlay_manager: OverlayManager,
   pub theme: Theme,
   pub editor: ThemeEditor,
   pub uniswap: UniswapUi,
   pub across_bridge: AcrossBridge,
   pub header: Header,
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
   pub confirm_window: ConfirmWindow,
   pub tx_confirmation_window: TxConfirmationWindow,
   pub tx_window: TxWindow,
   pub sign_msg_window: SignMsgWindow,
   pub fps_metrics: FPSMetrics,
   pub connected_dapps: ConnectedDappsUi,
   pub notification: Notification,
   pub update_window: UpdateWindow,
   pub dev: DevUi,
}

impl GUI {
   pub fn new(icons: Arc<Icons>, theme: Theme, egui_ctx: Context) -> Self {
      let ctx = ZeusCtx::new();
      let overlay_manager = theme.overlay_manager.clone();

      let token_selection = ui::TokenSelectionWindow::new(overlay_manager.clone());
      let recipient_selection = ui::RecipientSelectionWindow::new(overlay_manager.clone());
      let send_crypto = ui::SendCryptoUi::new();
      let across_bridge = ui::dapps::across::AcrossBridge::new(overlay_manager.clone());
      let header = Header::new(overlay_manager.clone());

      let msg_window = ui::MsgWindow::new(overlay_manager.clone());
      let loading_window = ui::LoadingWindow::new(overlay_manager.clone());
      let confirm_window = ui::misc::ConfirmWindow::new(overlay_manager.clone());
      let tx_confirmation_window = TxConfirmationWindow::new(overlay_manager.clone());
      let tx_window = TxWindow::new(overlay_manager.clone());
      let wallet_ui = ui::WalletUi::new(overlay_manager.clone());
      let settings = settings::SettingsUi::new(ctx.clone(), overlay_manager.clone());
      let tx_history = ui::tx_history::TxHistory::new();
      let sign_msg_window = SignMsgWindow::new(overlay_manager.clone());
      let connected_dapps = ConnectedDappsUi::new(overlay_manager.clone());
      let notification = Notification::new();
      let update_window = UpdateWindow::new(overlay_manager.clone());
      let fps_metrics = FPSMetrics::new(overlay_manager.clone());
      let uniswap = UniswapUi::new(overlay_manager.clone());
      let unlock_vault_ui = UnlockVault::new();
      let recover_wallet_ui = RecoverHDWallet::new();

      Self {
         qr_scanner: QRScanner::new(),
         egui_ctx,
         ctx: ctx.clone(),
         overlay_manager,
         theme,
         editor: ThemeEditor::new(),
         icons,
         header,
         token_selection,
         recipient_selection,
         wallet_ui,
         uniswap,
         across_bridge,
         unlock_vault_ui,
         recover_wallet_ui,
         portofolio: PortfolioUi::new(),
         send_crypto,
         msg_window,
         loading_window,
         settings,
         tx_history,
         data_inspection: false,
         confirm_window,
         tx_confirmation_window,
         tx_window,
         sign_msg_window,
         fps_metrics,
         connected_dapps,
         notification,
         update_window,
         dev: DevUi::new(),
      }
   }

   pub fn show_top_panel(&mut self, ui: &mut Ui) {
      ui::panels::top_panel::show(self, ui);
   }

   pub fn show_bottom_panel(&mut self, ui: &mut Ui) {
      ui::panels::bottom_panel::show(ui, self);
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

      let theme_kind = if let Ok(kind) = load_theme_kind() {
         kind
      } else {
         ThemeKind::Dark
      };

      let theme = Theme::new(theme_kind);

      GUI::new(icons, theme, Context::default())
   }
}