//! UI that allows the user to export a private key

use crate::core::ZeusContext;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{Align2, Order, RichText, Ui, Window, vec2};
use ncrypt_me::Credentials;
use zeus_theme::{OverlayManager, Theme};
use zeus_ui_components::{CredentialsForm, QrImage};
use zeus_wallet::Wallet;
use zeus_widgets::Button;
use tracing::{info, error};

pub struct ExportKeyUi {
   open: bool,
   overlay: OverlayManager,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   wallet_to_export: Option<Wallet>,
   private_key_qr: QrImage,
   show_key: bool,
   show_key_qrcode: bool,
   size: (f32, f32),
}

impl ExportKeyUi {
   pub fn new(overlay: OverlayManager) -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form =
         CredentialsForm::new().with_min_size(form_size).with_enabled_virtual_keyboard();
      Self {
         open: false,
         overlay: overlay.clone(),
         credentials_form,
         verified_credentials: false,
         wallet_to_export: None,
         private_key_qr: QrImage::empty_with_error("No QR code found".to_string()),
         show_key: false,
         show_key_qrcode: false,
         size: (550.0, 350.0),
      }
   }

   pub fn open(&mut self, wallet: Option<Wallet>) {
      if let Some(wallet) = &wallet {
         let key_hex = wallet.key_string();
         let uri = format!(
            "bytes://key-{}.png",
            &wallet.address().to_string()
         );

         RT.spawn_blocking(move || {
            let qr_image = key_hex.unlock_str(|key| QrImage::new(key, uri));
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.export_key_ui.private_key_qr = qr_image;
            });
         });
      }

      if !self.open {
         self.overlay.window_opened();
      }

      self.open = true;
      self.credentials_form.open();
      self.wallet_to_export = wallet;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   fn reset(&mut self) {
      self.close();
      *self = Self::new(self.overlay.clone());
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      self.verify_credentials_ui(theme, ui);
      self.show_key(ctx, theme, ui);
   }

   fn show_key(&mut self, _ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.show_key || !self.verified_credentials {
         return;
      }

      let title = RichText::new("Success").size(theme.text_sizes.heading);
      let window_frame = theme.frame1;

      Window::new(title)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();
            let button_size = vec2(100.0, 20.0);
            let button_size_area = vec2(210.0, 20.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(10.0);

               if let Some(wallet) = self.wallet_to_export.as_ref() {
                  let warning_text = "Make sure to save this key in a safe place!";
                  ui.label(RichText::new(warning_text).size(theme.text_sizes.large));

                  ui.allocate_ui(button_size_area, |ui| {
                     ui.horizontal(|ui| {
                        let text = RichText::new("Copy Key").size(theme.text_sizes.normal);
                        let button =
                           Button::new(text).visuals(button_visuals).min_size(button_size);

                        if ui.add(button).clicked() {
                           ui.ctx()
                              .copy_text(wallet.key_string().unlock_str(|key| key.to_string()));
                        }

                        let text = RichText::new("Show QR Code").size(theme.text_sizes.normal);
                        let button =
                           Button::new(text).visuals(button_visuals).min_size(button_size);

                        if ui.add(button).clicked() {
                           self.show_key_qrcode = true;
                        }
                     });
                  });

                  if self.show_key_qrcode {
                     if let Some(error) = self.private_key_qr.error() {
                        ui.label(RichText::new(error.to_string()).size(theme.text_sizes.large));
                     }

                     let image = self.private_key_qr.image().fit_to_exact_size(vec2(250.0, 250.0));
                     ui.add(image);
                  }
               } else {
                  ui.label(
                     RichText::new("No wallet found, this is a bug").size(theme.text_sizes.normal),
                  );
               }

               let text = RichText::new("Close").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  let erased = self.private_key_qr.clear(ui.ctx());
                  self.reset();

                  if erased {
                     info!("PK QR Image data zeroized");
                  } else {
                     error!("PK QR Image data zeroize failed");
                  }
               }
            });
         });
   }

   fn verify_credentials_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.credentials_form.is_open() || !self.open {
         return;
      }

      let mut open = self.credentials_form.is_open();
      let window_frame = theme.frame1;
      let mut clicked = false;

      Window::new(RichText::new("Verify Credentials").size(theme.text_sizes.heading))
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                  self.credentials_form.show(theme, ui);
               });

               let text = RichText::new("Confrim").size(theme.text_sizes.normal);
               let button = Button::new(text)
                  .visuals(button_visuals)
                  .min_size(vec2(ui.available_width() * 0.8, 45.0));

               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let username = self.credentials_form.username();
         let password = self.credentials_form.password();
         let confirm_password = self.credentials_form.confirm_password();
         let credentials = Credentials::new(username, password, confirm_password);

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Decrypting vault...");
               gui.request_repaint();
               gui.ctx.clone()
            });

            let mut vault = ctx.get_vault();
            vault.set_credentials(credentials);

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
                     gui.wallet_ui.export_key_ui.credentials_form.close();
                     gui.loading_window.reset();
                     gui.request_repaint();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to decrypt vault", e.to_string());
                     gui.loading_window.reset();
                     gui.request_repaint();
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
}
