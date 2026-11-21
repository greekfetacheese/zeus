//! The field controls a reader can never use.
//!
//! The show/hide eye, the QR button and the on-screen keyboard are all picture-first: they are
//! drawn for someone looking at the window, and each takes a tab stop away from a reader who gets
//! nothing from it. One switch per widget keeps the choice out of every call site.

use egui_elements::{CredentialsForm, InputField, SecureInputField};

/// Whether the field controls a sighted user needs are drawn.
///
/// The show/hide toggle, the QR button and the on-screen keyboard are visual-only: a reader
/// gains nothing from them and pays a tab stop each, the keyboard's forty included.
pub fn credentials_form(form: &mut CredentialsForm, shown: bool) {
   form.set_mask_toggle(shown);
   form.set_qr_enabled(shown);
   if shown {
      form.enable_virtual_keyboard();
   } else {
      form.disable_virtual_keyboard();
   }
}

/// Draw or drop the show/hide toggle and the QR button on one secret field.
///
/// The toggle is a control flag, not a masking state: a field built masked stays masked with the
/// button gone, so a reader still hears "Password, 3 characters". The QR button goes with it
/// because a scan is a picture exchange the reader has no way to see.
pub fn secure_field(field: &mut SecureInputField, shown: bool) {
   field.set_mask_toggle(shown);
   if shown {
      field.enable_qr_scanner();
   } else {
      field.disable_qr_scanner();
   }
}

/// Draw or drop the QR button on an ordinary input field.
///
/// The field is not secret, so there is no toggle to drop; the scan button is still a tab stop
/// that only a sighted user can act on.
pub fn input_field(field: &mut InputField, shown: bool) {
   if shown {
      field.enable_qr_scanner();
   } else {
      field.disable_qr_scanner();
   }
}
