use super::super::app::{main_window, wallet_in_cache};
use super::super::harness::{
   accesskit_action, drawn_names, frame_nodes, is_text_input, spoken_text,
};
use super::super::settings_pages::host_of;
use crate::core::ZeusCtx;
use crate::core::types::Recipient;
use crate::gui::GUI;
use crate::gui::ui::dapps::railgun::{
   RailgunMode,
   merge_notes::MERGE_STARTED,
   shield::{
      PAYMASTER_TIP, SELF_BROADCAST_TIP, SHIELD_DIRECTION, UNSHIELD_DIRECTION, UNWRAP_TO_ETH_TIP,
   },
};
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;
use zeus_railgun::privacy::RiskBand;

/// The shield form with Railgun enabled and synced on chain 1, so the card draws instead of the
/// "Railgun is disabled" notice. The currency is the one the frame picks for the mode.
fn shield_form(mode: RailgunMode) -> (Context, GUI, ZeusCtx) {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   zeus.write(|ctx| {
      ctx.railgun_config.set_enabled(1, true);
      ctx.railgun_status.set_synced(1, true);
   });

   gui.shield_ui.open(mode);
   gui.shield_ui.default_currency(1);

   (ctx, gui, zeus)
}

/// Render the shield page headlessly, twice: an egui area is measured on its first pass and a modal
/// spends one before it is painted.
fn render_shield_page(
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

      let icons = gui.icons.clone();
      let theme = gui.theme.clone();
      let shield = &mut gui.shield_ui;
      let token_selection = &mut gui.token_selection;
      let recipient_selection = &mut gui.recipient_selection;
      let contacts = &mut gui.settings.contacts_ui;

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| {
            shield.show(
               zeus,
               &theme,
               icons.clone(),
               token_selection,
               recipient_selection,
               contacts,
               ui,
            )
         });
      });
   }

   nodes
}

/// Render the merge card headlessly, twice, for the same reason as the shield page.
fn render_merge_notes(
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

      let theme = gui.theme.clone();
      let window = &mut gui.merge_notes_window;

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| window.show(zeus, &theme, ui));
      });
   }

   nodes
}

/// Draw the merge card until it is painted rather than measured.
fn settle_merge_notes(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_merge_notes(ctx, gui, zeus, Vec::new());
   render_merge_notes(ctx, gui, zeus, Vec::new())
}

/// Draw the shield page until the dialog it was opened on is painted rather than measured.
fn settle_shield_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_shield_page(ctx, gui, zeus, Vec::new());
   render_shield_page(ctx, gui, zeus, Vec::new())
}

/// The form's title is the page heading, and the one word it draws does not say which way the funds
/// move: the direction is the heading's description.
#[test]
fn the_shield_page_says_which_direction_it_moves() {
   // Case: the_shield_title_says_which_way_the_funds_move
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Shield);
   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let heading = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Heading && node.label() == Some("Shield"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the form's title must be a heading named Shield: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      heading.description(),
      Some(SHIELD_DIRECTION),
      "a shield must say that it moves funds into the private pool"
   );

   // Case: the_unshield_title_says_which_way_the_funds_move
   gui.shield_ui.open(RailgunMode::Unshield);
   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let heading = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Heading && node.label() == Some("Unshield"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the unshield form's title must be a heading: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      heading.description(),
      Some(UNSHIELD_DIRECTION),
      "an unshield must say that it moves funds out of the private pool"
   );
}

/// Both badges on the broadcast row carry their warning only as hover text, which a keyboard never
/// reaches: the name says what the badge costs, and the tip is the node's description.
#[test]
fn the_broadcast_badges_say_what_they_cost() {
   // Case: the_self_broadcast_badge_carries_its_warning
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);
   gui.shield_ui.set_switches_for_test(true, false);

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let badge = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Self-broadcast, breaks anonymity"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the self-broadcast badge must say what it costs in its name: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      badge.description(),
      Some(SELF_BROADCAST_TIP),
      "the warning only a hover answered belongs to the node"
   );

   // Case: the_unwrap_badge_carries_its_tip
   gui.shield_ui.set_switches_for_test(false, true);

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let badge = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Unwrap to ETH"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the unwrap badge must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      badge.description(),
      Some(UNWRAP_TO_ETH_TIP),
      "the unwrap tip belongs to the badge, not to a hover"
   );
}

/// The broadcast options dialog names the two boxes it draws, and the bundler URL is named after
/// its host: a URL can carry a provider key, so its text never enters the tree.
#[test]
fn the_broadcast_options_dialog_names_its_fields() {
   const BUNDLER_URL: &str = "https://rpc.example-bundler.dev/v1/secret-key-9";

   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);
   gui.shield_ui.set_bundler_url(BUNDLER_URL.to_string());
   gui.shield_ui.open_broadcast_options_for_test();

   let nodes = settle_shield_page(&ctx, &mut gui, &zeus);

   let memo = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Memo, optional"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the memo box must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      memo.value(),
      Some(""),
      "the memo is the user's text, and it is empty until they type"
   );

   let host = host_of(BUNDLER_URL);
   let expected = format!("Bundler URL at {host}");
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some(expected.as_str()))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the bundler field must be named after its host: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      field.value(),
      None,
      "the URL text must not be read back to the user"
   );

   let description = field.description().unwrap_or_default();

   assert!(
      description.contains("provider key"),
      "the field must say why its text is withheld: {description:?}"
   );

   for (_, node) in &nodes {
      for text in [node.label(), node.value(), node.description()] {
         let text = text.unwrap_or_default();

         assert!(
            !text.contains(BUNDLER_URL) && !text.contains("secret-key-9"),
            "the URL must never reach the tree: {text:?}"
         );
      }
   }

   let help = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Bundler URL help"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the help chip must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      help.description(),
      Some(PAYMASTER_TIP),
      "the chip's tip is the node's description, not only a hover"
   );
}

/// The reason a shield is blocked used to be the disabled button's own label, which no reader can
/// reach. It is a line of its own now, said once when it appears, and the button keeps its name.
#[test]
fn the_shield_button_keeps_its_name_and_says_why_it_is_blocked() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Shield);

   // One ETH typed with nothing to spend it: the balance is what is missing, and the private
   // recipient is there so nothing above it is what blocks the button.
   gui.shield_ui.type_amount_for_test("1");
   gui.recipient_selection.recipient = Recipient {
      zk_address: "0zk1qexample".to_string(),
      ..Default::default()
   };

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let reason = "Insufficient ETH Balance";

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Shield")
         && node.is_disabled()),
      "the button keeps the name of what it does, whatever is blocking it: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      spoken_text(&nodes).contains(&reason),
      "the reason must be a node of its own, reachable while the button is not: {:?}",
      spoken_text(&nodes)
   );

   assert_eq!(
      gui.shield_ui.take_notice().as_deref(),
      Some(reason),
      "the reason is said once when it appears"
   );

   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.shield_ui.take_notice(),
      None,
      "the same reason a frame later is not said again"
   );

   // The chain falls out of sync: the reason changes with it, and is said again.
   zeus.write(|ctx| ctx.railgun_status.set_synced(1, false));

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      spoken_text(&nodes).contains(&"Waiting for Railgun to sync"),
      "an unsynced chain is said in the form's own words: {:?}",
      spoken_text(&nodes)
   );

   assert_eq!(
      gui.shield_ui.take_notice().as_deref(),
      Some("Waiting for Railgun to sync"),
      "a reason that changes is a new sentence"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Shield")
         && node.is_disabled()),
      "and the button still says what it does: {:?}",
      drawn_names(&nodes)
   );
}

/// The merge card is drawn with the sentences it will act on, and it closes on the press that
/// starts the work — so the press is what says the merge began.
#[test]
fn the_merge_window_says_what_it_will_merge() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.merge_notes_window.seed_notes_for_test(&[
      1_000_000_000_000_000_000,
      2_000_000_000_000_000_000,
      3_000_000_000_000_000_000,
   ]);

   let nodes = settle_merge_notes(&ctx, &mut gui, &zeus);
   let texts = spoken_text(&nodes);

   assert!(
      texts.contains(&"Merge 3 notes into 1 note (proof circuit 03x01)"),
      "the card must say how many notes it merges and into what: {texts:?}"
   );

   for leaf in 0..3 {
      let prefix = format!("Note {}: ", leaf + 1);

      assert!(
         texts.iter().any(|text| text.starts_with(&prefix)),
         "each note must be named rather than numbered from the list: {texts:?}"
      );
   }

   assert!(
      texts.iter().any(|text| text.ends_with("(leaf 0)")),
      "a note still names the leaf it sits in: {texts:?}"
   );

   // The tree has nothing to merge: the card says so rather than drawing an empty list.
   gui.merge_notes_window.seed_no_merge_for_test();
   let nodes = settle_merge_notes(&ctx, &mut gui, &zeus);
   let texts = spoken_text(&nodes);

   assert!(
      texts.contains(&"No merge needed. This tree already has at most one note for this token."),
      "the nothing-to-do answer must be a sentence: {texts:?}"
   );

   // Back to a merge, and press it: the card goes and the start is said.
   gui.merge_notes_window
      .seed_notes_for_test(&[1_000_000_000_000_000_000, 2_000_000_000_000_000_000]);

   let nodes = settle_merge_notes(&ctx, &mut gui, &zeus);

   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm Merge"))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the card must offer its confirm button: {:?}",
            drawn_names(&nodes)
         )
      });

   render_merge_notes(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, confirm)],
   );

   assert_eq!(
      gui.merge_notes_window.take_notice().as_deref(),
      Some(MERGE_STARTED),
      "pressing Confirm Merge closes the card, so the press must say the merge began"
   );

   assert!(
      !gui.merge_notes_window.is_open(),
      "the work continues with no card on screen"
   );
}

/// The privacy band lands after the amount has settled, while the reader is still in the amount
/// field: it is said once when it changes, and never again for the same verdict.
#[test]
fn the_privacy_verdict_is_said_when_it_changes() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);

   let owner = zeus.read(|ctx| ctx.current_wallet_info().address);
   let recipient = Address::from([0x42; 20]);

   gui.recipient_selection.recipient = Recipient {
      evm_address: recipient.to_string(),
      ..Default::default()
   };
   gui.shield_ui.type_amount_for_test("1.02");

   // The form as it stands is blocked from the first frame — there is nothing private to spend —
   // and that sentence is drained first, so the band is the only new thing on the frame it lands.
   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());
   gui.shield_ui.take_notice();

   gui.shield_ui.seed_privacy_band_for_test(
      RiskBand::High,
      "1.02",
      ChainId::new(1).unwrap(),
      owner,
      recipient,
   );

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      spoken_text(&nodes).contains(&"Amount privacy risk: High"),
      "the band must still be drawn for a reader who reaches the block: {:?}",
      spoken_text(&nodes)
   );

   assert_eq!(
      gui.shield_ui.take_notice().as_deref(),
      Some("Privacy risk: High."),
      "a band that arrives is said once"
   );

   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.shield_ui.take_notice(),
      None,
      "the same band a frame later is not said again"
   );
}

/// The pool is thin: the sentences are drawn under a form the reader never left, so the first of
/// them is said once, and a set that does not change says nothing more.
#[test]
fn the_pool_warnings_are_said_when_they_change() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Shield);

   // The button's own block is the first sentence drained, so the pool's is the only new one.
   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());
   gui.shield_ui.take_notice();

   gui.shield_ui.seed_pool_warnings_for_test(ChainId::new(1).unwrap());

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());
   let spoken = spoken_text(&nodes);

   let notice = gui
      .shield_ui
      .take_notice()
      .expect("the first pool warning is the news of the frame it appears on");

   assert!(
      spoken.contains(&notice.as_str()),
      "the sentence said must be the one the block draws: {notice:?} not in {spoken:?}"
   );

   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.shield_ui.take_notice(),
      None,
      "the same set of warnings is not said again"
   );
}

/// The tips are drawn with a literal bullet, which a reader would say out loud: the row keeps the
/// bullet and the node is named with the sentence alone.
#[test]
fn the_priv_tips_have_no_bullet_in_their_names() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);
   gui.shield_ui.open_privacy_tips_for_test();

   let nodes = settle_shield_page(&ctx, &mut gui, &zeus);

   for (_, node) in &nodes {
      for text in [node.label(), node.value()] {
         let text = text.unwrap_or_default();

         assert!(
            !text.starts_with('•'),
            "a reader must not be read the bullet: {text:?}"
         );
      }
   }

   // The tips are the only long prose this page draws with no verdict: one IP tip and three
   // habits, each of them a sentence.
   let tips: Vec<&str> = nodes
      .iter()
      .filter_map(|(_, node)| node.label())
      .filter(|name| name.chars().count() > 60)
      .collect();

   assert_eq!(
      tips.len(),
      4,
      "the tips dialog must draw four tips: {tips:?}"
   );

   for tip in tips {
      assert!(
         tip.starts_with(char::is_alphabetic),
         "a tip must start with its first word: {tip:?}"
      );
   }
}
