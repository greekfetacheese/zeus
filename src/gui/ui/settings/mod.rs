//! Settings viewport: category nav + inlined pages.

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::SHARED_GUI;
use crate::gui::ui::{WindowCtx, common::privacy_mode_switch, window_frame};
use egui::{
   RichText, ScrollArea, Shadow, Stroke, Ui, ViewportBuilder, ViewportClass, ViewportId, vec2,
};
use egui_elements::{Frame as Frame2, Label, OverlayManager, Theme};
use egui_lucide::Lucide;
use std::sync::Arc;

pub mod change_credentials;
pub mod contacts;
pub mod encryption;
pub mod export;
pub mod general;
pub mod import;
pub mod networks;
pub mod railgun;
pub mod theme;

pub use change_credentials::ChangeCredentialsUi;
pub use contacts::ContactsUi;
pub use encryption::EncryptionSettings;
pub use export::ExportDataUi;
pub use general::GeneralSettings;
pub use import::ImportDataUi;
pub use networks::NetworkSettings;
pub use railgun::RailgunSettings;
pub use theme::ThemeSettings;

const SETTINGS_VIEWPORT_ID: &str = "zeus_settings_viewport";

pub fn settings_viewport_id() -> ViewportId {
   ViewportId::from_hash_of(SETTINGS_VIEWPORT_ID)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
   General,
   Appearance,
   Networks,
   Security,
   Contacts,
   Railgun,
   Data,
}

pub struct SettingsUi {
   open: bool,
   /// Apply default inner size only on the first builder after open.
   size_applied: bool,
   page: SettingsPage,
   general: GeneralSettings,
   pub encryption: EncryptionSettings,
   pub network: NetworkSettings,
   theme: ThemeSettings,
   pub contacts_ui: ContactsUi,
   pub change_credentials_ui: ChangeCredentialsUi,
   pub export: ExportDataUi,
   pub import: ImportDataUi,
   pub railgun: RailgunSettings,
}

impl SettingsUi {
   pub fn new(ctx: &mut ZeusContext, overlay: OverlayManager) -> Self {
      Self {
         open: false,
         size_applied: false,
         page: SettingsPage::General,
         general: GeneralSettings::new(ctx),
         encryption: EncryptionSettings::new(),
         network: NetworkSettings::new(),
         theme: ThemeSettings::new(),
         contacts_ui: ContactsUi::new(overlay.clone()),
         change_credentials_ui: ChangeCredentialsUi::new(),
         export: ExportDataUi::new(),
         import: ImportDataUi::new(overlay),
         railgun: RailgunSettings::new(ctx),
      }
   }

   pub fn erase(&mut self) {
      self.change_credentials_ui.erase();
      self.import.erase();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, ctx: &mut ZeusContext) {
      if !self.open {
         self.open = true;
         self.general.sync_from_ctx(ctx);
         self.encryption.sync_from_ctx(ctx);
         self.railgun.sync_from_ctx(ctx);
      }
   }

   pub fn close(&mut self, ctx: &mut ZeusContext) {
      if self.open {
         self.general.save_settings(ctx);
      }
      self.open = false;
      self.size_applied = false;
      self.change_credentials_ui.reset();
      self.contacts_ui.reset_page();
      self.network.reset_view();
   }

   pub fn open_page(&mut self, page: SettingsPage, ctx: &mut ZeusContext) {
      self.set_page(page, ctx);
      self.open(ctx);
   }

   pub fn open_network_settings(&mut self, ctx: &mut ZeusContext) {
      self.open_page(SettingsPage::Networks, ctx);
   }

   fn set_page(&mut self, page: SettingsPage, ctx: &mut ZeusContext) {
      if self.page == SettingsPage::General && page != SettingsPage::General {
         self.general.save_settings(ctx);
      }
      if page == SettingsPage::Security {
         self.encryption.sync_from_ctx(ctx);
      }
      if page == SettingsPage::Railgun {
         self.railgun.sync_from_ctx(ctx);
      }
      self.page = page;
   }

   fn viewport_builder(&mut self, egui_ctx: &egui::Context) -> ViewportBuilder {
      let mut builder = ViewportBuilder::default()
         .with_title("Settings")
         .with_decorations(false)
         .with_min_inner_size([800.0, 560.0])
         .with_resizable(true);

      // Size + position only on first open. Repeating InnerSize/Position every
      // parent frame fights user resize/drag (and Wayland cannot set position).
      if !self.size_applied {
         const SIZE: [f32; 2] = [1120.0, 720.0];
         builder = builder.with_inner_size(SIZE);
         if let Some(parent) = egui_ctx.input(|i| i.viewport().outer_rect) {
            let size = vec2(SIZE[0], SIZE[1]);
            builder = builder.with_position(parent.center() - size * 0.5);
         }
         self.size_applied = true;
      }

      builder
   }

   /// Register the Settings OS window. Uses a **deferred** viewport so the parent
   /// frame does not present/vsync the child mid-pass (immediate viewports do).
   ///
   /// Must be called from the root viewport while Settings is open. The actual
   /// paint runs later on the child viewport's own pass via [`paint_settings_viewport`].
   pub fn show(&mut self, ctx: &mut ZeusContext, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let builder = self.viewport_builder(ui.ctx());
      let id = settings_viewport_id();

      // Native eframe has embed_viewports=false. If a backend embeds, the deferred
      // callback would re-enter SHARED_GUI while the parent already holds it.
      if ui.ctx().embed_viewports() {
         self.paint(ctx, icons, theme, ui);
         return;
      }

      ui.ctx().show_viewport_deferred(id, builder, paint_settings_viewport);
   }

   fn paint(&mut self, ctx: &mut ZeusContext, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
      let window_ctx = WindowCtx::settings(theme);
      window_frame(ui, window_ctx, |ui| {
         self.show_shell(ctx, icons, theme, ui);
      });
   }

   fn show_shell(&mut self, ctx: &mut ZeusContext, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
      let nav_bg = theme.frame1.fill;
      egui::Panel::left("settings_nav")
         .min_size(180.0)
         .max_size(180.0)
         .resizable(false)
         .show_separator_line(false)
         .frame(egui::Frame::new().fill(nav_bg).inner_margin(8.0))
         .show(ui, |ui| {
            self.nav(ctx, theme, ui);
         });

      egui::CentralPanel::default()
         .frame(egui::Frame::new().fill(theme.colors.bg).inner_margin(16.0))
         .show(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
               egui::Frame::new().inner_margin(10.0).show(ui, |ui| {
                  ui.set_width(ui.available_width());
                  match self.page {
                     SettingsPage::General => self.general.show(ctx, theme, ui),
                     SettingsPage::Appearance => self.theme.show(theme, ui),
                     SettingsPage::Networks => self.network.show(ctx, theme, icons, ui),
                     SettingsPage::Security => {
                        ui.add_space(10.0);
                        self.change_credentials_ui.show(theme, ui);
                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(16.0);
                        self.encryption.show(theme, ui);
                     }
                     SettingsPage::Contacts => self.contacts_ui.show_page(ctx, theme, ui),
                     SettingsPage::Railgun => self.railgun.show(ctx, theme, icons, ui),
                     SettingsPage::Data => {
                        self.export.show(theme, ui);
                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(16.0);
                        self.import.show_page(theme, ui);
                     }
                  }
               });
            });
         });
   }

   fn nav(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.xs);

      let text_size = theme.typography.normal;
      let icon_color = theme.colors.text;
      let mut visuals = theme.frame2_visuals();
      visuals.bg = theme.frame1.fill;
      visuals.border = Stroke::NONE;
      visuals.shadow = Shadow::NONE;

      let frame = Frame2::from_egui(theme.frame2)
         .interactive(true)
         .fill_width(true)
         .visuals(visuals)
         .corner_radius(0);

      let items: [(SettingsPage, &str, egui::Image<'_>); 7] = [
         (
            SettingsPage::General,
            "General",
            Lucide::SlidersHorizontal.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Appearance,
            "Appearance",
            Lucide::Palette.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Networks,
            "Networks",
            Lucide::Globe.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Security,
            "Security",
            Lucide::LockKeyhole.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Contacts,
            "Contacts",
            Lucide::Users.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Railgun,
            "Railgun",
            Lucide::Shield.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Data,
            "Data",
            Lucide::Archive.size(20.0).color(icon_color).image(),
         ),
      ];

      privacy_mode_switch(ctx, theme, ui);
      ui.add_space(10.0);

      for (page, label, icon) in items {
         let selected = self.page == page;
         let response = frame.selected(selected).show(ui, |ui| {
            ui.add(
               Label::new(RichText::new(label).size(text_size), Some(icon))
                  .interactive(false)
                  .image_on_left(),
            );
         });

         if response.response.clicked() {
            self.set_page(page, ctx);
         }
      }
   }
}

/// Child-viewport paint. Runs on the Settings window's own eframe pass, not nested
/// inside the parent `App::ui`, so the parent does not wait on this present/vsync.
///
/// Captures nothing (`Send + Sync`). State is read through [`SHARED_GUI`].
fn paint_settings_viewport(ui: &mut Ui, _class: ViewportClass) {
   if ui.ctx().input(|i| i.viewport().close_requested()) {
      SHARED_GUI.write(|gui| {
         gui.ctx.clone().write(|ctx| {
            gui.settings.close(ctx);
         });
      });
      return;
   }

   SHARED_GUI.write(|gui| {
      let icons = gui.icons.clone();
      let theme = gui.theme.clone();
      gui.ctx.clone().write(|ctx| {
         gui.settings.paint(ctx, icons, &theme, ui);
      });
      // Overlay modals (msg / loading / confirm / update) are Areas on the
      // current viewport. Paint them here so they are visible on Settings.
      gui.show_overlay_modals(ui);
   });
}
