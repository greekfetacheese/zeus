//! Settings → Security: the auto-lock timeout choice.

use crate::core::ZeusContext;
use crate::core::types::AutoLock;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{RichText, Sense, Ui, vec2};
use egui_elements::{Button, ComboBox, Label, Theme};

pub struct AutoLockSettings {
   selected: AutoLock,
   /// True while a `security.data` write is in flight. The combobox is disabled
   /// meanwhile so two fast selections cannot start overlapping disk writes.
   saving: bool,
}

impl AutoLockSettings {
   pub fn new() -> Self {
      Self {
         selected: AutoLock::default(),
         saving: false,
      }
   }

   pub fn sync_from_ctx(&mut self, ctx: &mut ZeusContext) {
      self.selected = ctx.security.autolock;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, theme.spacing.md);
         ui.spacing_mut().button_padding = theme.button_padding;

         ui.label(RichText::new("Auto-Lock").size(theme.typography.very_large));
         ui.label(
            RichText::new(
               "Lock the UI after a period of inactivity. Re-enter your credentials to unlock.",
            )
            .size(theme.typography.normal)
            .color(theme.colors.text_muted),
         );

         // The stored choice could not be read, so what is shown (and enforced) is the default.
         // Saying so here is the only place the user can fix it, and picking a value below clears it.
         if ctx.security.load_failed {
            ui.label(
               RichText::new(
                  "Your saved auto-lock choice could not be read and is still on disk. Until you \
                   choose below, Zeus locks on the default. Set it again to replace the file.",
               )
               .size(theme.typography.normal)
               .color(theme.colors.error),
            );
         }

         let combo_visuals = theme.combo_box_visuals();
         let label_visuals = theme.label_visuals();

         let selected_text = RichText::new(self.selected.label()).size(theme.typography.normal);
         let label = Label::new(selected_text, None)
            .visuals(label_visuals)
            .sense(Sense::click())
            .expand(Some(6.0))
            .interactive(true)
            .fill_width(true);

         let mut chosen = self.selected;

         // Disabled while a save is in flight, so a second click cannot overlap
         // the disk write (and the shown value matches what is persisted).
         ui.add_enabled_ui(!self.saving, |ui| {
            ComboBox::new("autolock_settings_combobox", label)
               .width(200.0)
               .visuals(combo_visuals)
               .show_ui(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                  #[cfg(feature = "dev")]
                  let options: &[AutoLock] = &AutoLock::ALL_DEV;
                  #[cfg(not(feature = "dev"))]
                  let options: &[AutoLock] = &AutoLock::ALL;

                  for option in options {
                     let text = RichText::new(option.label()).size(theme.typography.normal);
                     let option_label = Label::new(text, None)
                        .visuals(label_visuals)
                        .expand(Some(6.0))
                        .sense(Sense::click())
                        .interactive(true)
                        .fill_width(true);

                     if ui.add(option_label).clicked() {
                        chosen = *option;
                     }
                  }
               });
         });

         if chosen != self.selected {
            self.selected = chosen;
            ctx.security.autolock = chosen;
            // An explicit choice: stop the "auto-lock is not configured" nudge.
            ctx.security.autolock_changed = true;
            // And it supersedes a file that did not read — writing the value the user just picked is
            // what `persist` requires before it will touch that file.
            ctx.security.load_failed = false;
            if persist(ctx) {
               self.saving = true;
            }
         }

         ui.add_space(theme.spacing.md);

         let text = RichText::new("Lock Now").size(theme.typography.large);
         let button = Button::new(text).visuals(theme.button_visuals()).min_size(vec2(200.0, 35.0));

         if ui.add(button).clicked() {
            ctx.locked = true;
            // Repaint the main window so the lock card shows immediately. Do not
            // touch SHARED_GUI here: this runs inside its write guard.
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
         }
      });
   }
}

/// Seal and write `security.data`. Reads the vault key here (cheap clone) so the
/// Argon2-free save can run off the frame path. Returns whether a save was
/// actually started — `false` leaves the combobox enabled so it cannot get stuck.
fn persist(ctx: &ZeusContext) -> bool {
   // Never seal the defaults over a `security.data` that did not read. Callers clear `load_failed`
   // first, because a value the user just picked in this UI is a deliberate replacement; `false`
   // leaves the combobox enabled so the refusal cannot get stuck.
   if ctx.security.load_failed {
      tracing::error!(
         "Security settings: refusing to overwrite security.data that could not be read"
      );
      return false;
   }

   let key = match ctx.read_vault(|vault| vault.wallet_state_key()) {
      Ok(key) => key,
      Err(e) => {
         tracing::error!("Security settings: missing wallet state key: {e}");
         return false;
      }
   };

   let settings = ctx.security.clone();
   RT.spawn_blocking(move || {
      if let Err(e) = settings.save(&key) {
         tracing::error!("Failed to save security settings: {e}");
      }

      // Re-enable the combobox once the write is done (repaints both the main
      // window and the Settings viewport, where the combobox lives).
      SHARED_GUI.write(|gui| {
         gui.settings.autolock.saving = false;
         gui.request_repaint();
      });
   });

   true
}
