use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::{
   SHARED_GUI,
   ui::{CredentialsForm, button, rich_text},
   utils,
};
use egui::{Align2, Frame, Grid, Slider, Ui, Window, vec2};
use egui_theme::{Theme, utils::*};
use ncrypt_me::Argon2Params;
use std::sync::Arc;

pub mod contacts;
pub mod networks;

pub use contacts::ContactsUi;
pub use networks::NetworkSettings;

const M_COST_TIP: &str =
    "How much memory the Argon2 algorithm uses. Higher values are more secure but way slower, make sure the memory cost does not exceed your computer RAM.
    You probably want to just increase the Memory cost to a sensible value 256mb - 1024mb as this is the most important parameter for security.";

const T_COST_TIP: &str =
   "The number of iterations the Argon2 algorithm will run. Higher values are more secure but slower.";

const P_COST_TIP: &str = "You should probably leave this to 1.";

pub struct SettingsUi {
   pub open: bool,
   pub main_ui: bool,
   pub encryption: EncryptionSettings,
   pub network: NetworkSettings,
   pub contacts_ui: ContactsUi,
   pub credentials: CredentialsForm,
   pub verified_credentials: bool,
   pub size: (f32, f32),
}

impl SettingsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         main_ui: true,
         encryption: EncryptionSettings::new(),
         network: NetworkSettings::new(),
         contacts_ui: ContactsUi::new(),
         credentials: CredentialsForm::new(),
         verified_credentials: false,
         size: (500.0, 400.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut main_ui = self.main_ui;
      self.main_ui(theme, &mut main_ui, ui);
      self.encryption.show(ctx.clone(), theme, ui);
      self.change_credentials_ui(ctx.clone(), theme, ui);
      self
         .network
         .show(ctx.clone(), theme, icons.clone(), &mut main_ui, ui);
      self.contacts_ui.show(ctx, theme, icons, &mut main_ui, ui);
      self.main_ui = main_ui;
   }

   pub fn main_ui(&mut self, theme: &Theme, open: &mut bool, ui: &mut Ui) {
      if !*open {
         return;
      }

      // Transparent window
      Window::new("settings_main_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::new())
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               let visuals = theme.get_button_visuals(theme.colors.bg_color);
               widget_visuals(ui, visuals);

               ui.label(rich_text("Settings").size(theme.text_sizes.heading));

               let size = vec2(self.size.0, 50.0);
               let credentials = button(rich_text("Change your Credentials").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(credentials).clicked() {
                  *open = false;
                  self.credentials.open = true;
               }

               let encryption_settings = button(rich_text("Encryption Settings").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(encryption_settings).clicked() {
                  *open = false;
                  self.encryption.open = true;
               }

               let contacts = button(rich_text("Contacts").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(contacts).clicked() {
                  *open = false;
                  self.contacts_ui.open = true;
               }

               let network = button(rich_text("Network Settings").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(network).clicked() {
                  *open = false;
                  self.network.open = true;
               }
            });
         });
   }

   fn change_credentials_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let title = if self.verified_credentials {
         "New Credentials"
      } else {
         "Verify Your Credentials"
      };

      let mut open = self.credentials.open;
      Window::new(rich_text(title).size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.add_space(20.0);

               if !self.verified_credentials {
                  self.credentials.confrim_password = false;
                  self.credentials.show(theme, ui);
                  ui.add_space(15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let verify = button(rich_text("Verify").size(theme.text_sizes.normal));
                  if ui.add(verify).clicked() {
                     let mut account = ctx.account();
                     account.credentials = self.credentials.credentials.clone();

                     std::thread::spawn(move || {
                        let dir = utils::get_account_dir();
                        utils::open_loading("Decrypting profile...".to_string());

                        match account.decrypt_zero(&dir) {
                           Ok(data) => {
                              let mut gui = SHARED_GUI.write().unwrap();
                              gui.settings.verified_credentials = true;
                              gui.settings.credentials.erase();
                              gui.loading_window.open = false;
                              data
                           }
                           Err(e) => {
                              let mut gui = SHARED_GUI.write().unwrap();
                              gui.loading_window.open = false;
                              gui.open_msg_window("Failed to decrypt profile", &format!("{}", e));
                              return;
                           }
                        };
                     });
                  }
               }

               if self.verified_credentials {
                  self.credentials.confrim_password = true;
                  self.credentials.show(theme, ui);
                  ui.add_space(15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let save = button(rich_text("Save").size(theme.text_sizes.normal));

                  if ui.add(save).clicked() {
                     let mut account = ctx.account();
                     account.credentials = self.credentials.credentials.clone();

                     std::thread::spawn(move || {
                        let dir = utils::get_account_dir();
                        let params = utils::get_encrypted_info(&dir).argon2_params;
                        utils::open_loading("Encrypting profile...".to_string());

                        match account.encrypt_and_save(&dir, params) {
                           Ok(_) => {
                              let mut gui = SHARED_GUI.write().unwrap();
                              gui.settings.credentials.erase();
                              gui.settings.verified_credentials = false;
                              gui.settings.credentials.open = false;
                              gui.settings.main_ui = true;
                              gui.loading_window.open = false;
                              gui.open_msg_window("Credentials have been updated", "");
                           }
                           Err(e) => {
                              let mut gui = SHARED_GUI.write().unwrap();
                              gui.loading_window.open = false;
                              gui.open_msg_window("Failed to update credentials", &format!("{}", e));
                              return;
                           }
                        }
                        ctx.write(|ctx| {
                           ctx.account = account;
                        });
                     });
                  }
               }
            });
         });
      self.credentials.open = open;

      // reset credentials
      if !self.credentials.open {
         self.credentials.erase();
         self.verified_credentials = false;
         self.main_ui = true;
      }
   }
}

pub struct EncryptionSettings {
   pub open: bool,
   pub argon_params: Argon2Params,
   pub size: (f32, f32),
}

impl EncryptionSettings {
   pub fn new() -> Self {
      Self {
         open: false,
         argon_params: Argon2Params::balanced(),
         size: (500.0, 400.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      Window::new("Encryption Settings")
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            ui.add_space(20.0);

            let content_width = ui.available_width() * 0.3;

            ui.horizontal(|ui| {
               ui.add_space((ui.available_width() - content_width) / 2.0);

               Grid::new("encryption_settings")
                  .spacing([0.0, 15.0])
                  .show(ui, |ui| {
                     ui.set_width(content_width);

                     ui.label(rich_text("Memory cost (MB):").size(theme.text_sizes.normal))
                        .on_hover_text(M_COST_TIP);
                     ui.end_row();

                     ui.add(
                        Slider::new(&mut self.argon_params.m_cost, 64_000..=4096_000)
                           .custom_formatter(|v, _ctx| format!("{:.0} MB", v / 1000.0)),
                     );
                     ui.end_row();

                     ui.label(rich_text("Iterations:").size(theme.text_sizes.normal))
                        .on_hover_text(T_COST_TIP);
                     ui.end_row();

                     ui.add(Slider::new(&mut self.argon_params.t_cost, 5..=200));
                     ui.end_row();

                     ui.label(rich_text("Parallelism:").size(theme.text_sizes.normal))
                        .on_hover_text(P_COST_TIP);
                     ui.end_row();

                     ui.add(Slider::new(&mut self.argon_params.p_cost, 1..=8));
                     ui.end_row();

                     let save = button(rich_text("Save").size(theme.text_sizes.normal));
                     if ui.add(save).clicked() {
                        let params = self.argon_params.clone();
                        let account = ctx.account();

                        std::thread::spawn(move || {
                           let dir = utils::get_account_dir();
                           utils::open_loading("Encrypting profile...".to_string());

                           match account.encrypt_and_save(&dir, params.clone()) {
                              Ok(_) => {
                                 let mut gui = SHARED_GUI.write().unwrap();
                                 gui.open_msg_window("Encryption settings have been updated", "");
                                 gui.settings.encryption.open = false;
                                 gui.settings.encryption.argon_params = params;
                                 gui.loading_window.open = false;
                              }
                              Err(e) => {
                                 let mut gui = SHARED_GUI.write().unwrap();
                                 gui.open_msg_window("Failed to update encryption settings", &format!("{}", e));
                                 gui.loading_window.open = false;
                                 return;
                              }
                           }
                        });
                     }
                     ui.end_row();
                  });
            });
         });
      self.open = open;
   }
}
