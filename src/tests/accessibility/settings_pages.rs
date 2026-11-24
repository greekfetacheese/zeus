use super::app::{open_settings_page, render_settings, settings_window};
use super::harness::{accesskit_action, frame, key_press, status_text};
use crate::core::ZeusCtx;
use crate::core::types::RailgunConfig;
use crate::gui::GUI;
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event, Key};

/// Open the Appearance page the way a reader does — an AccessKit Click on its tab — and return the
/// tree of the page that opened.
fn open_appearance_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let appearance = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Appearance"))
      .map(|(id, _)| *id)
      .expect("the Appearance page must be a tab in the tree");

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, appearance)],
   )
}

/// Every section the General page draws is a Heading node named exactly like the words it draws,
/// so a reader can jump between sections instead of walking every control. The page's own heading
/// is still there beside them.
#[test]
fn the_general_page_headings_are_headings() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   #[cfg(target_os = "linux")]
   let sections = [
      "External Data",
      "Window",
      "Desktop",
      "Pool Manager",
      "Balance Manager",
   ];
   #[cfg(not(target_os = "linux"))]
   let sections = ["External Data", "Window", "Pool Manager", "Balance Manager"];

   for section in sections {
      assert!(
         headings.contains(&section),
         "the {section} section title must be a heading node: {headings:?}"
      );
   }

   assert!(
      headings.contains(&"General"),
      "the page heading must still be there: {headings:?}"
   );
}

/// Each slider is one node that says what it sets and which range it covers: the sentence drawn
/// above it belongs to the slider, not to a sibling label a reader has to tie to it on its own.
#[test]
fn each_slider_says_what_it_sets() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let sliders: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Slider)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      sliders.len(),
      4,
      "the General page draws four sliders"
   );

   let sentences = [
      (
         "Concurrency for Discovering & Updating Pools",
         "1 to 10",
      ),
      ("Batch Size when updating pools state", "1 to 50"),
      ("Concurrency for syncing balances", "1 to 10"),
      ("Batch Size for syncing balances", "1 to 50"),
   ];

   for (sentence, range) in sentences {
      let slider = sliders
         .iter()
         .find(|node| node.label() == Some(sentence))
         .unwrap_or_else(|| panic!("the {sentence} slider must be named with its sentence"));

      assert_eq!(
         slider.description(),
         Some(range),
         "the {sentence} slider must say the range it covers"
      );
      assert!(
         slider.numeric_value().is_some(),
         "the {sentence} slider must still report the value the crate gave it"
      );
   }

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for (sentence, _) in sentences {
      assert!(
         !spoken.contains(&sentence),
         "the {sentence} label must be hidden, or the slider is spoken twice: {spoken:?}"
      );
   }
}

/// The (?) chip is not a destination of its own: its tip is the control's description, and the
/// chip is hidden so a reader hears the explanation beside the checkbox instead of hunting for a
/// question mark after it.
#[test]
fn the_help_text_reaches_the_control_it_explains() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let updates = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::CheckBox && node.label() == Some("Check for Updates"))
      .map(|(_, node)| node)
      .expect("the Check for Updates box must be in the tree");

   let prefix = "Allow Zeus to check GitHub for a newer Zeus release";
   let description = updates.description().unwrap_or_default();

   assert!(
      description.starts_with(prefix),
      "the checkbox must carry the tip as its description: {description:?}"
   );

   let questions: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .filter(|text| *text == "?")
      .collect();

   assert!(
      questions.is_empty(),
      "no question-mark chip may be left in the tree: {questions:?}"
   );
}

/// The Appearance page is two collapsible sections a reader can name and open: a named disclosure
/// node per title that reports its state, closed on arrival, with the page's own heading still
/// beside them.
#[test]
fn the_appearance_page_is_two_named_sections() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_appearance_page(&ctx, &mut gui, &zeus);

   for title in ["Hold to confirm", "Ambient"] {
      let header = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::DisclosureTriangle && node.label() == Some(title))
         .map(|(_, node)| node)
         .unwrap_or_else(|| panic!("the {title} section must be a named disclosure node"));

      assert_eq!(
         header.is_expanded(),
         Some(false),
         "the {title} section must start closed"
      );
   }

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert!(
      headings.contains(&"Appearance"),
      "the page heading must still be there: {headings:?}"
   );
}

/// A control row is one node: the sentence the page draws in its label cell is the slider's own
/// name, its "(?)" tip is the description, and the value the crate reports is still there.
#[test]
fn a_slider_in_the_appearance_page_says_what_it_sets() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_appearance_page(&ctx, &mut gui, &zeus);
   let hold = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::DisclosureTriangle && node.label() == Some("Hold to confirm")
      })
      .map(|(id, _)| *id)
      .expect("the Hold to confirm section must be a named disclosure node");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, hold)],
   );

   let slider = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Duration"))
      .map(|(_, node)| node)
      .expect("the hold duration row must be a slider named after its word");

   let description = slider.description().unwrap_or_default();

   assert!(
      description.starts_with("How long the affirmative button must be held"),
      "the slider must carry the row's tip as its description: {description:?}"
   );
   assert!(
      slider.numeric_value().is_some(),
      "the slider must still report the value the crate gave it"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      !spoken.contains(&"Duration"),
      "the row's word must be hidden, or the slider is spoken twice: {spoken:?}"
   );
}

/// A colour well is named after the row that draws it, and no "(?)" chip is left as a destination of
/// its own: the tip is the well's description and the chip leaves the tree.
#[test]
fn a_colour_well_says_what_it_colours() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_appearance_page(&ctx, &mut gui, &zeus);
   let hold = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::DisclosureTriangle && node.label() == Some("Hold to confirm")
      })
      .map(|(id, _)| *id)
      .expect("the Hold to confirm section must be a named disclosure node");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, hold)],
   );

   let well = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::ColorWell && node.label() == Some("End colour"))
      .map(|(_, node)| node)
      .expect("the End colour well must be named after its row");

   let description = well.description().unwrap_or_default();

   assert!(
      description.starts_with("Colour the sweep ends at"),
      "the well must carry the row's tip as its description: {description:?}"
   );

   // Only a row whose control names itself keeps its question mark: the "Kind" picker is the one
   // combo box in this section, and the crate draws its plate, so its tip has nowhere else to go.
   let questions: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .filter(|text| *text == "?")
      .collect();

   assert_eq!(
      questions,
      vec!["?"],
      "only the row the page cannot name keeps its question mark"
   );
}

/// Reset writes the defaults back and says so: the page raises the words, the app drains them
/// right after the window is drawn, and the announcer speaks them in the same frame.
#[test]
fn resetting_the_pool_and_balance_settings_says_so() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let reset = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Reset Settings"))
      .map(|(id, _)| *id)
      .expect("the General page must offer the Reset Settings button");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, reset)],
   );

   let text = gui.settings.take_notice().expect("a reset must raise the words the app says");

   assert_eq!(
      text,
      "Pool and balance settings reset to their defaults."
   );

   let nodes = frame(&ctx, |ui| {
      gui.announce(text.as_str());
      gui.show_announcer(ui);
   });

   assert_eq!(
      status_text(&nodes).as_deref(),
      Some("Pool and balance settings reset to their defaults."),
      "the reset's words must reach the announcement node"
   );
}

/// A save made by leaving the page is spoken too: a keyboard-changed slider is written back when
/// the General page hands over, and the window raises the words for the app to announce.
#[test]
fn leaving_the_general_page_speaks_the_save() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let (slider, before) = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Slider
            && node.label() == Some("Concurrency for Discovering & Updating Pools")
      })
      .map(|(id, node)| (*id, node.numeric_value()))
      .expect("the first General slider must be in the tree");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Focus, slider)],
   );

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::ArrowRight)],
   );

   let after = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Slider
            && node.label() == Some("Concurrency for Discovering & Updating Pools")
      })
      .and_then(|(_, node)| node.numeric_value());

   assert_eq!(
      after,
      before.map(|value| value + 1.0),
      "ArrowRight on the focused slider must move its value by one step"
   );

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let security = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Security"))
      .map(|(id, _)| *id)
      .expect("the Security tab must be in the tree");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, security)],
   );

   assert_eq!(
      gui.settings.take_notice().as_deref(),
      Some("Settings saved."),
      "leaving the page must speak the save it made"
   );
}

/// The Security page's three section titles are heading nodes as well.
#[test]
fn the_security_page_headings_are_headings() {
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

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   for section in ["Auto-Lock", "Change Credentials", "Vault encryption"] {
      assert!(
         headings.contains(&section),
         "the {section} section title must be a heading node: {headings:?}"
      );
   }
}

/// Open the Networks page the way a reader does — an AccessKit Click on its tab — and return the
/// tree of the page that opened.
fn open_networks_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let networks = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Networks"))
      .map(|(id, _)| *id)
      .expect("the Networks page must be a tab in the tree");

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, networks)],
   )
}

/// The host a row may be named by: what sits between `://` and the first path or query.
pub(super) fn host_of(url: &str) -> &str {
   url.split_once("://")
      .map(|(_, rest)| rest)
      .unwrap_or(url)
      .split(['/', '?'])
      .next()
      .unwrap_or(url)
}

/// The Networks page names itself and the controls it draws: the page heading, the chain toggle
/// that says which way a press flips the chain, the way into the add-RPC form, and the concurrency
/// field with the button that applies it.
#[test]
fn the_networks_page_names_its_controls() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_networks_page(&ctx, &mut gui, &zeus);

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert!(
      headings.contains(&"Networks"),
      "the page heading must name the page: {headings:?}"
   );

   // The drawn word flips with the chain; the name has to match the action the press performs.
   let disabled = zeus.read(|zeus| zeus.is_chain_disabled(1));
   let toggle = if disabled {
      "Enable this network"
   } else {
      "Disable this network"
   };

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(toggle)),
      "the chain toggle must be named after the action it performs ({toggle})"
   );

   for name in ["Add RPC Url", "Apply concurrency"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(name)),
         "the {name} button must be a named node in the tree"
      );
   }

   let concurrency = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::TextInput && node.label() == Some("Request Concurrency")
      })
      .map(|(_, node)| node)
      .expect("the concurrency field must be named after the sentence beside it");

   let description = concurrency.description().unwrap_or_default();

   assert!(
      description.contains("How many requests Zeus may run at once"),
      "the field must carry the range hint as its description: {description:?}"
   );

   // A row names its own controls: the URL field, the box that enables it, and the three actions.
   let rpcs = zeus.read(|zeus| zeus.client_manager.get_rpcs(1));
   let url = rpcs
      .values()
      .next()
      .map(|rpc| rpc.url.to_string())
      .expect("the default chain must ship at least one endpoint");
   let host = host_of(&url);

   for name in [
      format!("RPC URL at {host}"),
      format!("Enabled, {host}"),
      format!("Endpoint settings for {host}"),
      format!("Test {host}"),
      format!("Remove {host}"),
   ] {
      assert!(
         nodes.iter().any(|(_, node)| node.label() == Some(name.as_str())),
         "the row for {host} must carry a {name} node"
      );
   }
}

/// A row is identified without its URL: the URL text never enters the tree, and the row's host —
/// the part that holds no provider key — is what names the row's controls.
#[test]
fn no_rpc_url_reaches_the_tree() {
   let (ctx, mut gui, zeus) = settings_window();

   let rpcs = zeus.read(|zeus| zeus.client_manager.get_rpcs(1));
   let urls: Vec<String> = rpcs.values().map(|rpc| rpc.url.to_string()).collect();

   assert!(
      !urls.is_empty(),
      "the headless profile must ship the built-in endpoints this test reads"
   );

   let nodes = open_networks_page(&ctx, &mut gui, &zeus);

   for (_, node) in &nodes {
      for text in [node.label(), node.value(), node.description()] {
         for url in &urls {
            assert!(
               !text.unwrap_or_default().contains(url.as_str()),
               "a full URL must never reach the tree: {url:?} is in {text:?}"
            );
         }
      }
   }

   let field = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::TextInput
            && node.label().is_some_and(|label| label.starts_with("RPC URL at "))
      })
      .map(|(_, node)| node)
      .expect("a row's URL field must be named after its host");

   assert_eq!(
      field.value(),
      None,
      "the row's URL field must carry no value a reader could hear"
   );

   let names: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.label()).collect();

   for url in &urls {
      let host = host_of(url);

      assert!(
         names.iter().any(|name| name.contains(host)),
         "the row for {host} must be named after its host: {names:?}"
      );
   }
}

/// The add-RPC form draws its error inline instead of opening a dialog, so the page raises the
/// words through the window's announcement channel on the frame the line starts showing.
#[test]
fn an_invalid_rpc_url_is_said() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_networks_page(&ctx, &mut gui, &zeus);
   let add_url = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Add RPC Url"))
      .map(|(id, _)| *id)
      .expect("the page must offer the Add RPC Url button");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, add_url)],
   );

   let field = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::TextInput && node.label() == Some("RPC URL"))
      .map(|(id, _)| *id)
      .expect("the add-RPC form must name its URL field");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Focus, field)],
   );

   // "Add" stays disabled while the text is not a URL, so the error is the form's own line: it is
   // spoken the frame it appears rather than waiting for a press that cannot happen.
   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![Event::Text("not-a-url".to_owned())],
   );

   assert_eq!(
      gui.settings.take_notice().as_deref(),
      Some("Invalid URL"),
      "an invalid URL must be said, not only drawn"
   );
}

/// Each Railgun slider is one node named with the sentence that used to sit above it, says the
/// range it covers, and keeps the value the crate gave it; the sentence is not left behind as a
/// label too.
#[test]
fn the_railgun_page_names_its_sliders() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_settings_page(&ctx, &mut gui, &zeus, "Railgun");

   let sliders: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Slider)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      sliders.len(),
      3,
      "the Railgun page draws three sliders"
   );

   let block_range = format!(
      "{} to {}",
      RailgunConfig::min_block_range(),
      RailgunConfig::max_block_range()
   );
   let concurrency = format!(
      "{} to {}",
      RailgunConfig::min_concurrency(),
      RailgunConfig::max_concurrency()
   );
   let interval = format!(
      "{} to {}",
      RailgunConfig::min_state_update_interval(),
      RailgunConfig::max_state_update_interval()
   );

   let ranges = [
      ("RPC Syncer Block Range", block_range),
      ("RPC Syncer Concurrency", concurrency),
      ("State Update Interval (Minutes)", interval),
   ];

   for (sentence, range) in &ranges {
      let slider = sliders
         .iter()
         .find(|node| node.label() == Some(*sentence))
         .unwrap_or_else(|| panic!("the {sentence} slider must be named with its sentence"));

      assert_eq!(
         slider.description(),
         Some(range.as_str()),
         "the {sentence} slider must say the range it covers"
      );
      assert!(
         slider.numeric_value().is_some(),
         "the {sentence} slider must still report the value the crate gave it"
      );
   }

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for (sentence, _) in &ranges {
      assert!(
         !spoken.contains(sentence),
         "the {sentence} label must be hidden, or the slider is spoken twice: {spoken:?}"
      );
   }

   for name in ["Save", "Reset"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(name)),
         "the Railgun page must still offer the {name} button"
      );
   }
}
