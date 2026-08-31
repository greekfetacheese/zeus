//! Settings window to export Zeus `data/` into a zip archive.

use crate::core::{
   data_dir,
   data_export::{ExportOptions, export_data_to_zip},
   persisted::{ExportPolicy, PersistedTree},
};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{Align2, Order, RichText, Stroke, Ui, vec2};
use egui_elements::{Button, OverlayManager, Theme, widgets::Window};

pub struct ExportDataUi {
   open: bool,
   overlay: OverlayManager,
   options: ExportOptions,
   size: (f32, f32),
}

impl ExportDataUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         options: ExportOptions::default(),
         size: (420.0, 340.0),
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
      if self.open {
         self.overlay.window_closed();
         self.open = false;
      }
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let title = RichText::new("Export Data").size(theme.typography.heading);
      let window_frame = theme.window_frame.fill(theme.frame1.fill);
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Middle)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(5.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               let text = "Export your wallets and state files to a zip archive.";
               let text = RichText::new(text).size(theme.typography.normal);
               ui.label(text);

               let text = "The zip is not password-protected.";
               let text = RichText::new(text).size(theme.typography.normal);
               ui.label(text);

               let text = "All sensitive data is already encrypted with your credentials.";
               let text = RichText::new(text).size(theme.typography.normal);
               ui.label(text);

               ui.add_space(8.0);

               let text = RichText::new("Optional files").size(theme.typography.normal);
               ui.label(text);

               ui.vertical(|ui| {
                  for tree in PersistedTree::ALL {
                     if tree.export_policy() != ExportPolicy::Optional {
                        continue;
                     }
                     let text = RichText::new(tree.export_label()).size(theme.typography.normal);
                     ui.checkbox(self.options.flag_mut(*tree), text);
                  }
               });

               ui.add_space(20.0);

               let size = vec2(ui.available_width() * 0.6, 35.0);
               let text = RichText::new("Export").size(theme.typography.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  self.export();
               }
            });
         });

      if !open {
         self.close();
      }
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
                  gui.settings.export.close();
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
