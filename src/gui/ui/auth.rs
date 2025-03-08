use crate::gui::{
   SHARED_GUI,
   ui::{button, rich_text, text_edit_single},
   utils::{get_encrypted_info, get_profile_dir},
};
use crate::{core::ZeusCtx, gui::utils};
use eframe::egui::{Align2, Frame, Ui, Window, vec2};
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

   pub fn show(&mut self, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = 15.0;

         let username = self.credentials.user_mut();
         let text_edit = text_edit_single(username);

         ui.label(rich_text("Username"));
         ui.add(text_edit);

         let password = self.credentials.passwd_mut();
         let text_edit = text_edit_single(password).password(true);

         ui.label(rich_text("Password"));
         ui.add(text_edit);

         if self.confrim_password {
            let confirm_password = self.credentials.confirm_passwd_mut();
            let text_edit = text_edit_single(confirm_password).password(true);

            ui.label(rich_text("Confirm Password"));
            ui.add(text_edit);
         } else {
            // copy password to confirm password
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
         size: (450.0, 300.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, _theme: &Theme, ui: &mut Ui) {
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
               ui.spacing_mut().item_spacing.y = 15.0;

               ui.label(rich_text("Unlock your profile").size(18.0));

               self.credentials_form.show(ui);

               let button = button(rich_text("Unlock").size(16.0));

               if ui.add(button).clicked() {
                  let mut profile = ctx.profile();
                  profile.credentials = self.credentials_form.credentials.clone();

                  std::thread::spawn(move || {
                     utils::open_loading("Unlocking profile...".to_string());

                     let dir = get_profile_dir();
                     let info = get_encrypted_info(&dir);
                     match profile.decrypt_and_load(&dir) {
                        Ok(_) => {
                           let mut gui = SHARED_GUI.write().unwrap();
                           gui.login.credentials_form.erase();
                           gui.settings.encryption_settings.argon_params = info.argon2_params.clone();
                           gui.portofolio.open = true;
                           gui.top_left_area.open = true;
                           gui.top_left_area.wallet_select.wallet = profile.current_wallet.clone();
                           gui.send_crypto.wallet_select.wallet = profile.current_wallet.clone();
                           gui.loading_window.open = false;

                           ctx.write(|ctx| {
                              ctx.profile = profile;
                              ctx.logged_in = true;
                           });
                        }
                        Err(e) => {
                           let mut gui = SHARED_GUI.write().unwrap();
                           gui.open_msg_window("Failed to unlock profile", e.to_string());
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

   pub fn show(&mut self, ctx: ZeusCtx, _theme: &Theme, ui: &mut Ui) {
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

               ui.label(rich_text("Create a new profile").size(18.0));
               ui.add_space(15.0);

               self.credentials_form.show(ui);
               ui.add_space(15.0);

               {
                  let button = button(rich_text("Create").size(16.0));

                  if ui.add(button).clicked() {
                     let mut profile = ctx.profile();
                     profile.credentials = self.credentials_form.credentials.clone();

                     std::thread::spawn(move || {
                        {
                           let mut gui = SHARED_GUI.write().unwrap();
                           gui.loading_window.msg = "Creating profile...".to_string();
                           gui.loading_window.open = true;
                        }
                        let dir = get_profile_dir();
                        match profile.encrypt_and_save(&dir, Argon2Params::balanced()) {
                           Ok(_) => {
                              let mut gui = SHARED_GUI.write().unwrap();
                              gui.top_left_area.wallet_select.wallet = profile.current_wallet.clone();
                              gui.send_crypto.wallet_select.wallet = profile.current_wallet.clone();
                              gui.register.credentials_form.erase();
                              gui.portofolio.open = true;
                              gui.top_left_area.open = true;
                              gui.loading_window.open = false;

                              ctx.write(|ctx| {
                                 ctx.profile_exists = true;
                                 ctx.logged_in = true;
                                 ctx.profile = profile;
                              });
                           }
                           Err(e) => {
                              let mut gui = SHARED_GUI.write().unwrap();
                              gui.open_msg_window("Failed to create profile", e.to_string());
                              gui.loading_window.open = false;
                           }
                        };
                     });
                  }
               }
            });
         });
   }
}
