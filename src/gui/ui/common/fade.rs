//! Open fade-in for panel UIs that are not egui [`Window`]s.
//!
//! Left-nav surfaces share the central panel. A visible fade-out stacks with
//! the next view (e.g. Send → Home) and looks wrong — so close is **instant**.
//! Open still fades in via `animate_bool` + `multiply_opacity`.
//!
//! # Critical usage rule
//!
//! Call [`panel_fade`] / [`show_with_fade`] **every frame**, including when
//! `open == false`. Do **not** early-return on `!self.open` before the helper:
//!
//! ```ignore
//! // BAD — animator never sees `false`, so the next open snaps to 1.0
//! if !self.open { return; }
//! show_with_fade(ui, "my_ui_fade", self.open, |ui| { … });
//!
//! // GOOD — always tick the animator; paint only when open
//! show_with_fade(ui, "my_ui_fade", self.open, |ui| { … });
//! ```
//!
//! egui's first `animate_bool(..., true)` with no prior sample returns `1.0`
//! immediately. Only repeated closed-frame ticks drive the value back to 0 so
//! the next open can fade in.

use eframe::egui::{Id, Ui, emath};

/// Fade-in opacity for a panel. Close is instant (never paints while closed).
///
/// - Returns [`None`] when `open == false` or opacity has not left 0 yet.
/// - Returns [`Some(opacity)`] in `(0, 1]` only while `open` (fade-in / held open).
/// - Uses `cubic_out` (same family as [`Window`] fade).
/// - Requests repaints while the value is in motion (via egui).
///
/// ```ignore
/// let Some(opacity) = panel_fade(ui, "send_crypto_ui_fade", self.open) else {
///    return;
/// };
/// theme.frame1.show(ui, |ui| {
///    ui.multiply_opacity(opacity);
///    // …body…
/// });
/// ```
#[inline]
pub fn panel_fade(ui: &Ui, id: impl Into<Id>, open: bool) -> Option<f32> {
   // Always tick the animator so close resets toward 0 without painting.
   let opacity = ui.ctx().animate_bool_with_easing(id.into(), open, emath::easing::cubic_out);

   // Instant hide — no fade-out paint (avoids stacking with the next panel).
   if !open || opacity <= 0.0 {
      None
   } else {
      Some(opacity)
   }
}

/// Like [`panel_fade`], then runs `add_contents` under the animated opacity.
///
/// No-op while closed (instant hide). Fade-in only.
///
/// Call this every frame from `show` — do not gate on `if !open { return }` first.
#[inline]
pub fn show_with_fade(
   ui: &mut Ui,
   id: impl Into<Id>,
   open: bool,
   add_contents: impl FnOnce(&mut Ui),
) {
   let Some(opacity) = panel_fade(ui, id, open) else {
      return;
   };

   ui.scope(|ui| {
      ui.multiply_opacity(opacity);
      add_contents(ui);
   });
}
