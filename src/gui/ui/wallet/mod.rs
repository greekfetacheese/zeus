pub mod details;
pub mod add;
pub use details::WalletDetailsUi;
pub use add::AddWalletUi;

use eframe::egui::{ vec2, Align, Vec2, Frame, Sense, Label, Layout, Window, ScrollArea, Ui };
use egui::{ Align2, Color32 };
use egui_theme::{
    Theme,
    utils::{ frame_it, bg_color_on_idle },
};
use std::sync::Arc;
use crate::core::{Wallet, ZeusCtx};
use crate::assets::icons::Icons;
use crate::gui::ui::{ rich_text, button, img_button, text_edit_single };


/// Ui to manage the wallets
pub struct WalletUi {
    pub open: bool,
    pub main_ui: bool,
    pub add_wallet_ui: AddWalletUi,
    pub search_query: String,
    pub wallet_details: WalletDetailsUi,
    pub size: (f32, f32),
    pub anchor: (Align2, Vec2),
}

impl WalletUi {
    pub fn new() -> Self {
        let size = (400.0, 400.0);
        let offset = vec2(0.0, 150.0);
        let align = Align2::LEFT_TOP;

        Self {
            open: false,
            main_ui: true,
            add_wallet_ui: AddWalletUi::new(),
            search_query: String::new(),
            wallet_details: WalletDetailsUi::new(),
            size,
            anchor: (align, offset),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        self.main_ui(ctx.clone(), icons.clone(), theme, ui);
    }

    /// This is the first Ui we show to the user when this [WalletUi] is open.
    ///
    /// We can see, manage and add new wallets.
    pub fn main_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        let profile = ctx.profile();
        let current_wallet = &profile.current_wallet;
        let wallets = &profile.wallets;

        let mut open = self.open;
        Window::new("Current Wallet")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(self.anchor.0, self.anchor.1)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 12.0);

                ui.vertical_centered(|ui| {
                    ui.scope(|ui| {
                        if ui.add(button(rich_text("Add Wallet").small())).clicked() {
                            self.add_wallet_ui.open = true;
                            self.add_wallet_ui.main_ui = true;
                        }
                    });
                    self.wallet(ctx.clone(), icons.clone(), theme, current_wallet, true, ui);
                });
                ui.separator();

                // Search bar
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.add(
                        text_edit_single(&mut self.search_query)
                            .hint_text("Search...")
                            .desired_width(ui.available_width() * 0.8)
                    );
                    ui.add_space(8.0);
                });

                // Wallet list
                ScrollArea::vertical().show(ui, |ui| {
                    for wallet in wallets.iter().filter(|w| w != &current_wallet) {
                        if
                            self.search_query.is_empty() ||
                            wallet.name.to_lowercase().contains(&self.search_query.to_lowercase())
                        {
                            self.wallet(ctx.clone(), icons.clone(), theme, wallet, false, ui);

                            ui.add_space(4.0);
                        }
                    }
                });
            });
        self.open = open;
    }

    /// Show a wallet
    fn wallet(
        &mut self,
        ctx: ZeusCtx,
        icons: Arc<Icons>,
        theme: &Theme,
        wallet: &Wallet,
        is_current: bool,
        ui: &mut Ui
    ) {
        let bg_color = if is_current { theme.colors.overlay_color } else { Color32::TRANSPARENT };
        let stroke = if is_current { (0.0, Color32::TRANSPARENT) } else { (1.0, theme.colors.text_secondary) };

        let mut frame = Frame::group(ui.style()).inner_margin(8.0).fill(bg_color).stroke(stroke);

        let visuals = theme.frame1_visuals.clone();

        let res = frame_it(&mut frame, Some(visuals), ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                // Wallet info column
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() * 0.9);

                    ui.horizontal(|ui| {
                        // Name can only take 45% of the available width
                        ui.set_width(ui.available_width() * 0.45);

                        let name = Label::new(rich_text(wallet.name.clone()).heading()).wrap();
                        ui.add(name);

                        // Details button
                        bg_color_on_idle(ui, Color32::TRANSPARENT);
                        let button = img_button(icons.right_arrow(), rich_text("Details").small());
                        if ui.add(button).clicked() {
                            self.wallet_details.open = true;
                            self.wallet_details.wallet = wallet.clone();
                        }
                    });

                    // Address and balance
                    ui.horizontal(|ui| {
                        let res = ui.selectable_label(false, rich_text(wallet.address_truncated()).size(12.0));
                        if res.clicked() {
                            // Copy the address to the clipboard
                            ui.ctx().copy_text(wallet.address());
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // TODO: Calculate the total value of a wallet
                            ui.label(rich_text("$100,000.00").color(theme.colors.text_secondary).size(12.0));
                        });
                    });
                });
            });
        });

        if res.interact(Sense::click()).clicked() {
            ctx.write(|ctx| {
                ctx.profile.current_wallet = wallet.clone();
            });
        }
    }
}