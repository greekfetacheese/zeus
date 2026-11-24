use super::app::{main_window, render_central, render_unlock_screen, unlock_screen};
use super::harness::{button_names, focused_node, frame, frame_nodes, label_texts};
use crate::gui::GUI;
use crate::gui::ui::wallet::import::{ImportWallet, ImportWalletType};
use egui::Context;
use egui::accesskit::{Live, Role};
use egui_elements::{Theme, ThemeKind};

/// With `accesskit` compiled in, a frame has a tree at all: the root window node plus a node
/// per widget drawn in it.
#[test]
fn an_enabled_frame_produces_a_tree_update() {
   let ctx = Context::default();

   let nodes = frame(&ctx, |ui| {
      ui.label("Zeus");
   });

   assert!(
      nodes.iter().any(|node| node.role() == Role::Window),
      "the update must start from the root window node"
   );

   assert!(
      label_texts(&nodes).contains(&"Zeus"),
      "the drawn label must be a node of its own: {:?}",
      label_texts(&nodes)
   );
}

/// A real screen's prose reaches the tree: the unlock screen's heading is the first thing a
/// reader hears from Zeus after the window itself.
#[test]
fn the_unlock_screen_heading_is_a_label_node() {
   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);

   assert!(
      label_texts(&nodes).contains(&"Zeus is locked"),
      "the unlock screen's heading is missing from the tree: {:?}",
      label_texts(&nodes)
   );
}

/// The card counts the tries: after two refusals the next one is the third of five, drawn as
/// one label a reader reaches like the rest of the card's prose.
#[test]
fn the_lock_card_says_which_attempt_this_is() {
   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);
   locked.write(|ctx| ctx.security.credential_attempts = 2);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);

   assert!(
      label_texts(&nodes).contains(&"Attempt 3 of 5"),
      "the card must say which try this is: {:?}",
      label_texts(&nodes)
   );
}

/// Nothing has been refused yet, so the card keeps exactly the shape a first-time user sees:
/// no attempt line at all.
#[test]
fn the_lock_card_has_no_attempt_line_before_a_failure() {
   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);
   let texts = label_texts(&nodes);

   assert!(
      texts.contains(&"Zeus is locked"),
      "the card must still render: {texts:?}"
   );

   assert!(
      !texts.iter().any(|text| text.starts_with("Attempt")),
      "a first try must draw no attempt line: {texts:?}"
   );
}

/// The announcement channel: one `Role::Status` node marked `Live::Polite`
/// whose label is the announced text. A change to that label is what a reader speaks, so the
/// node has to outlive the message and the text has to go through `GUI::announce`.
#[test]
fn an_announcement_is_a_polite_status_node() {
   let ctx = Context::default();
   let mut gui = GUI::default();

   let nodes = frame(&ctx, |ui| {
      gui.announce("Transaction confirmed");
      gui.show_announcer(ui);
   });

   let status = nodes
      .iter()
      .find(|node| node.role() == Role::Status)
      .expect("the announcer must keep a status node in the tree");

   assert_eq!(status.live(), Some(Live::Polite));
   assert_eq!(status.label(), Some("Transaction confirmed"));
}

/// Detection: a frame that builds a tree means a reader is attached, and
/// the sticky flag is what hands egui `Options::screen_reader` — which nothing native sets —
/// so that `Label`s become focusable, i.e. prose becomes reachable with Tab.
#[test]
fn an_attached_screen_reader_turns_on_the_focus_model() {
   let ctx = Context::default();
   let mut gui = GUI::default();

   assert!(
      !gui.at_active,
      "no assistive technology before the first frame"
   );

   frame(&ctx, |ui| gui.track_assistive_tech(ui));

   assert!(
      gui.at_active,
      "a frame that builds a tree means an assistive technology is attached"
   );
   assert!(
      ctx.options(|options| options.screen_reader),
      "the flag must hand egui its screen-reader focus model"
   );
}

/// A reader is not offered the controls that only a sighted user can act on: the moment the frame
/// builds a tree, the password row's show/hide toggle and the on-screen keyboard leave the tree,
/// while the fields and the Unlock button stay. The toggle also proves the mask is untouched: the
/// row is still a `PasswordInput`.
#[test]
fn a_reader_is_not_offered_the_vision_only_field_controls() {
   let ctx = Context::default();
   let mut gui = GUI::default();
   gui.theme.install(&ctx);
   gui.ctx.write(|zeus| zeus.locked = true);

   let nodes = render_unlock_screen(&ctx, &gui.ctx, &mut gui.lock_screen, &gui.theme);
   let names = button_names(&nodes);

   assert!(
      names
         .iter()
         .any(|name| *name == "Show credentials" || *name == "Hide credentials"),
      "the password row must draw its show/hide toggle before a reader is seen: {names:?}"
   );

   for key in ["q", "Space", "Backspace"] {
      assert!(
         names.contains(&key),
         "the on-screen keyboard's {key} key must be drawn before a reader is seen: {names:?}"
      );
   }

   // A tree-building frame is the detection signal, and the same pass drops the controls.
   frame(&ctx, |ui| gui.track_assistive_tech(ui));

   assert!(
      gui.at_active,
      "the detecting frame must leave the reader flag set"
   );

   let nodes = render_unlock_screen(&ctx, &gui.ctx, &mut gui.lock_screen, &gui.theme);
   let names = button_names(&nodes);

   for name in [
      "Show credentials",
      "Hide credentials",
      "q",
      "Space",
      "Backspace",
   ] {
      assert!(
         !names.contains(&name),
         "a reader cannot use {name:?}, so it must not be a tab stop: {names:?}"
      );
   }

   // What the reader does use is still drawn: the masked rows keep their mask, the username is a
   // plain text field, and the page's words and its Unlock button are unchanged.
   assert_eq!(
      nodes.iter().filter(|node| node.role() == Role::PasswordInput).count(),
      1,
      "the password row must still be drawn and masked: {names:?}"
   );

   assert_eq!(
      nodes.iter().filter(|node| node.role() == Role::TextInput).count(),
      1,
      "the username row must still be a plain text input: {names:?}"
   );

   let texts = label_texts(&nodes);

   assert!(
      texts.contains(&"Zeus is locked") && texts.contains(&"Username"),
      "the page's words and the field labels must still be there: {texts:?}"
   );

   assert!(
      names.contains(&"Unlock"),
      "the Unlock button must still be there: {names:?}"
   );
}

/// The import dialog's secret field drops the eye and the QR button a reader cannot use, and keeps
/// the mask and the drawn label that name it: a reader still hears which secret goes where.
#[test]
fn a_reader_importing_a_wallet_is_not_offered_the_eye_or_qr() {
   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);

   let mut import_wallet = ImportWallet::new();
   import_wallet.open(ImportWalletType::PrivateKey);

   let mut nodes = Vec::new();

   // A shrink-wrapping modal spends its first passes sizing, where its body is invisible and egui
   // exposes no node.
   for _ in 0..3 {
      nodes = frame(&ctx, |ui| import_wallet.show(&theme, ui));
   }

   let names = button_names(&nodes);

   assert!(
      names
         .iter()
         .any(|name| *name == "Show credentials" || *name == "Hide credentials"),
      "the secret field must draw its show/hide toggle before a reader is seen: {names:?}"
   );

   #[cfg(any(target_os = "linux", target_os = "windows"))]
   assert!(
      names.contains(&"Scan a QR code"),
      "the secret field must draw its scan button before a reader is seen: {names:?}"
   );

   import_wallet.set_field_controls(false);

   for _ in 0..3 {
      nodes = frame(&ctx, |ui| import_wallet.show(&theme, ui));
   }

   let names = button_names(&nodes);

   assert!(
      !names
         .iter()
         .any(|name| *name == "Show credentials" || *name == "Hide credentials"),
      "the eye toggle is a control a reader cannot use: {names:?}"
   );

   #[cfg(any(target_os = "linux", target_os = "windows"))]
   assert!(
      !names.contains(&"Scan a QR code"),
      "the QR button is a control a reader cannot use: {names:?}"
   );

   assert_eq!(
      nodes.iter().filter(|node| node.role() == Role::PasswordInput).count(),
      1,
      "the secret field must still be drawn and masked: {names:?}"
   );

   let texts = label_texts(&nodes);

   assert!(
      texts.contains(&"Private Key"),
      "the field must still be named by the label the dialog draws: {texts:?}"
   );
}

/// `egui_kittest` drives the same screen and queries it by role and label: the shape the
/// interaction tests take, and an independent reader of the same tree.
#[test]
fn kittest_queries_the_unlock_screen_by_role_and_label() {
   use egui_kittest::Harness;
   use egui_kittest::kittest::Queryable;

   let (locked, mut screen, theme) = unlock_screen();

   let harness = Harness::new_ui(|ui| {
      locked.write(|zeus| screen.show(zeus, &theme, ui));
   });

   let heading = harness.get_by_label("Zeus is locked");
   assert_eq!(
      heading.value().as_deref(),
      Some("Zeus is locked"),
      "a label node carries its text in `value`"
   );

   harness.get_by_role_and_label(Role::Button, "Unlock");
}

/// The unlock prompt is what the window is for: it takes the focus on the first frame it paints,
/// after the page heading behind it had already asked for it.
#[test]
fn the_unlock_vault_window_opens_on_its_own_title() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| {
      zeus.vault_exists = true;
      zeus.vault_unlocked = false;
   });

   // With a reader attached the app hands egui its focus model (`GUI::track_assistive_tech`);
   // the unlock title is a plain label, and a label is focusable only under that model.
   ctx.options_mut(|options| options.screen_reader = true);

   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_central(&ctx, &mut gui, &zeus, Vec::new());
   }

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.value(),
      Some("Unlock your Vault"),
      "the unlock prompt must take the focus from the page heading behind it"
   );
}

/// The auto-lock card is the only thing on screen: it opens on its own title instead of leaving
/// the focus on the page heading that held it when the lock fired.
#[test]
fn the_lock_screen_opens_on_its_own_title() {
   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);
   // With a reader attached the app hands egui its focus model (`GUI::track_assistive_tech`);
   // the card's title is a plain label, and a label is focusable only under that model.
   ctx.options_mut(|options| options.screen_reader = true);

   // The heading of the page the lock interrupted held the focus when the lock fired.
   frame(&ctx, |ui| {
      crate::gui::ui::common::heading(ui, "Home").request_focus();
   });

   render_unlock_screen(&ctx, &locked, &mut screen, &theme);

   let nodes = frame_nodes(&ctx, Vec::new(), |ui| {
      locked.write(|zeus| screen.show(zeus, &theme, ui));
   });
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.value(),
      Some("Zeus is locked"),
      "the lock card must take the focus from the page heading behind the lock"
   );
}
