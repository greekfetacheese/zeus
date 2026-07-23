//! UI that allows the user to change their credentials.
//!
//! It only affects the vault, it has no effect on the master wallet recovery.

use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{Align2, Order, RichText, Ui, Window, vec2};
use ncrypt_me::Credentials;
use zeus_theme::{OverlayManager, Theme};
use zeus_ui_components::CredentialsForm;
use zeus_widgets::Button;

pub struct ChangeCredentialsUi {
   open: bool,
   overlay: OverlayManager,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   size: (f32, f32),
}

impl ChangeCredentialsUi {
   pub fn new(overlay: OverlayManager) -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form = CredentialsForm::new()
         .with_min_size(form_size)
         .with_enabled_virtual_keyboard()
         .with_open(true);
      Self {
         open: false,
         overlay,
         credentials_form,
         verified_credentials: false,
         size: (550.0, 350.0),
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

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new(self.overlay.clone());
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      self.verify_credentials_ui(theme, ui);
      self.change_credentials_ui(theme, ui);
   }

   fn verify_credentials_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let window_frame = theme.frame1;

      Window::new(RichText::new("Verify Credentials").size(theme.text_sizes.heading))
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

               let text = RichText::new("Confrim").size(theme.text_sizes.large);
               let button = Button::new(text)
                  .visuals(theme.button_visuals())
                  .min_size(vec2(ui.available_width() * 0.8, 45.0));

               if ui.add(button).clicked() {
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
                              // Allow the user to change the credentials
                              gui.settings.change_credentials_ui.verified_credentials = true;
                              // Erase the credentials form
                              gui.settings.change_credentials_ui.credentials_form.erase();
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
                     };
                  });
               }
            });
         });

      if !open {
         self.reset();
      }
   }

   fn change_credentials_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.verified_credentials || !self.open {
         return;
      }

      self.credentials_form.set_confirm_password(true);

      let mut open = self.open;
      let mut clicked = false;
      let window_frame = theme.frame1;

      Window::new(RichText::new("New Credentials").size(theme.text_sizes.heading))
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

               let visuals = theme.button_visuals();

               let text = RichText::new("Confirm").size(theme.text_sizes.large);
               let button = Button::new(text)
                  .visuals(visuals)
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

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            let mut new_vault = ctx.get_vault();

            let credentials = Credentials::new(username, password, confirm_password);

            match credentials.is_valid() {
               Ok(_) => {}
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Credentials are not valid", e.to_string());
                     gui.request_repaint();
                  });
                  return;
               }
            }

            new_vault.set_credentials(credentials);

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting vault...");
               gui.request_repaint();
            });

            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.settings.change_credentials_ui.reset();
                     gui.loading_window.reset();
                     gui.open_msg_window("Credentials have been updated", "");
                     gui.request_repaint();
                  });
                  ctx.set_vault(new_vault);
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.open_msg_window("Failed to update credentials", format!("{}", e));
                     gui.request_repaint();
                  });
                  return;
               }
            };
         });
      }

      // If the window was open and now we closed it
      if !open {
         self.reset();
      }
   }
}
