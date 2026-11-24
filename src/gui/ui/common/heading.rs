//! The heading that names the open page to a screen reader.

use egui::{Rect, Response, Role, Sense, Ui, Vec2, WidgetInfo};

/// Mark `text` as the heading of what the surrounding page draws.
///
/// The node is focusable but not interactive, and it draws nothing: a real heading would take a
/// row above the page's content and shift everything below it, while the only thing a reader needs
/// is a node it can report as `Heading`, named after the page. The rect is therefore zero-sized
/// and placed with [`Ui::interact`] rather than allocated, so the layout cursor and the item
/// spacing are left exactly as they were.
///
/// The id is derived from the text: when the page changes, the heading is a different node, so the
/// focus request that follows a page change is a focus change to a reader, not a rename of a node
/// it is already sitting on.
pub fn heading(ui: &mut Ui, text: impl Into<String>) -> Response {
   let text = text.into();

   let id = ui.scope_id().with(("heading", text.as_str()));
   let rect = Rect::from_min_size(ui.next_widget_position(), Vec2::ZERO);
   let response = ui.interact(rect, id, Sense::focusable_noninteractive());

   response.widget_info(|| WidgetInfo::labeled(Role::Heading, ui.is_enabled(), text.clone()));

   response
}
