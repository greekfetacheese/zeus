use crate::core::{M_COST, Vault, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{Align2, FontId, Margin, RichText, Ui, Window, vec2};
use ncrypt_me::{Argon2, Credentials};
use std::time::Instant;
use zeus_theme::Theme;
use zeus_ui_components::CredentialsForm;
use zeus_widgets::{Button, Label, SecureTextEdit};

#[cfg(feature = "dev")]
use secure_types::SecureString;

pub struct UnlockVault {
   pub credentials_form: CredentialsForm,
   pub size: (f32, f32),
}

impl UnlockVault {
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form = CredentialsForm::new()
         .with_min_size(form_size)
         .with_open(true)
         .with_enabled_virtual_keyboard();
      Self {
         credentials_form,
         size: (550.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let vault_exists = ctx.vault_exists();
      let vault_unlocked = ctx.vault_unlocked();

      let open = vault_exists && !vault_unlocked;

      if !open {
         return;
      }

      let frame = theme.frame1;

      Window::new("Unlock_Vault_Ui")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(frame)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.add_space(10.0);
               ui.spacing_mut().item_spacing.y = 25.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let ui_width = ui.available_width();

               ui.label(RichText::new("Unlock your Vault").size(theme.text_sizes.heading));

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                  self.credentials_form.show(theme, ui);
               });

               let text = RichText::new("Unlock").size(theme.text_sizes.large);
               let button =
                  Button::new(text).visuals(button_visuals).min_size(vec2(ui_width * 0.50, 35.0));

               if ui.add(button).clicked() {
                  let username = self.credentials_form.username();
                  let password = self.credentials_form.password();
                  let confirm_password = self.credentials_form.confirm_password();

                  let credentials = Credentials::new(username, password, confirm_password);
                  let mut vault = ctx.get_vault();
                  vault.set_credentials(credentials);
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
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form = CredentialsForm::new()
         .with_min_size(form_size)
         .with_confirm_password(true)
         .with_open(true)
         .with_enabled_virtual_keyboard();

      Self {
         credentials_form,
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if ctx.vault_exists() {
         return;
      }

      self.credentials_input(theme, ui);
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

   fn credentials_input(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.credentials_input {
         return;
      }

      let frame = theme.frame1;

      Window::new("Recover_HD_Wallet_credentials_input")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(frame)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               self.show_requirements_warning(theme, ui);

               let ui_width = ui.available_width();

               ui.label(RichText::new("No vault was found").size(theme.text_sizes.heading));
               ui.label(
                  RichText::new("Recover your wallet from your credentials")
                     .size(theme.text_sizes.large),
               );

               // Credentials input
               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                  self.credentials_form.show(theme, ui);
               });

               let text = RichText::new("Next").size(theme.text_sizes.large);
               let next_button =
                  Button::new(text).visuals(button_visuals).min_size(vec2(ui_width * 0.25, 25.0));

               if ui.add(next_button).clicked() {
                  let username = self.credentials_form.username();
                  let password = self.credentials_form.password();
                  let confirm_password = self.credentials_form.confirm_password();
                  let credentials = Credentials::new(username, password, confirm_password);

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

      let frame = theme.frame1;

      Window::new("Recover_HD_Wallet_wallet_name")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(frame)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size2.0, self.size2.1));
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               self.show_requirements_warning(theme, ui);

               ui.label(RichText::new("Wallet Name").size(theme.text_sizes.heading));

               SecureTextEdit::singleline(&mut self.wallet_name)
                  .visuals(text_edit_visuals)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let text = RichText::new("Recover").size(theme.text_sizes.large);
               let recover_button = Button::new(text)
                  .visuals(button_visuals)
                  .min_size(vec2(ui.available_width() * 0.9, 25.0));

               if ui.add_enabled(!self.recover_button_clicked, recover_button).clicked() {
                  self.recover_button_clicked = true;
                  let mut vault = ctx.get_vault();
                  let name = self.wallet_name.clone();

                  let username = self.credentials_form.username();
                  let password = self.credentials_form.password();
                  let confirm_password = self.credentials_form.confirm_password();
                  let credentials = Credentials::new(username, password, confirm_password);

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.new_size((300.0, 150.0));
                        gui.loading_window.open(
                           "Recovering Wallet... (Grab a coffee this will take 10-15 minutes)",
                        );
                     });

                     vault.set_credentials(credentials);

                     match vault.recover_hd_wallet(name) {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              gui.loading_window.reset();
                           });
                        }
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

      let frame = theme.frame1;

      Window::new("Recover_HD_Wallet_wallet_name")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(frame)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);


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

         let label1 = Label::new(text1, None).wrap().interactive(false);
         let label2 = Label::new(text2, None).wrap().interactive(false);
         let label3 = Label::new(text3, None).wrap().interactive(false);
         let label4 = Label::new(text4, None).wrap().interactive(false);
         let label5 = Label::new(text5, None).wrap().interactive(false);
         let label_warning = Label::new(warning_text, None).wrap().interactive(false);

         ui.horizontal(|ui| {
         ui.add(label1);
         });

         ui.horizontal(|ui| {
         ui.add(label2);
         });

         ui.horizontal(|ui| {
         ui.add(label3);
         });

         ui.horizontal(|ui| {
         ui.add(label4);
         });

         ui.horizontal(|ui| {
         ui.add(label5);
         });

         ui.horizontal(|ui| {
         ui.add(label_warning);
         });

         let button_visuals = theme.button_visuals();
         let text = RichText::new("Ok").size(theme.text_sizes.large);
         let ok_button = Button::new(text).visuals(button_visuals).min_size(vec2(ui.available_width() * 0.25, 25.0));

         ui.vertical_centered(|ui| {

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
