//! UI that allows the user to add a new wallet from 3 different sources:
//!
//! - Import from a private key
//! - Import from a seed phrase
//! - Derive a child wallet from the master wallet

use crate::assets::Icons;
use crate::core::ZeusContext;
use egui::{Align2, Order, RichText, Stroke, Ui, vec2};

use egui_elements::{Button, OverlayManager, Theme, widgets::Window};

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

   pub fn erase(&mut self) {
      self.import_wallet.erase();
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

      let window_frame = theme.window_frame;
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(RichText::new("Add a new Wallet").size(theme.typography.heading))
         .open(&mut open)
         .order(Order::Middle)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .title_frame(title_frame)
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
               let text = RichText::new("Derive from Master Wallet").size(theme.typography.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  derive_clicked = true;
               }

               // From private key
               let text = RichText::new("Import from a Private Key").size(theme.typography.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  import_from_pk_clicked = true;
               }

               // From seed phrase
               let text = RichText::new("Import from a Seed Phrase").size(theme.typography.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  import_from_seed_clicked = true;
               }
            });
         });

      if derive_clicked {
         open = false;
         self.discover_child_wallets_ui.open();
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
}
