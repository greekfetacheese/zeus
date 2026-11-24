use egui::accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId};
use egui::{Context, Event, Id, Key, Modifiers, RawInput, Rect, Sense, Ui, WidgetInfo, pos2, vec2};

/// Run one AccessKit-enabled frame and return the nodes of the tree update it produced.
///
/// egui's own recipe: enable the tree, run a pass, take
/// `platform_output.accesskit_update` — the same value the platform gives a reader.
pub(super) fn frame(ctx: &Context, add: impl FnMut(&mut Ui)) -> Vec<Node> {
   frame_nodes(ctx, Vec::new(), add).into_iter().map(|(_, node)| node).collect()
}

/// Run one AccessKit-enabled frame with `events` in its input, keeping the id each node is
/// addressed by: a reader sends an action back to that id, so a test that drives a widget needs it.
pub(crate) fn frame_nodes(
   ctx: &Context,
   events: Vec<Event>,
   add: impl FnMut(&mut Ui),
) -> Vec<(NodeId, Node)> {
   frame_nodes_and_output(ctx, events, add).0
}

/// The same frame, with the commands it handed the platform — a test that copies an address needs
/// the words that went to the clipboard, which the tree does not carry.
pub(super) fn frame_nodes_and_output(
   ctx: &Context,
   events: Vec<Event>,
   add: impl FnMut(&mut Ui),
) -> (Vec<(NodeId, Node)>, Vec<egui::OutputCommand>) {
   ctx.enable_accesskit();

   let mut output = ctx.run_ui(
      RawInput {
         screen_rect: Some(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(1290.0, 950.0),
         )),
         events,
         ..Default::default()
      },
      add,
   );

   let update = output
      .platform_output
      .accesskit_update
      .take()
      .expect("an AccessKit-enabled frame must produce a tree update");

   let nodes = update.nodes.into_iter().collect();
   let commands = std::mem::take(&mut output.platform_output.commands);

   output.drop_without_applying_deltas();

   (nodes, commands)
}

/// The text of every `Role::Label` node. AccessKit puts a label's text in `value` rather than
/// `label` so that `labelled_by` still works.
pub(super) fn label_texts(nodes: &[Node]) -> Vec<&str> {
   nodes
      .iter()
      .filter(|node| node.role() == Role::Label)
      .filter_map(|node| node.value())
      .collect()
}

/// The name of every button node: what a reader hears as it Tabs through the controls.
pub(super) fn button_names(nodes: &[Node]) -> Vec<&str> {
   nodes
      .iter()
      .filter(|node| node.role() == Role::Button)
      .filter_map(|node| node.label())
      .collect()
}

/// The id of the widget the keyboard is on, the way a reader addresses it.
pub(super) fn focused_id(ctx: &Context) -> Option<NodeId> {
   ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id())
}

/// The name a reader would hear for one node of the frame that drew it.
pub(super) fn named(nodes: &[(NodeId, Node)], id: Option<NodeId>) -> Option<String> {
   let id = id?;

   nodes
      .iter()
      .find(|(node_id, _)| *node_id == id)
      .and_then(|(_, node)| node.label())
      .map(str::to_string)
}

/// Whether a node carries `name` as its label or its value: a drawn word lands on one or the
/// other.
pub(crate) fn is_named(node: &Node, name: &str) -> bool {
   node.label() == Some(name) || node.value() == Some(name)
}

/// What a reader hears for the node the keyboard is on: a control's name, or a line's text.
pub(super) fn focused_text(ctx: &Context, nodes: &[(NodeId, Node)]) -> Option<String> {
   let id = focused_id(ctx)?;

   let (_, node) = nodes.iter().find(|(node_id, _)| *node_id == id)?;

   node.label().or_else(|| node.value()).map(str::to_string)
}

/// A key press as the window delivers it.
pub(super) fn key_press(key: Key) -> Event {
   Event::Key {
      key,
      physical_key: None,
      pressed: true,
      repeat: false,
      modifiers: Modifiers::NONE,
   }
}

/// An action a screen reader sends to the widget at `target`.
pub(super) fn accesskit_action(action: Action, target: NodeId) -> Event {
   Event::AccessKitActionRequest(ActionRequest {
      action,
      target_tree: TreeId::ROOT,
      target_node: target,
      data: None,
   })
}

/// The text of this frame's announcement node.
pub(super) fn status_text(nodes: &[Node]) -> Option<String> {
   nodes
      .iter()
      .find(|node| node.role() == Role::Status)
      .and_then(|node| node.label().map(str::to_string))
}

/// The roles egui's single-line text fields use.
pub(super) fn is_text_input(role: Role) -> bool {
   matches!(
      role,
      Role::TextInput | Role::MultilineTextInput | Role::PasswordInput
   )
}

/// A node's accessible name: its own label, or the value of the label that names it.
///
/// `InputField` draws a visible label and ties it to the field with `labelled_by`, so the field
/// itself carries no label.
pub(super) fn node_name(nodes: &[(NodeId, Node)], node: &Node) -> Option<String> {
   node.label().map(str::to_owned).or_else(|| {
      node.labelled_by().iter().find_map(|id| {
         nodes
            .iter()
            .find(|(node_id, _)| node_id == id)
            .and_then(|(_, label)| label.value())
            .map(str::to_owned)
      })
   })
}

/// The node `row` is a direct child of, and that node's id.
pub(super) fn parent_of(nodes: &[(NodeId, Node)], row: NodeId) -> (NodeId, &Node) {
   nodes
      .iter()
      .find(|(_, node)| node.children().contains(&row))
      .map(|(id, node)| (*id, node))
      .unwrap_or_else(|| panic!("no node holds {row:?} as a direct child"))
}

/// The node ids in reading order: a depth-first walk from the root, children in the order the
/// accesskit node says they are read.
///
/// The `TreeUpdate` itself is a map and comes out in no particular order, so a tree-order
/// assertion has to walk the children rather than trust the vector.
pub(super) fn tree_order(nodes: &[(NodeId, Node)]) -> Vec<NodeId> {
   let root = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Window)
      .map(|(id, _)| *id)
      .expect("the tree starts from the root window node");

   let mut order = Vec::new();
   let mut stack = vec![root];

   while let Some(id) = stack.pop() {
      order.push(id);

      if let Some((_, node)) = nodes.iter().find(|(node_id, _)| *node_id == id) {
         for child in node.children().iter().rev() {
            stack.push(*child);
         }
      }
   }

   order
}

/// Where a node whose name starts with `prefix` sits in the tree's reading order.
pub(super) fn node_position(nodes: &[(NodeId, Node)], prefix: &str) -> usize {
   tree_order(nodes)
      .into_iter()
      .position(|id| {
         nodes
            .iter()
            .find(|(node_id, _)| *node_id == id)
            .and_then(|(_, node)| node.label().or_else(|| node.value()))
            .is_some_and(|text| text.starts_with(prefix))
      })
      .unwrap_or_else(|| panic!("no node named {prefix:?} in the tree"))
}

/// Every name in the tree, for the failure message of a missing node.
pub(super) fn drawn_names(nodes: &[(NodeId, Node)]) -> Vec<&str> {
   nodes
      .iter()
      .filter_map(|(_, node)| node.label().or_else(|| node.value()))
      .collect()
}

/// The text of this frame's announcement node, in a tree that keeps node ids.
pub(super) fn announced(nodes: &[(NodeId, Node)]) -> Option<String> {
   nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Status)
      .and_then(|(_, node)| node.label().map(str::to_string))
}

/// The sentence a copy leaves for the announcement channel, built the way the buttons build it.
pub(super) fn copy_said(address: &str) -> String {
   crate::gui::ui::copied_words(address)
}

/// The text a frame handed the platform's clipboard.
pub(super) fn clipboard_text(commands: &[egui::OutputCommand]) -> Option<String> {
   commands.iter().find_map(|command| match command {
      egui::OutputCommand::CopyText(text) => Some(text.clone()),
      _ => None,
   })
}

/// The id of the button named `name` in this frame's tree.
pub(super) fn button_named(nodes: &[(NodeId, Node)], name: &str) -> NodeId {
   nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the tree must offer a button named {name:?}: {:?}",
            drawn_names(nodes)
         )
      })
}

/// Every word a node carries in a form a reader can reach.
pub(super) fn node_texts(node: &Node) -> Vec<&str> {
   [node.label(), node.value(), node.description()].into_iter().flatten().collect()
}

/// The text of every label node a reader can reach.
pub(super) fn spoken_text(nodes: &[(NodeId, Node)]) -> Vec<&str> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect()
}

/// Every name and description in the tree, for a failure message.
pub(crate) fn spoken(nodes: &[(NodeId, Node)]) -> Vec<String> {
   nodes.iter().flat_map(|(_, node)| node_texts(node)).map(str::to_owned).collect()
}

/// A button a test knows the id of, drawn before a dialog: it is where the dialog has to put the
/// focus back when it closes.
pub(super) fn opener_button(ui: &mut Ui, id: Id, name: &str) {
   let rect = Rect::from_min_size(ui.next_widget_position(), vec2(180.0, 24.0));
   let response = ui.interact(rect, id, Sense::click());
   response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), name));
}

/// The node a frame left focused, looked up by the id the tree addresses it by.
pub(super) fn focused_node<'a>(ctx: &Context, nodes: &'a [(NodeId, Node)]) -> &'a Node {
   let focused = ctx
      .memory(|memory| memory.focused())
      .expect("the frame must leave a node focused");

   nodes
      .iter()
      .find(|(id, _)| *id == focused.accesskit_id())
      .map(|(_, node)| node)
      .expect("the focused node must be in the frame's tree")
}

/// A key press with modifiers, as the window delivers it.
pub(super) fn key_combo(key: Key, modifiers: Modifiers) -> Event {
   Event::Key {
      key,
      physical_key: None,
      pressed: true,
      repeat: false,
      modifiers,
   }
}

/// A key released, as the window delivers it.
pub(super) fn key_release(key: Key) -> Event {
   Event::Key {
      key,
      physical_key: None,
      pressed: false,
      repeat: false,
      modifiers: Modifiers::NONE,
   }
}

/// Whether a node is something a reader presses, types into or drags: the stops `Ctrl+Tab` is for.
pub(super) fn is_control(role: Role) -> bool {
   matches!(
      role,
      Role::Button
         | Role::CheckBox
         | Role::ComboBox
         | Role::Link
         | Role::MenuItem
         | Role::RadioButton
         | Role::Slider
         | Role::Switch
         | Role::Tab
         | Role::TextInput
   )
}

/// The words of every label in the tree, in the order they are drawn.
pub(super) fn label_words(nodes: &[(NodeId, Node)]) -> Vec<String> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label)
      .filter_map(|(_, node)| node.value().map(str::to_owned))
      .collect()
}
