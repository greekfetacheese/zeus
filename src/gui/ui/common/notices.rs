//! Notice panels shared by the dapp flows.

use egui::{RichText, Ui, vec2};
use egui_elements::Theme;

/// A framed, centred panel telling the user this flow cannot run in the current mode.
///
/// The message is the caller's own — each flow states what it cannot do — but the frame, the width
/// budget and the type size are shared, so the three surfaces read as one notice.
pub fn unsupported_panel(theme: &Theme, size: (f32, f32), message: &str, ui: &mut Ui) {
   ui.vertical_centered(|ui| {
      theme.frame1.show(ui, |ui| {
         ui.set_width(size.0);
         ui.set_max_height(size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
         ui.label(RichText::new(message).size(theme.typography.very_large));
      });
   });
}
