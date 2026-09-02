//! Settings window to import Zeus `data/` from an exported zip archive.

use crate::core::{data_dir, data_import::import_data_from_zip};
use crate::gui::SHARED_GUI;
use crate::gui::ui::dapps::railgun::BundlerUrl;
use crate::utils::{RT, state};
use egui::{Align2, Frame, Margin, Order, RichText, Stroke, Ui, vec2};
use egui_elements::{Button, CredentialsForm, OverlayManager, Theme, widgets::Window};
use ncrypt_me::Credentials;
use std::path::PathBuf;

pub struct ImportDataUi {
   open: bool,
   overlay: OverlayManager,
   credentials_form: CredentialsForm,
   zip_path: Option<PathBuf>,
   /// True when opened from first-run recover (no vault on disk yet).
   first_run: bool,
   size: (f32, f32),
}

impl ImportDataUi {
   pub fn new(overlay: OverlayManager) -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form = CredentialsForm::new()
         .with_min_size(form_size)
         .with_open(true)
         .with_enabled_virtual_keyboard();
      Self {
         open: false,
         overlay,
         credentials_form,
         zip_path: None,
         first_run: false,
         size: (600.0, 600.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open_with(false);
   }

   /// First-run recover: import a backup instead of regenerating the HD wallet.
   pub fn open_first_run(&mut self) {
      self.open_with(true);
   }

   fn open_with(&mut self, first_run: bool) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
      self.first_run = first_run;
      self.credentials_form.open();
   }

   pub fn close(&mut self) {
      if self.open {
         self.overlay.window_closed();
         self.open = false;
      }
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new(self.overlay.clone());
   }

   pub fn erase(&mut self) {
      self.credentials_form.erase();
   }

   /// First-run recover window on the main app (vault still locked).
   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open || !self.first_run {
         return;
      }

      let mut open = self.open;
      let title = RichText::new("Import Data").size(theme.typography.heading);
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
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(5.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            let frame = Frame::new().inner_margin(Margin::same(10));

            frame.show(ui, |ui| {
               self.body(theme, ui);
            });
         });

      if !open {
         self.reset();
      }
   }

   /// Settings Data page (vault already unlocked).
   pub fn show_page(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(5.0, 12.0);
      ui.spacing_mut().button_padding = vec2(10.0, 4.0);
      ui.label(RichText::new("Import").size(theme.typography.very_large));
      self.body(theme, ui);
   }

   fn body(&mut self, theme: &Theme, ui: &mut Ui) {
      let button_visuals = theme.button_visuals();

      ui.vertical(|ui| {
         let text = if self.first_run {
            "Import a Zeus data backup instead of recovering from credentials."
         } else {
            "This will replace your existing wallets and state files!!"
         };
         let text = RichText::new(text).size(theme.typography.large).color(theme.colors.warning);
         ui.label(text);

         ui.add_space(8.0);

         let size = vec2(ui.available_width() * 0.5, 35.0);
         let text = RichText::new("Choose a file").size(theme.typography.large);
         let button = Button::new(text).visuals(button_visuals).min_size(size);
         if ui.add(button).clicked() {
            if let Some(path) = rfd::FileDialog::new()
               .set_title("Import Zeus data")
               .add_filter("ZIP archive", &["zip"])
               .pick_file()
            {
               self.zip_path = Some(path);
            }
         }

         let selected = self
            .zip_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "No archive selected".to_string());
         ui.label(
            RichText::new(selected)
               .size(theme.typography.normal)
               .color(theme.colors.text_muted),
         );

         ui.scope(|ui| {
            ui.spacing_mut().button_padding = vec2(4.0, 4.0);
            let text = "Enter the credentials that will unlock the vault";
            let text = RichText::new(text).size(theme.typography.large);
            ui.label(text);

            self.credentials_form.show(ui);
         });

         ui.add_space(5.0);

         let size = vec2(ui.available_width() * 0.6, 45.0);
         let text = RichText::new("Import").size(theme.typography.large);
         let button = Button::new(text).visuals(button_visuals).min_size(size);

         if ui.add(button).clicked() {
            self.import();
         }
      });
   }

   fn import(&self) {
      let Some(zip_path) = self.zip_path.clone() else {
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window("Select a zip file first");
            });
         });
         return;
      };

      let username = self.credentials_form.username();
      let password = self.credentials_form.password();
      let confirm_password = self.credentials_form.confirm_password();
      let credentials = Credentials::new(username, password, confirm_password);

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Verifying imported vault...");
            gui.request_repaint();
            gui.ctx.clone()
         });

         let imported =
            match data_dir().and_then(|dir| import_data_from_zip(&zip_path, &dir, credentials)) {
               Ok(imported) => imported,
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.open_msg_window(format!("Import failed: {e}"));
                     gui.request_repaint();
                  });
                  return;
               }
            };
         let n = imported.files_written;

         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Loading imported data...");
            gui.request_repaint();
         });

         match ctx.reload_from_data_dir(imported.vault) {
            Ok((master_wallet, argon)) => {
               let key = ctx.read_vault(|vault| vault.wallet_state_key().ok());
               let bundler_url = match key {
                  Some(key) => match BundlerUrl::exists() {
                     Ok(true) => match BundlerUrl::load(&key) {
                        Ok(url) => Some(url.url),
                        Err(e) => {
                           tracing::error!("Error loading Bundler URL: {:?}", e);
                           None
                        }
                     },
                     _ => None,
                  },
                  None => None,
               };

               let ctx = ctx.clone();
               RT.spawn(async move {
                  state::on_startup(ctx).await;
               });

               SHARED_GUI.write(|gui| {
                  gui.settings.import.reset();
                  gui.settings.encryption.set_argon2(argon);
                  gui.header.open();
                  gui.header.set_current_wallet(master_wallet);
                  gui.portofolio.open();
                  if let Some(url) = bundler_url {
                     gui.shield_ui.set_bundler_url(url);
                  }
                  gui.loading_window.reset();
                  gui.open_msg_window(format!("Imported {n} files"));
                  gui.request_repaint();
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.reset();
                  gui.open_msg_window(format!(
                     "Files were written but Zeus could not load them: {e}"
                  ));
                  gui.request_repaint();
               });
            }
         }
      });
   }
}
