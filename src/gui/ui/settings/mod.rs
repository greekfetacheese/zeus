//! UI that shows all the settings.

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use egui::{Align2, Frame, RichText, Ui, Window, vec2};
use std::sync::Arc;
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::Button;

pub mod change_credentials;
pub mod contacts;
pub mod encryption;
pub mod general;
pub mod networks;
pub mod theme;

pub use change_credentials::ChangeCredentialsUi;
pub use contacts::ContactsUi;
pub use encryption::EncryptionSettings;
pub use general::GeneralSettings;
pub use networks::NetworkSettings;
pub use theme::ThemeSettings;

pub struct SettingsUi {
   open: bool,
   general: GeneralSettings,
   pub encryption: EncryptionSettings,
   pub network: NetworkSettings,
   theme: ThemeSettings,
   pub contacts_ui: ContactsUi,
   pub change_credentials_ui: ChangeCredentialsUi,
   size: (f32, f32),
}

impl SettingsUi {
   pub fn new(ctx: &mut ZeusContext, overlay: OverlayManager) -> Self {
      Self {
         open: false,
         general: GeneralSettings::new(ctx, overlay.clone()),
         encryption: EncryptionSettings::new(overlay.clone()),
         network: NetworkSettings::new(overlay.clone()),
         theme: ThemeSettings::new(overlay.clone()),
         contacts_ui: ContactsUi::new(overlay.clone()),
         change_credentials_ui: ChangeCredentialsUi::new(overlay.clone()),
         size: (550.0, 350.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn open_network_settings(&mut self) {
      self.network.open();
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(theme, ui);
      self.encryption.show(theme, ui);
      self.change_credentials_ui.show(theme, ui);
      self.contacts_ui.show(ctx, theme, icons, ui);
      self.general.show(ctx, theme, ui);
      self.theme.show(theme, ui);
   }

   pub fn main_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      Window::new("settings_main_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::new())
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;

               ui.label(RichText::new("Settings").size(theme.text_sizes.heading));

               let size = vec2(self.size.0, 50.0);

               let text = RichText::new("Change your Credentials").size(theme.text_sizes.large);
               let button = Button::new(text).min_size(size).visuals(button_visuals);

               if ui.add(button).clicked() {
                  self.change_credentials_ui.open();
               }

               let text = RichText::new("Encryption Settings").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  self.encryption.open();
               }

               let text = RichText::new("Contacts").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  self.contacts_ui.open();
               }

               let text = RichText::new("Network Settings").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  self.network.open();
               }

               let text = RichText::new("General Settings").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  self.general.open();
               }

               let text = RichText::new("Theme Settings").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  self.theme.open();
               }
            });
         });
   }
}
