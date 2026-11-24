//! The names the common rows and pickers put on their AccessKit nodes.

use egui::{FocusDirection, Id, Response, Role, Ui, WidgetInfo};
use egui_elements::Frame;

/// Name a picker's node from the face it shows: `"{caption}: {value}"`.
///
/// A picker names its own node from the closed face alone, which is the value, so two pickers on one
/// form both read as bare values and a picker beside its own caption reads as the value twice. The
/// caption is the word the row already draws; this puts it on the node the picker owns. The shape
/// came from `chain_select.rs`, where a chain picker had the same gap.
pub fn name_picker(ui: &Ui, id: Id, caption: &str, value: &str) {
   ui.ctx().accesskit_node_builder(id, |node| {
      node.set_label(format!("{caption}: {value}"))
   });
}

/// Put one sentence on the node behind a themed label, whatever the label reported for itself.
///
/// AccessKit names a `Role::Label` node from the node's value rather than its label, so the
/// sentence goes in both: whichever of the two a consumer reads, the label says exactly the one
/// thing it was given. A widget that made no node of its own is given a label node here, so the
/// drawn text is never the only thing a reader gets; one that did keeps the role it has and only
/// takes the words.
pub fn name_text(ui: &Ui, response: &Response, text: impl Into<String>) {
   let text = text.into();
   let egui_ctx = ui.ctx();

   if !egui_ctx.has_accesskit_node(response.id) {
      let name = text.as_str();
      response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), name));
   }

   egui_ctx.accesskit_node_builder(response.id, |node| {
      if node.role() == Role::Label {
         node.set_value(text.clone());
      }

      node.set_label(text.clone());
   });
}

/// Make a drawn line the alert that says `sentence` — once.
///
/// A label's words are already in its node's value, and a widget given the alert role needs them as
/// the node's name as well. That naming left the sentence in both places, and the Windows reader
/// says both: the unlimited-allowance warning came out doubled. The name is what an alert is
/// announced by, so the value is let go.
pub fn name_alert(ui: &Ui, response: &Response, sentence: impl Into<String>) {
   let sentence = sentence.into();

   response.widget_info(|| WidgetInfo::labeled(Role::Alert, ui.is_enabled(), sentence.as_str()));

   ui.ctx().accesskit_node_builder(response.id, |node| node.set_value(""));
}

/// A plate that is one clickable row to a reader: the frame, its contents, and the node behind them.
///
/// A row drawn as a frame with a click sense bolted on afterwards is a hit target and nothing more:
/// a reader finds no role, no name, and no way to activate it. The frame's own widget carries all
/// three here — [`WidgetInfo::selected`] gives it the button role, the name the caller writes and,
/// when the caller says so, the selected state that tells a reader which row the surface points at.
///
/// The frame registers itself with an empty sense before its contents, under the id it is about to
/// take, and that withdraws the focus a Tab put on it: a row reached by keyboard would lose focus
/// every frame and Space/Enter would never reach it. `show` is `begin` + the contents + `end`, so
/// writing the three out leaves a moment to put the focus back before the final registration, which
/// is the one that makes the row focusable again.
///
/// That final registration happens *after* the contents, and egui walks the focus in the order the
/// widgets ask for it: the row's own controls — the options button, the copy button — would take
/// every Tab a step before the row itself, so a keyboard user reaches the settings of the thing
/// before the thing. The row therefore asks for the focus here, between `begin` and the contents,
/// and the buttons inside it follow it in the order they are drawn.
///
/// Asking twice has a price the second ask has to be watched for: it arrives while egui is handing
/// the focus to the next widget, and can take that hand-off, which leaves Tab from a row's copy
/// button coming back to the row instead of moving on. A row that did not hold the focus before the
/// ask and holds it after it is exactly that case, and the hand-off is passed on there.
///
/// `covered` is the ids of the nodes the contents added for words the row's own name already says:
/// they are hidden, so a reader hears the row once instead of once per label inside it.
pub fn clickable_row(
   ui: &mut Ui,
   frame: Frame,
   name: impl Into<String>,
   description: Option<String>,
   selected: bool,
   add_contents: impl FnOnce(&mut Ui) -> Vec<Id>,
) -> Response {
   let focused_before = ui.ctx().memory(|memory| memory.focused());

   // Read before `begin` takes the frame: a row that cannot take a click cannot take a Tab either,
   // and pre-registering it would leave a focus stop that does nothing.
   let focusable = frame.sense.is_focusable();
   let layer_id = ui.layer_id();

   let mut row = frame.begin(ui);
   let id = ui.next_auto_id();

   if focused_before == Some(id) {
      ui.ctx().memory_mut(|memory| memory.request_focus(id));
   }

   // Ahead of the contents, so Tab reaches the row before the buttons drawn inside it.
   if focusable {
      ui.ctx().memory_mut(|memory| memory.interested_in_focus(id, layer_id));
   }

   let covered = add_contents(&mut row.content_ui);

   // The frame asks for the focus again when it ends — a focusable widget has to, or it loses the
   // focus it already had — and that ask arrives while egui is handing the focus to the next widget.
   // It can take that hand-off, and Tab from a row's own button would then come back to the row
   // instead of moving on to the next one. Holding the focus is the tell: a row that did not hold it
   // before the ask and holds it after took the hand-off, and passes it on.
   let held_before = ui.ctx().memory(|memory| memory.focused()) == Some(id);
   let response = row.end(ui);

   if focusable && !held_before && ui.ctx().memory(|memory| memory.focused()) == Some(id) {
      ui.ctx().memory_mut(|memory| {
         memory.surrender_focus(id);
         memory.move_focus(FocusDirection::Next);
      });
   }

   let name = name.into();

   response.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), selected, &name));

   if let Some(description) = description {
      ui.ctx().accesskit_node_builder(response.id, |node| {
         node.set_description(description)
      });
   }

   for covered in covered {
      ui.ctx().accesskit_node_builder(covered, |node| node.set_hidden());
   }

   response
}

/// Put a list row at its place, and give the list its size.
///
/// AccessKit puts the position on the item (`set_position_in_set`, zero-based) and the count on
/// the container the items belong to (`set_size_of_set`), unlike ARIA where the size sits on the
/// item too. The container is the `Ui` the rows are drawn into: egui's node for a `Ui` is keyed
/// by its `unique_id`, so that is where the count goes. A widget id no widget registered would
/// put the count beside the real node instead of on it.
pub fn place_in_list(ui: &Ui, row_id: Id, index: usize, len: usize) {
   let egui_ctx = ui.ctx();

   egui_ctx.accesskit_node_builder(ui.unique_id(), |node| node.set_size_of_set(len));
   egui_ctx.accesskit_node_builder(row_id, |node| node.set_position_in_set(index));
}
