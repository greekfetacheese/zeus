//! UI that allows the user to add a new wallet from 3 different sources:
//!
//! - Import from a private key
//! - Import from a seed phrase
//! - Derive a child wallet from the master wallet

use crate::assets::Icons;
use crate::core::{DiscoveredWallets, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{Align2, FontId, Frame, Margin, Order, RichText, Ui, Window, vec2};

use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, SecureTextEdit};

use std::sync::Arc;

use super::discover::DiscoverChildWallets;
use super::import::{ImportWallet, ImportWalletType};

pub struct AddWalletUi {
   open: bool,
   overlay: OverlayManager,
   pub import_wallet: ImportWallet,
   pub discover_child_wallets_ui: DiscoverChildWallets,
   #[allow(dead_code)]
   generate_wallet: bool,
   #[allow(dead_code)]
   wallet_name: String,
   size: (f32, f32),
}

impl AddWalletUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay: overlay.clone(),
         import_wallet: ImportWallet::new(overlay.clone()),
         discover_child_wallets_ui: DiscoverChildWallets::new(overlay),
         generate_wallet: false,
         wallet_name: String::new(),
         size: (450.0, 250.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.main_ui(theme, ui);
      self.import_wallet.show(theme, ui);
      self.discover_child_wallets_ui.show(ctx, theme, icons, ui);
   }

   fn main_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let mut derive_clicked = false;
      let mut import_from_pk_clicked = false;
      let mut import_from_seed_clicked = false;

      let window_frame = theme.frame1;

      Window::new(RichText::new("Add a new Wallet").size(theme.text_sizes.heading))
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.add_space(30.0);
               let size = vec2(ui.available_width() * 0.9, 50.0);

               // Derive a new child wallet from the master wallet
               let text = RichText::new("Derive from Master Wallet").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  derive_clicked = true;
               }

               // From private key
               let text = RichText::new("Import from a Private Key").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  import_from_pk_clicked = true;
               }

               // From seed phrase
               let text = RichText::new("Import from a Seed Phrase").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  import_from_seed_clicked = true;
               }
            });
         });

      if derive_clicked {
         open = false;
         self.discover_child_wallets_ui.open();
         self.discover_child_wallets_ui.loading = true;

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            let vault = ctx.get_vault();
            let master = ctx.master_wallet_address();

            let mut discovered_wallets = match DiscoveredWallets::load_from_file() {
               Ok(wallets) => wallets,
               Err(e) => {
                  tracing::error!("Error loading discovered wallets: {:?}", e);
                  let mut discovered_wallets = DiscoveredWallets::new();
                  discovered_wallets.master_wallet_address = Some(master);
                  discovered_wallets
               }
            };

            // If wallets are empty, set master wallet address
            if discovered_wallets.wallets.is_empty() {
               discovered_wallets.master_wallet_address = Some(master);
            }

            // If master address is different, reset
            if let Some(master_wallet_address) = discovered_wallets.master_wallet_address {
               if master_wallet_address != master {
                  discovered_wallets = DiscoveredWallets::new();
                  discovered_wallets.master_wallet_address = Some(master);
                  tracing::warn!("Discovered wallets master address is different, resetting");
               }
            }

            if discovered_wallets.is_corrupted() {
               discovered_wallets = DiscoveredWallets::new();
               discovered_wallets.master_wallet_address = Some(master);
               tracing::warn!("Discovered wallets index is corrupted, resetting");
            }

            discovered_wallets.rediscover_wallets(vault.get_hd_wallet());

            SHARED_GUI.write(|gui| {
               gui.wallet_ui
                  .add_wallet_ui
                  .discover_child_wallets_ui
                  .set_hd_wallet(vault.get_hd_wallet());
               gui.wallet_ui
                  .add_wallet_ui
                  .discover_child_wallets_ui
                  .set_discovery_wallet(vault.get_hd_wallet());
               gui.wallet_ui
                  .add_wallet_ui
                  .discover_child_wallets_ui
                  .set_discovered_wallets(discovered_wallets);
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.current_page = 0;
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.open();
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.loading = false;
            });
         });
      }

      if import_from_pk_clicked {
         self.import_wallet.open(ImportWalletType::PrivateKey);
         open = false;
      }

      if import_from_seed_clicked {
         self.import_wallet.open(ImportWalletType::MnemonicPhrase);
         open = false;
      }

      if !open {
         self.close();
      }
   }

   fn _generate_wallet_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.generate_wallet {
         return;
      }

      let mut open = self.generate_wallet;
      let mut clicked = false;
      Window::new(RichText::new("Generate Wallet").size(theme.text_sizes.large))
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let size = vec2(ui.available_width() * 0.5, 20.0);
               ui.add_space(20.0);

               // Wallet Name
               ui.label(RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal));
               ui.add(
                  SecureTextEdit::singleline(&mut self.wallet_name)
                     .visuals(text_edit_visuals)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               // Generate Button
               let text = RichText::new("Generate").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            let mut new_vault = ctx.get_vault();

            match new_vault.new_wallet_rng(name) {
               Ok(_) => {}
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to generate wallet", e.to_string());
                  });
                  return;
               }
            }

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting vault...");
            });

            // Encrypt the vault
            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                     gui.open_msg_window("Wallet generated successfully", "");
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
      self.generate_wallet = open;
   }
}
