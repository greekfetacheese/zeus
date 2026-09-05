//! Settings page to export Zeus `data/` into a zip archive.

use crate::core::{
   data_dir,
   data_export::{ExportOptions, export_data_to_zip},
   persisted::{ExportPolicy, PersistedTree},
};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{RichText, Ui, vec2};
use egui_elements::{Button, Theme};

pub struct ExportDataUi {
   options: ExportOptions,
}

impl ExportDataUi {
   pub fn new() -> Self {
      Self {
         options: ExportOptions::default(),
      }
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, theme.spacing.md);
      ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);
      ui.add_space(10.0);

      let button_visuals = theme.button_visuals();

      ui.vertical_centered(|ui| {
         ui.label(RichText::new("Export Data").size(theme.typography.heading));

         ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing.xs;
            let text = "Export your wallets and state files to a zip archive.";
            let text = RichText::new(text).size(theme.typography.normal);
            ui.label(text);

            let text = "The zip is not password-protected.";
            let text = RichText::new(text).size(theme.typography.normal);
            ui.label(text);

            let text = "All sensitive data is already encrypted with your credentials.";
            let text = RichText::new(text).size(theme.typography.normal);
            ui.label(text);
         });

         ui.add_space(8.0);

         let text = RichText::new("Optional files").size(theme.typography.normal);
         ui.label(text);

         for tree in PersistedTree::ALL {
            if tree.export_policy() != ExportPolicy::Optional {
               continue;
            }
            let text = RichText::new(tree.export_label()).size(theme.typography.normal);
            ui.checkbox(self.options.flag_mut(*tree), text);
         }

         ui.add_space(12.0);

         let size = vec2(100.0, 35.0);
         let text = RichText::new("Export").size(theme.typography.large);
         let button = Button::new(text).visuals(button_visuals).min_size(size);

         if ui.add(button).clicked() {
            self.export();
         }
      });
   }

   fn export(&self) {
      let Some(path) = rfd::FileDialog::new()
         .set_title("Export Zeus data")
         .set_file_name("zeus-data.zip")
         .add_filter("ZIP archive", &["zip"])
         .save_file()
      else {
         return;
      };

      let options = self.options;

      RT.spawn_blocking(move || {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Exporting data...");
            gui.request_repaint();
         });

         let result = data_dir().and_then(|dir| export_data_to_zip(&dir, &path, options));

         match result {
            Ok(n) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.reset();
                  gui.open_msg_window(format!(
                     "Exported {n} files to {}",
                     path.display()
                  ));
                  gui.request_repaint();
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.reset();
                  gui.open_msg_window(format!("Export failed: {e}"));
                  gui.request_repaint();
               });
            }
         }
      });
   }
}
