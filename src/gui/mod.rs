pub mod ui;
pub mod utils;
pub mod window;
pub mod app;

use eframe::egui::{Ui, Context};
use std::sync::{Arc, RwLock};

use egui_theme::{Theme, ThemeKind, ThemeEditor};
use crate::core::context::ZeusCtx;
use crate::assets::icons::Icons;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SHARED_GUI: Arc<RwLock<GUI>> = Arc::new(RwLock::new(GUI::default()));
}

pub struct GUI {

    pub egui_ctx: Context,

    pub ctx: ZeusCtx,

    pub theme: Theme,

    /// True if there is any [egui::Window] open
    pub show_overlay: bool,

    pub editor: ThemeEditor,

    pub icons: Arc<Icons>,

    pub swap_ui: ui::SwapUi,

    pub token_selection: ui::TokenSelectionWindow,

    pub wallet_select: ui::WalletSelect,

    pub login: ui::LoginUi,

    pub register: ui::RegisterUi,

    pub portofolio: ui::PortfolioUi,

    pub send_crypto: ui::SendCryptoUi,

    pub msg_window: ui::MsgWindow,

    pub loading_window: ui::LoadingWindow,

    pub profile_area: ui::panels::top_panel::ProfileAreaUi,

    pub settings: ui::SettingsUi,

    pub data_inspection: bool,
}

impl GUI {
    pub fn new(icons: Arc<Icons>, theme: Theme, egui_ctx: Context) -> Self {
        let token_selection = ui::TokenSelectionWindow::new();
        let send_crypto = ui::SendCryptoUi::new();

        let wallet_select = ui::WalletSelect::new("wallet_select_1").width(100.0);
        let msg_window = ui::MsgWindow::new(Some(theme.colors.bg_color));

        Self {
            egui_ctx,
            ctx: ZeusCtx::new(),
            theme,
            show_overlay: false,
            editor: ThemeEditor::new(),
            icons,
            token_selection,
            wallet_select,
            swap_ui: ui::SwapUi::new(),
            login: ui::LoginUi::new(),
            register: ui::RegisterUi::new(),
            portofolio: ui::PortfolioUi::new(),
            send_crypto,
            msg_window,
            loading_window: ui::LoadingWindow::new(),
            profile_area: ui::panels::top_panel::ProfileAreaUi::new(),
            settings: ui::SettingsUi::new(),
            data_inspection: false,
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

}

impl Default for GUI {
    fn default() -> Self {
        let icons = Arc::new(Icons::new(&Context::default()).unwrap());
        GUI::new(icons, Theme::new(ThemeKind::Midnight), Context::default())
    }
}