use super::super::app::main_window;
use super::super::harness::{
   drawn_names, frame, frame_nodes, is_text_input, node_name, status_text,
};
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::Context;
use egui::accesskit::{Node, NodeId, Role};

/// The recovery flow, with no vault behind it: these windows are the whole screen when Zeus has
/// nothing to unlock.
fn recovery_flow() -> (Context, GUI, ZeusCtx) {
   let (ctx, gui, zeus) = main_window();

   // Whether a vault exists comes out of the user's data directory, so the test states it rather
   // than inheriting the machine's setup.
   zeus.write(|zeus| zeus.vault_exists = false);

   (ctx, gui, zeus)
}

/// A window node's name: the title it was created with, wherever egui puts it.
fn window_name(node: &Node) -> Option<String> {
   node.label().or_else(|| node.value()).map(str::to_owned)
}

/// The name of every `egui::Window` in the tree, for the windows that are named by a sentence.
fn window_names(nodes: &[(NodeId, Node)]) -> Vec<String> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Window)
      .filter_map(|(_, node)| window_name(node))
      .collect()
}

/// Draw the recovery windows until they are painted: each is an `egui::Window`, measured on the pass
/// it opens, and egui exposes no node for a widget it considers invisible.
fn draw_recovery_flow(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let theme = gui.theme.clone();
   let mut nodes = Vec::new();

   for _ in 0..3 {
      let recovery = &mut gui.recover_wallet_ui;
      let import = &mut gui.settings.import;

      nodes = frame_nodes(ctx, Vec::new(), |ui| {
         zeus.write(|zeus| recovery.show(zeus, &theme, import, ui));
      });
   }

   nodes
}

/// Draw the app-wide overlay dialogs and put this frame's announcement in the tree, the way the app
/// draws the announcer right after the panels.
fn draw_overlay_modals(ctx: &Context, gui: &mut GUI) -> Vec<Node> {
   frame(ctx, |ui| {
      gui.show_overlay_modals(ui);
      gui.show_announcer(ui);
   })
}

/// The first window of the recovery flow is named by a sentence a reader can act on, not by the
/// identifier the code gives it: the title bar is hidden, so the name is all a reader gets.
#[test]
fn the_first_run_window_is_named_by_a_sentence() {
   let (ctx, mut gui, zeus) = recovery_flow();

   let nodes = draw_recovery_flow(&ctx, &mut gui, &zeus);

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Label && node.value() == Some("No vault was found")),
      "the credentials window must be drawn: {:?}",
      drawn_names(&nodes)
   );

   let names = window_names(&nodes);

   assert!(
      names.iter().any(|name| name == "Set up Zeus"),
      "the window must be named by a sentence, got {names:?}"
   );
}

/// The wallet-name window is named by a sentence too, and the box it opens on says what it is for
/// and takes the focus the credentials form just gave up.
#[test]
fn the_wallet_name_window_opens_on_a_field_that_says_what_it_is() {
   let (ctx, mut gui, zeus) = recovery_flow();
   gui.recover_wallet_ui.begin_wallet_name_for_test();

   let nodes = draw_recovery_flow(&ctx, &mut gui, &zeus);

   let names = window_names(&nodes);

   assert!(
      names.iter().any(|name| name == "Name your master wallet"),
      "the window must be named by a sentence, got {names:?}"
   );

   let (field, node) = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role())
            && node_name(&nodes, node).as_deref() == Some("Wallet name, optional")
      })
      .unwrap_or_else(|| {
         panic!(
            "the window must name its field: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.label(),
      Some("Wallet name, optional"),
      "the field carries that name itself; its hint stays the placeholder"
   );

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(*field),
      "the field must take the focus the credentials form gave up"
   );
}

/// The loading window is a dialog with no heading, so it says what it is doing twice over: its
/// message on the frame's announcement node, and a spinner named for the wait.
#[test]
fn the_loading_window_says_what_it_is_doing() {
   let (ctx, mut gui, _) = main_window();
   let message = "Recovering your wallet. This takes 10 to 15 minutes.";

   gui.loading_window.new_size((300.0, 150.0));
   gui.loading_window.open(message);

   let announcing = draw_overlay_modals(&ctx, &mut gui);

   assert_eq!(
      status_text(&announcing).as_deref(),
      Some(message),
      "the message must be said on the frame the window appears"
   );

   let mut painted = Vec::new();

   for _ in 0..3 {
      painted = draw_overlay_modals(&ctx, &mut gui);
   }

   assert!(
      painted
         .iter()
         .any(|node| node.role() == Role::ProgressIndicator && node.label() == Some("Working")),
      "the spinner must say what it is waiting for: {:?}",
      painted.iter().filter_map(|node| node.label()).collect::<Vec<_>>()
   );

   assert_ne!(
      status_text(&painted).as_deref(),
      Some(message),
      "a message that did not change is not said again"
   );

   // The same wait continuing is not a new phase: opening the window again with the sentence it
   // already carries must not interrupt the reader with it.
   gui.loading_window.open(message);

   let repeated = draw_overlay_modals(&ctx, &mut gui);

   assert_ne!(
      status_text(&repeated).as_deref(),
      Some(message),
      "a sentence the window already shows is not announced again"
   );

   // Progress text moves while the wait runs: it is drawn on the window, never said.
   let progress = "Download progress: 42.0%";

   gui.loading_window.progress(progress);

   let progressing = draw_overlay_modals(&ctx, &mut gui);

   assert_ne!(
      status_text(&progressing).as_deref(),
      Some(progress),
      "a percentage that changes is visible, not narrated"
   );
   assert!(
      progressing
         .iter()
         .any(|node| node.role() == Role::Label && node.value() == Some(progress)),
      "the progress must be drawn on the window: {:?}",
      progressing.iter().filter_map(|node| node.value()).collect::<Vec<_>>()
   );

   // A new sentence is a new phase, and that one is said once.
   let revoking = "Revoking the approval…";

   gui.loading_window.open(revoking);

   let changed = draw_overlay_modals(&ctx, &mut gui);

   assert_eq!(
      status_text(&changed).as_deref(),
      Some(revoking),
      "a different sentence opens a new announcement"
   );
}

/// The requirements warning is drawn only once the memory sample lands, so it can appear after the
/// user has started typing: each line is an alert carrying the words it draws, instead of a
/// coloured line nobody hears.
#[test]
fn the_memory_warning_is_heard_when_it_appears() {
   let (ctx, mut gui, zeus) = recovery_flow();

   // 16 GB in the machine and 4 GB free, against the 8.39 GB the recovery needs: both lines of the
   // warning are due.
   gui.recover_wallet_ui
      .set_memory_for_test(16 * 1024 * 1024 * 1024, 4 * 1024 * 1024 * 1024);

   let nodes = draw_recovery_flow(&ctx, &mut gui, &zeus);

   let alerts: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Alert)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert_eq!(
      alerts.len(),
      2,
      "both lines of the warning must be alerts: {alerts:?}"
   );

   for sentence in [
      "You need at least 8.39 GB of free RAM to recover your wallet",
      "You currently have 4.00 GB of free RAM",
   ] {
      assert!(
         alerts.contains(&sentence),
         "the warning line must be an alert carrying what it draws: {sentence:?} missing from \
          {alerts:?}"
      );
   }
}
