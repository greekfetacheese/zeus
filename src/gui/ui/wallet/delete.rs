//! UI that allows the user to delete a wallet

use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{Align2, Id, Order, RichText, Ui, Window, vec2};
use ncrypt_me::Credentials;
use zeus_theme::{OverlayManager, Theme};
use zeus_ui_components::CredentialsForm;
use zeus_widgets::Button;

pub struct DeleteWalletUi {
   open: bool,
   overlay: OverlayManager,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   wallet_to_delete: Option<WalletInfo>,
   size: (f32, f32),
}

impl DeleteWalletUi {
   pub fn new(overlay: OverlayManager) -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form =
         CredentialsForm::new().with_min_size(form_size).with_enabled_virtual_keyboard();
      Self {
         open: false,
         overlay: overlay.clone(),
         credentials_form,
         verified_credentials: false,
         wallet_to_delete: None,
         size: (550.0, 350.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, wallet: WalletInfo) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.open = true;
      self.wallet_to_delete = Some(wallet);
      self.credentials_form.open();
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new(self.overlay.clone());
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      self.verify_credentials_ui(ctx.clone(), theme, ui);
      self.delete_wallet_ui(ctx, theme, ui);
   }

   fn verify_credentials_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.credentials_form.is_open() || !self.open {
         return;
      }

      let button_visuals = theme.button_visuals();
      let window_frame = theme.frame1;
      let mut open = self.credentials_form.is_open();
      let mut clicked = false;

      let id = Id::new("verify_credentials_delete_wallet_ui");
      Window::new(RichText::new("Verify Credentials").size(theme.text_sizes.heading))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                  self.credentials_form.show(theme, ui);
               });

               let text = RichText::new("Confirm").size(theme.text_sizes.normal);
               let button = Button::new(text)
                  .visuals(button_visuals)
                  .min_size(vec2(ui.available_width() * 0.8, 45.0));

               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let mut vault = ctx.get_vault();
         let username = self.credentials_form.username();
         let password = self.credentials_form.password();
         let confirm_password = self.credentials_form.confirm_password();

         let credentials = Credentials::new(username, password, confirm_password);
         vault.set_credentials(credentials);

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
                     gui.wallet_ui.delete_wallet_ui.credentials_form.close();
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

      if !open {
         self.close();
         self.credentials_form.erase();
      }
   }

   fn delete_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.verified_credentials || !self.open {
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
      let window_frame = theme.frame1;

      Window::new(RichText::new("Delete this wallet?").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(200.0);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               ui.label(RichText::new(wallet.name_with_source()).size(theme.text_sizes.normal));
               ui.label(RichText::new(wallet.address.to_string()).size(theme.text_sizes.normal));

               // TODO: Maybe adjust for privacy mode
               let value = ctx.get_total_value(wallet.address);
               ui.label(
                  RichText::new(format!("Value ${}", value.public.abbreviated()))
                     .size(theme.text_sizes.normal),
               );

               let text = RichText::new("Yes").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
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

            // Set the master wallet as selected to avoid state inconsistencies
            if is_current {
               let master_wallet = new_vault.get_master_wallet();
               ctx.write(|ctx| {
                  ctx.current_wallet = master_wallet.clone();
               });
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
            ctx.build_wallet_info_cache();
            
            // Recalculate the wallets
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.open(ctx.clone());
            });
         });
      }

      if !open {
         self.reset();
      }
   }
}
