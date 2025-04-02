pub mod add;
pub mod details;
pub use add::AddWalletUi;
pub use details::{ViewKeyUi, DeleteWalletUi};

use crate::assets::icons::Icons;
use crate::core::{Wallet, ZeusCtx};
use crate::gui::{
   SHARED_GUI,
   ui::{button, rich_text},
};
use eframe::egui::{
   Align, Align2, FontId, Frame, Label, Layout, Margin, ScrollArea, Sense, TextEdit, Ui, Vec2, Window, vec2,
};
use egui_theme::{Theme, utils::*};
use std::sync::Arc;

/// Ui to manage the wallets
pub struct WalletUi {
   pub open: bool,
   pub main_ui: bool,
   pub add_wallet_ui: AddWalletUi,
   pub search_query: String,
   pub view_key_ui: ViewKeyUi,
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
         view_key_ui: ViewKeyUi::new(),
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
      self.view_key_ui.show(ctx.clone(), theme, ui);
      self.delete_wallet_ui.show(ctx.clone(), theme, ui);
   }

   /// This is the first Ui we show to the user when this [WalletUi] is open.
   ///
   /// We can see, manage and add new wallets.
   pub fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, _icons: Arc<Icons>, ui: &mut Ui) {
      if !self.main_ui {
         return;
      }

      let account = ctx.account();
      let current_wallet = &account.current_wallet;
      let wallets = &account.wallets;

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
                     .add(button(
                        rich_text("Add Wallet").size(theme.text_sizes.normal),
                     ))
                     .clicked()
                  {
                     self.add_wallet_ui.open = true;
                     self.add_wallet_ui.main_ui = true;
                  }
               });

               ui.add_space(10.0);
               ui.label(rich_text("Selected Wallet").size(theme.text_sizes.large));
               self.wallet(ctx.clone(), theme, current_wallet, true, ui);

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

                  for wallet in wallets.iter().filter(|w| w != &current_wallet) {
                     if self.search_query.is_empty()
                        || wallet
                           .name
                           .to_lowercase()
                           .contains(&self.search_query.to_lowercase())
                     {
                        self.wallet(ctx.clone(), theme, wallet, false, ui);

                        ui.add_space(4.0);
                     }
                  }
               });
            });
         });
   }

   /// Show a wallet
   fn wallet(&mut self, ctx: ZeusCtx, theme: &Theme, wallet: &Wallet, is_current: bool, ui: &mut Ui) {
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

         ui.horizontal(|ui| {
            // Wallet info column
            ui.vertical(|ui| {
               ui.set_width(ui.available_width() * 0.7);

               ui.horizontal(|ui| {
                  ui.spacing_mut().button_padding = Vec2::new(10.0, 8.0);
                  ui.set_width(ui.available_width() * 0.45);

                  let name = Label::new(rich_text(wallet.name.clone()).heading()).wrap();
                  ui.add(name);

                  // Details button
                  let visuals = theme.get_button_visuals(bg_color);
                  widget_visuals(ui, visuals);
                  let view_key = button(rich_text("View Key").size(theme.text_sizes.small));
                  if ui.add(view_key).clicked() {
                     self.view_key_ui.open = true;
                     self.view_key_ui.exporter.wallet = Some(wallet.clone());
                     self.view_key_ui.credentials_form.open = true;
                  }
                  ui.add_space(5.0);

                  let delete_wallet = button(rich_text("Delete Wallet").size(theme.text_sizes.small));
                  if ui.add(delete_wallet).clicked() {
                     self.delete_wallet_ui.wallet_to_delete = Some(wallet.clone());
                     self.delete_wallet_ui.credentials_form.open = true;
                  }
               });

               // Address and value
               ui.horizontal(|ui| {
                  let res = ui.selectable_label(false, rich_text(wallet.address_truncated()).size(theme.text_sizes.small));
                  if res.clicked() {
                     // Copy the address to the clipboard
                     ui.ctx().copy_text(wallet.address_string());
                  }

                  ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                     let chain = ctx.chain().id();
                     let owner = wallet.key.borrow().address();
                     let portfolio = ctx.get_portfolio(chain, owner);
                     ui.label(
                        rich_text(format!("${}", portfolio.value.formatted()))
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
         ctx.write(|ctx| {
            ctx.account.current_wallet = wallet.clone();
         });
         std::thread::spawn(move || {
            SHARED_GUI.write(|gui| {
            gui.top_left_area.wallet_select.wallet = wallet_clone;
            });
         });
      }
   }
}
