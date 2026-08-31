//! UI that allows the user to import a wallet from a private key or a seed phrase

use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{Align2, FontId, Margin, Order, RichText, Stroke, Ui, vec2};
use egui_elements::{
   Button, OverlayManager, SecureInputField, SecureTextEdit, Theme, widgets::Window,
};
use zeus_eth::types::SUPPORTED_CHAINS;

#[derive(PartialEq, Eq)]
pub enum ImportWalletType {
   PrivateKey,
   MnemonicPhrase,
}

pub struct ImportWallet {
   open: bool,
   overlay: OverlayManager,
   import_key_or_phrase: ImportWalletType,
   input_field: SecureInputField,
   wallet_name: String,
   size: (f32, f32),
}

impl ImportWallet {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         import_key_or_phrase: ImportWalletType::PrivateKey,
         input_field: SecureInputField::new("Import Wallet", true, true),
         wallet_name: String::new(),
         size: (450.0, 250.0),
      }
   }

   pub fn erase(&mut self) {
      self.input_field.erase();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, import_type: ImportWalletType) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
      self.import_key_or_phrase = import_type;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let was_open = self.open;
      let mut is_open = self.open;
      let mut clicked = false;

      let window_frame = theme.window_frame;
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(RichText::new("Import Wallet").size(theme.typography.heading))
         .open(&mut is_open)
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
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let size = vec2(ui.available_width() * 0.5, 40.0);
               ui.add_space(20.0);

               // Wallet Name
               ui.label(RichText::new("Wallet Name (Optional)").size(theme.typography.large));
               ui.add(
                  SecureTextEdit::singleline(&mut self.wallet_name)
                     .visuals(text_edit_visuals)
                     .font(FontId::proportional(theme.typography.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               // Private Key or Phrase
               let text = if self.import_key_or_phrase == ImportWalletType::PrivateKey {
                  "Private Key"
               } else {
                  "Seed Phrase"
               };

               // Input Field
               let size = vec2(ui.available_width() * 0.7, 40.0);
               self.input_field.set_min_size(size);
               self.input_field.set_id(text);

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                  self.input_field.show(ui);
               });

               // Import Button
               let text = RichText::new("Import").size(theme.typography.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();
         let key_or_phrase = self.input_field.text();
         let from_key = self.import_key_or_phrase == ImportWalletType::PrivateKey;

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            let mut new_vault = ctx.get_vault();

            // Import the wallet
            let new_wallet_address =
               match new_vault.new_wallet_from_key_or_phrase(name, from_key, key_or_phrase) {
                  Ok(address) => address,
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.open_msg_window(format!(
                           "Failed to import wallet: {}",
                           e.to_string()
                        ));
                        gui.request_repaint();
                     });
                     return;
                  }
               };

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting account...");
               gui.request_repaint();
            });

            // Encrypt the account
            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.open_msg_window("Wallet imported successfully");
                     gui.wallet_ui.add_wallet_ui.import_wallet.input_field.erase();
                     gui.wallet_ui.add_wallet_ui.import_wallet.wallet_name.clear();
                     gui.request_repaint();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.open_msg_window(format!(
                        "Failed to encrypt account: {}",
                        e.to_string()
                     ));
                     gui.request_repaint();
                  });
                  return;
               }
            };

            ctx.set_vault(new_vault);
            ctx.build_wallet_info_cache();

            let ctx_clone = ctx.clone();
            RT.spawn(async move {
               for chain in SUPPORTED_CHAINS {
                  if let Err(e) = ctx_clone.register_railgun_signers(chain, false).await {
                     tracing::error!("Error registering Railgun signers: {:?}", e);
                  }
               }
            });

            // Recalculate the wallets
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.calc_wallet_value();
            });

            // Fetch the balance for the new wallet across all chains and add it to the portfolio db
            RT.spawn(async move {
               let manager = ctx.balance_manager();
               for chain in SUPPORTED_CHAINS {
                  if ctx.is_chain_disabled(chain) {
                     continue;
                  }

                  match manager
                     .update_eth_balance(
                        ctx.clone(),
                        chain,
                        vec![new_wallet_address],
                        false,
                     )
                     .await
                  {
                     Ok(_) => {}
                     Err(e) => {
                        tracing::error!("Failed to update ETH balance: {}", e);
                     }
                  }

                  // Portfolio is created and saved here
                  ctx.update_public_data(chain, new_wallet_address);
               }
            });
         });
      }

      if !is_open {
         self.close();
      }

      if was_open && !self.open {
         self.input_field.erase();
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.open();
            });
         });
      }
   }
}
