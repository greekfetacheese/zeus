//! Fixed-size cells for the list views.

use egui::{Align, Layout, Sense, TextWrapMode, Ui, UiBuilder, vec2};

/// One cell of a list row: a fixed-size box whose content truncates rather than wraps.
///
/// Don't clip — Button chrome expands a few px and clip cuts the left edge of a Revoke button, and
/// the truncating wrap is what keeps a long label inside the cell.
pub fn row_cell(ui: &mut Ui, width: f32, height: f32, add_contents: impl FnOnce(&mut Ui)) {
   let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
   let mut child =
      ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
   child.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
   add_contents(&mut child);
}
