use crate::gui::{
   SHARED_GUI,
   ui::{button, rich_text},
   utils::{get_encrypted_info, get_account_dir},
};
use crate::{core::ZeusCtx, gui::utils};
use eframe::egui::{Align2, FontId, Frame, TextEdit, Ui, Window, vec2};
use egui::Margin;
use egui_theme::Theme;
use ncrypt_me::{Argon2Params, Credentials};



pub struct CredentialsForm {
   pub open: bool,
   pub confrim_password: bool,
   pub credentials: Credentials,
}

impl CredentialsForm {
   pub fn new() -> Self {
      Self {
         open: false,
         confrim_password: false,
         credentials: Credentials::default(),
      }
   }

   pub fn open(mut self, open: bool) -> Self {
      self.open = open;
      self
   }

   pub fn confirm_password(mut self, confirm_password: bool) -> Self {
      self.confrim_password = confirm_password;
      self
   }

   pub fn erase(&mut self) {
      self.credentials.erase();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = 15.0;
         let ui_width = ui.available_width();
         let text_edit_size = vec2(ui_width * 0.6, 20.0);


         ui.label(rich_text("Username").size(theme.text_sizes.large));
         // ! Username still remains in the buffer
         self.credentials.username.secure_mut(|username| {
           let text_edit = TextEdit::singleline(username)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.text_sizes.normal));
            let mut output = text_edit.show(ui);
            output.state.clear_undoer();
         });


         ui.label(rich_text("Password").size(theme.text_sizes.large));
         self.credentials.password.secure_mut(|password| {
           let text_edit = TextEdit::singleline(password)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.text_sizes.normal))
               .password(true);
            let mut output = text_edit.show(ui);
            output.state.clear_undoer();
         });

         if self.confrim_password {
            ui.label(rich_text("Confirm Password").size(theme.text_sizes.large));
            self.credentials.confirm_password.secure_mut(|confirm_password| {
              let text_edit = TextEdit::singleline(confirm_password)
                  .min_size(text_edit_size)
                  .margin(Margin::same(10))
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .password(true);
               let mut output = text_edit.show(ui);
               output.state.clear_undoer();
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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

               ui.label(rich_text("Unlock your account").size(theme.text_sizes.heading));

               self.credentials_form.show(theme, ui);

               let button =
                  button(rich_text("Unlock").size(theme.text_sizes.large)).min_size(vec2(ui_width * 0.25, 25.0));

               if ui.add(button).clicked() {
                  let mut account = ctx.account();
                  account.credentials = self.credentials_form.credentials.clone();

                  std::thread::spawn(move || {
                     utils::open_loading("Unlocking account...".to_string());

                     let dir = get_account_dir();
                     let info = get_encrypted_info(&dir);
                     match account.decrypt_and_load(&dir) {
                        Ok(_) => {
                           let mut gui = SHARED_GUI.write().unwrap();
                           gui.login.credentials_form.erase();
                           gui.settings.encryption.argon_params = info.argon2_params.clone();
                           gui.portofolio.open = true;
                           gui.top_left_area.open = true;
                           gui.top_left_area.wallet_select.wallet = account.current_wallet.clone();
                           gui.send_crypto.wallet_select.wallet = account.current_wallet.clone();
                           gui.loading_window.open = false;

                           ctx.write(|ctx| {
                              ctx.account = account;
                              ctx.logged_in = true;
                           });
                        }
                        Err(e) => {
                           let mut gui = SHARED_GUI.write().unwrap();
                           gui.open_msg_window("Failed to unlock account", e.to_string());
                           gui.loading_window.open = false;
                        }
                     };
                  });
               }
            });
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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

               ui.label(rich_text("Create a new account").size(theme.text_sizes.heading));
               ui.add_space(15.0);

               self.credentials_form.show(theme, ui);
               ui.add_space(15.0);

               let button =
                  button(rich_text("Create").size(theme.text_sizes.large)).min_size(vec2(ui_width * 0.25, 25.0));

               if ui.add(button).clicked() {
                  let mut account = ctx.account();
                  account.credentials = self.credentials_form.credentials.clone();

                  std::thread::spawn(move || {
                     {
                        let mut gui = SHARED_GUI.write().unwrap();
                        gui.loading_window.msg = "Creating account...".to_string();
                        gui.loading_window.open = true;
                     }
                     let dir = get_account_dir();
                     match account.encrypt_and_save(&dir, Argon2Params::balanced()) {
                        Ok(_) => {
                           let mut gui = SHARED_GUI.write().unwrap();
                           gui.top_left_area.wallet_select.wallet = account.current_wallet.clone();
                           gui.send_crypto.wallet_select.wallet = account.current_wallet.clone();
                           gui.register.credentials_form.erase();
                           gui.portofolio.open = true;
                           gui.top_left_area.open = true;
                           gui.loading_window.open = false;

                           ctx.write(|ctx| {
                              ctx.account_exists = true;
                              ctx.logged_in = true;
                              ctx.account = account;
                           });
                        }
                        Err(e) => {
                           let mut gui = SHARED_GUI.write().unwrap();
                           gui.open_msg_window("Failed to create account", e.to_string());
                           gui.loading_window.open = false;
                        }
                     };
                  });
               }
            });
         });
   }
}
