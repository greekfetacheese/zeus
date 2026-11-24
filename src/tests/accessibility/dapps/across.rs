use super::super::app::{main_window, wallet_in_cache};
use super::super::harness::{drawn_names, frame_nodes, is_text_input, key_press};
use crate::core::ZeusCtx;
use crate::core::types::Recipient;
use crate::gui::GUI;
use crate::gui::ui::dapps::across::{
   ACROSS_RISK_TIP, ACROSS_RISK_WARNING, ApiResCache, ClientResponse, SuggestedFeesResponse,
};
use egui::accesskit::{Node, NodeId, Role};
use egui::{Context, Event, Key};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

/// Draw the Across bridge page the way the central panel does, and return its tree.
///
/// Twice: the page measures its frame on the first pass and draws into it on the second. `events`
/// go to the first pass only, so a key that closes a dialog is not pressed once per pass.
fn render_bridge_with(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         let icons = gui.icons.clone();
         let theme = gui.theme.clone();
         let bridge = &mut gui.across_bridge;
         let recipient_selection = &mut gui.recipient_selection;
         let contacts = &mut gui.settings.contacts_ui;

         zeus.write(|zeus| {
            bridge.show(
               zeus,
               &theme,
               icons,
               recipient_selection,
               contacts,
               ui,
            )
         });
      });
   }

   nodes
}

/// The page on its own, with nothing in its way.
fn render_bridge(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_bridge_with(ctx, gui, zeus, Vec::new())
}

/// Open the bridge page for a test that is not about the first-run API notice.
fn open_bridge_for_test(gui: &mut GUI) {
   gui.across_bridge.open();
   gui.across_bridge.set_api_notice_shown_for_test(true);
}

/// Draw the bridge page until its dialogs are painted rather than measured.
///
/// A modal spends its first passes measuring itself, and a key only belongs to a dialog that is
/// on screen.
fn settle_bridge(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) {
   render_bridge(ctx, gui, zeus);
   render_bridge(ctx, gui, zeus);
}

/// The fee card figures and wait, seeded the way a landed quote leaves them.
///
/// The wait comes out of the API cache and the three amounts out of the quote cache, which is
/// keyed to the form as it stands — so the fill time goes in first and the cache is seeded after.
fn seed_bridge_fee(
   gui: &mut GUI,
   zeus: &ZeusCtx,
   cost_usd: f64,
   bridge_fee: f64,
   total_fee: f64,
   fill_secs: u32,
) {
   let from_chain = gui.across_bridge.from_chain.chain.id();
   let to_chain = gui.across_bridge.to_chain.chain.id();

   gui.across_bridge.api_res_cache.insert(
      (from_chain, to_chain),
      ApiResCache {
         res: ClientResponse {
            origin_chain: from_chain,
            destination_chain: to_chain,
            suggested_fees: SuggestedFeesResponse {
               estimated_fill_time_sec: fill_secs,
               ..Default::default()
            },
            ..Default::default()
         },
         last_updated: None,
      },
   );

   zeus.write(|ctx| {
      gui.across_bridge.seed_quote_for_test(
         ctx,
         NumericValue::from_f64(cost_usd),
         NumericValue::from_f64(bridge_fee),
         NumericValue::from_f64(total_fee),
      )
   });
}

/// The card total line, as the tree carries it.
fn bridge_total(nodes: &[(NodeId, Node)]) -> String {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label)
      .filter_map(|(_, node)| node.value())
      .find(|text| text.starts_with("Total about $"))
      .map(str::to_owned)
      .expect("the fee card must print its total")
}

/// The header is the page title and its two warnings: a heading, a link whose name says where it
/// goes, the risk as an alert, and the hover-only tip on the chip that carries it.
#[test]
fn the_bridge_header_says_what_the_risk_is() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   open_bridge_for_test(&mut gui);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Heading && node.label() == Some("Bridge")),
      "the page title must be a heading: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Alert && node.label() == Some(ACROSS_RISK_WARNING)),
      "the risk warning must be an alert carrying its own words: {:?}",
      drawn_names(&nodes)
   );

   let chip = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Bridge risk"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the chip must be named for what it explains: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      chip.description(),
      Some(ACROSS_RISK_TIP),
      "the tip the chip only showed on hover belongs to the node"
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button
            && node.label() == Some("Across bridge settings")),
      "the image-only settings button must say what it opens: {:?}",
      drawn_names(&nodes)
   );

   let link = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Link)
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the powered-by line must be a link: {:?}",
            drawn_names(&nodes)
         )
      });

   let name = link.label().unwrap_or_default();

   assert!(
      name.contains("opens across.to in your browser"),
      "the link name must say where pressing it goes: {name:?}"
   );
}

/// The card is four figures with no words of their own: each says what it is, the wait says what
/// it counts, and a total that moves is said once.
#[test]
fn the_bridge_fee_card_says_what_each_number_is() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   open_bridge_for_test(&mut gui);

   seed_bridge_fee(&mut gui, &zeus, 0.01, 0.05, 0.06, 42);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let labels: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label)
      .filter_map(|(_, node)| node.value())
      .collect();

   for prefix in ["Network fee about $", "Bridge fee about $", "Total about $"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && node.value().is_some_and(|text| text.starts_with(prefix))),
         "the card must say what the {prefix:?} figure is: {labels:?}",
      );
   }

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("Estimated fill time: 42 seconds")),
      "the wait must say what it counts: {labels:?}"
   );

   let first = bridge_total(&nodes);

   // The first frame belongs to the block above the button, said once; the total it printed was
   // remembered, so the figure that moves next is the page news.
   gui.across_bridge.take_notice();

   seed_bridge_fee(&mut gui, &zeus, 0.01, 0.05, 0.07, 42);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);
   let second = bridge_total(&nodes);

   assert_ne!(
      first, second,
      "the seeded totals must differ, or this asserts nothing"
   );

   let figure = second.trim_start_matches("Total about ");
   let expected = format!("Bridge fee: total about {figure}.");

   assert_eq!(
      gui.across_bridge.take_notice().as_deref(),
      Some(expected.as_str()),
      "a total that moved must be said once"
   );

   render_bridge(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.across_bridge.take_notice(),
      None,
      "the same total a frame later is not said again"
   );
}

/// The reason a Bridge is blocked used to be the disabled button label, which no reader can
/// reach. It is a line of its own now, said once when it changes, and the button keeps its name.
#[test]
fn the_bridge_button_keeps_its_name_and_says_why_it_cannot_send() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   open_bridge_for_test(&mut gui);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let bridge = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Bridge"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the action button must be named for what it does: {:?}",
            drawn_names(&nodes)
         )
      });

   assert!(
      bridge.is_disabled(),
      "an empty amount leaves nothing to send, which is the state this test is about"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      spoken.contains(&"Enter Amount"),
      "the reason must be a node of its own, reachable while the button is not: {spoken:?}"
   );

   assert_eq!(
      gui.across_bridge.take_notice().as_deref(),
      Some("Enter Amount"),
      "the reason is said once when it appears"
   );

   render_bridge(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.across_bridge.take_notice(),
      None,
      "the same reason a frame later is not said again"
   );

   // A recipient resolved for another chain: the bridge can point the destination at it, and the
   // reason says so instead of only refusing.
   let elsewhere = ChainId::new(1).unwrap();

   gui.recipient_selection.recipient = Recipient {
      evm_address: Address::from([0x42; 20]).to_string(),
      chain: Some(elsewhere.id()),
      ..Default::default()
   };

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let reason = format!(
      "Bridge is off: recipient is on {}",
      elsewhere.name()
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      spoken.contains(&reason.as_str()),
      "the wrong-chain reason must be a line of its own: {spoken:?}"
   );

   assert_eq!(
      gui.across_bridge.take_notice().as_deref(),
      Some(reason.as_str()),
      "a reason that changes is a new sentence"
   );

   let fix = format!("Set destination to {}", elsewhere.name());

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(fix.as_str())),
      "the button that fixes the chain must stay where it is: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Bridge")
         && node.is_disabled()),
      "the button keeps its name whatever is blocking it: {:?}",
      drawn_names(&nodes)
   );
}

/// The settings dialog names the controls it draws, and the API URL is named after its host: a URL
/// can carry a provider key, so its text never enters the tree.
#[test]
fn the_across_settings_name_their_controls() {
   const API_URL: &str = "https://rpc.example-provider.dev/v3/secret-key-1234";

   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.across_bridge.open();
   gui.across_bridge.set_api_for_test(API_URL, false);
   gui.across_bridge.open_settings_for_test();

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let field = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role())
            && node.label() == Some("Across API URL at rpc.example-provider.dev")
      })
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the URL field must be named after its host: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      field.value(),
      None,
      "the URL text must not be read back to the user"
   );

   for (_, node) in &nodes {
      for text in [node.label(), node.value(), node.description()] {
         let text = text.unwrap_or_default();

         assert!(
            !text.contains(API_URL) && !text.contains("secret-key-1234"),
            "the URL must never reach the tree: {text:?}"
         );
      }
   }

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::CheckBox && node.label() == Some("Use API")),
      "the box that draws no words must be named: {:?}",
      drawn_names(&nodes)
   );

   let slider = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Slider && node.label() == Some("Fee to pay, in percent")
      })
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the fee slider must say what its number counts: {:?}",
            drawn_names(&nodes)
         )
      });

   let description = slider.description().unwrap_or_default();

   assert!(
      description.starts_with("Now ") && description.ends_with(" percent"),
      "the slider must carry the number it sets: {description:?}"
   );

   // The two captions the controls now say themselves leave the reading.
   for text in ["API URL", "Use API"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && node.is_hidden()
            && node.value() == Some(text)),
         "the drawn {text:?} must be hidden rather than read beside its control"
      );
   }
}

/// Escape is the second way out of the settings dialog: it closes without saving, so a keyboard
/// is not trapped into keeping an accidental edit.
#[test]
fn escape_closes_the_across_settings_without_saving() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.across_bridge.open();
   gui.across_bridge.open_settings_for_test();

   // The dialog is measured on its first passes, and the page behind it says its block once.
   settle_bridge(&ctx, &mut gui, &zeus);
   gui.across_bridge.take_notice();

   let drawn = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      drawn.iter().any(|(_, node)| is_text_input(node.role())
         && node.label().is_some_and(|label| label.starts_with("Across API URL at "))),
      "the settings dialog must be drawn before the key is pressed, or this asserts nothing"
   );

   let before = gui.across_bridge.settings_for_test();

   let nodes = render_bridge_with(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.across_bridge.settings_open,
      "one Escape must close the dialog: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      gui.across_bridge.settings_for_test(),
      before,
      "closing without saving must leave the settings as they were"
   );

   assert_eq!(
      gui.across_bridge.take_notice(),
      None,
      "leaving without saving is not a save"
   );
}

/// The first-run API notice was `.closable(false)`, so OK was the only exit and a keyboard was
/// trapped in it. Escape gives the same answer, and a dismissed notice does not come back.
#[test]
fn the_across_api_notice_closes_on_escape() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.across_bridge.open();
   gui.across_bridge.set_api_notice_shown_for_test(false);

   let notice = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      notice
         .iter()
         .any(|(_, node)| node.role() == Role::Dialog && node.label() == Some("Across API")),
      "the notice must be up before the key is pressed, or this asserts nothing: {:?}",
      drawn_names(&notice)
   );

   assert!(
      !gui.across_bridge.api_notice_shown_for_test(),
      "the notice must still be unread before the key is pressed"
   );

   settle_bridge(&ctx, &mut gui, &zeus);

   render_bridge_with(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      gui.across_bridge.api_notice_shown_for_test(),
      "one Escape must dismiss the notice the way OK does"
   );

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Dialog && node.label() == Some("Across API")),
      "a dismissed notice must not come back: {:?}",
      drawn_names(&nodes)
   );
}

// ---- Railgun shield --------------------------------------------------------
