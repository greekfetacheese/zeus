pub mod add;
pub use add::AddWalletUi;

use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::{SHARED_GUI, ui::CredentialsForm};
use crate::utils::{RT, data_to_qr};
use eframe::egui::{
   Align, Align2, Button, FontId, Frame, Id, Image, ImageSource, Label, Layout, Margin, Order,
   RichText, ScrollArea, Sense, TextEdit, Ui, Vec2, Window, load::Bytes, vec2,
};
use std::{collections::HashMap, sync::Arc};
use zeus_eth::{alloy_primitives::Address, types::SUPPORTED_CHAINS, utils::NumericValue};
use zeus_theme::{Theme, utils::frame_it};
use zeus_wallet::Wallet;

/// Ui to manage the wallets
pub struct WalletUi {
   open: bool,
   main_ui: bool,
   rename_wallet: bool,
   new_wallet_name: String,
   wallet_to_rename: Option<Wallet>,
   add_wallet_ui: AddWalletUi,
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
         main_ui: true,
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

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, ctx: ZeusCtx) {
      self.open = true;

      let mut wallets = ctx.get_all_wallets_info();
      let mut portfolios = Vec::new();
      for chain in SUPPORTED_CHAINS {
         for wallet in &wallets {
            portfolios.push(ctx.get_portfolio(chain, wallet.address));
         }
      }

      wallets.sort_by(|a, b| {
         let wallet_a = a.address;
         let wallet_b = b.address;

         let value_a = ctx.get_portfolio_value_all_chains(wallet_a);
         let value_b = ctx.get_portfolio_value_all_chains(wallet_b);

         // Sort in descending order (highest value first)
         value_b
            .f64()
            .partial_cmp(&value_a.f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name().cmp(&b.name()))
      });

      let mut wallet_value = HashMap::new();
      let mut wallet_chains = HashMap::new();

      for wallet in &wallets {
         let value = ctx.get_portfolio_value_all_chains(wallet.address);
         wallet_value.insert(wallet.address, value);

         let chains = ctx.get_chains_that_have_balance(wallet.address);
         wallet_chains.insert(wallet.address, chains);
      }

      self.wallets = wallets;
      self.wallet_value = wallet_value;
      self.wallet_chains = wallet_chains;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(ctx.clone(), theme, icons.clone(), ui);
      self.rename_wallet(ctx.clone(), theme, ui);
      self.add_wallet_ui.show(ctx.clone(), theme, icons.clone(), ui);
      self.export_key_ui.show(ctx.clone(), theme, icons.clone(), ui);
      self.delete_wallet_ui.show(ctx, theme, icons.clone(), ui);
   }

   /// This is the first Ui we show to the user when this [WalletUi] is open.
   ///
   /// We can see, manage and add new wallets.
   fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.main_ui {
         return;
      }

      Window::new("wallet_main_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 100.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 15.0);
            ui.spacing_mut().button_padding = Vec2::new(10.0, 8.0);

            ui.vertical_centered(|ui| {
               // Add Wallet Button
               if ui
                  .add(Button::new(
                     RichText::new("Add Wallet").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.add_wallet_ui.open();
                  self.add_wallet_ui.open_main_ui();
               }

               let current_wallet = ctx.current_wallet_info();

               ui.add_space(10.0);
               ui.label(RichText::new("Selected Wallet").size(theme.text_sizes.large));
               self.wallet(
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  &current_wallet,
                  true,
                  ui,
               );

               // Search bar
               ui.add_space(8.0);

               let hint = RichText::new("Search...").color(theme.colors.text_muted);

               ui.add(
                  TextEdit::singleline(&mut self.search_query)
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
                        || wallet.name().to_lowercase().contains(&self.search_query.to_lowercase())
                     {
                        self.wallet(
                           ctx.clone(),
                           theme,
                           icons.clone(),
                           wallet,
                           false,
                           ui,
                        );

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
      let mut frame = theme.frame2;
      let tint = theme.image_tint_recommended;

      let visuals = if is_current {
         None
      } else {
         Some(theme.frame2_visuals)
      };

      let res = frame_it(&mut frame, visuals, ui, |ui| {
         ui.set_width(ui.available_width() * 0.7);
         ui.spacing_mut().button_padding = Vec2::new(8.0, 8.0);

         // Wallet info column
         ui.vertical(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               // Wallet name
               let name =
                  Label::new(RichText::new(wallet.name()).size(theme.text_sizes.normal)).wrap();

               ui.scope(|ui| {
                  ui.set_width(ui.available_width() * 0.45);
                  ui.add(name);
               });

               // Export button
               let enabled = !wallet.is_master();
               let export_key = Button::new(RichText::new("Export").size(theme.text_sizes.small));
               if ui.add_enabled(enabled, export_key).clicked() {
                  let wallet = ctx.get_wallet(wallet.address);
                  self.export_key_ui.open(ctx.clone(), wallet);
               }

               ui.add_space(8.0);

               // Rename button
               let rename_wallet =
                  Button::new(RichText::new("Rename").size(theme.text_sizes.small));
               if ui.add(rename_wallet).clicked() {
                  let wallet_opt = ctx.get_wallet(wallet.address);
                  self.rename_wallet = true;
                  self.wallet_to_rename = wallet_opt;
               }

               ui.add_space(8.0);

               // Delete button
               let delete_wallet =
                  Button::new(RichText::new("Delete").size(theme.text_sizes.small));
               if ui.add_enabled(enabled, delete_wallet).clicked() {
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
            ctx.write(|ctx| {
               ctx.current_wallet = new_selected_wallet.clone();
            });
            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  gui.header.set_current_wallet(new_selected_wallet);
               });
            });
         }
      }
   }

   fn rename_wallet(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.rename_wallet;

      let title = RichText::new("Rename Wallet").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(200.0);

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

               TextEdit::singleline(&mut self.new_wallet_name)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let rename_button =
                  Button::new(RichText::new("Rename").size(theme.text_sizes.normal));

               if ui.add(rename_button).clicked() {
                  let new_wallet_name = self.new_wallet_name.clone();
                  let old_wallet = old_wallet.clone();
                  let old_wallet_addr = old_wallet.address();

                  // On failure, revert the changes
                  RT.spawn_blocking(move || {
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

                     ctx.set_vault(new_vault.clone());

                     match ctx.encrypt_and_save_vault(Some(new_vault), None) {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              // Update header
                              if is_current {
                                 gui.header.set_current_wallet(new_wallet);
                              }

                              // Calculate the wallets again
                              gui.wallet_ui.open(ctx.clone());

                              // Reset state
                              gui.wallet_ui.rename_wallet = false;
                              gui.wallet_ui.new_wallet_name.clear();
                              gui.wallet_ui.wallet_to_rename = None;

                              gui.open_msg_window("Success", "");
                              gui.request_repaint();
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window(
                                 "Failed to encrypt vault, changes reverted",
                                 e.to_string(),
                              );
                              gui.request_repaint();
                           });
                           ctx.set_vault(old_vault);
                           ctx.write(|ctx| {
                              ctx.current_wallet = old_wallet;
                           });
                        }
                     };
                  });
               }
            });
         });

      self.rename_wallet = open;
      if !self.rename_wallet {
         self.new_wallet_name.clear();
         self.wallet_to_rename = None;
      }
   }
}

pub struct ExportKeyUi {
   open: bool,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   wallet_to_export: Option<Wallet>,
   image_uri: Option<String>,
   image_error: Option<String>,
   show_key: bool,
   size: (f32, f32),
}

impl ExportKeyUi {
   pub fn new() -> Self {
      Self {
         open: false,
         credentials_form: CredentialsForm::new(),
         verified_credentials: false,
         wallet_to_export: None,
         image_uri: None,
         image_error: None,
         show_key: false,
         size: (400.0, 400.0),
      }
   }

   fn open(&mut self, ctx: ZeusCtx, wallet: Option<Wallet>) {
      if let Some(wallet) = &wallet {
         let key_hex = wallet.key_string();
         let png_bytes_res = key_hex.unlock_str(|key| data_to_qr(key));

         match png_bytes_res {
            Ok(png_bytes) => {
               ctx.set_qr_image_data(png_bytes);

               let uri = format!(
                  "bytes://key-{}.png",
                  &wallet.address().to_string()
               );

               self.image_uri = Some(uri);
               self.image_error = None;
            }
            Err(e) => {
               self.image_uri = None;
               self.image_error = Some(format!("Failed to generate QR Code: {}", e));
            }
         }
      }

      self.open = true;
      self.credentials_form.open = true;
      self.wallet_to_export = wallet;
   }

   fn reset(&mut self) {
      *self = Self::new();
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.verify_credentials_ui(ctx.clone(), theme, icons, ui);
      self.show_key(ctx, theme, ui);
   }

   fn show_key(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.show_key || !self.verified_credentials {
         return;
      }

      let title = RichText::new("Success").size(theme.text_sizes.heading);

      Window::new(title)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               if let Some(wallet) = self.wallet_to_export.as_ref() {
                  let warning_text = "Make sure to save this key in a safe place!";
                  ui.label(RichText::new(warning_text).size(theme.text_sizes.large));

                  let text = RichText::new("Copy Key").size(theme.text_sizes.normal);
                  if ui.add(Button::new(text)).clicked() {
                     ui.ctx().copy_text(wallet.key_string().unlock_str(|key| key.to_string()));
                  }

                  if self.image_error.is_some() {
                     ui.label(
                        RichText::new(self.image_error.as_ref().unwrap())
                           .size(theme.text_sizes.large),
                     );
                  }

                  if let Some(image_uri) = self.image_uri.clone() {
                     let data = ctx.qr_image_data();
                     let image = Image::new(ImageSource::Bytes {
                        uri: image_uri.into(),
                        bytes: Bytes::Shared(data),
                     });
                     ui.add(image);
                  }
               } else {
                  ui.label(
                     RichText::new("No wallet found, this is a bug").size(theme.text_sizes.normal),
                  );
               }

               let text = RichText::new("Close").size(theme.text_sizes.normal);
               if ui.add(Button::new(text)).clicked() {
                  if let Some(image_uri) = &self.image_uri {
                     ui.ctx().forget_image(image_uri);
                  }
                  self.reset();
                  ctx.erase_qr_image_data();
               }
            });
         });
   }

   fn verify_credentials_ui(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let mut open = self.credentials_form.open;
      let mut clicked = false;

      Window::new(RichText::new("Verify Credentials").size(theme.text_sizes.large))
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               self.credentials_form.show(theme, icons, ui);

               let button = Button::new(RichText::new("Confrim").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let mut vault = ctx.get_vault();
         vault.set_credentials(self.credentials_form.credentials.clone());
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Decrypting vault...");
            });

            // Verify the credentials by just decrypting the vault
            match vault.decrypt(None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     // Allow the user to export the key
                     gui.wallet_ui.export_key_ui.show_key = true;
                     // Mark the credentials as verified
                     gui.wallet_ui.export_key_ui.verified_credentials = true;
                     // Erase the credentials form
                     gui.wallet_ui.export_key_ui.credentials_form.erase();
                     // Close the credentials form
                     gui.wallet_ui.export_key_ui.credentials_form.open = false;
                     gui.loading_window.reset();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to decrypt vault", e.to_string());
                     gui.loading_window.reset();
                  });
               }
            }
         });
      }

      self.credentials_form.open = open;
      if !self.credentials_form.open {
         self.credentials_form.erase();
      }
   }
}

pub struct DeleteWalletUi {
   open: bool,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   wallet_to_delete: Option<WalletInfo>,
   size: (f32, f32),
}

impl DeleteWalletUi {
   pub fn new() -> Self {
      Self {
         open: false,
         credentials_form: CredentialsForm::new(),
         verified_credentials: false,
         wallet_to_delete: None,
         size: (550.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.verify_credentials_ui(ctx.clone(), theme, icons, ui);
      self.delete_wallet_ui(ctx, theme, ui);
   }

   fn verify_credentials_ui(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let mut open = self.credentials_form.open;
      let mut clicked = false;

      let id = Id::new("verify_credentials_delete_wallet_ui");
      Window::new(RichText::new("Verify Credentials").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               self.credentials_form.show(theme, icons, ui);

               let button = Button::new(RichText::new("Confrim").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let mut vault = ctx.get_vault();
         vault.set_credentials(self.credentials_form.credentials.clone());
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Decrypting vault...");
            });

            // Verify the credentials by just decrypting the vault
            match vault.decrypt(None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     // Mark the credentials as verified
                     gui.wallet_ui.delete_wallet_ui.verified_credentials = true;
                     // Close the verify credentials ui
                     gui.wallet_ui.delete_wallet_ui.credentials_form.open = false;
                     // Open the delete wallet ui
                     gui.wallet_ui.delete_wallet_ui.open = true;
                     // Erase the credentials form
                     gui.wallet_ui.delete_wallet_ui.credentials_form.erase();
                     gui.loading_window.reset();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to decrypt vault", e.to_string());
                     gui.loading_window.reset();
                  });
               }
            }
         });
      }

      self.credentials_form.open = open;
      if !self.credentials_form.open {
         self.credentials_form.erase();
      }
   }

   fn delete_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.verified_credentials {
         return;
      }
      let mut open = self.open;
      let mut clicked = false;

      let wallet = self.wallet_to_delete.clone();
      if wallet.is_none() {
         return;
      }
      let wallet = wallet.unwrap();

      let id = Id::new("delete_wallet_ui_delete_wallet");
      Window::new(RichText::new("Delete this wallet?").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               ui.label(RichText::new(wallet.name()).size(theme.text_sizes.normal));
               ui.label(RichText::new(wallet.address.to_string()).size(theme.text_sizes.normal));

               let value = ctx.get_portfolio_value_all_chains(wallet.address);
               ui.label(
                  RichText::new(format!("Value ${}", value.abbreviated()))
                     .size(theme.text_sizes.normal),
               );

               if ui
                  .add(Button::new(
                     RichText::new("Yes").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  clicked = true;
               }
            });
         });

      if clicked {
         open = false;
         let mut new_vault = ctx.get_vault();
         let is_current = ctx.is_current_wallet(wallet.address);

         RT.spawn_blocking(move || {
            new_vault.remove_wallet(wallet.address);

            // Set the master wallet as selected to avoid an empty ComboBox
            if is_current {
               let master_wallet = new_vault.get_master_wallet();
               SHARED_GUI.write(|gui| {
                  gui.header.set_current_wallet(master_wallet);
               });
            }

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting vault...");
            });

            // Encrypt the vault
            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.wallet_ui.delete_wallet_ui.wallet_to_delete = None;
                     gui.wallet_ui.delete_wallet_ui.verified_credentials = false;
                     gui.open_msg_window("Wallet Deleted", "");
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.open_msg_window("Failed to encrypt vault", e.to_string());
                  });
                  return;
               }
            };

            ctx.set_vault(new_vault);
         });
      }
      self.open = open;
   }
}
