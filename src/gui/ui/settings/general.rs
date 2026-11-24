//! UI that allows the user to change the general settings.

use crate::core::ZeusContext;
use crate::core::urls::{UrlPurpose, purpose_tip};
use crate::gui::ui::common::section_heading;
use egui::{Align, Layout, RichText, Ui, vec2};
use egui_elements::{Button, Checkbox, Chip, ChipRole, Theme};
use std::sync::OnceLock;

const ICONS_TIP: &str = "Allow Zeus to download token icons and NFT images";
const SOURCIFY_TIP: &str = "Allow Zeus to look up verified contract names on sourcify.dev";
const UPDATES_TIP: &str = "Allow Zeus to check GitHub for a newer Zeus release";
const TRAY_TIP: &str = "Hide Zeus to the system tray when the window is minimized. Zeus keeps \
                        running in the background; the tray icon brings it back.";
#[cfg(target_os = "linux")]
const DESKTOP_TIP: &str = "Add Zeus to your application menu, with its own icon, so you can \
                           start it and pin it like any other app. This writes a desktop entry \
                           and its icon under ~/.local/share - outside Zeus's own folder. Both \
                           are removed again when you turn this off.";

/// The External Data hover tips: the one-line explanation plus the endpoints the
/// opt-in actually contacts. Built once — `on_hover_text` takes its value every
/// frame, so this must not allocate on the frame path.
fn icons_tip() -> &'static str {
   static TIP: OnceLock<String> = OnceLock::new();
   TIP.get_or_init(|| purpose_tip(ICONS_TIP, UrlPurpose::AssetImages))
}

fn names_tip() -> &'static str {
   static TIP: OnceLock<String> = OnceLock::new();
   TIP.get_or_init(|| purpose_tip(SOURCIFY_TIP, UrlPurpose::ContractNames))
}

fn updates_tip() -> &'static str {
   static TIP: OnceLock<String> = OnceLock::new();
   TIP.get_or_init(|| purpose_tip(UPDATES_TIP, UrlPurpose::Updates))
}

pub struct GeneralSettings {
   fetch_asset_images: bool,
   fetch_contract_names: bool,
   check_for_updates: bool,
   minimize_to_tray: bool,
   desktop_integration: bool,
   /// Whether the shortcut button was pressed since the last draw.
   shortcuts_requested: bool,
}

impl GeneralSettings {
   pub fn new(ctx: &mut ZeusContext) -> Self {
      let mut this = Self {
         fetch_asset_images: false,
         fetch_contract_names: false,
         check_for_updates: false,
         minimize_to_tray: true,
         desktop_integration: false,
         shortcuts_requested: false,
      };
      this.sync_from_ctx(ctx);
      this
   }

   fn persist_misc(ctx: &ZeusContext) {
      let config = ctx.misc_config.clone();
      crate::runtime::rt().spawn_blocking(move || {
         if let Err(e) = config.save() {
            tracing::error!("Failed to save misc config: {e}");
         }
      });
   }

   /// Add or remove the application-menu entry.
   ///
   /// The only setting whose effect is file work rather than a `misc_config` field, so it
   /// never runs on the frame path.
   fn apply_desktop_integration(enabled: bool) {
      crate::runtime::rt()
         .spawn_blocking(move || crate::utils::desktop_integration::apply(enabled));
   }

   pub fn sync_from_ctx(&mut self, ctx: &mut ZeusContext) {
      self.fetch_asset_images = ctx.misc_config.fetch_asset_images();
      self.fetch_contract_names = ctx.misc_config.fetch_contract_names();
      self.check_for_updates = ctx.misc_config.check_for_updates();
      self.minimize_to_tray = ctx.misc_config.minimize_to_tray();
      self.desktop_integration = ctx.misc_config.desktop_integration();
   }

   /// Whether the shortcut button was pressed since the last call.
   ///
   /// The page cannot put the sheet up itself — the frame draws it, over the window this page is
   /// inside — so the press is handed up instead.
   pub fn take_shortcuts_request(&mut self) -> bool {
      std::mem::take(&mut self.shortcuts_requested)
   }

   /// The Window section: whether a minimize hides Zeus in the tray.
   ///
   /// Only drawn for a sighted user — a reader has no way to click a tray icon, and the option
   /// would be a switch that changes nothing they can hear.
   fn window_section(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.separator();
      ui.add_space(10.0);

      section_heading(ui, theme.typography.very_large, "Window");

      let tray_text = RichText::new("Minimize to Tray").size(theme.typography.normal);
      let qmark = Chip::new(RichText::new("?").size(theme.typography.normal)).role(ChipRole::Info);
      let ui_size = vec2(ui.available_width() * 0.3, 30.0);

      ui.allocate_ui_with_layout(
         ui_size,
         Layout::left_to_right(Align::Center),
         |ui| {
            let checkbox = Checkbox::new(&mut self.minimize_to_tray, tray_text);
            let control = ui.add(checkbox);

            if control.changed() {
               ctx.misc_config.set_minimize_to_tray(self.minimize_to_tray);
               Self::persist_misc(ctx);
            }

            let chip = ui.add(qmark).on_hover_text(TRAY_TIP);

            ui.ctx()
               .accesskit_node_builder(control.id, |node| node.set_description(TRAY_TIP));
            ui.ctx().accesskit_node_builder(chip.id, |node| node.set_hidden());
         },
      );
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      reader_attached: bool,
      ui: &mut Ui,
   ) {
      ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, theme.spacing.lg);
      ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);

      ui.add_space(10.0);

      section_heading(ui, theme.typography.very_large, "Keyboard");

      let sentence = RichText::new(
         "Every page can be reached from the keyboard. F1 shows these keys from any page.",
      )
      .size(theme.typography.normal);
      ui.label(sentence);

      let text = RichText::new("Show Keyboard Shortcuts").size(theme.typography.normal);
      let button = Button::new(text).visuals(theme.button_visuals());

      if ui.add(button).clicked() {
         self.shortcuts_requested = true;
      }

      ui.separator();
      ui.add_space(10.0);

      section_heading(ui, theme.typography.very_large, "External Data");

      let q_mark_text = RichText::new("?").size(theme.typography.normal);
      let qmark = Chip::new(q_mark_text.clone()).role(ChipRole::Info);

      let ui_size = vec2(ui.available_width() * 0.3, 30.0);
      let icons_text =
         RichText::new("Download Token Icons & NFT Images").size(theme.typography.normal);

      ui.allocate_ui_with_layout(
         ui_size,
         Layout::left_to_right(Align::Center),
         |ui| {
            let checkbox = Checkbox::new(&mut self.fetch_asset_images, icons_text);
            let control = ui.add(checkbox);

            if control.changed() {
               ctx.misc_config.set_fetch_asset_images(self.fetch_asset_images);
               Self::persist_misc(ctx);
            }

            let help = icons_tip();
            let chip = ui.add(qmark).on_hover_text(help);

            // The tip is the control's own description and the chip leaves the tree: a reader
            // hears the explanation on the checkbox instead of visiting a separate "?".
            ui.ctx().accesskit_node_builder(control.id, |node| node.set_description(help));
            ui.ctx().accesskit_node_builder(chip.id, |node| node.set_hidden());
         },
      );

      let names_text = RichText::new("Fetch Contract Names").size(theme.typography.normal);
      let qmark = Chip::new(q_mark_text.clone()).role(ChipRole::Info);

      ui.allocate_ui_with_layout(
         ui_size,
         Layout::left_to_right(Align::Center),
         |ui| {
            let checkbox = Checkbox::new(&mut self.fetch_contract_names, names_text);
            let control = ui.add(checkbox);

            if control.changed() {
               ctx.misc_config.set_fetch_contract_names(self.fetch_contract_names);
               Self::persist_misc(ctx);
            }

            let help = names_tip();
            let chip = ui.add(qmark).on_hover_text(help);

            ui.ctx().accesskit_node_builder(control.id, |node| node.set_description(help));
            ui.ctx().accesskit_node_builder(chip.id, |node| node.set_hidden());
         },
      );

      let updates_text = RichText::new("Check for Updates").size(theme.typography.normal);
      let qmark = Chip::new(q_mark_text.clone()).role(ChipRole::Info);

      ui.allocate_ui_with_layout(
         ui_size,
         Layout::left_to_right(Align::Center),
         |ui| {
            let checkbox = Checkbox::new(&mut self.check_for_updates, updates_text);
            let control = ui.add(checkbox);

            if control.changed() {
               ctx.misc_config.set_check_for_updates(self.check_for_updates);
               Self::persist_misc(ctx);
            }

            let help = updates_tip();
            let chip = ui.add(qmark).on_hover_text(help);

            ui.ctx().accesskit_node_builder(control.id, |node| node.set_description(help));
            ui.ctx().accesskit_node_builder(chip.id, |node| node.set_hidden());
         },
      );

      // A reader cannot reach a tray icon, so their window never hides there and the option that
      // would is not offered.
      if !reader_attached {
         self.window_section(ctx, theme, ui);
      }

      ui.separator();
      ui.add_space(10.0);

      // Linux only: there is no application menu to add Zeus to on the other platforms, so the
      // section is not offered there at all.
      #[cfg(target_os = "linux")]
      {
         section_heading(ui, theme.typography.very_large, "Desktop");

         let desktop_text = RichText::new("Add to Application Menu").size(theme.typography.normal);
         let qmark = Chip::new(q_mark_text.clone()).role(ChipRole::Info);

         ui.allocate_ui_with_layout(
            ui_size,
            Layout::left_to_right(Align::Center),
            |ui| {
               let checkbox = Checkbox::new(&mut self.desktop_integration, desktop_text);
               let control = ui.add(checkbox);

               if control.changed() {
                  ctx.misc_config.set_desktop_integration(self.desktop_integration);
                  Self::persist_misc(ctx);
                  Self::apply_desktop_integration(self.desktop_integration);
               }

               let chip = ui.add(qmark).on_hover_text(DESKTOP_TIP);

               ui.ctx().accesskit_node_builder(control.id, |node| {
                  node.set_description(DESKTOP_TIP)
               });
               ui.ctx().accesskit_node_builder(chip.id, |node| node.set_hidden());
            },
         );
      }
   }
}
