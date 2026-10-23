//! Notice panels shared by the dapp flows.

use egui::{RichText, Ui, vec2};
use egui_elements::Theme;

/// A framed, centred panel telling the user this flow cannot run in its current state.
///
/// The copy is the caller's own — each flow states what it cannot do — but the frame, the width
/// budget and the type sizes are shared, so every one of these notices reads the same way.
pub fn notice_panel(theme: &Theme, size: (f32, f32), title: &str, hint: Option<&str>, ui: &mut Ui) {
   ui.vertical_centered(|ui| {
      theme.frame1.show(ui, |ui| {
         ui.set_width(size.0);
         ui.set_max_height(size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
         ui.label(RichText::new(title).size(theme.typography.very_large));
         if let Some(hint) = hint {
            ui.label(RichText::new(hint).size(theme.typography.large));
         }
      });
   });
}
