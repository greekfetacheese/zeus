use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, theme_kind_dir},
};
use crate::gui::{SHARED_GUI, ui::CredentialsForm};
use egui::{
   Align2, Button, Frame, Order, RichText, ScrollArea, Sense, Slider, Ui,
   Window, vec2,
};
use egui_theme::{Theme, ThemeKind};
use egui_widgets::{ComboBox, Label};
use ncrypt_me::Argon2;
use std::collections::HashSet;
use std::sync::Arc;
use zeus_eth::types::ChainId;

pub mod contacts;
pub mod networks;

pub use contacts::ContactsUi;
pub use networks::NetworkSettings;

const MAX_M_COST: u32 = 8096_000;
const MAX_T_COST: u32 = 2048;
const MAX_P_COST: u32 = 512;

const M_COST_TIP: &str =
    "How much memory the Argon2 algorithm uses. Higher values are more secure but way slower, make sure the memory cost does not exceed your computer RAM.
    This is the most improtant parameter against GPU/ASIC brute-forcing attacks. 
    You probably want to just increase the Memory cost to a sensible value 512 - 1024mb or even more if your RAM can afford it";

const T_COST_TIP: &str = "The number of iterations the Argon2 algorithm will run over the memory. Higher values are more secure but slower.";

const P_COST_TIP: &str = "How many parallel lanes (threads) the Argon2 algorithm will use.
 Normally this number should not be greater than the number of cores on your CPU,
 but you can safely increase it to as much as you want since even if the P_COST exceeds the cores of the CPU
  there is some performance penalty but in a case of a GPU/ASIC attack it will be even harder for the attacker to compute the hash";

pub struct ThemeSettings {
   open: bool,
   size: (f32, f32),
}

impl ThemeSettings {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (400.0, 120.0),
      }
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new("Theme Settings").size(theme.text_sizes.large);
      Window::new(title)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 20.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let selected_text = RichText::new(theme.kind.to_str()).size(theme.text_sizes.normal);
               let label = Label::new(selected_text, None);
               ComboBox::new("theme_settings_combobox", label).width(200.0).show_ui(ui, |ui| {
                  for kind in ThemeKind::to_vec() {
                     let text = RichText::new(kind.to_str()).size(theme.text_sizes.normal);
                     let label = Label::new(text, None).sense(Sense::click());

                     if ui.add(label).clicked() {
                        let new_theme = Theme::new(kind);
                        ui.ctx().set_style(new_theme.style.clone());
                        RT.spawn_blocking(move || {
                           SHARED_GUI.write(|gui| {
                              gui.theme = new_theme;
                           });
                        });
                     }
                  }
               });

               let text = RichText::new("Save").size(theme.text_sizes.normal);
               let button = Button::new(text).min_size(vec2(ui.available_width() * 0.7, 35.0));
               if ui.add(button).clicked() {
                  self.open = false;

                  RT.spawn_blocking(move || {
                     let dir = match theme_kind_dir() {
                        Ok(dir) => dir,
                        Err(e) => {
                           tracing::error!("Error saving theme: {:?}", e);
                           SHARED_GUI.write(|gui| {
                              gui.msg_window.open("Failed to save theme", e.to_string());
                           });
                           return;
                        }
                     };

                     let theme = SHARED_GUI.read(|gui| gui.theme.clone());
                     let theme_kind_str = serde_json::to_string(&theme.kind).unwrap();
                     match std::fs::write(dir, theme_kind_str) {
                        Ok(_) => {
                           tracing::info!("Saved theme");
                        }
                        Err(e) => {
                           tracing::error!("Error saving theme: {:?}", e);
                           SHARED_GUI.write(|gui| {
                              gui.msg_window.open("Failed to save theme", e.to_string());
                           });
                           return;
                        }
                     }
                  });
               }
            });
         });
   }
}

pub struct SettingsUi {
   pub open: bool,
   pub performance: PerformanceSettings,
   pub encryption: EncryptionSettings,
   pub network: NetworkSettings,
   pub theme: ThemeSettings,
   pub contacts_ui: ContactsUi,
   pub credentials: CredentialsForm,
   pub verified_credentials: bool,
   pub size: (f32, f32),
}

impl SettingsUi {
   pub fn new(ctx: ZeusCtx) -> Self {
      Self {
         open: false,
         performance: PerformanceSettings::new(ctx),
         encryption: EncryptionSettings::new(),
         network: NetworkSettings::new(),
         theme: ThemeSettings::new(),
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

      self.main_ui(theme, ui);
      self.encryption.show(ctx.clone(), theme, ui);
      self.change_credentials_ui(ctx.clone(), theme, icons.clone(), ui);
      self.network.show(ctx.clone(), theme, icons.clone(), ui);
      self.contacts_ui.show(ctx.clone(), theme, icons, ui);
      self.performance.show(ctx, theme, ui);
      self.theme.show(theme, ui);
   }

   pub fn main_ui(&mut self, theme: &Theme, ui: &mut Ui) {
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

               ui.label(RichText::new("Settings").size(theme.text_sizes.heading));

               let size = vec2(self.size.0, 50.0);
               let credentials = Button::new(
                  RichText::new("Change your Credentials").size(theme.text_sizes.large),
               )
               .corner_radius(5)
               .min_size(size);
               if ui.add(credentials).clicked() {
                  self.credentials.open = true;
               }

               let encryption_settings =
                  Button::new(RichText::new("Encryption Settings").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(encryption_settings).clicked() {
                  self.encryption.open = true;
               }

               let contacts = Button::new(RichText::new("Contacts").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(contacts).clicked() {
                  self.contacts_ui.open = true;
               }

               let network =
                  Button::new(RichText::new("Network Settings").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(network).clicked() {
                  self.network.open = true;
               }

               let performance =
                  Button::new(RichText::new("Performance Settings").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(performance).clicked() {
                  self.performance.open = true;
               }

               let theme =
                  Button::new(RichText::new("Theme Settings").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(theme).clicked() {
                  self.theme.open = true;
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
                  let verify = Button::new(RichText::new("Verify").size(theme.text_sizes.large))
                     .min_size(size);

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
                  let save =
                     Button::new(RichText::new("Save").size(theme.text_sizes.large)).min_size(size);

                  if ui.add(save).clicked() {
                     let new_credentials = self.credentials.credentials.clone();
                     let mut new_account = ctx.get_account();
                     new_account.set_credentials(new_credentials.clone());

                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.loading_window.open("Encrypting account...");
                        });

                        match ctx.encrypt_and_save_account(Some(new_account.clone()), None) {
                           Ok(_) => {
                              SHARED_GUI.write(|gui| {
                                 gui.settings.credentials.erase();
                                 gui.loading_window.open = false;
                                 gui.settings.verified_credentials = false;
                                 gui.settings.credentials.open = false;
                                 gui.settings.credentials.additional_frame = true;
                                 gui.open_msg_window("Credentials have been updated", "");
                              });
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

                        ctx.set_account(new_account);
                     });
                  }
               }
            });
         });

      // If the window was open in the first place
      if self.credentials.open {
         if !open {
            self.credentials.erase();
            self.credentials.open = false;
            self.credentials.additional_frame = true;
            self.verified_credentials = false;
         }
      }
   }
}

pub struct EncryptionSettings {
   pub open: bool,
   pub argon_params: Argon2,
   pub size: (f32, f32),
}

impl EncryptionSettings {
   pub fn new() -> Self {
      Self {
         open: false,
         argon_params: Argon2::balanced(),
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
                     Slider::new(&mut self.argon_params.m_cost, 64_000..=MAX_M_COST)
                        .custom_formatter(|v, _ctx| format!("{:.0}", v / 1000.0)),
                  );
               });

               ui.label(RichText::new("Iterations:").size(theme.text_sizes.normal))
                  .on_hover_text(T_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.argon_params.t_cost,
                     5..=MAX_T_COST,
                  ));
               });

               ui.label(RichText::new("Parallelism:").size(theme.text_sizes.normal))
                  .on_hover_text(P_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.argon_params.p_cost,
                     1..=MAX_P_COST,
                  ));
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
      let new_params = self.argon_params.clone();

      RT.spawn_blocking(move || {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Encrypting account...");
         });

         // Encrypt the account with the new params
         // let time = std::time::Instant::now();
         match ctx.encrypt_and_save_account(None, Some(new_params.clone())) {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.open = false;
                  gui.open_msg_window("Encryption settings have been updated", "");
                  gui.settings.encryption.open = false;
                  gui.settings.encryption.argon_params = new_params;
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.open = false;
                  gui.open_msg_window(
                     "Failed to update encryption settings",
                     &format!("{}", e),
                  );
               });
               return;
            }
         };
         // tracing::info!("Encryption took {} secs", time.elapsed().as_secs_f32());
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
   ignore_chains: HashSet<u64>,
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
         ignore_chains: pool_manager.ignore_chains(),
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
      let mut open = self.open;

      let title = RichText::new("Performance Settings").size(theme.text_sizes.heading);
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
            ui.spacing_mut().item_spacing = vec2(5.0, 20.0);
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            ui.vertical_centered(|ui| {
               ScrollArea::vertical().show(ui, |ui| {
                  let slider_size = vec2(ui.available_width() * 0.4, 20.0);

                  let header = RichText::new("Pool Manager").size(theme.text_sizes.very_large);
                  ui.label(header);

                  let text =
                     RichText::new("Sync V4 Pools on startup").size(theme.text_sizes.normal);
                  ui.checkbox(&mut self.sync_v4_pools_on_startup, text);

                  let text = RichText::new("Chains to ignore at V4 historic sync")
                     .size(theme.text_sizes.normal);
                  ui.label(text);
                  for chain in ChainId::supported_chains() {
                     let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
                     let mut ignore = self.ignore_chains.contains(&chain.id());
                     ui.checkbox(&mut ignore, text);
                     if ignore {
                        self.ignore_chains.insert(chain.id());
                     } else {
                        self.ignore_chains.remove(&chain.id());
                     }
                  }

                  ui.label(
                     RichText::new("Concurrency for Syncing & Updating Pools")
                        .size(theme.text_sizes.normal),
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
                     RichText::new("Concurrency for syncing balances")
                        .size(theme.text_sizes.normal),
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
               });
            });
         });
      let closed = self.open && !open;
      self.open = open;

      if closed {
         self.save_settings(ctx);
      }
   }

   fn save_settings(&self, ctx: ZeusCtx) {
      let save_balance_manager =
         if self.concurrency_for_syncing_balances != ctx.balance_manager().concurrency() {
            ctx.balance_manager().set_concurrency(self.concurrency_for_syncing_balances);
            true
         } else if self.batch_size_for_syncing_balances != ctx.balance_manager().batch_size() {
            ctx.balance_manager().set_batch_size(self.batch_size_for_syncing_balances);
            true
         } else {
            false
         };

      let save_pool_manager =
         if self.concurrency_for_syncing_pools != ctx.pool_manager().concurrency() {
            ctx.pool_manager().set_concurrency(self.concurrency_for_syncing_pools);
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
            ctx.pool_manager().set_sync_v4_pools(self.sync_v4_pools_on_startup);
            true
         } else if self.ignore_chains != ctx.pool_manager().ignore_chains() {
            ctx.pool_manager().set_ignore_chains(self.ignore_chains.clone());
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
