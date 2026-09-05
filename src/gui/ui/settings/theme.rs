//! UI that allows the user to change the theme settings.

use crate::core::context::theme_kind_dir;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{RichText, Sense, Ui, vec2};
use egui_elements::{ComboBox, Label, Theme, ThemeKind};

pub struct ThemeSettings {}

impl ThemeSettings {
   pub fn new() -> Self {
      Self {}
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.lg);
      ui.spacing_mut().button_padding = theme.button_padding;

      ui.label(RichText::new("Theme").size(theme.typography.large));

      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();

      let selected_text = RichText::new(theme.kind.to_str()).size(theme.typography.normal);
      let label = Label::new(selected_text, None)
         .visuals(label_visuals)
         .sense(Sense::click())
         .expand(Some(6.0))
         .interactive(true)
         .fill_width(true);

      ComboBox::new("theme_settings_combobox", label)
         .width(200.0)
         .visuals(combo_visuals)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing.sm;

            for kind in ThemeKind::to_vec() {
               let text = RichText::new(kind.to_str()).size(theme.typography.normal);
               let label = Label::new(text, None)
                  .visuals(label_visuals)
                  .expand(Some(6.0))
                  .sense(Sense::click())
                  .interactive(true)
                  .fill_width(true);

               if ui.add(label).clicked() {
                  let mut new_theme = Theme::new(kind);

                  new_theme.install(ui.ctx());

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        gui.theme = new_theme;
                        gui.request_repaint();
                     });
                     save();
                  });
               }
            }
         });
   }
}

fn save() {
   let dir = match theme_kind_dir() {
      Ok(dir) => dir,
      Err(e) => {
         SHARED_GUI.write(|gui| {
            gui.msg_window.open(e.to_string());
         });
         return;
      }
   };

   let theme = SHARED_GUI.read(|gui| gui.theme.clone());
   let theme_kind_str = serde_json::to_string(&theme.kind).unwrap();
   match std::fs::write(dir, theme_kind_str) {
      Ok(_) => {}
      Err(e) => {
         SHARED_GUI.write(|gui| {
            gui.msg_window.open(e.to_string());
         });
      }
   }
}
