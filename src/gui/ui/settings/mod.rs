use crate::assets::icons::Icons;
use crate::core::{ZeusCtx, utils::RT};
use crate::gui::{SHARED_GUI, ui::CredentialsForm};
use egui::{Align2, Button, Frame, Order, RichText, Slider, Ui, Window, vec2};
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

const T_COST_TIP: &str = "The number of iterations the Argon2 algorithm will run. Higher values are more secure but slower.";

const P_COST_TIP: &str = "You should probably leave this to 1.";

pub struct SettingsUi {
   pub open: bool,
   pub main_ui: bool,
   pub performance: PerformanceSettings,
   pub encryption: EncryptionSettings,
   pub network: NetworkSettings,
   pub contacts_ui: ContactsUi,
   pub credentials: CredentialsForm,
   pub verified_credentials: bool,
   pub size: (f32, f32),
}

impl SettingsUi {
   pub fn new(ctx: ZeusCtx) -> Self {
      Self {
         open: false,
         main_ui: true,
         performance: PerformanceSettings::new(ctx),
         encryption: EncryptionSettings::new(),
         network: NetworkSettings::new(),
         contacts_ui: ContactsUi::new(),
         credentials: CredentialsForm::new(),
         verified_credentials: false,
         size: (550.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut main_ui = self.main_ui;

      self.main_ui(theme, &mut main_ui, ui);
      self.encryption.show(ctx.clone(), theme, ui);
      self.change_credentials_ui(ctx.clone(), theme, icons.clone(), ui);
      self.network.show(
         ctx.clone(),
         theme,
         icons.clone(),
         &mut main_ui,
         ui,
      );
      self.contacts_ui.show(ctx.clone(), theme, icons, ui);
      self.performance.show(ctx, theme, ui);
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

               ui.label(RichText::new("Settings").size(theme.text_sizes.heading));

               let size = vec2(self.size.0, 50.0);
               let credentials = Button::new(
                  RichText::new("Change your Credentials").size(theme.text_sizes.large),
               )
               .corner_radius(5)
               .min_size(size);
               if ui.add(credentials).clicked() {
                  *open = false;
                  self.credentials.open = true;
               }

               let encryption_settings =
                  Button::new(RichText::new("Encryption Settings").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(encryption_settings).clicked() {
                  *open = false;
                  self.encryption.open = true;
               }

               let contacts = Button::new(RichText::new("Contacts").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(contacts).clicked() {
                  *open = false;
                  self.contacts_ui.open = true;
               }

               let network =
                  Button::new(RichText::new("Network Settings").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(network).clicked() {
                  *open = false;
                  self.network.open = true;
               }

               let performance =
                  Button::new(RichText::new("Performance Settings").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(performance).clicked() {
                  *open = false;
                  self.performance.open = true;
               }
            });
         });
   }

   fn change_credentials_ui(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let title = if self.verified_credentials {
         "New Credentials"
      } else {
         "Verify Your Credentials"
      };

      let mut open = if self.credentials.additional_frame {
         true
      } else {
         self.credentials.open
      };

      Window::new(RichText::new(title).size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            if self.credentials.additional_frame {
               tracing::info!("Running additional frame");
               self.credentials.additional_frame = false;
            }

            ui.vertical_centered(|ui| {
               ui.add_space(20.0);

               // Credentials Not Verified
               if !self.verified_credentials {
                  self.credentials.confrim_password = false;
                  self.credentials.show(theme, icons.clone(), ui);
                  ui.add_space(15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let size = vec2(ui.available_width() * 0.7, 35.0);
                  let verify = Button::new(RichText::new("Verify").size(theme.text_sizes.large)).min_size(size);

                  if ui.add(verify).clicked() {
                     let mut account = ctx.get_account();
                     account.set_credentials(self.credentials.credentials.clone());

                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.loading_window.open("Decrypting account...");
                        });

                        // Verify the credentials by just decrypting the account
                        match account.decrypt(None) {
                           Ok(_) => {
                              SHARED_GUI.write(|gui| {
                                 tracing::info!("Set verified credentials state");
                                 gui.settings.verified_credentials = true;
                                 gui.settings.credentials.erase();
                                 gui.settings.credentials.additional_frame = true;
                                 gui.loading_window.open = false;
                              });
                           }
                           Err(e) => {
                              SHARED_GUI.write(|gui| {
                                 gui.loading_window.open = false;
                                 gui.open_msg_window(
                                    "Failed to decrypt account",
                                    &format!("{}", e),
                                 );
                              });
                              return;
                           }
                        };
                     });
                  }
               }

               // Credentials Verified
               // Allow the user to change the credentials
               if self.verified_credentials {
                  self.credentials.confrim_password = true;
                  self.credentials.show(theme, icons, ui);
                  ui.add_space(15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let size = vec2(ui.available_width() * 0.7, 35.0);
                  let save = Button::new(RichText::new("Save").size(theme.text_sizes.large)).min_size(size);

                  if ui.add(save).clicked() {
                     let mut account = ctx.get_account();
                     account.set_credentials(self.credentials.credentials.clone());

                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.loading_window.open("Encrypting account...");
                        });

                        let data = match account.encrypt(None) {
                           Ok(data) => {
                              SHARED_GUI.write(|gui| {
                                 tracing::info!("Set changed credentials state");
                                 gui.settings.credentials.erase();
                                 gui.settings.verified_credentials = false;
                                 gui.settings.credentials.open = false;
                                 gui.settings.credentials.additional_frame = true;
                                 gui.settings.main_ui = true;
                                 gui.loading_window.open = false;
                                 gui.open_msg_window("Credentials have been updated", "");
                              });
                              data
                           }
                           Err(e) => {
                              SHARED_GUI.write(|gui| {
                                 gui.loading_window.open = false;
                                 gui.open_msg_window(
                                    "Failed to update credentials",
                                    &format!("{}", e),
                                 );
                              });
                              return;
                           }
                        };

                        // Save the encrypted data to the account file
                        match account.save(None, data) {
                           Ok(_) => {
                              SHARED_GUI.write(|gui| {
                                 gui.loading_window.open = false;
                              });
                           }
                           Err(e) => {
                              SHARED_GUI.write(|gui| {
                                 gui.loading_window.open = false;
                                 gui.open_msg_window("Failed to save account", &format!("{}", e));
                              });
                           }
                        }

                        ctx.set_account(account);
                     });
                  }
               }
            });
         });

      // If the window was open in the first place
      if self.credentials.open {
         if !open {
            // window closed
            tracing::info!("Close Settings credentials");
            self.credentials.erase();
            self.credentials.open = false;
            self.credentials.additional_frame = true;
            self.verified_credentials = false;
            self.main_ui = true;
         }
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
         size: (450.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let title = RichText::new("Encryption Settings").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(5.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            let slider_size = vec2(ui.available_width() * 0.4, 20.0);

            ui.vertical_centered(|ui| {
               ui.label(RichText::new("Memory cost (MB):").size(theme.text_sizes.normal))
                  .on_hover_text(M_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(
                     Slider::new(&mut self.argon_params.m_cost, 64_000..=4096_000)
                        .custom_formatter(|v, _ctx| format!("{:.0} MB", v / 1000.0)),
                  );
               });

               ui.label(RichText::new("Iterations:").size(theme.text_sizes.normal))
                  .on_hover_text(T_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.argon_params.t_cost,
                     5..=200,
                  ));
               });

               ui.label(RichText::new("Parallelism:").size(theme.text_sizes.normal))
                  .on_hover_text(P_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(&mut self.argon_params.p_cost, 1..=8));
               });

               ui.add_space(20.0);

               let size = vec2(ui.available_width() * 0.7, 35.0);
               let save =
                  Button::new(RichText::new("Save").size(theme.text_sizes.large)).min_size(size);

               if ui.add(save).clicked() {
                  self.save(ctx);
               }
            });
         });
      self.open = open;
   }

   fn save(&self, ctx: ZeusCtx) {
      let params = self.argon_params.clone();
      let account = ctx.get_account();

      RT.spawn_blocking(move || {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Encrypting account...");
         });

         // Encrypt the account with the new params
         let data = match account.encrypt(Some(params.clone())) {
            Ok(data) => data,
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.open_msg_window(
                     "Failed to update encryption settings",
                     &format!("{}", e),
                  );
                  gui.loading_window.open = false;
               });
               return;
            }
         };

         // Save the encrypted data to the account file
         match account.save(None, data) {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.open = false;
                  gui.open_msg_window("Encryption settings have been updated", "");
                  gui.settings.encryption.open = false;
                  gui.settings.encryption.argon_params = params;
                  gui.loading_window.open = false;
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.open = false;
                  gui.open_msg_window("Failed to save account", &format!("{}", e));
               });
            }
         };
      });
   }
}

pub struct PerformanceSettings {
   open: bool,
   sync_v4_pools_on_startup: bool,
   concurrency_for_syncing_balances: usize,
   concurrency_for_syncing_pools: usize,
   batch_size_for_syncing_balances: usize,
   batch_size_for_updating_pools_state: usize,
   batch_size_for_syncing_pools: usize,
   pub size: (f32, f32),
}

impl PerformanceSettings {
   pub fn new(ctx: ZeusCtx) -> Self {
      let pool_manager = ctx.pool_manager();
      let balance_manager = ctx.balance_manager();
      Self {
         open: false,
         sync_v4_pools_on_startup: pool_manager.do_we_sync_v4_pools(),
         concurrency_for_syncing_balances: balance_manager.concurrency(),
         concurrency_for_syncing_pools: pool_manager.concurrency(),
         batch_size_for_syncing_balances: balance_manager.batch_size(),
         batch_size_for_updating_pools_state: pool_manager.batch_size_for_updating_pools_state(),
         batch_size_for_syncing_pools: pool_manager.batch_size_for_syncing_pools(),
         size: (400.0, 550.0),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new("Performance Settings").size(theme.text_sizes.heading);
      Window::new(title)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(5.0, 20.0);
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            ui.vertical_centered(|ui| {
               let slider_size = vec2(ui.available_width() * 0.4, 20.0);

               let header = RichText::new("Pool Manager").size(theme.text_sizes.very_large);
               ui.label(header);

               let text = RichText::new("Sync V4 Pools on startup").size(theme.text_sizes.normal);
               ui.checkbox(&mut self.sync_v4_pools_on_startup, text);

               ui.label(
                  RichText::new("Concurrency for Syncing Pools").size(theme.text_sizes.normal),
               );
               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.concurrency_for_syncing_pools,
                     1..=10,
                  ));
               });

               ui.label(
                  RichText::new("Batch Size for Syncing Pools").size(theme.text_sizes.normal),
               );
               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.batch_size_for_syncing_pools,
                     1..=60,
                  ));
               });

               ui.label(
                  RichText::new("Batch Size when updating pools state")
                     .size(theme.text_sizes.normal),
               );
               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.batch_size_for_updating_pools_state,
                     1..=50,
                  ));
               });

               let header = RichText::new("Balance Manager").size(theme.text_sizes.very_large);
               ui.label(header);

               ui.label(
                  RichText::new("Concurrency for syncing balances").size(theme.text_sizes.normal),
               );
               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.concurrency_for_syncing_balances,
                     1..=10,
                  ));
               });

               ui.label(
                  RichText::new("Batch Size for syncing balances").size(theme.text_sizes.normal),
               );
               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.batch_size_for_syncing_balances,
                     1..=50,
                  ));
               });

               let btn_size = vec2(ui.available_width() * 0.7, 45.0);
               let button = Button::new(RichText::new("Save").size(theme.text_sizes.normal))
                  .min_size(btn_size);

               if ui.add(button).clicked() {
                  self.open = false;
                  self.save_settings(ctx);
               }
            });
         });
   }

   fn save_settings(&self, ctx: ZeusCtx) {
      let save_balance_manager =
         if self.concurrency_for_syncing_balances != ctx.balance_manager().concurrency() {
            ctx.balance_manager()
               .set_concurrency(self.concurrency_for_syncing_balances);
            true
         } else if self.batch_size_for_syncing_balances != ctx.balance_manager().batch_size() {
            ctx.balance_manager()
               .set_batch_size(self.batch_size_for_syncing_balances);
            true
         } else {
            false
         };

      let save_pool_manager =
         if self.concurrency_for_syncing_pools != ctx.pool_manager().concurrency() {
            ctx.pool_manager()
               .set_concurrency(self.concurrency_for_syncing_pools);
            true
         } else if self.batch_size_for_updating_pools_state
            != ctx.pool_manager().batch_size_for_updating_pools_state()
         {
            ctx.pool_manager()
               .set_batch_size_for_updating_pools_state(self.batch_size_for_updating_pools_state);
            true
         } else if self.batch_size_for_syncing_pools
            != ctx.pool_manager().batch_size_for_syncing_pools()
         {
            ctx.pool_manager()
               .set_batch_size_for_syncing_pools(self.batch_size_for_syncing_pools);
            true
         } else if self.sync_v4_pools_on_startup != ctx.pool_manager().do_we_sync_v4_pools() {
            ctx.pool_manager()
               .set_sync_v4_pools(self.sync_v4_pools_on_startup);
            true
         } else {
            false
         };

      RT.spawn_blocking(move || {
         if save_balance_manager {
            ctx.save_balance_manager();
         }

         if save_pool_manager {
            let _ = ctx.save_pool_manager();
         }
      });
   }
}
