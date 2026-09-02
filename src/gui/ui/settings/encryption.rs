//! UI that allows the user to change the encryption settings.
//!
//! It only affects the vault, it has no effect on the master wallet recovery.

use crate::utils::RT;
use crate::{core::ZeusContext, gui::SHARED_GUI};
use egui::{RichText, Ui, vec2};
use egui_elements::{Button, Theme};
use elegance::{Badge, BadgeTone, Slider};
use ncrypt_me::Argon2;

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
   argon_params: Argon2,
}

impl EncryptionSettings {
   pub fn new() -> Self {
      Self {
         argon_params: Argon2::balanced(),
      }
   }

   pub fn sync_from_ctx(&mut self, ctx: &mut ZeusContext) {
      self.argon_params = ctx.argon_params.clone();
   }

   pub fn set_argon2(&mut self, argon_params: Argon2) {
      self.argon_params = argon_params;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing = vec2(5.0, 12.0);
         ui.spacing_mut().button_padding = vec2(10.0, 4.0);

         ui.label(RichText::new("Vault encryption").size(theme.typography.very_large));
         ui.label(
            RichText::new(
               "This only affects vault encryption. It does not change master wallet recovery.",
            )
            .size(theme.typography.normal)
            .color(theme.colors.text_muted),
         );

         let slider_size = vec2((ui.available_width() * 0.6).min(420.0), 20.0);
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

         let mem_fmt = |mb: f64| format!("{:.0}", mb / 1000.0);

         let q_mark = RichText::new("?").size(theme.typography.normal);

         ui.allocate_ui(slider_size, |ui| {
            ui.horizontal(|ui| {
               let info_tip = Badge::new(q_mark.clone(), BadgeTone::Info);
               ui.label(RichText::new("Memory cost (MB):").size(theme.typography.normal));
               ui.add(info_tip).on_hover_text(M_COST_TIP);
            });
         });

         ui.allocate_ui(slider_size, |ui| {
            ui.add(
               Slider::new(
                  &mut self.argon_params.m_cost,
                  min_m_cost..=MAX_M_COST,
               )
               .value_fmt(mem_fmt),
            );
         });

         ui.allocate_ui(slider_size, |ui| {
            ui.horizontal(|ui| {
               let info_tip = Badge::new(q_mark.clone(), BadgeTone::Info);
               ui.label(RichText::new("Iterations:").size(theme.typography.normal));
               ui.add(info_tip).on_hover_text(T_COST_TIP);
            });
         });

         ui.allocate_ui(slider_size, |ui| {
            ui.add(Slider::new(
               &mut self.argon_params.t_cost,
               min_t_cost..=MAX_T_COST,
            ));
         });

         ui.allocate_ui(slider_size, |ui| {
            ui.horizontal(|ui| {
               let info_tip = Badge::new(q_mark, BadgeTone::Info);
               ui.label(RichText::new("Parallelism:").size(theme.typography.normal));
               ui.add(info_tip).on_hover_text(P_COST_TIP);
            });
         });

         ui.allocate_ui(slider_size, |ui| {
            ui.add(Slider::new(
               &mut self.argon_params.p_cost,
               MIN_P_COST..=max_p_cost,
            ));
         });

         ui.add_space(12.0);

         let size = vec2(200.0, 35.0);
         let text = RichText::new("Save").size(theme.typography.large);
         let button = Button::new(text).visuals(button_visuals).min_size(size);

         if ui.add(button).clicked() {
            self.save();
         }
      });
   }

   fn save(&self) {
      let new_params = self.argon_params.clone();

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Encrypting vault...");
            gui.request_repaint();
            gui.ctx.clone()
         });

         match ctx.encrypt_and_save_vault(None, Some(new_params.clone())) {
            Ok(_) => {
               ctx.write(|ctx| {
                  ctx.argon_params = new_params.clone();
               });
               SHARED_GUI.write(|gui| {
                  gui.loading_window.reset();
                  gui.open_msg_window("Encryption settings have been updated");
                  gui.settings.encryption.argon_params = new_params;
                  gui.request_repaint();
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.reset();
                  gui.open_msg_window(format!("Error: {}", e));
                  gui.request_repaint();
               });
            }
         };
      });
   }
}
