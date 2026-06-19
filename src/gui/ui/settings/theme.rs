//! UI that allows the user to change the theme settings.

use crate::core::context::theme_kind_dir;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{Align2, Order, RichText, Sense, Ui, Window, vec2};
use zeus_theme::{OverlayManager, Theme, ThemeKind};
use zeus_widgets::{Button, ComboBox, Label};

pub struct ThemeSettings {
   open: bool,
   overlay: OverlayManager,
   size: (f32, f32),
}

impl ThemeSettings {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         size: (400.0, 120.0),
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

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new("Theme Settings").size(theme.text_sizes.large);
      let window_frame = theme.frame1;

      Window::new(title)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 20.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let combo_visuals = theme.combo_box_visuals();
               let label_visuals = theme.label_visuals();

               let selected_text = RichText::new(theme.kind.to_str()).size(theme.text_sizes.normal);
               let label = Label::new(selected_text, None)
                  .visuals(label_visuals)
                  .sense(Sense::click())
                  .expand(Some(6.0))
                  .fill_width(true);

               ComboBox::new("theme_settings_combobox", label)
                  .width(200.0)
                  .visuals(combo_visuals)
                  .show_ui(ui, |ui| {
                     ui.spacing_mut().item_spacing.y = 10.0;

                     for kind in ThemeKind::to_vec() {
                        let text = RichText::new(kind.to_str()).size(theme.text_sizes.normal);
                        let label = Label::new(text, None)
                           .visuals(label_visuals)
                           .expand(Some(6.0))
                           .sense(Sense::click())
                           .fill_width(true);

                        if ui.add(label).clicked() {
                           let new_theme = Theme::new(kind);
                           ui.ctx().set_global_style(new_theme.style.clone());
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
                  self.close();

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
                        }
                     }
                  });
               }
            });
         });
   }
}
