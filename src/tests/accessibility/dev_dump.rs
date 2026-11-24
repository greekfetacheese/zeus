use super::app::{render_unlock_screen, unlock_screen};
use egui::Context;
use egui::accesskit::Node;

/// Write the frame's `TreeUpdate` to `logs/accessibility-tree.txt` for review: one
/// line per node, `role | name | value | description | actions`, which is what a reviewer reads
/// and what a later slice diffs.
///
/// Dev builds only, and only when asked:
/// `ZEUS_ACCESSIBILITY_DUMP=1 cargo test --features dev --bin zeus accessibility`.
///
/// The dump is taken from a headless frame rather than the running app on purpose: eframe builds
/// `platform_output.accesskit_update` in `Context::end_pass`, after `App::ui` has returned, and
/// owns the `FullOutput` it lands in, so the app itself can never see the frame's tree.
#[test]
fn dump_the_unlock_screen_tree() {
   if std::env::var("ZEUS_ACCESSIBILITY_DUMP").as_deref() != Ok("1") {
      return;
   }

   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);
   let tree: String = nodes.iter().map(|node| format!("{}\n", node_line(node))).collect();

   if let Err(e) = std::fs::create_dir_all("logs") {
      panic!("could not create logs/: {e}");
   }
   if let Err(e) = std::fs::write("logs/accessibility-tree.txt", tree) {
      panic!("could not write logs/accessibility-tree.txt: {e}");
   }
}

/// One dump line for a node: `role | name | value | description | actions`.
fn node_line(node: &Node) -> String {
   use egui::accesskit::Action;

   let mut actions = String::new();

   for action in [
      Action::Click,
      Action::Focus,
      Action::Expand,
      Action::Collapse,
      Action::SetValue,
      Action::ScrollIntoView,
   ] {
      if !node.supports_action(action) {
         continue;
      }
      if !actions.is_empty() {
         actions.push(',');
      }
      actions.push_str(&format!("{action:?}"));
   }

   format!(
      "{:?} | {} | {} | {} | {}",
      node.role(),
      node.label().unwrap_or_default(),
      node.value().unwrap_or_default(),
      node.description().unwrap_or_default(),
      actions,
   )
}
