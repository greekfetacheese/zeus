use super::app::{
   main_window, render_central_with_settings, render_nav, render_settings, settings_window,
   wallet_in_cache,
};
use super::harness::{accesskit_action, drawn_names, focused_node, key_press};
use crate::gui::Page;
use crate::gui::ui::settings::SettingsPage;
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Event, Key};

/// Settings is a window in the tree: one named window node a reader can hear, and the Close button
/// egui draws for it. Neither existed while Settings was a second OS window.
#[test]
fn the_settings_window_is_one_named_window() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   // The tree starts from the viewport's own window node; the one below it is Settings.
   let root = egui::accesskit_root_id().accesskit_id();
   let windows: Vec<&Node> = nodes
      .iter()
      .filter(|(id, node)| *id != root && node.role() == Role::Window)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      windows.len(),
      1,
      "Settings must be exactly one window node, got {}",
      windows.len()
   );

   let name = windows[0].label().or_else(|| windows[0].value());
   assert_eq!(
      name,
      Some("Settings"),
      "the window node must be named"
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some("Close window")),
      "the window must offer the Close button a reader can activate"
   );
}

/// Escape closes the window, and the close asks the left panel for the focus back so the Settings
/// plate is where a reader lands again.
#[test]
fn the_settings_window_closes_on_escape_and_says_so_to_the_left_panel() {
   let (ctx, mut gui, zeus) = settings_window();

   render_settings(&ctx, &mut gui, &zeus, Vec::new());
   assert!(
      gui.settings.is_open(),
      "the window must be drawn before it can close"
   );

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.settings.is_open(),
      "Escape must close the Settings window"
   );
   assert!(
      gui.settings.take_focus_return(),
      "a close the user asked for must ask for the focus back"
   );

   // The panel reads the request while it draws the plate, so close it once more with the flag
   // still set, then draw the nav.
   zeus.write(|zeus| gui.settings.open(zeus, &mut gui.appearance_settings));
   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   let nodes = render_nav(&ctx, &mut gui, &zeus, Vec::new());
   let settings_plate = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Settings"))
      .map(|(id, _)| *id)
      .expect("the Settings plate must be in the nav tree");

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(settings_plate),
      "the close must put the focus back on the Settings plate"
   );

   render_nav(&ctx, &mut gui, &zeus, Vec::new());
   assert_eq!(
      ctx.memory(|memory| memory.focused()),
      focused,
      "the plate must keep the focus on the next frame"
   );
}

/// The keyboard a closed window hands back is not marked: a band or an outline for a focus the
/// user did not reach for is the only thing the window would leave behind, so the plate it lands on
/// is drawn untouched until the user reaches for the keyboard.
#[test]
fn a_handed_back_focus_is_not_marked_until_the_keyboard_is_used() {
   let (ctx, mut gui, zeus) = settings_window();

   render_settings(&ctx, &mut gui, &zeus, Vec::new());
   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );
   assert!(
      !gui.settings.is_open(),
      "Escape must close the window"
   );

   // The left panel takes the request as it draws the plate that gets the focus back.
   render_nav(&ctx, &mut gui, &zeus, Vec::new());
   assert!(
      gui.nav_focus_quiet,
      "the focus the close handed back must be drawn unmarked"
   );

   render_nav(&ctx, &mut gui, &zeus, vec![key_press(Key::Tab)]);
   assert!(
      !gui.nav_focus_quiet,
      "the first key must bring the mark back"
   );
}

/// The plate a closed window hands the keyboard to must not go inert: a focused `Frame` paints its
/// press chrome over the hover colour, so while the focus sits there the entry never lights up under
/// the cursor. The pointer asking for anything gives that focus up, and with it the plate's own
/// hover and press chrome comes back.
#[test]
fn the_pointer_gives_the_handed_back_focus_up() {
   let (ctx, mut gui, zeus) = settings_window();

   render_settings(&ctx, &mut gui, &zeus, Vec::new());
   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   render_nav(&ctx, &mut gui, &zeus, Vec::new());
   assert!(
      gui.nav_focus_quiet,
      "the close must hand the focus back before the pointer can take it"
   );

   let pointer = Event::PointerMoved(egui::pos2(120.0, 240.0));
   render_nav(&ctx, &mut gui, &zeus, vec![pointer]);

   assert!(
      !gui.nav_focus_quiet,
      "the pointer must end the hand-back"
   );
   assert!(
      ctx.memory(|memory| memory.focused()).is_none(),
      "the plate must give the focus up so its own hover chrome is back"
   );
}

/// A locked wallet must not leave Settings usable: the window closes and nothing of it is drawn.
#[test]
fn a_locked_wallet_hides_the_settings_window() {
   let (ctx, mut gui, zeus) = settings_window();
   assert!(
      gui.settings.is_open(),
      "Settings must be open before the lock"
   );

   zeus.write(|zeus| zeus.locked = true);

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      !gui.settings.is_open(),
      "a locked wallet must close Settings"
   );

   let root = egui::accesskit_root_id().accesskit_id();
   assert!(
      !nodes.iter().any(|(id, node)| *id != root && node.role() == Role::Window),
      "no Settings window may be drawn while the wallet is locked"
   );
}

/// The Settings pages are a tab list: one `TabList` named after what the tabs switch between, seven
/// `Tab` nodes named exactly like the pages, only the open page's tab selected, and no page word
/// spoken a second time as the label inside a plate.
#[test]
fn every_settings_page_is_a_named_tab() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let lists: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::TabList)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      lists.len(),
      1,
      "the page switcher must be one tab list"
   );
   assert_eq!(
      lists[0].label(),
      Some("Settings pages"),
      "the tab list must name what the tabs switch between"
   );

   let names = [
      "General",
      "Appearance",
      "Networks",
      "Security",
      "Contacts",
      "Railgun",
      "Data",
   ];

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for &name in &names {
      let tabs: Vec<&Node> = nodes
         .iter()
         .filter(|(_, node)| node.role() == Role::Tab && node.label() == Some(name))
         .map(|(_, node)| node)
         .collect();

      assert_eq!(
         tabs.len(),
         1,
         "the {name} page must be exactly one tab named {name}"
      );

      assert_eq!(
         tabs[0].toggled() == Some(Toggled::True),
         name == "General",
         "only the open page's tab may report itself selected: {name}"
      );

      assert!(
         !spoken.contains(&name),
         "the label inside the {name} tab must be hidden, or the tab is spoken twice: {spoken:?}"
      );
   }
}

/// A reader opens a Settings page the way it opens any tab: a `Click` request on the tab's node. The
/// page changes, its heading names it, and a word only that page draws is in the tree.
#[test]
fn activating_a_settings_tab_opens_its_page() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let security = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Security"))
      .map(|(id, _)| *id)
      .expect("the Security page must be a tab in the tree");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, security)],
   );

   // The page heading is the one the page change focused: the page draws section headings of
   // its own now, and the tree hands its nodes back in no particular order.
   let focused = ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id());
   let heading = nodes
      .iter()
      .find(|(id, node)| node.role() == Role::Heading && Some(*id) == focused)
      .map(|(_, node)| node)
      .expect("the Security page must have a heading node");

   assert_eq!(
      heading.label(),
      Some("Security"),
      "the heading must name the page the tab opened"
   );

   // The page's words, whether they are drawn as prose or as a section heading.
   let texts: Vec<&str> = nodes
      .iter()
      .filter_map(|(_, node)| match node.role() {
         Role::Label => node.value(),
         Role::Heading => node.label(),
         _ => None,
      })
      .collect();

   assert!(
      texts.contains(&"Change Credentials"),
      "the Security page must draw its own contents: {texts:?}"
   );
}

/// Opening Settings lands on the page switcher, the window's first control, so the first Tab walks
/// the other pages instead of diving into the page body. Changing the page lands on the new page's
/// heading, and that focus move is how the page's name reaches a reader.
#[test]
fn the_page_switcher_opens_and_a_page_change_names_the_page() {
   let (ctx, mut gui, zeus) = settings_window();

   let first = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let focused = focused_node(&ctx, &first);
   assert_eq!(
      focused.role(),
      Role::Tab,
      "opening must land on the switcher, not inside the page"
   );
   assert_eq!(
      focused.label(),
      Some("General"),
      "the switcher opens on the page in front"
   );
   assert_eq!(
      focused.toggled(),
      Some(Toggled::True),
      "the page in front must report itself selected"
   );

   let appearance = first
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Appearance"))
      .map(|(id, _)| *id)
      .expect("the Appearance page must be a tab in the tree");

   let second = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, appearance)],
   );

   let focused = ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id());
   let (id, node) = second
      .iter()
      .find(|(id, node)| node.role() == Role::Heading && Some(*id) == focused)
      .expect("the new page must have a heading node");

   assert_eq!(
      node.label(),
      Some("Appearance"),
      "the heading names the page that just opened"
   );

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(*id),
      "the page change must move focus to the new page's heading"
   );
}

/// The window holds the keyboard. Once a reader had walked every control of the open page, the next
/// Tab handed the keyboard back to the page behind — the chain select of the page that opened
/// Settings — and the reader left the window without asking to. The walk now comes back to the page
/// switcher in the same press, and never touches a layer under the window.
#[test]
fn the_settings_window_keeps_the_keyboard_when_the_page_runs_out() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   zeus.write(|zeus| {
      crate::gui::ui::panels::left_panel::open_page(&mut gui, zeus, Page::Send);
   });
   zeus.write(|zeus| gui.settings.open(zeus, &mut gui.appearance_settings));

   // An `egui::Window` is measured on its first pass; the keyboard is on the switcher after both.
   render_central_with_settings(&ctx, &mut gui, &zeus, Vec::new());
   render_central_with_settings(&ctx, &mut gui, &zeus, Vec::new());

   let mut window_layer = None;
   let mut saw_switcher = false;
   let mut saw_page = false;

   for _ in 0..40 {
      let nodes = render_central_with_settings(&ctx, &mut gui, &zeus, vec![key_press(Key::Tab)]);
      let focused = focused_node(&ctx, &nodes);

      // The window claims the modal layer, so every widget of the page behind it stops wanting the
      // keyboard; a Tab that leaves the window shows up here as a different layer.
      let layer = ctx
         .memory(|memory| memory.focused())
         .and_then(|id| ctx.read_response(id))
         .map(|response| response.layer_id)
         .expect("the frame must leave the keyboard on a widget");

      assert_eq!(
         *window_layer.get_or_insert(layer),
         layer,
         "Tab left the Settings window and landed on {:?}",
         focused.label().or_else(|| focused.value())
      );

      match focused.role() {
         Role::Tab => saw_switcher = true,
         Role::CheckBox | Role::Slider | Role::Button => saw_page = true,
         _ => {}
      }
   }

   assert!(
      saw_switcher,
      "the walk must come back to the page switcher"
   );
   assert!(
      saw_page,
      "the walk must cover the open page, or it proves nothing about running out of it"
   );
}

/// Every Settings page draws when its tab is opened: the heading names the page for all seven, so a
/// page that stops being drawn fails here without asserting any one page's contents.
#[test]
fn every_settings_page_appears_in_the_tree() {
   let (ctx, mut gui, zeus) = settings_window();

   for name in [
      "General",
      "Appearance",
      "Networks",
      "Security",
      "Contacts",
      "Railgun",
      "Data",
   ] {
      let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
      let tab = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(name))
         .map(|(id, _)| *id)
         .unwrap_or_else(|| panic!("the {name} page must be a tab in the tree"));

      let nodes = render_settings(
         &ctx,
         &mut gui,
         &zeus,
         vec![accesskit_action(Action::Click, tab)],
      );

      // The page heading is the one the page change focused.
      let focused = ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id());
      let heading = nodes
         .iter()
         .find(|(id, node)| node.role() == Role::Heading && Some(*id) == focused)
         .map(|(_, node)| node)
         .unwrap_or_else(|| panic!("the {name} page must draw a heading"));

      assert_eq!(
         heading.label(),
         Some(name),
         "the heading must name the page the tab opened"
      );
   }
}

/// The Appearance page is the theme, the ambient particles and the hold-to-confirm gesture: how
/// Zeus looks and feels. A reader is not offered it, the page it was on falls back to General, and
/// the tray option that cannot work for a reader goes with it.
#[test]
fn a_reader_is_not_offered_the_appearance_page() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Appearance,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let tabs = |nodes: &[(NodeId, Node)]| -> Vec<String> {
      nodes
         .iter()
         .filter(|(_, node)| node.role() == Role::Tab)
         .filter_map(|(_, node)| node.label())
         .map(str::to_string)
         .collect()
   };

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      tabs(&nodes).contains(&"Appearance".to_string()),
      "a sighted user keeps the page: {:?}",
      tabs(&nodes)
   );

   gui.settings.set_reader_attached(true);

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      !tabs(&nodes).contains(&"Appearance".to_string()),
      "a reader must not be offered the page: {:?}",
      tabs(&nodes)
   );

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert!(
      headings.contains(&"General"),
      "the page a reader cannot be offered must fall back to General: {headings:?}"
   );

   assert!(
      !headings.contains(&"Appearance"),
      "the Appearance page must not stay in front of a reader: {headings:?}"
   );

   assert!(
      !nodes.iter().any(|(_, node)| node.role() == Role::CheckBox
         && node.label() == Some("Minimize to Tray")),
      "a reader must not be offered a tray option that cannot work for them: {:?}",
      drawn_names(&nodes)
   );
}
