use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{
   Align, Align2, FontId, Layout, Margin, Order, RichText, ScrollArea, Sense, Spinner, Ui, Vec2,
   Window, vec2,
};
use std::{collections::HashMap, sync::Arc};
use zeus_eth::{alloy_primitives::Address, types::SUPPORTED_CHAINS, utils::NumericValue};
use zeus_theme::{OverlayManager, Theme, utils::frame_it};
use zeus_wallet::Wallet;
use zeus_widgets::{Button, Label, SecureTextEdit};

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
   overlay: OverlayManager,
   main_ui: bool,
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
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         loading: false,
         overlay: overlay.clone(),
         main_ui: true,
         rename_wallet: false,
         new_wallet_name: String::new(),
         wallet_to_rename: None,
         add_wallet_ui: AddWalletUi::new(overlay.clone()),
         search_query: String::new(),
         export_key_ui: ExportKeyUi::new(overlay.clone()),
         delete_wallet_ui: DeleteWalletUi::new(overlay),
         wallets: Vec::new(),
         wallet_value: HashMap::new(),
         wallet_chains: HashMap::new(),
         size: (550.0, 600.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open_rename_wallet(&mut self, wallet: Option<Wallet>) {
      if !self.rename_wallet {
         self.overlay.window_opened();
      }
      self.rename_wallet = true;
      self.wallet_to_rename = wallet;
   }

   pub fn close_rename_wallet(&mut self) {
      self.overlay.window_closed();
      self.rename_wallet = false;
      self.wallet_to_rename = None;
      self.new_wallet_name.clear();
   }

   pub fn open(&mut self, ctx: ZeusCtx) {
      self.open = true;
      self.loading = true;

      // TODO: This op is the same as the one in RecipientSelectionWindow
      // We should make it a helper function at some point
      RT.spawn_blocking(move || {
         let mut wallets = ctx.get_all_wallets_info();
         let mut portfolios = Vec::new();
         for chain in SUPPORTED_CHAINS {
            if ctx.is_chain_disabled(chain) {
               continue;
            }

            for wallet in &wallets {
               portfolios.push(ctx.get_portfolio(chain, wallet.address));
            }
         }

         let include_testnets = ctx.chain().is_testnet();

         // TODO: Adjust for Privacy mode
         wallets.sort_by(|a, b| {
            let wallet_a = a.address;
            let wallet_b = b.address;

            let value_a = ctx.get_total_value(wallet_a, include_testnets);
            let value_b = ctx.get_total_value(wallet_b, include_testnets);

            // Sort in descending order (highest value first)
            value_b
               .public
               .f64()
               .partial_cmp(&value_a.public.f64())
               .unwrap_or(std::cmp::Ordering::Equal)
         });

         let mut wallet_value = HashMap::new();
         let mut wallet_chains = HashMap::new();

         for wallet in &wallets {
            let value = ctx.get_total_value(wallet.address, include_testnets);
            wallet_value.insert(wallet.address, value.public);

            let chains = ctx.get_chains_that_have_balance(wallet.address);
            wallet_chains.insert(wallet.address, chains);
         }

         SHARED_GUI.write(|gui| {
            gui.wallet_ui.loading = false;
            gui.wallet_ui.wallets = wallets;
            gui.wallet_ui.wallet_value = wallet_value;
            gui.wallet_ui.wallet_chains = wallet_chains;
         });
      });
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(ctx, theme, icons.clone(), ui);
      self.rename_wallet(theme, ui);
      self.add_wallet_ui.show(ctx, theme, icons.clone(), ui);
      self.export_key_ui.show(ctx, theme, ui);
      self.delete_wallet_ui.show(ctx, theme, ui);
   }

   /// This is the first Ui we show to the user when this [WalletUi] is open.
   ///
   /// We can see, manage and add new wallets.
   fn main_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.main_ui {
         return;
      }

      let frame = theme.frame1;

      Window::new("wallet_main_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 100.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 15.0);
            ui.spacing_mut().button_padding = Vec2::new(10.0, 8.0);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               if self.loading {
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  return;
               }

               // Add Wallet Button
               let text = RichText::new("Add Wallet").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  self.add_wallet_ui.open();
               }

               let current_wallet = ctx.current_wallet_info();

               ui.add_space(10.0);
               ui.label(RichText::new("Selected Wallet").size(theme.text_sizes.large));
               self.wallet(
                  ctx,
                  theme,
                  icons.clone(),
                  &current_wallet,
                  true,
                  ui,
               );

               // Search bar
               ui.add_space(8.0);

               let hint = RichText::new("Search...")
                  .color(theme.colors.text_muted)
                  .size(theme.text_sizes.normal);

               ui.add(
                  SecureTextEdit::singleline(&mut self.search_query)
                     .visuals(text_edit_visuals)
                     .hint_text(hint)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .min_size(vec2(ui.available_width() * 0.7, 20.0)),
               );
               ui.add_space(8.0);

               let wallets = self.wallets.clone();

               // Wallet list
               ScrollArea::vertical().show(ui, |ui| {
                  ui.set_width(ui.available_width());

                  for wallet in wallets.iter().filter(|w| *w != &current_wallet) {
                     if self.search_query.is_empty()
                        || wallet
                           .name_with_source()
                           .to_lowercase()
                           .contains(&self.search_query.to_lowercase())
                     {
                        self.wallet(ctx, theme, icons.clone(), wallet, false, ui);

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
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      wallet: &WalletInfo,
      is_current: bool,
      ui: &mut Ui,
   ) {
      let mut frame = theme.frame2;
      let tint = theme.image_tint_recommended;

      let visuals = if is_current {
         None
      } else {
         Some(theme.frame2_visuals)
      };

      let button_visuals = theme.button_visuals();

      let res = frame_it(&mut frame, visuals, ui, |ui| {
         ui.set_width(ui.available_width() * 0.7);
         ui.spacing_mut().button_padding = Vec2::new(8.0, 8.0);

         // Wallet info column
         ui.vertical(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               // Wallet name
               let text = RichText::new(wallet.name_with_source()).size(theme.text_sizes.normal);
               let label = Label::new(text, None).wrap().interactive(false);

               ui.scope(|ui| {
                  ui.set_width(ui.available_width() * 0.35);
                  ui.add(label);
               });

               // Export button
               let enabled = !wallet.is_master();
               let text = RichText::new("Export").size(theme.text_sizes.small);
               let export_key = Button::new(text).visuals(button_visuals);

               if ui.add_enabled(enabled, export_key).clicked() {
                  let wallet = ctx.get_wallet(wallet.address);
                  self.export_key_ui.open(ctx, wallet);
               }

               ui.add_space(8.0);

               // Rename button
               let text = RichText::new("Rename").size(theme.text_sizes.small);
               let rename_wallet = Button::new(text).visuals(button_visuals);

               if ui.add(rename_wallet).clicked() {
                  let wallet_opt = ctx.get_wallet(wallet.address);
                  self.open_rename_wallet(wallet_opt);
               }

               ui.add_space(8.0);

               // Delete button
               let text = RichText::new("Delete").size(theme.text_sizes.small);
               let delete_wallet = Button::new(text).visuals(button_visuals);

               if ui.add_enabled(enabled, delete_wallet).clicked() {
                  self.delete_wallet_ui.open(wallet.clone());
               }
            });

            // Address and value
            ui.horizontal(|ui| {
               let text =
                  RichText::new(wallet.evm_address_truncated()).size(theme.text_sizes.small);
               let label = Button::selectable(false, text).visuals(button_visuals);

               if ui.add(label).clicked() {
                  // Copy the address to the clipboard
                  ui.ctx().copy_text(wallet.address.to_string());
               }

               ui.add_space(10.0);
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                  ui.vertical(|ui| {
                     let value =
                        self.wallet_value.get(&wallet.address).cloned().unwrap_or_default();
                     let chains =
                        self.wallet_chains.get(&wallet.address).cloned().unwrap_or_default();

                     ui.horizontal(|ui| {
                        for chain in chains {
                           if ctx.is_chain_disabled(chain) {
                              continue;
                           }

                           let icon = icons.chain_icon_x16(chain, tint);
                           ui.add(icon);
                        }
                     });
                     ui.label(
                        RichText::new(format!("${}", value.abbreviated()))
                           .size(theme.text_sizes.small),
                     );
                  });
               });
            });
         });
      });

      if res.interact(Sense::click()).clicked() {
         let new_selected_wallet = ctx.get_wallet(wallet.address);
         if let Some(new_selected_wallet) = new_selected_wallet {
            ctx.current_wallet = new_selected_wallet.clone();

            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  gui.header.set_current_wallet(new_selected_wallet);
               });
            });
         }
      }
   }

   /// Rename wallet UI
   fn rename_wallet(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.rename_wallet {
         return;
      }

      let mut open = self.rename_wallet;

      let title = RichText::new("Rename Wallet").size(theme.text_sizes.heading);
      let window_frame = theme.frame1;

      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(200.0);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               let wallet = self.wallet_to_rename.as_ref();

               if wallet.is_none() {
                  ui.label(RichText::new("No wallet selected").size(theme.text_sizes.large));
                  return;
               }

               let old_wallet = wallet.unwrap();

               ui.label(RichText::new("Wallet Name").size(theme.text_sizes.large));
               ui.add_space(10.0);

               SecureTextEdit::singleline(&mut self.new_wallet_name)
                  .visuals(text_edit_visuals)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let text = RichText::new("Rename").size(theme.text_sizes.normal);
               let rename_button = Button::new(text).visuals(button_visuals);

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
                           gui.open_msg_window(
                              "Error",
                              format!(
                                 "Wallet with name {} already exists",
                                 &new_wallet_name
                              ),
                           );
                           gui.request_repaint();
                        });
                        return;
                     }

                     if new_wallet_name.is_empty() {
                        SHARED_GUI.write(|gui| {
                           gui.open_msg_window("Error", "Wallet name cannot be empty");
                           gui.request_repaint();
                        });
                        return;
                     }

                     let max_chars = old_vault.name_max_chars();

                     if new_wallet_name.chars().count() > max_chars {
                        SHARED_GUI.write(|gui| {
                           gui.open_msg_window(
                              "Error",
                              format!(
                                 "Wallet name cannot be longer than {} characters",
                                 max_chars
                              ),
                           );
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
                              gui.open_msg_window("Success", "");
                              gui.request_repaint();
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.loading_window.reset();
                              gui.open_msg_window(
                                 "Failed to encrypt vault, changes reverted",
                                 e.to_string(),
                              );
                              gui.request_repaint();
                           });
                           return;
                        }
                     };

                     ctx.set_vault(new_vault);
                     ctx.build_wallet_info_cache();

                     // Calculate the wallets again
                     SHARED_GUI.write(|gui| {
                        gui.wallet_ui.open(ctx.clone());
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
