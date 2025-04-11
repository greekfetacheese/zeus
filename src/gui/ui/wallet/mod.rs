pub mod add;
pub mod details;
pub use add::AddWalletUi;
pub use details::{DeleteWalletUi, ExportKeyUi};

use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use eframe::egui::{
   Align, Align2, Button, FontId, Frame, Label, Layout, Margin, RichText, ScrollArea, Sense, TextEdit, Ui, Vec2,
   Window, vec2,
};
use egui_theme::{Theme, utils::*};
use std::sync::Arc;
use zeus_eth::{types::SUPPORTED_CHAINS, utils::NumericValue};

/// Ui to manage the wallets
pub struct WalletUi {
   pub open: bool,
   pub main_ui: bool,
   pub add_wallet_ui: AddWalletUi,
   pub search_query: String,
   pub export_key_ui: ExportKeyUi,
   pub delete_wallet_ui: DeleteWalletUi,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl WalletUi {
   pub fn new() -> Self {
      let size = (550.0, 650.0);
      let offset = vec2(0.0, 0.0);
      let align = Align2::CENTER_CENTER;

      Self {
         open: false,
         main_ui: true,
         add_wallet_ui: AddWalletUi::new((450.0, 250.0), offset, align),
         search_query: String::new(),
         export_key_ui: ExportKeyUi::new(),
         delete_wallet_ui: DeleteWalletUi::new(),
         size,
         anchor: (align, offset),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(ctx.clone(), theme, icons.clone(), ui);
      self.add_wallet_ui.show(ctx.clone(), theme, ui);
      self.export_key_ui.show(ctx.clone(), theme, ui);
      self.delete_wallet_ui.show(ctx.clone(), theme, ui);
   }

   /// This is the first Ui we show to the user when this [WalletUi] is open.
   ///
   /// We can see, manage and add new wallets.
   pub fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.main_ui {
         return;
      }

      let mut wallets = ctx.wallets_info();
      let current_wallet = ctx.current_wallet();
      let mut portfolios = Vec::new();
      for chain in SUPPORTED_CHAINS {
         for wallet in &wallets {
            portfolios.push(ctx.get_portfolio(chain, wallet.address));
         }
      }

      wallets.sort_by(|a, b| {
         let addr_a = a.address;
         let addr_b = b.address;

         // Find the portfolio for each wallet
         let portfolio_a = portfolios.iter().find(|p| p.owner == addr_a);
         let portfolio_b = portfolios.iter().find(|p| p.owner == addr_b);

         // Extract the portfolio value (or use a default if not found)
         let value_a = portfolio_a
            .map(|p| p.value.clone())
            .unwrap_or(NumericValue::default());
         let value_b = portfolio_b
            .map(|p| p.value.clone())
            .unwrap_or(NumericValue::default());

         // Sort in descending order (highest value first)
         // If values are equal, sort by name as a secondary criterion
         value_b
            .f64()
            .partial_cmp(&value_a.f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
      });

      let frame = theme.frame1;
      Window::new("wallet_main_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(self.anchor.0, self.anchor.1)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 15.0);
            ui.spacing_mut().button_padding = Vec2::new(10.0, 8.0);

            ui.vertical_centered(|ui| {
               // Add Wallet Button
               ui.scope(|ui| {
                  let visuals = theme.get_button_visuals(frame.fill);
                  widget_visuals(ui, visuals);
                  if ui
                     .add(Button::new(
                        RichText::new("Add Wallet").size(theme.text_sizes.normal),
                     ))
                     .clicked()
                  {
                     self.add_wallet_ui.open = true;
                     self.add_wallet_ui.main_ui = true;
                  }
               });

               ui.add_space(10.0);
               ui.label(RichText::new("Selected Wallet").size(theme.text_sizes.large));
               self.wallet(ctx.clone(), theme, icons.clone(), &current_wallet, true, ui);

               // Search bar
               ui.add_space(8.0);
               ui.add(
                  TextEdit::singleline(&mut self.search_query)
                     .hint_text("Search...")
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .background_color(theme.colors.text_edit_bg2)
                     .min_size(vec2(ui.available_width() * 0.7, 20.0)),
               );
               ui.add_space(8.0);

               // Wallet list
               ScrollArea::vertical().show(ui, |ui| {
                  ui.set_width(ui.available_width());

                  for wallet in wallets.iter().filter(|w| *w != &current_wallet) {
                     if self.search_query.is_empty()
                        || wallet
                           .name
                           .to_lowercase()
                           .contains(&self.search_query.to_lowercase())
                     {
                        self.wallet(ctx.clone(), theme, icons.clone(), wallet, false, ui);

                        ui.add_space(4.0);
                     }
                  }
               });
            });
         });
   }

   /// Show a wallet
   fn wallet(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      wallet: &WalletInfo,
      is_current: bool,
      ui: &mut Ui,
   ) {
      let overlay = theme.colors.extreme_bg_color2;
      let bg_color = if is_current {
         overlay
      } else {
         theme.colors.bg_color
      };

      let mut frame = Frame::group(ui.style()).inner_margin(8.0).fill(bg_color);

      let visuals = if is_current {
         None
      } else {
         Some(theme.frame1_visuals.clone())
      };

      let res = frame_it(&mut frame, visuals, ui, |ui| {
         ui.set_width(ui.available_width() * 0.7);
         ui.spacing_mut().button_padding = Vec2::new(8.0, 8.0);
         let visuals = theme.get_button_visuals(bg_color);
         widget_visuals(ui, visuals);

         // Wallet info column
         ui.vertical(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               // Wallet name
               let name = Label::new(RichText::new(wallet.name.clone()).size(theme.text_sizes.normal));
               ui.scope(|ui| {
                  ui.set_width(ui.available_width() * 0.45);
                  ui.add(name);
               });

               // Export button
               let export_key = Button::new(RichText::new("Export Key").size(theme.text_sizes.small));
               if ui.add(export_key).clicked() {
                  self.export_key_ui.open = true;
                  self.export_key_ui.exporter.wallet = Some(wallet.clone());
                  self.export_key_ui.credentials_form.open = true;
               }
               ui.add_space(8.0);

               // Delete button
               let delete_wallet = Button::new(RichText::new("Delete Wallet").size(theme.text_sizes.small));
               if ui.add(delete_wallet).clicked() {
                  self.delete_wallet_ui.wallet_to_delete = Some(wallet.clone());
                  self.delete_wallet_ui.credentials_form.open = true;
               }
            });

            // Address and value
            ui.horizontal(|ui| {
               let res = ui.selectable_label(
                  false,
                  RichText::new(wallet.address_truncated()).size(theme.text_sizes.small),
               );
               if res.clicked() {
                  // Copy the address to the clipboard
                  ui.ctx().copy_text(wallet.address_string());
               }

               ui.add_space(10.0);
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                  ui.vertical(|ui| {
                     let chains = ctx.get_owner_chains(wallet.address);
                     let value = ctx.get_portfolio_value_all_chains(wallet.address);
                     ui.horizontal(|ui| {
                        for chain in chains {
                           let icon = icons.chain_icon_x16(&chain);
                           ui.add(icon);
                        }
                     });
                     ui.label(
                        RichText::new(format!("${}", value.formatted()))
                           .color(theme.colors.text_secondary)
                           .size(theme.text_sizes.small),
                     );
                  });
               });
            });
         });
      });

      if res.interact(Sense::click()).clicked() {
         let wallet_clone = wallet.clone();
         ctx.write_account(|account| {
            account.current_wallet = wallet.clone();
         });
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.wallet_selection.wallet_select.wallet = wallet_clone;
            });
         });
      }
   }
}
