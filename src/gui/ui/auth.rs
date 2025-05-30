use crate::assets::icons::Icons;
use crate::core::{Account, ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use eframe::egui::{Align, Align2, Button, FontId, Frame, Layout, RichText, Ui, Window, vec2};
use egui::{Color32, Margin};
use egui_theme::utils::{bg_color_on_hover, bg_color_on_idle};
use egui_theme::Theme;
use egui_widgets::SecureTextEdit;
#[cfg(feature = "dev")]
use ncrypt_me::secure_types::SecureString;
use ncrypt_me::{Argon2Params, Credentials};
use std::sync::Arc;

pub struct CredentialsForm {
   pub open: bool,
   pub confrim_password: bool,
   /// Flag to allow the Ui to run for one more frame
   pub additional_frame: bool,
   pub credentials: Credentials,
   pub hide_username: bool,
   pub hide_password: bool,
   pub y_spacing: f32,
   pub x_spacing: f32,
   // By how much to add horizontal space for the text edits
   // Eg. 0.2 means 20% of the available width
   pub text_edit_h_space: f32,
}

impl CredentialsForm {
   pub fn new() -> Self {
      Self {
         open: false,
         confrim_password: false,
         additional_frame: false,
         credentials: Credentials::new_with_capacity(1024).unwrap(),
         hide_username: false,
         hide_password: true,
         y_spacing: 15.0,
         x_spacing: 10.0,
         text_edit_h_space: 0.2,
      }
   }

   pub fn open(mut self, open: bool) -> Self {
      self.open = open;
      self
   }

   pub fn with_text_edit_h_space(mut self, text_edit_h_space: f32) -> Self {
      self.text_edit_h_space = text_edit_h_space;
      self
   }

   pub fn y_spacing(mut self, y_spacing: f32) -> Self {
      self.y_spacing = y_spacing;
      self
   }

   pub fn x_spacing(mut self, x_spacing: f32) -> Self {
      self.x_spacing = x_spacing;
      self
   }

   pub fn confirm_password(mut self, confirm_password: bool) -> Self {
      self.confrim_password = confirm_password;
      self
   }

   pub fn erase(&mut self) {
      self.credentials.erase();
   }

   pub fn show(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = self.y_spacing;
         ui.spacing_mut().item_spacing.x = self.x_spacing;
         bg_color_on_idle(ui, Color32::TRANSPARENT);
         bg_color_on_hover(ui, Color32::TRANSPARENT);
         let ui_width = ui.available_width();
         let text_edit_size = vec2(ui_width * 0.6, 20.0);

         ui.label(RichText::new("Username").size(theme.text_sizes.large));
         self.credentials.username.mut_scope(|username| {
            let text_edit = SecureTextEdit::singleline(username)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .password(self.hide_username)
               .font(FontId::proportional(theme.text_sizes.normal));

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.add_space(ui_width * self.text_edit_h_space);
               text_edit.show(ui);
               ui.spacing_mut().button_padding = vec2(0.0, 0.0);


               let icon = if self.hide_username {
                  icons.hide_light()
               } else {
                  icons.view_light()
               };

               let hide_view = Button::image(icon);
               if ui.add(hide_view).clicked() {
                  self.hide_username = !self.hide_username;
               }
            });
         });

         ui.label(RichText::new("Password").size(theme.text_sizes.large));
         self.credentials.password.mut_scope(|password| {
            let text_edit = SecureTextEdit::singleline(password)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.text_sizes.normal))
               .password(self.hide_password);

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.add_space(ui_width * self.text_edit_h_space);
               text_edit.show(ui);
               ui.spacing_mut().button_padding = vec2(0.0, 0.0);

               let icon = if self.hide_password {
                  icons.hide_light()
               } else {
                  icons.view_light()
               };

               let hide_view = Button::image(icon);
               if ui.add(hide_view).clicked() {
                  self.hide_password = !self.hide_password;
               }
            });
         });

         if self.confrim_password {
            ui.label(RichText::new("Confirm Password").size(theme.text_sizes.large));
            self
               .credentials
               .confirm_password
               .mut_scope(|confirm_password| {
                  let text_edit = SecureTextEdit::singleline(confirm_password)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .password(self.hide_password);

                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     ui.add_space(ui_width * self.text_edit_h_space);
                     text_edit.show(ui);
                     ui.spacing_mut().button_padding = vec2(0.0, 0.0);

                     let icon = if self.hide_password {
                        icons.hide_light()
                     } else {
                        icons.view_light()
                     };

                     let hide_view = Button::image(icon);
                     if ui.add(hide_view).clicked() {
                        self.hide_password = !self.hide_password;
                     }
                  });
               });
         } else {
            self.credentials.copy_passwd_to_confirm();
         }
      });
   }
}

pub struct LoginUi {
   pub credentials_form: CredentialsForm,
   pub size: (f32, f32),
}

impl LoginUi {
   pub fn new() -> Self {
      Self {
         credentials_form: CredentialsForm::new().open(true),
         size: (550.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let account_exists = ctx.account_exists();
      let logged_in = ctx.logged_in();

      let open = self.credentials_form.additional_frame || (account_exists && !logged_in);

      if !open {
         return;
      }

      if self.credentials_form.additional_frame {
         tracing::info!("Running additional frame");
         self.credentials_form.additional_frame = false;
      }

      Window::new("Login_ui")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.add_space(10.0);
               ui.spacing_mut().item_spacing.y = 25.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let ui_width = ui.available_width();

               ui.label(RichText::new("Unlock your account").size(theme.text_sizes.heading));

               self.credentials_form.show(theme, icons, ui);

               let button = Button::new(RichText::new("Unlock").size(theme.text_sizes.large))
                  .min_size(vec2(ui_width * 0.25, 25.0));

               if ui.add(button).clicked() {
                  let mut account = ctx.get_account();
                  account.set_credentials(self.credentials_form.credentials.clone());
                  self.login(ctx.clone(), account);
               }

               #[cfg(feature = "dev")]
               if ui.button("Dev Login").clicked() {
                  let credentials = Credentials::new(
                     SecureString::from("dev"),
                     SecureString::from("dev"),
                     SecureString::from("dev"),
                  );
                  let mut account = ctx.get_account();
                  account.set_credentials(credentials);
                  self.login(ctx, account);
               }
            });
         });
   }

   fn login(&self, ctx: ZeusCtx, mut account: Account) {
      RT.spawn_blocking(move || {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Unlocking account...");
         });

         // Decrypt the account
         let data = match account.decrypt(None) {
            Ok(data) => data,
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.open_msg_window("Failed to unlock account", e.to_string());
                  gui.loading_window.open = false;
               });
               return;
            }
         };

         let info = account.encrypted_info().unwrap();

         // Load the account
         match account.load(data) {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.login.credentials_form.erase();
                  gui.login.credentials_form.additional_frame = true;
                  gui.portofolio.open = true;
                  gui.wallet_selection.open = true;
                  gui.chain_selection.open = true;
                  gui.loading_window.open = false;
                  gui.settings.encryption.argon_params = info.argon2_params.clone();
                  gui.wallet_selection.wallet_select.wallet = account.current_wallet.clone();
               });

               ctx.write(|ctx| {
                  ctx.logged_in = true;
               });
               ctx.set_account(account);
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.open_msg_window("Failed to load account", e.to_string());
                  gui.loading_window.open = false;
               });
            }
         }
      });
   }
}

pub struct RegisterUi {
   pub credentials_form: CredentialsForm,
   pub size: (f32, f32),
}

impl RegisterUi {
   pub fn new() -> Self {
      Self {
         credentials_form: CredentialsForm::new().open(true).confirm_password(true),
         size: (450.0, 300.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let account_exists = ctx.account_exists();
      let open = self.credentials_form.additional_frame || !account_exists;

      if !open {
         return;
      }

      if self.credentials_form.additional_frame {
         tracing::info!("Running additional frame");
         self.credentials_form.additional_frame = false;
      }

      Window::new("Register_ui")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.add_space(10.0);
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let ui_width = ui.available_width();

               ui.label(RichText::new("Create a new account").size(theme.text_sizes.heading));
               ui.add_space(15.0);

               self.credentials_form.show(theme, icons, ui);
               ui.add_space(15.0);

               let button = Button::new(RichText::new("Create").size(theme.text_sizes.large))
                  .min_size(vec2(ui_width * 0.25, 25.0));

               if ui.add(button).clicked() {
                  let mut account = ctx.get_account();
                  account.set_credentials(self.credentials_form.credentials.clone());

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Creating account...");
                     });

                     // Encrypt the account
                     let data = match account.encrypt(Some(Argon2Params::balanced())) {
                        Ok(data) => data,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Failed to create account", e.to_string());
                              gui.loading_window.open = false;
                           });
                           return;
                        }
                     };

                     // Save the new account encrypted data to the account file
                     match account.save(None, data) {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              gui.wallet_selection.wallet_select.wallet =
                                 account.current_wallet.clone();
                              gui.register.credentials_form.erase();
                              gui.register.credentials_form.additional_frame = true;
                              gui.portofolio.open = true;
                              gui.wallet_selection.open = true;
                              gui.chain_selection.open = true;
                              gui.loading_window.open = false;
                           });

                           ctx.set_account(account);
                           ctx.write(|ctx| {
                              ctx.account_exists = true;
                              ctx.logged_in = true;
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.loading_window.open = false;
                              gui.open_msg_window("Failed to save account", e.to_string());
                           });
                           return;
                        }
                     };
                  });
               }
            });
         });
   }
}
