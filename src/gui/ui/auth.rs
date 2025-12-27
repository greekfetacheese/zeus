use crate::assets::icons::Icons;
use crate::core::{M_COST, Vault, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{
   Align, Align2, Button, FontId, Frame, Layout, RichText, TextEdit, Ui, Window, vec2,
};
use egui::Margin;
use ncrypt_me::{Argon2, Credentials};
use secure_types::SecureString;
use std::sync::Arc;
use std::time::Instant;
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Label, SecureTextEdit};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputField {
   Username,
   Password,
   ConfirmPassword,
}

pub struct VirtualKeyboard {
   pub open: bool,
   active_target: InputField,
   shift_active: bool,
   caps_lock_active: bool,
}

impl VirtualKeyboard {
   pub fn new(open: bool) -> Self {
      Self {
         open,
         active_target: InputField::Username,
         shift_active: false,
         caps_lock_active: false,
      }
   }

   pub fn show(&mut self, ui: &mut Ui, theme: &Theme, credentials: &mut Credentials) {
      if !self.open {
         return;
      }

      let target_str = match self.active_target {
         InputField::Username => &mut credentials.username,
         InputField::Password => &mut credentials.password,
         InputField::ConfirmPassword => &mut credentials.confirm_password,
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

      let frame = theme.frame2;

      frame.show(ui, |ui| {
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
                     let key_button = Button::new(RichText::new(key).size(theme.text_sizes.normal))
                        .min_size(vec2(30.0, 30.0));
                     if ui.add(key_button).clicked() {
                        self.handle_key_press(key, target_str);
                     }
                  }
               });
            }
            // Spacebar
            ui.horizontal(|ui| {
               if ui.add(Button::new(" ").min_size(vec2(30.0 * 5.0, 30.0))).clicked() {
                  target_str.push_str(" ");
               }
            });
         });
      });
   }

   fn handle_key_press(&mut self, key: &str, target: &mut SecureString) {
      match key {
         "Backspace" => {
            target.unlock_mut(|s| {
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
   overlay: OverlayManager,
   pub confrim_password: bool,
   pub credentials: Credentials,
   pub hide_username: bool,
   pub hide_password: bool,
   pub y_spacing: f32,
   pub x_spacing: f32,
   pub virtual_keyboard: VirtualKeyboard,
}

impl CredentialsForm {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         confrim_password: false,
         credentials: Credentials::new_with_capacity(1024).unwrap(),
         hide_username: false,
         hide_password: true,
         y_spacing: 15.0,
         x_spacing: 10.0,
         virtual_keyboard: VirtualKeyboard::new(true),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.overlay.window_opened();
      self.open = true;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn with_open(mut self, open: bool) -> Self {
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

      let tint = theme.image_tint_recommended;

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = self.y_spacing;
         ui.spacing_mut().item_spacing.x = self.x_spacing;

         let ui_width = ui.available_width();
         let text_edit_size = vec2(ui_width * 0.6, 20.0);

         // Username Field
         ui.label(RichText::new("Username").size(theme.text_sizes.large));
         self.credentials.username.unlock_mut(|username| {
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
                     self.virtual_keyboard.active_target = InputField::Username;
                  }

                  let icon = if self.hide_username {
                     match theme.dark_mode {
                        true => icons.hide_light(tint),
                        _ => icons.hide_dark(),
                     }
                  } else {
                     match theme.dark_mode {
                        true => icons.view_light(tint),
                        _ => icons.view_dark(),
                     }
                  };

                  let hide_view = Button::image(icon);
                  if ui.add(hide_view).clicked() {
                     self.hide_username = !self.hide_username;
                  }
               });
            });
         });

         // Password Field
         ui.label(RichText::new("Password").size(theme.text_sizes.large));
         self.credentials.password.unlock_mut(|password| {
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
                     self.virtual_keyboard.active_target = InputField::Password;
                  }

                  let icon = if self.hide_password {
                     match theme.dark_mode {
                        true => icons.hide_light(tint),
                        _ => icons.hide_dark(),
                     }
                  } else {
                     match theme.dark_mode {
                        true => icons.view_light(tint),
                        _ => icons.view_dark(),
                     }
                  };

                  let hide_view = Button::image(icon);
                  if ui.add(hide_view).clicked() {
                     self.hide_password = !self.hide_password;
                  }
               });
            });
         });

         // Confirm Password Field
         if self.confrim_password {
            ui.label(RichText::new("Confirm Password").size(theme.text_sizes.large));
            self.credentials.confirm_password.unlock_mut(|confirm_password| {
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
                        self.virtual_keyboard.active_target = InputField::ConfirmPassword;
                     }

                     let icon = if self.hide_password {
                        match theme.dark_mode {
                           true => icons.hide_light(tint),
                           _ => icons.hide_dark(),
                        }
                     } else {
                        match theme.dark_mode {
                           true => icons.view_light(tint),
                           _ => icons.view_dark(),
                        }
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

pub struct UnlockVault {
   pub credentials_form: CredentialsForm,
   pub size: (f32, f32),
}

impl UnlockVault {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         credentials_form: CredentialsForm::new(overlay).with_open(true),
         size: (550.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let vault_exists = ctx.vault_exists();
      let vault_unlocked = ctx.vault_unlocked();

      let open = vault_exists && !vault_unlocked;

      if !open {
         return;
      }

      Window::new("Unlock_Vault_Ui")
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

               ui.label(RichText::new("Unlock your Vault").size(theme.text_sizes.heading));

               self.credentials_form.show(theme, icons, ui);

               let button = Button::new(RichText::new("Unlock").size(theme.text_sizes.large))
                  .min_size(vec2(ui_width * 0.25, 25.0));

               if ui.add(button).clicked() {
                  let mut vault = ctx.get_vault();
                  vault.set_credentials(self.credentials_form.credentials.clone());
                  self.unlock_vault(ctx.clone(), vault);
               }

               #[cfg(feature = "dev")]
               if ui.button("Dev Login").clicked() {
                  let credentials = Credentials::new(
                     SecureString::from("dev"),
                     SecureString::from("dev"),
                     SecureString::from("dev"),
                  );
                  let mut vault = ctx.get_vault();
                  vault.set_credentials(credentials);
                  self.unlock_vault(ctx, vault);
               }
            });
         });
   }

   fn unlock_vault(&self, ctx: ZeusCtx, mut vault: Vault) {
      RT.spawn_blocking(move || {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Unlocking vault...");
         });

         // Decrypt the vault
         let data = match vault.decrypt(None) {
            Ok(data) => data,
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.open_msg_window("Failed to unlock vault", e.to_string());
                  gui.loading_window.reset();
               });
               return;
            }
         };

         let info = match vault.encrypted_info() {
            Ok(info) => info,
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.open_msg_window(
                     "Error while reading encrypted info, corrupted vault?",
                     e.to_string(),
                  );
                  gui.loading_window.reset();
               });
               return;
            }
         };

         // Load the vault
         match vault.load(data) {
            Ok(_) => {
               let current_wallet = vault.get_master_wallet();
               SHARED_GUI.write(|gui| {
                  gui.unlock_vault_ui.credentials_form.erase();
                  gui.portofolio.open();
                  gui.loading_window.reset();
                  gui.settings.encryption.set_argon2(info.argon2);
                  gui.header.open();
                  gui.header.set_current_wallet(current_wallet);
               });

               ctx.write(|ctx| {
                  ctx.vault_unlocked = true;
                  ctx.current_wallet = vault.get_master_wallet();
               });

               ctx.set_vault(vault);
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.open_msg_window("Failed to load vault", e.to_string());
                  gui.loading_window.reset();
               });
            }
         }
      });
   }
}

struct SystemMemory {
   total: u64,
   available: u64,
   last_time_checked: Instant,
}

impl SystemMemory {
   pub fn new() -> Self {
      let mut sys = sysinfo::System::new();
      sys.refresh_all();
      let total = sys.total_memory();
      let available = sys.available_memory();
      Self {
         total,
         available,
         last_time_checked: Instant::now(),
      }
   }

   fn update(&mut self) {
      let now = Instant::now();
      if now.duration_since(self.last_time_checked).as_secs() > 1 {
         let mut sys = sysinfo::System::new();
         sys.refresh_all();

         self.total = sys.total_memory();
         self.available = sys.available_memory();
         self.last_time_checked = Instant::now();
      }
   }

   fn total_gb(&self) -> f64 {
      self.total as f64 / 1024f64 / 1024f64 / 1024f64
   }

   fn available_gb(&self) -> f64 {
      self.available as f64 / 1024f64 / 1024f64 / 1024f64
   }
}

/// Recover an HD wallet from the credentials and create a Vault
pub struct RecoverHDWallet {
   credentials_form: CredentialsForm,
   wallet_name: String,
   credentials_input: bool,
   recover_button_clicked: bool,
   show_recover_wallet: bool,
   show_tips: bool,
   memory: SystemMemory,
   pub size: (f32, f32),
   size2: (f32, f32),
}

impl RecoverHDWallet {
   pub fn new(ovelay: OverlayManager) -> Self {
      Self {
         credentials_form: CredentialsForm::new(ovelay).with_open(true).confirm_password(true),
         wallet_name: String::new(),
         credentials_input: true,
         recover_button_clicked: false,
         show_recover_wallet: false,
         show_tips: false,
         memory: SystemMemory::new(),
         size: (550.0, 350.0),
         size2: (350.0, 250.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if ctx.vault_exists() {
         return;
      }

      self.credentials_input(theme, icons, ui);
      self.recover_hd_wallet(ctx.clone(), theme, ui);
      self.show_tips(ctx, theme, ui);
   }

   fn show_requirements_warning(&mut self, theme: &Theme, ui: &mut Ui) {
      self.memory.update();

      ui.add_space(10.0);

      let m_cost_bytes = M_COST as u64 * 1024;
      let m_cost_gb = m_cost_bytes as f64 / 1_000_000_000.0;

      // Maybe also consider swap as free memory?
      let mem_avail = self.memory.available > m_cost_bytes;
      let meets_min_mem = self.memory.total > m_cost_bytes;

      if !mem_avail && meets_min_mem {
         let text1 = format!(
            "You need at least {:.2} GB of free RAM to recover your wallet",
            m_cost_gb
         );

         let text2 = format!(
            "You currently have {:.2} GB of free RAM",
            self.memory.available_gb()
         );

         ui.label(RichText::new(text1).size(theme.text_sizes.normal).color(theme.colors.warning));
         ui.label(RichText::new(text2).size(theme.text_sizes.normal).color(theme.colors.warning));
      }

      if !meets_min_mem {
         let text = format!(
            "Your system doesn't meet the minimum requirements,\n
                  detected {:.2} GB of RAM, need {:.2} GB",
            self.memory.total_gb(),
            m_cost_gb
         );

         ui.label(RichText::new(text).size(theme.text_sizes.normal).color(theme.colors.warning));
      }
   }

   fn credentials_input(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.credentials_input {
         return;
      }

      Window::new("Recover_HD_Wallet_credentials_input")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               self.show_requirements_warning(theme, ui);

               let ui_width = ui.available_width();

               ui.label(RichText::new("No vault was found").size(theme.text_sizes.heading));
               ui.label(
                  RichText::new("Recover your wallet from your credentials")
                     .size(theme.text_sizes.large),
               );

               // Credentials input
               self.credentials_form.show(theme, icons, ui);

               let next_button = Button::new(RichText::new("Next").size(theme.text_sizes.large))
                  .min_size(vec2(ui_width * 0.25, 25.0));

               if ui.add(next_button).clicked() {
                  let credentials = self.credentials_form.credentials.clone();
                  RT.spawn_blocking(move || match credentials.is_valid() {
                     Ok(_) => {
                        SHARED_GUI.write(|gui| {
                           gui.recover_wallet_ui.credentials_input = false;
                           gui.recover_wallet_ui.show_recover_wallet = true;
                        });
                     }
                     Err(e) => {
                        SHARED_GUI.write(|gui| {
                           gui.open_msg_window("Credentials Error", e.to_string());
                        });
                     }
                  });
               }
            });
         });
   }

   fn recover_hd_wallet(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.show_recover_wallet {
         return;
      }

      Window::new("Recover_HD_Wallet_wallet_name")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size2.0, self.size2.1));
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               self.show_requirements_warning(theme, ui);

               ui.label(RichText::new("Wallet Name").size(theme.text_sizes.heading));

               TextEdit::singleline(&mut self.wallet_name)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let recover_button =
                  Button::new(RichText::new("Recover").size(theme.text_sizes.large))
                     .min_size(vec2(ui.available_width() * 0.9, 25.0));

               if ui.add_enabled(!self.recover_button_clicked, recover_button).clicked() {
                  self.recover_button_clicked = true;
                  let mut vault = ctx.get_vault();
                  let name = self.wallet_name.clone();
                  let credentials = self.credentials_form.credentials.clone();

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.new_size((300.0, 150.0));
                        gui.loading_window.open(
                           "Recovering Wallet... (Grab a coffee this will take 10-15 minutes)",
                        );
                     });

                     vault.set_credentials(credentials);

                     match vault.recover_hd_wallet(name) {
                        Ok(_) => {}
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.loading_window.reset();
                              gui.open_msg_window("Failed to recover wallet", e.to_string());
                           });
                           return;
                        }
                     };

                     let params = if cfg!(feature = "dev") {
                        Some(Argon2::very_fast())
                     } else {
                        Some(Argon2::balanced())
                     };

                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Encrypting Vault...");
                     });

                     // Encrypt the vault
                     match ctx.encrypt_and_save_vault(Some(vault.clone()), params.clone()) {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              gui.recover_wallet_ui.show_recover_wallet = false;
                              gui.recover_wallet_ui.show_tips = true;
                              gui.recover_wallet_ui.credentials_form.erase();

                              gui.loading_window.reset();
                           });

                           ctx.write(|ctx| {
                              ctx.current_wallet = vault.get_master_wallet();
                           });

                           ctx.set_vault(vault);
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Failed to create vault", e.to_string());
                              gui.loading_window.reset();
                           });
                           return;
                        }
                     };
                  });
               }
            });
         });
   }

   fn show_tips(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.show_tips {
         return;
      }

      Window::new("Recover_HD_Wallet_wallet_name")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

      ui.vertical_centered(|ui| {

         let tip1 = "You just created a new Hierarchical Deterministic (HD) wallet";
         let tip2 = "This wallet can always be recovered with the same credentials even if you lose your Vault";
         let tip3 = "A Vault has been created with the credentials you just used for faster access to your wallets and contacts";
         let tip4 = "If you want to create new wallets, it is recommended to derive them from the HD wallet you just created";
         let tip5 = "You can import wallets from a seed phrase or a private key, but those can be lost forever if you lose your Vault";

         let warning = "Make sure to never forget your credentials, it is the only way to recover your wallet";

         let text1 = RichText::new(tip1).size(theme.text_sizes.large);
         let text2 = RichText::new(tip2).size(theme.text_sizes.large);
         let text3 = RichText::new(tip3).size(theme.text_sizes.large);
         let text4 = RichText::new(tip4).size(theme.text_sizes.large);
         let text5 = RichText::new(tip5).size(theme.text_sizes.large);
         let warning_text = RichText::new(warning).size(theme.text_sizes.very_large).color(theme.colors.warning);

         let label1 = Label::new(text1, None).wrap();
         let label2 = Label::new(text2, None).wrap();
         let label3 = Label::new(text3, None).wrap();
         let label4 = Label::new(text4, None).wrap();
         let label5 = Label::new(text5, None).wrap();
         let label_warning = Label::new(warning_text, None).wrap();

         ui.add(label1);
         ui.add(label2);
         ui.add(label3);
         ui.add(label4);
         ui.add(label5);
         ui.add(label_warning);

         let ok_button = Button::new(RichText::new("Ok").size(theme.text_sizes.large))
            .min_size(vec2(ui.available_width() * 0.25, 25.0));

         if ui.add(ok_button).clicked() {
            let vault = ctx.get_vault();
            RT.spawn_blocking(move || {
            let current_wallet = vault.get_master_wallet();
            SHARED_GUI.write(|gui| {
               gui.recover_wallet_ui.show_tips = false;
               gui.portofolio.open();
               gui.header.open();
               gui.header.set_current_wallet(current_wallet);

               ctx.write(|ctx| {
                  ctx.vault_exists = true;
                  ctx.vault_unlocked = true;
               });
            });
         });
         }

      });
   });
   }
}
