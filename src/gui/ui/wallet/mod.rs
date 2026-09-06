use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusContext};
use crate::gui::{
   SHARED_GUI, dots_button,
   ui::{WalletListByValue, show_with_fade},
};
use crate::utils::RT;
use eframe::egui::{
   Align, Context, FontId, Id, Layout, Margin, Order, RichText, ScrollArea, Spinner, Ui, vec2,
};
use egui_elements::Modal;
use egui_elements::{Button, Label, SecureTextEdit, Theme};
use elegance::{Menu, MenuItem};
use std::{collections::HashMap, sync::Arc};
use zeus_eth::{alloy_primitives::Address, utils::NumericValue};
use zeus_wallet::Wallet;

pub mod add;
pub mod delete;
pub mod discover;
pub mod export;
pub mod import;

pub use add::AddWalletUi;
pub use delete::DeleteWalletUi;
pub use export::ExportKeyUi;

/// Ui to manage the wallets
pub struct WalletUi {
   open: bool,
   loading: bool,
   rename_wallet: bool,
   new_wallet_name: String,
   wallet_to_rename: Option<Wallet>,
   pub add_wallet_ui: AddWalletUi,
   search_query: String,
   export_key_ui: ExportKeyUi,
   delete_wallet_ui: DeleteWalletUi,
   wallets: Vec<WalletInfo>,
   /// Wallet value by address
   wallet_value: HashMap<Address, NumericValue>,
   /// Chains that the wallet has balance on
   wallet_chains: HashMap<Address, Vec<u64>>,
   size: (f32, f32),
}

impl WalletUi {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         rename_wallet: false,
         new_wallet_name: String::new(),
         wallet_to_rename: None,
         add_wallet_ui: AddWalletUi::new(),
         search_query: String::new(),
         export_key_ui: ExportKeyUi::new(),
         delete_wallet_ui: DeleteWalletUi::new(),
         wallets: Vec::new(),
         wallet_value: HashMap::new(),
         wallet_chains: HashMap::new(),
         size: (550.0, 600.0),
      }
   }

   pub fn erase(&mut self, ctx: &Context) {
      self.export_key_ui.erase(ctx);
      self.delete_wallet_ui.erase();
      self.add_wallet_ui.erase();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open_rename_wallet(&mut self, wallet: Option<Wallet>) {
      self.rename_wallet = true;
      self.wallet_to_rename = wallet;
   }

   pub fn close_rename_wallet(&mut self) {
      self.rename_wallet = false;
      self.wallet_to_rename = None;
      self.new_wallet_name.clear();
   }

   pub fn open(&mut self) {
      self.open = true;
      self.calc_wallet_value();
   }

   pub fn calc_wallet_value(&mut self) {
      self.loading = true;

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let list = WalletListByValue::collect(&ctx);

         SHARED_GUI.write(|gui| {
            gui.wallet_ui.loading = false;
            gui.wallet_ui.wallets = list.wallets;
            gui.wallet_ui.wallet_value = list.values;
            gui.wallet_ui.wallet_chains = list.chains;
         });
      });
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      show_with_fade(ui, "wallet_ui_fade", self.open, |ui| {
         self.main_ui(ctx, theme, icons.clone(), ui);
      });

      self.rename_wallet(theme, ui);
      self.add_wallet_ui.show(ctx, theme, icons.clone(), ui);
      self.export_key_ui.show(ctx, theme, ui);
      self.delete_wallet_ui.show(ctx, theme, ui);
   }

   /// This is the first Ui we show to the user when this [WalletUi] is open.
   ///
   /// We can see, manage and add new wallets.
   fn main_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let frame = theme.frame1;

      ui.vertical_centered(|ui| {
         frame.show(ui, |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
            ui.spacing_mut().button_padding = theme.button_padding;

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();
            let content_width = ui.available_width() * 0.9;

            ui.vertical_centered(|ui| {
               let text = RichText::new("Add Wallet").size(theme.typography.large);
               let button =
                  Button::new(text).visuals(button_visuals).min_size(vec2(content_width, 45.0));

               if ui.add(button).clicked() {
                  self.add_wallet_ui.open();
               }

               ui.add_space(10.0);

               let current_wallet = ctx.current_wallet_info();

               ui.label(RichText::new("Selected Wallet").size(theme.typography.large));
               self.wallet(
                  ctx,
                  theme,
                  icons.clone(),
                  &current_wallet,
                  content_width,
                  ui,
               );

               ui.add_space(10.0);

               let hint = RichText::new("Search...")
                  .color(theme.colors.text_muted)
                  .size(theme.typography.normal);

               ui.allocate_ui(vec2(content_width, 45.0), |ui| {
                  SecureTextEdit::singleline(&mut self.search_query)
                     .visuals(text_edit_visuals)
                     .hint_text(hint)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.typography.normal))
                     .desired_width(ui.available_width())
                     .show(ui);
               });

               if self.loading {
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  return;
               }

               let wallets = self.wallets.clone();
               let query = self.search_query.to_lowercase();

               ScrollArea::vertical().content_margin(5).auto_shrink([false; 2]).show(ui, |ui| {
                  ui.set_width(ui.available_width());

                  for wallet in wallets.iter().filter(|w| *w != &current_wallet) {
                     if query.is_empty()
                        || wallet.name_with_source().to_lowercase().contains(&query)
                     {
                        self.wallet(
                           ctx,
                           theme,
                           icons.clone(),
                           wallet,
                           content_width,
                           ui,
                        );
                     }
                  }
               });
            });
         });
      });
   }

   /// Show a wallet
   fn wallet(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      wallet: &WalletInfo,
      width: f32,
      ui: &mut Ui,
   ) {
      let frame = theme.frame2;
      let tint = theme.image_tint_recommended;
      let button_visuals = theme.button_visuals();

      frame.show(ui, |ui| {
         ui.set_width(width);
         ui.spacing_mut().button_padding = theme.button_padding;

         ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let id = format!("{}_more_options", wallet.address);
               let more = dots_button(theme, ui);
               let enabled = !wallet.is_master();

               Menu::new(id).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("Export").shortcut("⌘ E")).clicked() {
                     let wallet = ctx.get_wallet(wallet.address);
                     self.export_key_ui.open(wallet);
                  }

                  if ui.add(MenuItem::new("Rename").shortcut("⌘ R")).clicked() {
                     let wallet_opt = ctx.get_wallet(wallet.address);
                     self.open_rename_wallet(wallet_opt);
                  }

                  if ui.add(MenuItem::new("Show QR Code").shortcut("⌘ Q")).clicked() {
                     let wallet_clone = wallet.clone();
                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.header.qrcode_window.open(wallet_clone);
                           gui.request_repaint();
                        });
                     });
                  }

                  if ui.add_enabled(enabled, MenuItem::new("Delete").shortcut("⌘ D")).clicked() {
                     self.delete_wallet_ui.open(wallet.clone());
                  }
               });

               ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                  let text = RichText::new(wallet.name_with_source()).size(theme.typography.normal);
                  let label = Label::new(text, None).interactive(false);
                  ui.add(label);
               });
            });
         });

         ui.horizontal(|ui| {
            let text = RichText::new(wallet.evm_address_truncated()).size(theme.typography.small);
            let label = Button::selectable(false, text).visuals(button_visuals);

            if ui.add(label).clicked() {
               ui.ctx().copy_text(wallet.address.to_string());
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.sm;

               let value = self.wallet_value.get(&wallet.address).cloned().unwrap_or_default();
               let value_text =
                  RichText::new(format!("${}", value.abbreviated())).size(theme.typography.small);
               let label = Label::new(value_text, None).interactive(false);
               ui.add(label);

               let chains = self.wallet_chains.get(&wallet.address).cloned().unwrap_or_default();
               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.xs;
                  for chain in chains {
                     if ctx.is_chain_disabled(chain) {
                        continue;
                     }

                     let icon = icons.chain_icon(chain, tint).fit_to_exact_size(vec2(16.0, 16.0));
                     ui.add(icon);
                  }
               });
            });
         });
      });
   }

   /// Rename wallet UI
   fn rename_wallet(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.rename_wallet {
         return;
      }

      let mut open = self.rename_wallet;

      let title = RichText::new("Rename Wallet").size(theme.typography.heading);
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let id = Id::new("rename_wallet_window");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .closable(true)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(450.0);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().button_padding = theme.button_padding;

               let Some(old_wallet) = self.wallet_to_rename.as_ref() else {
                  ui.label(RichText::new("No wallet selected").size(theme.typography.large));
                  return;
               };

               ui.label(RichText::new("Wallet Name").size(theme.typography.large));

               let field_size = vec2(ui.available_width() * 0.9, 45.0);
               ui.allocate_ui(field_size, |ui| {
                  SecureTextEdit::singleline(&mut self.new_wallet_name)
                     .visuals(text_edit_visuals)
                     .font(FontId::proportional(theme.typography.normal))
                     .margin(Margin::same(10))
                     .desired_width(ui.available_width())
                     .show(ui);
               });

               let text = RichText::new("Rename").size(theme.typography.large);
               let rename_button = Button::new(text).visuals(button_visuals).min_size(field_size);

               if ui.add(rename_button).clicked() {
                  let new_wallet_name = self.new_wallet_name.clone();
                  let old_wallet = old_wallet.clone();
                  let old_wallet_addr = old_wallet.address();

                  // On failure, revert the changes
                  RT.spawn_blocking(move || {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                     let old_vault = ctx.get_vault();

                     if old_vault.wallet_name_exists(&new_wallet_name) {
                        SHARED_GUI.write(|gui| {
                           gui.open_msg_window(format!(
                              "Wallet with name {} already exists",
                              &new_wallet_name
                           ));
                           gui.request_repaint();
                        });
                        return;
                     }

                     if new_wallet_name.is_empty() {
                        SHARED_GUI.write(|gui| {
                           gui.open_msg_window("Wallet name cannot be empty");
                           gui.request_repaint();
                        });
                        return;
                     }

                     let max_chars = old_vault.name_max_chars();

                     if new_wallet_name.chars().count() > max_chars {
                        SHARED_GUI.write(|gui| {
                           gui.open_msg_window(format!(
                              "Wallet name cannot be longer than {} characters",
                              max_chars
                           ));
                           gui.request_repaint();
                        });
                        return;
                     }

                     let mut new_wallet = old_wallet.clone();
                     new_wallet.name = new_wallet_name;

                     let mut new_vault = old_vault.clone();

                     for wallet in new_vault.all_wallets_mut() {
                        if wallet.address() == old_wallet_addr {
                           *wallet = new_wallet.clone();
                        }
                     }

                     let is_current = ctx.is_current_wallet(new_wallet.address());

                     if is_current {
                        ctx.write(|ctx| {
                           ctx.current_wallet = new_wallet.clone();
                        });
                     }

                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Encrypting vault...");
                     });

                     match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              // Update header
                              if is_current {
                                 gui.header.set_current_wallet(new_wallet);
                              }

                              // Reset state
                              gui.wallet_ui.close_rename_wallet();

                              gui.loading_window.reset();
                              gui.open_msg_window("Success");
                              gui.request_repaint();
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.loading_window.reset();
                              gui.open_msg_window(format!(
                                 "Failed to encrypt vault, changes reverted: {}",
                                 e.to_string()
                              ));
                              gui.request_repaint();
                           });
                           return;
                        }
                     };

                     ctx.set_vault(new_vault);
                     ctx.build_wallet_info_cache();

                     // Calculate the wallets again
                     SHARED_GUI.write(|gui| {
                        gui.wallet_ui.calc_wallet_value();
                     });
                  });
               }
            });
         });

      if !open {
         self.close_rename_wallet();
      }
   }
}
