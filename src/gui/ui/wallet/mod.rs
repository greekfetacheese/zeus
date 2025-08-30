pub mod add;
pub use add::AddWalletUi;
use egui::Spinner;
use zeus_eth::utils::truncate_address;

use crate::assets::icons::Icons;
use crate::core::SecureHDWallet;
use crate::core::{Portfolio, Wallet, WalletInfo, ZeusCtx, utils::RT};
use crate::gui::{SHARED_GUI, ui::CredentialsForm};
use eframe::egui::{
   Align, Align2, Button, FontId, Frame, Id, Label, Layout, Margin, Order, RichText, ScrollArea,
   Sense, TextEdit, Ui, Vec2, Window, vec2,
};
use egui_theme::{Theme, utils::*};
use std::{collections::HashMap, sync::Arc};
use tokio::{sync::Semaphore, task::JoinHandle};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   currency::{Currency, NativeCurrency},
   types::SUPPORTED_CHAINS,
   utils::NumericValue,
};

/// Ui to manage the wallets
pub struct WalletUi {
   pub open: bool,
   pub main_ui: bool,
   pub add_wallet_ui: AddWalletUi,
   pub search_query: String,
   pub export_key_ui: ExportKeyUi,
   pub delete_wallet_ui: DeleteWalletUi,
   pub discover_wallets_ui: DiscoverWallets,
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
         discover_wallets_ui: DiscoverWallets::new(),
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
      self.export_key_ui.show(ctx.clone(), theme, icons.clone(), ui);
      self.delete_wallet_ui.show(ctx.clone(), theme, icons.clone(), ui);
      self.discover_wallets_ui.show(ctx.clone(), theme, icons, ui);
   }

   /// This is the first Ui we show to the user when this [WalletUi] is open.
   ///
   /// We can see, manage and add new wallets.
   pub fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.main_ui {
         return;
      }

      let mut wallets = ctx.get_all_wallets_info();
      let current_wallet = ctx.current_wallet_info();
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
         let value_a = portfolio_a.map(|p| p.value.clone()).unwrap_or(NumericValue::default());
         let value_b = portfolio_b.map(|p| p.value.clone()).unwrap_or(NumericValue::default());

         // Sort in descending order (highest value first)
         // If values are equal, sort by name as a secondary criterion
         value_b
            .f64()
            .partial_cmp(&value_a.f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name().cmp(&b.name()))
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
               if ui
                  .add(Button::new(
                     RichText::new("Add Wallet").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.add_wallet_ui.open = true;
                  self.add_wallet_ui.main_ui = true;
               }

               // let enabled = !ctx.wallet_discovery_in_progress();
               let discover_button =
                  Button::new(RichText::new("Discover Wallets").size(theme.text_sizes.normal));

               if ui.add(discover_button).clicked() {
                  let vault = ctx.get_vault();
                  self.discover_wallets_ui.set_wallets(vault.get_hd_wallet());
                  self.discover_wallets_ui.open = true;

                  /*
                  RT.spawn(async move {
                     match update::wallet_discovery(ctx_clone.clone()).await {
                        Ok(_) => {
                           tracing::info!("Wallet discovery finished");
                           update::on_startup(ctx_clone.clone()).await;
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              ctx_clone.write(|ctx| {
                                 ctx.wallet_discovery_in_progress = false;
                              });
                              gui.open_msg_window("Failed to discover wallets", e.to_string());
                           });
                        }
                     }
                  });
                  */
               }

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

               let hint = RichText::new("Search...").color(theme.colors.text_secondary);

               ui.add(
                  TextEdit::singleline(&mut self.search_query)
                     .hint_text(hint)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .background_color(theme.colors.text_edit_bg)
                     .min_size(vec2(ui.available_width() * 0.7, 20.0)),
               );
               ui.add_space(8.0);

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
      let mut frame = Frame::group(ui.style()).inner_margin(8.0).fill(theme.colors.bg_color);

      let visuals = if is_current {
         None
      } else {
         Some(theme.frame1_visuals.clone())
      };

      let res = frame_it(&mut frame, visuals, ui, |ui| {
         ui.set_width(ui.available_width() * 0.7);
         ui.spacing_mut().button_padding = Vec2::new(8.0, 8.0);

         // Wallet info column
         ui.vertical(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               // Wallet name
               let name = Label::new(RichText::new(wallet.name()).size(theme.text_sizes.normal));

               ui.scope(|ui| {
                  ui.set_width(ui.available_width() * 0.45);
                  ui.add(name);
               });

               // Export button
               let enabled = !wallet.is_master();
               let export_key =
                  Button::new(RichText::new("Export Key").size(theme.text_sizes.small));
               if ui.add_enabled(enabled, export_key).clicked() {
                  let wallet = ctx.get_wallet(wallet.address);
                  self.export_key_ui.open = true;
                  self.export_key_ui.set_wallet_to_export(wallet);
                  self.export_key_ui.credentials_form.open = true;
               }

               ui.add_space(8.0);

               // Delete button
               let delete_wallet =
                  Button::new(RichText::new("Delete Wallet").size(theme.text_sizes.small));
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
                     let chains = ctx.get_chains_that_have_balance(wallet.address);
                     let value = ctx.get_portfolio_value_all_chains(wallet.address);
                     ui.horizontal(|ui| {
                        for chain in chains {
                           let icon = icons.chain_icon_x16(chain);
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
         let new_selected_wallet = ctx.get_wallet(wallet.address);
         if let Some(new_selected_wallet) = new_selected_wallet {
            ctx.write(|ctx| {
               ctx.current_wallet = new_selected_wallet.clone();
            });
            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  gui.wallet_selection.wallet_select.wallet = new_selected_wallet;
               });
            });
         }
      }
   }
}

pub struct ExportKeyUi {
   pub open: bool,
   pub credentials_form: CredentialsForm,
   pub verified_credentials: bool,
   wallet_to_export: Option<Wallet>,
   show_key: bool,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl ExportKeyUi {
   pub fn new() -> Self {
      Self {
         open: false,
         credentials_form: CredentialsForm::new(),
         verified_credentials: false,
         wallet_to_export: None,
         show_key: false,
         size: (550.0, 350.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn set_wallet_to_export(&mut self, wallet: Option<Wallet>) {
      self.wallet_to_export = wallet;
   }

   pub fn reset(&mut self) {
      *self = Self::new();
      tracing::info!("ExportKeyUi resetted");
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.verify_credentials_ui(ctx.clone(), theme, icons, ui);
      self.show_key(theme, ui);
   }

   fn show_key(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.show_key || !self.verified_credentials {
         return;
      }

      let title = RichText::new("Success").size(theme.text_sizes.heading);
      let mut open = self.open;

      Window::new(title)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(100.0, 150.0));

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               if let Some(wallet) = self.wallet_to_export.as_ref() {
                  let warning_text = "Make sure to save this key in a safe place!";
                  ui.label(RichText::new(warning_text).size(theme.text_sizes.large));

                  let text = RichText::new("Copy Key").size(theme.text_sizes.normal);
                  if ui.add(Button::new(text)).clicked() {
                     ui.ctx().copy_text(wallet.key_string().str_scope(|key| key.to_string()));
                  }
               } else {
                  ui.label(
                     RichText::new("No wallet found, this is a bug").size(theme.text_sizes.normal),
                  );
               }
            });
         });

      self.open = open;

      if !self.open {
         self.reset();
      }
   }

   pub fn verify_credentials_ui(
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
                     gui.wallet_ui.export_key_ui.show_key = true;
                     gui.wallet_ui.export_key_ui.verified_credentials = true;
                     gui.wallet_ui.export_key_ui.credentials_form.erase();
                     gui.wallet_ui.export_key_ui.credentials_form.open = false;
                     gui.loading_window.open = false;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to decrypt account", e.to_string());
                     gui.loading_window.open = false;
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
   pub open: bool,
   pub credentials_form: CredentialsForm,
   pub verified_credentials: bool,
   pub wallet_to_delete: Option<WalletInfo>,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl DeleteWalletUi {
   pub fn new() -> Self {
      Self {
         open: false,
         credentials_form: CredentialsForm::new(),
         verified_credentials: false,
         wallet_to_delete: None,
         size: (550.0, 350.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.verify_credentials_ui(ctx.clone(), theme, icons, ui);
      self.delete_wallet_ui(ctx, theme, ui);
   }

   pub fn verify_credentials_ui(
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
                     // credentials are verified
                     gui.wallet_ui.delete_wallet_ui.verified_credentials = true;

                     // close the verify credentials ui
                     gui.wallet_ui.delete_wallet_ui.credentials_form.open = false;

                     // open the delete wallet ui
                     gui.wallet_ui.delete_wallet_ui.open = true;

                     // erase the credentials form
                     gui.wallet_ui.delete_wallet_ui.credentials_form.erase();
                     gui.loading_window.open = false;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to decrypt account", e.to_string());
                     gui.loading_window.open = false;
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

   pub fn delete_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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
         .anchor(self.anchor.0, self.anchor.1)
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
                  RichText::new(format!("Value ${}", value.formatted()))
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

            if is_current {
               let master_wallet = new_vault.get_master_wallet();
               SHARED_GUI.write(|gui| {
                  gui.wallet_selection.wallet_select.wallet = master_wallet;
               });
            }

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting vault...");
            });

            // Encrypt the vault
            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.wallet_ui.delete_wallet_ui.wallet_to_delete = None;
                     gui.wallet_ui.delete_wallet_ui.verified_credentials = false;
                     gui.open_msg_window("Wallet Deleted", "");
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
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

// ! Still WIP
pub struct DiscoverWallets {
   open: bool,
   /// The master wallet used to show the wallets that are already in the vault
   master_wallet: SecureHDWallet,
   /// Clone of the master to start the discovery process from 0 index again
   discovery_wallet: SecureHDWallet,
   nonce_balance_map: HashMap<(u64, Address), (U256, u64)>,
   syncing: bool,
   add_wallet_window: bool,
   index_to_add: u32,
   wallet_name: String,
   size: (f32, f32),
}

impl DiscoverWallets {
   pub fn new() -> Self {
      Self {
         open: false,
         master_wallet: SecureHDWallet::random(),
         discovery_wallet: SecureHDWallet::random(),
         nonce_balance_map: HashMap::new(),
         syncing: false,
         add_wallet_window: false,
         index_to_add: 0,
         wallet_name: String::new(),
         size: (600.0, 450.0),
      }
   }

   pub fn set_wallets(&mut self, master_wallet: SecureHDWallet) {
      let mut discovery_wallet = master_wallet.clone();
      discovery_wallet.children = Vec::new();
      discovery_wallet.next_child_index = 0;

      self.master_wallet = master_wallet;
      self.discovery_wallet = discovery_wallet;
   }

   pub fn set_master_wallet(&mut self, master_wallet: SecureHDWallet) {
      self.master_wallet = master_wallet;
   }

   pub fn reset(&mut self) {
      *self = Self::new();
   }

   fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.add_wallet(ctx.clone(), theme, ui);

      let mut open = self.open;

      let title = RichText::new("Discover Wallets").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = Vec2::new(20.0, 15.0);
            ui.spacing_mut().button_padding = Vec2::new(10.0, 8.0);
            ui.vertical_centered(|ui| {
               ui.add_space(15.0);

               let children = self.discovery_wallet.children.len();
               let text = format!("Showing {} wallets", children);
               ui.label(RichText::new(text).size(theme.text_sizes.normal));

               let text = RichText::new("Generate next 10").size(theme.text_sizes.normal);
               if ui.add(Button::new(text)).clicked() {
                  self.syncing = true;
                  let ctx_clone = ctx.clone();
                  let mut addresses = Vec::new();

                  for _ in 0..10 {
                     if let Ok(address) = self.discovery_wallet.derive_child("".into()) {
                        addresses.push(address);
                     }
                  }

                  RT.spawn(async move {
                     match sync_wallets(ctx_clone, addresses).await {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui.discover_wallets_ui.syncing = false;
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui.discover_wallets_ui.syncing = false;
                           });
                           tracing::error!("Error syncing wallets: {:?}", e);
                        }
                     }
                  });
               }

               if self.syncing {
                  ui.add(Spinner::new().size(15.0));
               }

               let column_widths = [
                  ui.available_width() * 0.2,  // Derivation Path
                  ui.available_width() * 0.2,  // Address
                  ui.available_width() * 0.15, // Value
                  ui.available_width() * 0.1,  // TxCount
                  ui.available_width() * 0.05, // Import Button
               ];

               Frame::new().inner_margin(20).show(ui, |ui| {
                  // Header
                  ui.horizontal(|ui| {
                     ui.scope(|ui| {
                        ui.set_width(column_widths[0]);
                        let text = RichText::new("Derivation Path").size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                     ui.scope(|ui| {
                        ui.set_width(column_widths[1]);
                        let text = RichText::new("Address").size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                     ui.scope(|ui| {
                        ui.set_width(column_widths[2]);
                        let text = RichText::new("Value").size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                     ui.scope(|ui| {
                        ui.set_width(column_widths[3]);
                        let text = RichText::new("TxCount").size(theme.text_sizes.normal);
                        ui.label(text);
                     });
                     // Just occupy space
                     ui.scope(|ui| {
                        ui.set_width(column_widths[4]);
                     });
                  });

                  ScrollArea::vertical()
                     .id_salt("children_wallets_in_discovery")
                     .auto_shrink([false; 2])
                     .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        self.show_wallets(
                           ctx.clone(),
                           theme,
                           icons.clone(),
                           &column_widths,
                           ui,
                        );
                     });
               });
            });
         });

      self.open = open;
      if !self.open {
         self.reset();
      }
   }

   fn show_wallets(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      column_widths: &[f32],
      ui: &mut Ui,
   ) {
      for child in self.discovery_wallet.children.iter() {
         // If child already exists it will displayed as disabled in the Ui
         // let child_index = child.xkey_info.as_ref().unwrap().index;
         let exists = self.master_wallet.children.contains(&child);

         let mut tx_count = 0;
         let mut chains = Vec::new();
         let mut total_value = 0.0;
         let ctx = ctx.clone();
         let current_chain = ctx.chain();

         // get the chains which the wallet has balance in
         for chain in SUPPORTED_CHAINS {
            let key = (chain, child.address());
            let (balance, nonce) = self.nonce_balance_map.get(&key).cloned().unwrap_or_default();
            if !balance.is_zero() {
               chains.push(chain);
            }
            tx_count += nonce;

            let native = Currency::from(NativeCurrency::from(chain));
            let balance = NumericValue::currency_balance(balance, native.decimals());
            let value = ctx.get_currency_value_for_amount(balance.f64(), &native);
            total_value += value.f64();
         }

         ui.horizontal(|ui| {
            ui.add_enabled_ui(!exists, |ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  // Derivation Path
                  ui.scope(|ui| {
                     ui.set_width(column_widths[0]);
                     let text = child.derivation_path_string();
                     let rich_text = RichText::new(text).size(theme.text_sizes.small);
                     ui.label(rich_text);
                  });

                  // Address
                  ui.scope(|ui| {
                     ui.set_width(column_widths[1]);
                     let address = child.address().to_string();
                     let text = truncate_address(&address, 20);
                     let explorer = current_chain.block_explorer();
                     let link = format!("{}/address/{}", explorer, address);
                     ui.hyperlink_to(
                        RichText::new(text)
                           .size(theme.text_sizes.small)
                           .color(theme.colors.hyperlink_color),
                        link,
                     );
                  });
               });

               ui.add_space(10.0);

               // Value
               ui.vertical(|ui| {
                  ui.set_width(column_widths[2]);
                  let value = NumericValue::from_f64(total_value);

                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = 1.0;
                     for chain in SUPPORTED_CHAINS {
                        let icon = icons.chain_icon_x16(chain);
                        ui.add(icon);
                     }
                  });

                  ui.label(
                     RichText::new(format!("${}", value.format_abbreviated()))
                        .color(theme.colors.text_secondary)
                        .size(theme.text_sizes.small),
                  );
               });

               // TxCount
               ui.scope(|ui| {
                  ui.set_width(column_widths[3]);
                  ui.label(
                     RichText::new(tx_count.to_string())
                        .color(theme.colors.text_secondary)
                        .size(theme.text_sizes.small),
                  );
               });

               ui.scope(|ui| {
                  ui.set_width(column_widths[4]);
                  let text = RichText::new("Add").size(theme.text_sizes.small);
                  if ui.button(text).clicked() {
                     let index = child.xkey_info.as_ref().unwrap().index;
                     self.add_wallet_window = true;
                     self.index_to_add = index;
                  }
               });
            });
         });
      }
   }

   fn add_wallet(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.add_wallet_window;

      let title = RichText::new("Add Wallet").size(theme.text_sizes.heading);
      Window::new(title)
         .id(Id::new("discover_wallets_add_wallet_window"))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 20.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               let text = RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal);
               ui.label(text);

               TextEdit::singleline(&mut self.wallet_name)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let text = RichText::new("Add Wallet").size(theme.text_sizes.large);

               if ui.button(text).clicked() {
                  let index = self.index_to_add;
                  let name = self.wallet_name.clone();
                  let balances = self.nonce_balance_map.clone();

                  RT.spawn_blocking(move || {
                     let res = ctx.write_vault(|vault| vault.derive_child_wallet_at(name, index));

                     let address = match res {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Failed to add wallet", e.to_string());
                           });
                           return;
                        }
                     };

                     for chain in SUPPORTED_CHAINS {
                        let eth = NativeCurrency::from(chain);
                        let balance = balances.get(&(chain, address)).cloned().unwrap_or_default();
                        let balance_manager = ctx.balance_manager();
                        balance_manager.insert_eth_balance(chain, address, balance.0, &eth);

                        ctx.write(|ctx| {
                           ctx.portfolio_db.insert_portfolio(
                              chain,
                              address,
                              Portfolio::new(address, chain),
                           );
                        });
                     }

                     match ctx.encrypt_and_save_vault(None, None) {
                        Ok(_) => {
                           let hd_wallet = ctx.get_vault().get_hd_wallet();
                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui.discover_wallets_ui.add_wallet_window = false;
                              gui.wallet_ui.discover_wallets_ui.wallet_name = String::new();
                              gui.wallet_ui.discover_wallets_ui.set_master_wallet(hd_wallet);
                              gui.open_msg_window("Wallet Added", "");
                           });
                        }
                        Err(e) => {
                           ctx.write_vault(|vault| {
                              vault.remove_child(address);
                           });

                           let hd_wallet = ctx.get_vault().get_hd_wallet();

                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui.discover_wallets_ui.add_wallet_window = false;
                              gui.wallet_ui.discover_wallets_ui.wallet_name = String::new();
                              gui.wallet_ui.discover_wallets_ui.set_master_wallet(hd_wallet);

                              gui.open_msg_window("Failed to encrypt vault", e.to_string());
                           });
                        }
                     }
                  });
               }
            });
         });

      self.add_wallet_window = open;
      if !self.add_wallet_window {
         self.wallet_name.clear();
      }
   }
}

async fn sync_wallets(ctx: ZeusCtx, addresses: Vec<Address>) -> Result<(), anyhow::Error> {
   let mut tasks: Vec<JoinHandle<Result<(u64, Address, U256, u64), anyhow::Error>>> = Vec::new();

   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client(chain).await?;
      let semaphore = Arc::new(Semaphore::new(5));

      for address in &addresses {
         if ctx.wallet_exists(*address) {
            continue;
         }

         let client = client.clone();
         let semaphore = semaphore.clone();
         let address = address.clone();
         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let balance = client.get_balance(address).into_future();
            let nonce = client.get_transaction_count(address).into_future();
            let balance = balance.await?;
            let nonce = nonce.await?;

            Ok((chain, address, balance, nonce))
         });
         tasks.push(task);
      }
   }

   let mut nonce_balance_map = HashMap::new();

   SHARED_GUI.read(|gui| {
      nonce_balance_map = gui.wallet_ui.discover_wallets_ui.nonce_balance_map.clone();
   });

   for task in tasks {
      if let Ok(result) = task.await {
         let (chain, address, balance, nonce) = result?;
         nonce_balance_map.insert((chain, address), (balance, nonce));
      }
   }

   SHARED_GUI.write(|gui| {
      gui.wallet_ui.discover_wallets_ui.nonce_balance_map = nonce_balance_map;
   });

   Ok(())
}
