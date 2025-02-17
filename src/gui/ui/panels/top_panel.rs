use egui::{ Align, Grid, Layout, Ui };
use std::sync::Arc;
use crate::core::ZeusCtx;
use crate::assets::icons::Icons;
use crate::gui::{ GUI, ui::{ wallet::WalletUi, ChainSelect, rich_text, img_button } };
use egui_theme::Theme;

pub fn show(gui: &mut GUI, ui: &mut Ui) {
    let ctx = gui.ctx.clone();
    let icons = gui.icons.clone();
    let theme = &gui.theme;

    ui.vertical(|ui| {
        ui.set_width(gui.profile_area.size.0);
        ui.set_height(gui.profile_area.size.1);
        ui.spacing_mut().item_spacing.y = 20.0;

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let clicked = gui.profile_area.chain_select.show(ui, theme, icons.clone());
            if clicked {
                // if we select a new chain update the necessary state
                let chain = gui.profile_area.chain_select.chain.clone();
                gui.swap_ui.default_currency_in(chain.id());
                gui.swap_ui.default_currency_out(chain.id());

                // update the chain
                ctx.write(|ctx| {
                    ctx.chain = chain.clone();
                });
            }
        });

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            gui.profile_area.show(ctx, icons, &gui.theme, ui);
        });
    });
}

// ! Rename to something else?
pub struct ProfileAreaUi {
    pub open: bool,
    pub main_ui: bool,
    pub wallet_ui: WalletUi,
    pub chain_select: ChainSelect,
    pub size: (f32, f32),
}

impl ProfileAreaUi {
    pub fn new() -> Self {
        Self {
            open: false,
            main_ui: true,
            wallet_ui: WalletUi::new(),
            chain_select: ChainSelect::new("chain_select_1").width(100.0),
            size: (300.0, 140.0),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        self.main_ui(ctx.clone(), icons.clone(), ui);
        self.wallet_ui.show(ctx.clone(), icons.clone(), theme, ui);
    }

    pub fn main_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        let wallet = ctx.wallet();
        ui.vertical(|ui| {
            // Show the current wallet, if clicked open the wallet_ui
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                Grid::new("profile_grid")
                    .min_row_height(30.0)
                    .show(ui, |ui| {
                        // Wallet button
                        let text = rich_text(wallet.name.clone()).size(16.0);
                        if ui.add(img_button(icons.right_arrow(), text)).clicked() {
                            self.wallet_ui.open = !self.wallet_ui.open;
                        }
                    });
            });

        });
    }
}