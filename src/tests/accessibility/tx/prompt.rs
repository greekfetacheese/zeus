use super::super::harness::{frame_nodes, is_text_input, node_name};
use egui::accesskit::{Node, NodeId, Role};
use egui::{Context, Event, Ui};

/// The dialogs a window's tree is made of, by the headings they are named with.
pub(crate) fn dialogs(nodes: &[(NodeId, Node)]) -> Vec<String> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Dialog)
      .filter_map(|(_, node)| node.label())
      .map(str::to_string)
      .collect()
}

/// Draw a modal the way the window hosting it draws it, into a reader's tree.
///
/// Three passes, for the same reason as the confirmation and the sign window: a
/// shrink-wrapping modal spends two invisible sizing passes before the first frame it actually
/// paints, and egui exposes no node for a widget it considers invisible. The events go to the
/// last pass only, so a key is not pressed three times.
pub(crate) fn render_modal(
   ctx: &Context,
   events: Vec<Event>,
   mut add: impl FnMut(&mut Ui),
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, &mut add);
   }

   nodes
}

/// The text field a reader reaches under `name`, however the tree spells that name: the field's
/// own label, or the value of the label it points at.
pub(crate) fn text_field<'a>(nodes: &'a [(NodeId, Node)], name: &str) -> Option<&'a Node> {
   nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| is_text_input(node.role()) && node_name(nodes, node).as_deref() == Some(name))
}
