pub mod app;
pub mod ui;

use eframe::egui::{Color32, Context, Ui};
use std::sync::{Arc, RwLock};
use ui::settings;

use crate::assets::icons::Icons;
use crate::core::context::{ZeusContext, ZeusCtx, load_theme_kind};
use lazy_static::lazy_static;
use zeus_theme::{OverlayManager, Theme, ThemeEditor, ThemeKind};

pub use crate::gui::ui::{
   ConfirmWindow, Header, LoadingWindow, MsgWindow, Notification, PortfolioUi,
   RecipientSelectionWindow, RecoverHDWallet, SendCryptoUi, SettingsUi, TokenSelectionWindow,
   TxConfirmationWindow, TxWindow, UnlockVault, UpdateWindow, WalletUi,
   common::dots_button,
   dapps::{across::AcrossBridge, railgun::ShieldUi, uniswap::UniswapUi},
   dev::DevUi,
   panels::{central_panel::FPSMetrics, left_panel::ConnectedDappsUi},
   settings::RailgunSettings,
   sign_msg_window::SignMsgWindow,
   tx_history::TxHistory,
};

use elegance::Theme as EleganceTheme;

lazy_static! {
   pub static ref SHARED_GUI: SharedGUI = SharedGUI::default();
}

pub fn elegance_theme_key() -> egui::Id {
   egui::Id::new("elegance::theme")
}

/// Zeus theme colours to use for the elegance palette.
#[derive(Clone, Copy, PartialEq, Eq)]
struct EleganceThemeKey {
   dark: bool,
   bg: Color32,
   widget_bg: Color32,
   border: Color32,
   text: Color32,
   text_muted: Color32,
   accent: Color32,
   info: Color32,
   success: Color32,
   error: Color32,
   warning: Color32,
}

impl EleganceThemeKey {
   fn from_theme(theme: &Theme) -> Self {
      let c = &theme.colors;
      Self {
         dark: theme.dark_mode,
         bg: c.bg,
         widget_bg: c.widget_bg,
         border: c.border,
         text: c.text,
         text_muted: c.text_muted,
         accent: c.accent,
         info: c.info,
         success: c.success,
         error: c.error,
         warning: c.warning,
      }
   }
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
   pub overlay_manager: OverlayManager,
   pub theme: Theme,
   pub editor: ThemeEditor,
   pub shield_ui: ShieldUi,
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
   pub railgun_settings: RailgunSettings,
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
   pub merge_notes_window: ui::dapps::railgun::MergeNotesWindow,

   /// Last Zeus colour fingerprint we injected into `ctx.data`.
   /// `None` = never injected this session.
   injected_elegance_key: Option<EleganceThemeKey>,
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

      let msg_window = ui::MsgWindow::new();
      let loading_window = ui::LoadingWindow::new();
      let confirm_window = ui::common::ConfirmWindow::new(overlay_manager.clone());
      let tx_confirmation_window = TxConfirmationWindow::new(overlay_manager.clone());
      let tx_window = TxWindow::new(overlay_manager.clone());
      let wallet_ui = ui::WalletUi::new(overlay_manager.clone());

      let settings = ctx.write(|ctx| settings::SettingsUi::new(ctx, overlay_manager.clone()));
      let railgun_settings =
         ctx.write(|ctx| settings::RailgunSettings::new(ctx, overlay_manager.clone()));

      let tx_history = ui::tx_history::TxHistory::new();
      let sign_msg_window = SignMsgWindow::new(overlay_manager.clone());
      let connected_dapps = ConnectedDappsUi::new(overlay_manager.clone());
      let notification = Notification::new();
      let update_window = UpdateWindow::new(overlay_manager.clone());
      let fps_metrics = FPSMetrics::new(overlay_manager.clone());
      let uniswap = UniswapUi::new(overlay_manager.clone());
      let shield_ui = ShieldUi::new();
      let merge_notes_window = ui::dapps::railgun::MergeNotesWindow::new();
      let unlock_vault_ui = UnlockVault::new();
      let recover_wallet_ui = RecoverHDWallet::new();

      Self {
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
         shield_ui,
         uniswap,
         across_bridge,
         unlock_vault_ui,
         recover_wallet_ui,
         portofolio: PortfolioUi::new(),
         send_crypto,
         msg_window,
         loading_window,
         settings,
         railgun_settings,
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
         merge_notes_window,
         injected_elegance_key: None,
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
      self.msg_window.open(msg);
   }

   pub fn request_repaint(&self) {
      self.egui_ctx.request_repaint();
   }

   pub fn should_show_right_panel(&self) -> bool {
      self.uniswap.is_open()
   }

   /// Inject an elegance [`Theme`] built from the active Zeus theme into
   /// `ctx.data` under the key elegance reads, so elegance widgets
   /// (`TabBar`, `Card`, `StatusPill`, `Indicator`) take Zeus's colours and
   /// respect light/dark without disturbing the rest of the UI.
   pub fn inject_elegance_theme(&mut self, ctx: &egui::Context) {
      let key = EleganceThemeKey::from_theme(&self.theme);
      if self.injected_elegance_key == Some(key) {
         return;
      }

      let c = &self.theme.colors;
      let mut pal = if key.dark {
         elegance::Palette::charcoal()
      } else {
         elegance::Palette::frost()
      };

      pal.is_dark = key.dark;
      pal.bg = c.bg;
      pal.card = c.widget_bg;
      pal.input_bg = c.widget_bg;
      pal.border = c.border;
      pal.text = c.text;
      pal.text_muted = c.text_muted;
      pal.text_faint = c.text_muted;
      pal.focus = c.accent;
      pal.blue = c.info;
      pal.green = c.success;
      pal.green_hover = c.success;
      pal.red = c.error;
      pal.red_hover = c.error;
      pal.amber = c.warning;
      pal.amber_hover = c.warning;
      pal.purple = c.accent;
      pal.purple_hover = c.accent;
      pal.success = c.success;
      pal.danger = c.error;
      pal.warning = c.warning;

      let elegance_theme = EleganceTheme {
         palette: pal,
         ..EleganceTheme::slate()
      };

      ctx.data_mut(|d| d.insert_temp(elegance_theme_key(), elegance_theme));
      self.injected_elegance_key = Some(key);
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
