use eframe::egui::{ Ui, Color32, FontId, ScrollArea, Vec2b, TextEdit, Layout, Align };

use std::sync::Arc;
use crate::core::ZeusCtx;
use crate::assets::{ icons::Icons, fonts::roboto_regular };
use crate::gui::ui::{ rich_text, widgets::{ ChainSelect, WalletSelect } };
use egui_theme::Theme;
use zeus_eth::ChainId;

pub struct SendCryptoUi {
    pub open: bool,
    pub chain: ChainId,
    pub chain_select: ChainSelect,
    pub wallet_select: WalletSelect,
    pub contact_query: String,
    pub contact_search_open: bool,
    pub recipient: String,
}

impl SendCryptoUi {
    pub fn new() -> Self {
        Self {
            open: false,
            chain: ChainId::new(1).unwrap(),
            chain_select: ChainSelect::new("chain_select_2"),
            wallet_select: WalletSelect::new("wallet_select_2"),
            contact_query: String::new(),
            contact_search_open: false,
            recipient: String::new(),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        ui.set_width(400.0);
        ui.set_height(200.0);

        ui.spacing_mut().item_spacing.y = 10.0;

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(rich_text("Chain").size(16.0));
        });
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            self.chain_select.show(ui, theme, icons.clone());
        });

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(rich_text("From").size(16.0));
        });

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            self.wallet_select.show(ctx.clone(), ui);
        });

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(rich_text("To").size(16.0));
        });

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            self.search_contacts(ctx, ui);
        });

        //  });
    }

    fn search_contacts(&mut self, ctx: ZeusCtx, ui: &mut Ui) {
        let font = FontId::new(18.0, roboto_regular());

        let res = ui.add(
            TextEdit::singleline(&mut self.contact_query)
                .hint_text(rich_text("Search for contacts or enter an address"))
                .min_size((200.0, 30.0).into())
                .text_color(Color32::WHITE)
                .font(font)
                .frame(true)
        );

        // if the search query is empty keep the search area closed unless we clicked the text edit
        if self.contact_query.is_empty() {
            self.contact_search_open = res.has_focus();
        } else {
            self.contact_search_open = true;
        }

        self.contact_search_results(ctx, ui);
    }

    fn contact_search_results(&mut self, ctx: ZeusCtx, ui: &mut Ui) {
        if !self.contact_search_open {
            return;
        }

        let contacts = ctx.profile().contacts.clone();

        ScrollArea::vertical()
            .auto_shrink(Vec2b::new(false, false))
            .show(ui, |ui| {
                ui.label(rich_text("Contacts"));
                for contact in contacts {
                    if ui.label(rich_text(contact.name)).clicked() {
                        self.recipient = contact.address.to_string();
                    }
                }
            });
    }
}