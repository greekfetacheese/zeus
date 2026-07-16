//! UI that allows the user to change the encryption settings.
//!
//! It only affects the vault, it has no effect on the master wallet recovery.

use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{Align2, Order, RichText, Slider, Ui, Window, vec2};
use ncrypt_me::Argon2;
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::Button;

const MIN_M_COST: u32 = 1024_000;
const MIN_T_COST: u32 = 8;
const MIN_P_COST: u32 = 1;

const MAX_M_COST: u32 = 8192_000;
const MAX_T_COST: u32 = 2048;
const MAX_P_COST: u32 = 1;

const DEV_M_MIN_COST: u32 = 8_000;
const DEV_T_MIN_COST: u32 = 1;
const DEV_P_MAX_COST: u32 = 4;

const M_COST_TIP: &str =
    "How much memory the Argon2 algorithm uses. Higher values are more secure but way slower, make sure the memory cost does not exceed your computer RAM.
    This is the most improtant parameter against GPU/ASIC brute-forcing attacks.
    You probably want to just increase the Memory cost to a sensible value 512 - 1024mb or even more if your RAM can afford it";

const T_COST_TIP: &str = "The number of iterations the Argon2 algorithm will run over the memory. Higher values are more secure but slower.";

const P_COST_TIP: &str = "How many parallel lanes (threads) the Argon2 algorithm will use.
You should keep this number as low as possible, best value for maximum security is 1";

pub struct EncryptionSettings {
   open: bool,
   overlay: OverlayManager,
   argon_params: Argon2,
   pub size: (f32, f32),
}

impl EncryptionSettings {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         argon_params: Argon2::balanced(),
         size: (450.0, 350.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn set_argon2(&mut self, argon_params: Argon2) {
      self.argon_params = argon_params;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let title = RichText::new("Encryption Settings").size(theme.text_sizes.heading);
      let window_frame = theme.frame1;

      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(5.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            let slider_size = vec2(ui.available_width() * 0.4, 20.0);
            let button_visuals = theme.button_visuals();

            let min_m_cost = if cfg!(feature = "dev") {
               DEV_M_MIN_COST
            } else {
               MIN_M_COST
            };

            let min_t_cost = if cfg!(feature = "dev") {
               DEV_T_MIN_COST
            } else {
               MIN_T_COST
            };

            let max_p_cost = if cfg!(feature = "dev") {
               DEV_P_MAX_COST
            } else {
               MAX_P_COST
            };

            ui.vertical_centered(|ui| {
               ui.label(RichText::new("Memory cost (MB):").size(theme.text_sizes.normal))
                  .on_hover_text(M_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(
                     Slider::new(
                        &mut self.argon_params.m_cost,
                        min_m_cost..=MAX_M_COST,
                     )
                     .custom_formatter(|v, _ctx| format!("{:.0}", v / 1000.0)),
                  );
               });

               ui.label(RichText::new("Iterations:").size(theme.text_sizes.normal))
                  .on_hover_text(T_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.argon_params.t_cost,
                     min_t_cost..=MAX_T_COST,
                  ));
               });

               ui.label(RichText::new("Parallelism:").size(theme.text_sizes.normal))
                  .on_hover_text(P_COST_TIP);

               ui.allocate_ui(slider_size, |ui| {
                  ui.add(Slider::new(
                     &mut self.argon_params.p_cost,
                     MIN_P_COST..=max_p_cost,
                  ));
               });

               ui.add_space(20.0);

               let size = vec2(ui.available_width() * 0.7, 35.0);
               let text = RichText::new("Save").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  self.save();
               }
            });
         });

      if !open {
         self.close();
      }
   }

   fn save(&self) {
      let new_params = self.argon_params.clone();

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Encrypting vault...");
            gui.request_repaint();
            gui.ctx.clone()
         });

         // Encrypt the vault with the new params
         match ctx.encrypt_and_save_vault(None, Some(new_params.clone())) {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.reset();
                  gui.open_msg_window("Encryption settings have been updated", "");
                  gui.settings.encryption.close();
                  gui.settings.encryption.argon_params = new_params;
                  gui.request_repaint();
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.reset();
                  gui.open_msg_window(
                     "Failed to update encryption settings",
                     format!("{}", e),
                  );
                  gui.request_repaint();
               });
            }
         };
      });
   }
}
