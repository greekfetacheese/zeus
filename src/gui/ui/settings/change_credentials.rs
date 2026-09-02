//! UI that allows the user to change their credentials.
//!
//! It only affects the vault, it has no effect on the master wallet recovery.

use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{RichText, Ui, Vec2, vec2};
use egui_elements::{Button, CredentialsForm, Theme};
use ncrypt_me::Credentials;

pub struct ChangeCredentialsUi {
   ui_size: Vec2,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
}

impl ChangeCredentialsUi {
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form = CredentialsForm::new()
         .with_min_size(form_size)
         .with_enabled_virtual_keyboard()
         .with_open(true);
      Self {
         ui_size: vec2(550.0, 500.0),
         credentials_form,
         verified_credentials: false,
      }
   }

   pub fn erase(&mut self) {
      self.credentials_form.erase();
   }

   pub fn reset(&mut self) {
      *self = Self::new();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
      ui.label(RichText::new("Change Credentials").size(theme.typography.very_large));
      ui.label(
         RichText::new("This only changes vault encryption credentials. It does not change master wallet recovery.")
            .size(theme.typography.normal)
            .color(theme.colors.text_muted),
      );

      ui.add_space(8.0);

      let frame = theme.frame1;

      frame.show(ui, |ui| {
         ui.set_max_size(self.ui_size);

      if !self.verified_credentials {
         self.verify_credentials_ui(theme, ui);
      } else {
         ui.set_max_height(self.ui_size.y + 50.0);
         self.change_credentials_ui(theme, ui);
      }
   });

   });
   }

   fn verify_credentials_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing.y = 16.0;
      ui.spacing_mut().button_padding = theme.button_padding;

      ui.label(RichText::new("Verify current credentials").size(theme.typography.large));

      ui.scope(|ui| {
         ui.spacing_mut().button_padding = vec2(4.0, 4.0);
         self.credentials_form.show(ui);
      });

      let text = RichText::new("Confirm").size(theme.typography.large);
      let button = Button::new(text).visuals(theme.button_visuals()).min_size(vec2(200.0, 45.0));

      if ui.add(button).clicked() {
         let username = self.credentials_form.username();
         let password = self.credentials_form.password();
         let confirm_password = self.credentials_form.confirm_password();

         let credentials = Credentials::new(username, password, confirm_password);

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Checking credentials...");
               gui.request_repaint();
               gui.ctx.clone()
            });

            let creds_match = ctx.read_vault(|vault| vault.credentials_match(&credentials));

            match creds_match {
               true => {
                  SHARED_GUI.write(|gui| {
                     gui.settings.change_credentials_ui.verified_credentials = true;
                     gui.settings.change_credentials_ui.credentials_form.erase();
                     gui.loading_window.reset();
                     gui.request_repaint();
                  });
               }
               false => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Credentials do not match");
                     gui.loading_window.reset();
                     gui.request_repaint();
                  });
                  return;
               }
            }
         });
      }
   }

   fn change_credentials_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      self.credentials_form.set_confirm_password(true);

      ui.spacing_mut().item_spacing.y = 16.0;
      ui.spacing_mut().button_padding = theme.button_padding;

      ui.label(RichText::new("Enter new credentials").size(theme.typography.large));

      ui.scope(|ui| {
         ui.spacing_mut().button_padding = vec2(4.0, 4.0);
         self.credentials_form.show(ui);
      });

      let visuals = theme.button_visuals();

      let text = RichText::new("Confirm").size(theme.typography.large);
      let button = Button::new(text).visuals(visuals).min_size(vec2(200.0, 45.0));

      if ui.add(button).clicked() {
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
                     gui.open_msg_window(e.to_string());
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
                     gui.open_msg_window("Credentials have been updated");
                     gui.request_repaint();
                  });
                  ctx.set_vault(new_vault);
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.open_msg_window(format!("Failed to update credentials: {}", e));
                     gui.request_repaint();
                  });
                  return;
               }
            };
         });
      }
   }
}
