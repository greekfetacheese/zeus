//! The auto-lock screen shown when Zeus is idle-locked.
//!
//! Re-verifies the credentials against the in-memory vault — no Argon2, no
//! disk. Same shape as the vault-unlock window, but it is a *re-login*: the
//! wallet stays in memory, we only gate the UI.

use crate::core::ZeusContext;
use crate::core::types::{CredentialCheck, MAX_CREDENTIAL_ATTEMPTS};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::{self, DeferredFocus};
use crate::utils::TimeStamp;
use egui::{Align2, RichText, Ui, Window, vec2};
use egui_elements::{Button, CredentialsForm, Theme};
use ncrypt_me::Credentials;

pub struct LockScreen {
   credentials_form: CredentialsForm,
   size: (f32, f32),
   focus: DeferredFocus,
}

impl LockScreen {
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form = CredentialsForm::new()
         .with_min_size(form_size)
         .with_open(true)
         .with_enabled_virtual_keyboard();
      Self {
         credentials_form,
         size: (550.0, 300.0),
         focus: DeferredFocus::requested(),
      }
   }

   /// Draw or drop the field controls that only a sighted user can use.
   pub fn set_field_controls(&mut self, shown: bool) {
      common::field_controls::credentials_form(&mut self.credentials_form, shown);
   }

   pub fn erase(&mut self) {
      self.credentials_form.erase();
      // The next lock is a new card: it takes its own focus when it first paints.
      self.focus.request();
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !ctx.locked {
         // The card is only drawn while locked: an ask left pending by a lock that ended before
         // it painted must not fire on a later one.
         self.focus.cancel();
         return;
      }

      let frame = theme.frame1;

      Window::new("Lock_Screen")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(frame)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.add_space(10.0);
               ui.spacing_mut().item_spacing.y = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;
               let ui_width = ui.available_width();

               let heading_text = RichText::new("Zeus is locked").size(theme.typography.heading);
               let title = ui.label(heading_text);

               // The page behind the card is not drawn while locked, so nothing else has asked
               // for the focus: the card takes it here, on the first frame it paints.
               self.focus.grab(ui, title.id);

               ui.label(
                  RichText::new("Enter your credentials to continue")
                     .size(theme.typography.normal)
                     .color(theme.colors.text_muted),
               );

               if ctx.security.credential_attempts > 0 {
                  ui.label(
                     RichText::new(attempt_text(ctx.security.credential_attempts))
                        .size(theme.typography.normal)
                        .color(theme.colors.error),
                  );
               }

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs);
                  self.credentials_form.show(ui);
               });

               let text = RichText::new("Unlock").size(theme.typography.large);
               let button =
                  Button::new(text).visuals(button_visuals).min_size(vec2(ui_width * 0.50, 35.0));

               if ui.add(button).clicked() {
                  let username = self.credentials_form.username();
                  let password = self.credentials_form.password();
                  let confirm_password = self.credentials_form.confirm_password();
                  let credentials = Credentials::new(username, password, confirm_password);
                  on_unlock(credentials);
               }
            });
         });
   }
}

/// Verify the credentials against the in-memory vault and unlock on success.
///
/// Uses [`crate::core::Vault::credentials_match_login`] (username + password),
/// not `credentials_match`, which also compares the confirm field the login
/// form does not have.
fn on_unlock(credentials: Credentials) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

      match ctx.check_credentials(&credentials, true) {
         CredentialCheck::Matched => {}
         CredentialCheck::Mismatch => {
            let failed = ctx.read(|ctx| ctx.security.credential_attempts);

            SHARED_GUI.write(|gui| {
               gui.open_msg_window("Incorrect credentials");
               gui.announce(failure_text(failed));
               gui.request_repaint();
            });
            return;
         }
         // The attempt cap was reached: Zeus is shutting down.
         CredentialCheck::LockedOut => return,
      }

      let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();

      ctx.write(|ctx| {
         ctx.locked = false;
         ctx.last_activity_ms = now;
      });

      SHARED_GUI.write(|gui| {
         gui.lock_screen.erase();
         gui.request_repaint();
      });
   });
}

/// The try the card is asking for: the failures so far plus this one, never past the cap.
fn attempt_text(failed: u8) -> String {
   let attempt = failed.saturating_add(1).min(MAX_CREDENTIAL_ATTEMPTS);

   format!("Attempt {attempt} of {MAX_CREDENTIAL_ATTEMPTS}")
}

/// Why the last try was refused, and how many are left before Zeus stops accepting them.
///
/// `failed` is the count `check_credentials` has already incremented for that try. Kept in one
/// function so the sentence is written once and the count a reader hears cannot drift.
fn failure_text(failed: u8) -> String {
   let left = MAX_CREDENTIAL_ATTEMPTS.saturating_sub(failed);
   let noun = if left == 1 { "attempt" } else { "attempts" };

   format!("Incorrect credentials. {left} {noun} left.")
}

#[cfg(test)]
mod tests {
   use super::*;

   /// A refusal says how many tries are left, and the last one stays singular.
   #[test]
   fn failure_text_counts_the_tries_left() {
      assert_eq!(
         failure_text(1),
         "Incorrect credentials. 4 attempts left."
      );
      assert_eq!(
         failure_text(4),
         "Incorrect credentials. 1 attempt left."
      );
   }

   /// The number is the try being asked for, and the cap is the last one it can name.
   #[test]
   fn attempt_text_stops_at_the_cap() {
      assert_eq!(attempt_text(2), "Attempt 3 of 5");
      assert_eq!(
         attempt_text(MAX_CREDENTIAL_ATTEMPTS - 1),
         "Attempt 5 of 5"
      );
      assert_eq!(attempt_text(u8::MAX), "Attempt 5 of 5");
   }
}
