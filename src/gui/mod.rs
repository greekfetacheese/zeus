pub mod ui;
pub mod utils;
pub mod window;

use eframe::egui::{Ui, Context};
use ui::theme;
use std::sync::{Arc, RwLock};

use egui_theme::{Theme, ThemeKind, ThemeEditor};
use crate::assets::icons::Icons;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SHARED_GUI: Arc<RwLock<GUI>> = Arc::new(RwLock::new(GUI::default()));
}

pub struct GUI {

    pub theme: Theme,

    pub editor: ThemeEditor,

    pub icons: Arc<Icons>,

    pub swap_ui: ui::SwapUi,

    pub token_selection: ui::TokenSelectionWindow,

    pub wallet_select: ui::WalletSelect,

    pub login: ui::LoginUi,

    pub register: ui::RegisterUi,

    pub portofolio: ui::PortfolioUi,

    pub send_crypto: ui::SendCrypto,

    pub msg_window: ui::MsgWindow,

    pub profile_area: ui::panels::top_panel::ProfileArea

}

impl GUI {
    pub fn new(icons: Arc<Icons>, theme: Theme) -> Self {
        let token_selection = ui::TokenSelectionWindow::new();
        let send_crypto = ui::SendCrypto::new();

        let wallet_select = ui::WalletSelect::new("wallet_select_1").width(100.0);

        Self {
            theme,
            editor: ThemeEditor::new(),
            icons,
            token_selection,
            wallet_select,
            swap_ui: ui::SwapUi::new(),
            login: ui::LoginUi::new(),
            register: ui::RegisterUi::new(),
            portofolio: ui::PortfolioUi::new(),
            send_crypto,
            msg_window: ui::MsgWindow::default(),
            profile_area: ui::panels::top_panel::ProfileArea::new()
        }
    }

    pub fn show_top_panel(&mut self, ui: &mut Ui) {
        ui::panels::top_panel::show(ui, self);
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
        GUI::new(icons, Theme::new(ThemeKind::Midnight))
    }
}