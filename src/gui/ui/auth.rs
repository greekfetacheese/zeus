use crate::assets::icons::Icons;
use crate::core::{Account, ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use eframe::egui::{Align, Align2, Button, FontId, Frame, Layout, RichText, Ui, Window, vec2};
use egui::{Color32, Margin};
use egui_theme::Theme;
use egui_theme::utils::{bg_color_on_hover, bg_color_on_idle};
use egui_widgets::SecureTextEdit;
use ncrypt_me::{Argon2Params, Credentials};
use secure_types::SecureString;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputField {
   Username,
   Password,
   ConfirmPassword,
}

pub struct VirtualKeyboard {
   pub open: bool,
   active_target: Option<InputField>,
   shift_active: bool,
   caps_lock_active: bool,
}

impl VirtualKeyboard {
   pub fn new() -> Self {
      Self {
         open: false,
         active_target: None,
         shift_active: false,
         caps_lock_active: false,
      }
   }

   pub fn show(&mut self, ui: &mut Ui, theme: &Theme, credentials: &mut Credentials) {
      if !self.open {
         return;
      }

      // Get a mutable reference to the currently focused SecureString
      let target_str = match self.active_target {
         Some(InputField::Username) => &mut credentials.username,
         Some(InputField::Password) => &mut credentials.password,
         Some(InputField::ConfirmPassword) => &mut credentials.confirm_password,
         None => return, // Don't render if no field is targeted
      };

      // Define the keyboard layout
      let keys_layout_lower = vec![
         vec![
            "`",
            "1",
            "2",
            "3",
            "4",
            "5",
            "6",
            "7",
            "8",
            "9",
            "0",
            "-",
            "=",
            "Backspace",
         ],
         vec![
            "q", "w", "e", "r", "t", "y", "u", "i", "o", "p", "[", "]", "\\",
         ],
         vec![
            "Caps", "a", "s", "d", "f", "g", "h", "j", "k", "l", ";", "'", "Enter",
         ],
         vec![
            "Shift", "z", "x", "c", "v", "b", "n", "m", ",", ".", "/", "Shift",
         ],
      ];
      let keys_layout_upper = vec![
         vec![
            "~",
            "!",
            "@",
            "#",
            "$",
            "%",
            "^",
            "&",
            "*",
            "(",
            ")",
            "_",
            "+",
            "Backspace",
         ],
         vec![
            "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "{", "}", "|",
         ],
         vec![
            "Caps", "A", "S", "D", "F", "G", "H", "J", "K", "L", ":", "\"", "Enter",
         ],
         vec![
            "Shift", "Z", "X", "C", "V", "B", "N", "M", "<", ">", "?", "Shift",
         ],
      ];

      ui.add_space(10.0);
      Frame::group(&theme.style)
         .fill(theme.colors.secondary_bg_color)
         .show(ui, |ui| {
            ui.vertical(|ui| {
               let is_uppercase = self.shift_active ^ self.caps_lock_active;
               let layout = if is_uppercase {
                  &keys_layout_upper
               } else {
                  &keys_layout_lower
               };

               for row in layout {
                  ui.horizontal(|ui| {
                     for &key in row {
                        let key_button =
                           Button::new(RichText::new(key).size(theme.text_sizes.normal))
                              .min_size(vec2(30.0, 30.0));
                        if ui.add(key_button).clicked() {
                           self.handle_key_press(key, target_str);
                        }
                     }
                  });
               }
               // Spacebar
               ui.horizontal(|ui| {
                  if ui
                     .add(Button::new(" ").min_size(vec2(30.0 * 5.0, 30.0)))
                     .clicked()
                  {
                     target_str.push_str(" ");
                  }
               });
            });
         });
   }

   fn handle_key_press(&mut self, key: &str, target: &mut SecureString) {
      match key {
         "Backspace" => {
            target.mut_scope(|s| {
               let len = s.char_len();
               if len > 0 {
                  s.delete_text_char_range(len - 1..len);
               }
            });
         }
         "Shift" => {
            self.shift_active = !self.shift_active;
         }
         "Caps" => {
            self.caps_lock_active = !self.caps_lock_active;
            self.shift_active = false; // Typically, pressing Caps disables Shift
         }
         "Enter" => {
            // For now, we do nothing.
         }
         _ => {
            target.push_str(key);
            // Deactivate shift after a character press
            if self.shift_active {
               self.shift_active = false;
            }
         }
      }
   }
}

pub struct CredentialsForm {
   pub open: bool,
   pub confrim_password: bool,
   /// Flag to allow the Ui to run for one more frame
   ///
   /// Running an extra frame on a text edit can help clear out any strings left in memory
   /// in case we dont use the hide option on a field
   /// But it doesnt always work, maybe i should remove this
   pub additional_frame: bool,
   pub credentials: Credentials,
   pub hide_username: bool,
   pub hide_password: bool,
   pub y_spacing: f32,
   pub x_spacing: f32,
   pub virtual_keyboard: VirtualKeyboard,
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
         virtual_keyboard: VirtualKeyboard::new(),
      }
   }

   pub fn open(mut self, open: bool) -> Self {
      self.open = open;
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

         // --- Username Field ---
         ui.label(RichText::new("Username").size(theme.text_sizes.large));
         self.credentials.username.mut_scope(|username| {
            let text_edit = SecureTextEdit::singleline(username)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .password(self.hide_username)
               .font(FontId::proportional(theme.text_sizes.normal));

            ui.allocate_ui(text_edit_size, |ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.spacing_mut().button_padding = vec2(0.0, 0.0);

                  if text_edit.show(ui).response.gained_focus() {
                     self.virtual_keyboard.open = true;
                     self.virtual_keyboard.active_target = Some(InputField::Username);
                  }

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
         });

         // --- Password Field ---
         ui.label(RichText::new("Password").size(theme.text_sizes.large));
         self.credentials.password.mut_scope(|password| {
            let text_edit = SecureTextEdit::singleline(password)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.text_sizes.normal))
               .password(self.hide_password);

            ui.allocate_ui(text_edit_size, |ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.spacing_mut().button_padding = vec2(0.0, 0.0);

                  if text_edit.show(ui).response.gained_focus() {
                     self.virtual_keyboard.open = true;
                     self.virtual_keyboard.active_target = Some(InputField::Password);
                  }

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
         });

         // --- Confirm Password Field ---
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

                  ui.allocate_ui(text_edit_size, |ui| {
                     ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        ui.spacing_mut().button_padding = vec2(0.0, 0.0);

                        if text_edit.show(ui).response.gained_focus() {
                           self.virtual_keyboard.open = true;
                           self.virtual_keyboard.active_target = Some(InputField::ConfirmPassword);
                        }

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
               });
         } else {
            self.credentials.copy_passwd_to_confirm();
         }

         self.virtual_keyboard.show(ui, theme, &mut self.credentials);
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
         size: (550.0, 350.0),
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
