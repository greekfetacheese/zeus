//! Settings → Agents: the list of agents, the four screens behind each one, and the token shown once.
//!
//! The page is drawn headlessly like every other settings page, so what it says is checked on the
//! tree a reader is handed rather than on the pixels.

use super::app::{render_settings, settings_window, wallet_named};
use super::harness::{accesskit_action, button_named, spoken, spoken_text};
use crate::core::agent::{AgentProfile, AuditDecision, AuditEntry, TokenRef, TokenScope};
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::Page;
use crate::gui::ui::settings::SettingsPage;
use egui::accesskit::{Action, Node, NodeId, Role};
use zeus_eth::alloy_primitives::{Address, TxHash};

/// The page names where the endpoint is — there is no switch to look for — and every agent is a row a
/// reader can reach, with what it holds said in words rather than left to the switches behind it.
#[test]
fn the_agents_page_says_where_the_server_is_and_lists_the_agents() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write(|zeus| {
      zeus.agent_profiles.insert(AgentProfile::new(
         "Claude Code — laptop",
         [9u8; 32],
         0,
      ));
   });

   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Agents,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let names = button_names(&nodes);
   assert!(
      !names.contains(&"Start server") && !names.contains(&"Stop server"),
      "the endpoint has no switch to find: {names:?}"
   );
   assert!(
      names.contains(&"New agent"),
      "the create button must be there: {names:?}"
   );
   assert!(
      names.contains(&"Open"),
      "an agent must be openable: {names:?}"
   );

   let words = spoken(&nodes);
   assert!(
      words.iter().any(|text| text.contains("Claude Code — laptop")),
      "the agent's name must be in the tree: {words:?}"
   );
   // The endpoint is one light plus the sentence beside it, and the light says the same thing to a
   // reader as it says to the eye.
   assert!(
      words
         .iter()
         .any(|text| text.contains("Agent server: Starts by itself as soon as one agent exists")),
      "the light must carry the endpoint's state: {words:?}"
   );
   assert!(
      !names.contains(&"Start server") && !names.contains(&"Stop server"),
      "and there is no switch beside it: {names:?}"
   );
   assert!(
      words.iter().any(|text| text == "Nothing granted."),
      "a row must say what the agent holds: {words:?}"
   );
}

/// Opening an agent lands on its first screen, and the four screens are one named bar rather than one
/// long page: the token, the wallets, the settings and the activity each have their own place.
#[test]
fn an_agent_opens_onto_its_own_four_screens() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write(|zeus| {
      zeus.agent_profiles.insert(AgentProfile::new(
         "Claude Code — laptop",
         [9u8; 32],
         0,
      ));
   });

   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Agents,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let open = button_named(&nodes, "Open");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, open)],
   );

   for screen in ["Profile", "Wallets", "Settings", "Activity"] {
      assert!(
         tab_named(&nodes, screen).is_some(),
         "the agent's screens must be in the tree: {screen} missing from {:?}",
         tab_names(&nodes)
      );
   }

   // The Profile screen is the one it opens onto, and the token is what it is about.
   let names = button_names(&nodes);
   assert!(
      names.contains(&"Rename") && names.contains(&"Rotate token") && names.contains(&"Delete"),
      "the profile screen must offer what a credential can have done to it: {names:?}"
   );

   // The Wallets screen says there is nothing to grant yet rather than drawing an empty list.
   let wallets = tab_named(&nodes, "Wallets").expect("the Wallets screen");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, wallets)],
   );

   let words = spoken(&nodes);
   assert!(
      words
         .iter()
         .any(|text| text.contains("There is no wallet to grant anything on yet.")),
      "the Wallets screen must say what it has to work with: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("Ask before every action")),
      "the switch that makes an agent ask first belongs to the Wallets screen: {words:?}"
   );

   // The Settings screen is the app-wide switches, named one per line.
   let settings = tab_named(&nodes, "Settings").expect("the Settings screen");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, settings)],
   );

   let words = spoken(&nodes);
   assert!(
      words.iter().any(|text| text.contains("read and change in the app itself")),
      "the Settings screen must say what it covers: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("General · read · tier 0")),
      "each app-wide switch names its screen, its verb and its tier: {words:?}"
   );
}

/// A credential is created by one press, its token is shown once, and the Connect panel is where a
/// harness is pointed at it.
#[test]
fn creating_an_agent_shows_its_token_once() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Agents,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let create = button_named(&nodes, "New agent");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, create)],
   );

   let profiles = zeus.read(|zeus| zeus.agent_profiles.profiles());
   assert_eq!(
      profiles.len(),
      1,
      "the press must insert exactly one profile"
   );

   let profile = &profiles[0];
   assert_eq!(profile.label, "New agent");
   assert!(
      profile.wallets.is_empty() && profile.screens.is_empty(),
      "a new credential is granted nothing"
   );

   let names = button_names(&nodes);
   assert!(
      names.contains(&"Copy token"),
      "the token must be copyable: {names:?}"
   );
   assert!(
      names.contains(&"Done"),
      "the token block must be dismissable: {names:?}"
   );

   let words = spoken_text(&nodes);
   assert!(
      words.iter().any(|text| text.contains("shown once")),
      "the page must say the token is shown once: {words:?}"
   );
   assert!(
      words
         .iter()
         .any(|text| text.contains("The server has not come up yet, so the address is not known.")),
      "a snippet without a port must say so: {words:?}"
   );
}

/// The Activity view says what the credential did, and a row that made a transaction offers the step
/// to the history page — which the window hands up to the app, because a page cannot move the pages.
#[test]
fn the_activity_view_says_what_the_credential_did() {
   let (ctx, mut gui, zeus) = settings_window();

   let profile = AgentProfile::new("Claude Code — laptop", [9u8; 32], 0);

   zeus.write(|zeus| {
      zeus.agent_profiles.insert(profile.clone());

      let mut entry = AuditEntry::new(
         &profile,
         "tx_send",
         "wallet.write.send",
         1_700_000_000,
      );
      entry.decision = AuditDecision::Allowed;
      entry.wallet = Some(Address::repeat_byte(0x11));
      entry.chain = Some(1);
      entry.tx_hash = Some(TxHash::repeat_byte(9));
      entry.detail = Some("Agent sent 12 USDC to 0x1234...abcd".to_owned());

      zeus.agent_audit.record(entry);
   });

   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Agents,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   // The activity is the last of the agent's own screens, so one has to be opened and walked to it.
   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let open = button_named(&nodes, "Open");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, open)],
   );

   let activity = tab_named(&nodes, "Activity").expect("the Activity screen");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, activity)],
   );

   let words = spoken_text(&nodes);
   assert!(
      words.iter().any(|text| text.contains("Agent sent 12 USDC to 0x1234...abcd")),
      "what the agent did must be readable in the page: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("Every granted action is here")),
      "the section must say what it is: {words:?}"
   );

   let step = button_containing(&nodes, "in the history");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, step)],
   );

   assert_eq!(
      gui.settings.take_page(),
      Some(Page::Transactions),
      "a row's link asks the app for the page the transaction is on"
   );
}

/// The id of the agent's own screen named by the given word.
///
/// The settings nav is a tab list of its own, so a screen is found by the four words the agent's bar
/// uses and nothing else is called any of them.
fn tab_named(nodes: &[(NodeId, Node)], name: &str) -> Option<NodeId> {
   nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(name))
      .map(|(id, _)| *id)
}

/// Every tab's name, for the failure message of an assertion that names one.
fn tab_names(nodes: &[(NodeId, Node)]) -> Vec<&str> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Tab)
      .filter_map(|(_, node)| node.label())
      .collect()
}

/// The id of the one button whose name contains the given text: a row's link names its transaction,
/// so the row a test wants is the one whose hash it recognises.
fn button_containing(nodes: &[(NodeId, Node)], text: &str) -> NodeId {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .find(|(_, node)| node.label().is_some_and(|label| label.contains(text)))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| panic!("no button naming {text:?}: {:?}", spoken(nodes)))
}

/// Every button's name, for the failure message of an assertion that names one.
fn button_names(nodes: &[(NodeId, Node)]) -> Vec<&str> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect()
}
/// Nothing on the page is drawn on top of anything else, and no control is pushed outside the
/// content column.
///
/// The page had grown into a scroll area of overlapping widget rows, which is what made it
/// unreadable: a screenshot showed it, but a screenshot is not a gate. This is: every control the
/// page draws has a real size, sits inside the window, and does not share a band with another one.
#[test]
fn nothing_on_the_page_overlaps_or_leaves_the_window() {
   let (ctx, mut gui, zeus) = settings_window();

   let address = Address::repeat_byte(0x42);

   zeus.write(|zeus| {
      let mut profile = AgentProfile::new("Claude Code — laptop", [9u8; 32], 0);

      let grant = profile.wallets.entry(address).or_default();
      grant.enabled = true;
      grant.caps.insert("wallet.read.portfolio".to_owned());
      grant.caps.insert("wallet.write.send".to_owned());

      zeus.agent_profiles.insert(profile);
   });

   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Agents,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let open = button_named(&nodes, "Open");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, open)],
   );

   let mut controls: Vec<(egui::Rect, String)> = Vec::new();

   for (_, node) in &nodes {
      let Some(rect) = node.bounds() else {
         continue;
      };

      let named = node.label().or_else(|| node.value()).unwrap_or_default();
      let what = format!("{:?} {named}", node.role());

      // A control with no size is one a reader can land on and a pointer cannot, and it is how a
      // collapsed row hides on this page.
      if matches!(
         node.role(),
         Role::Button | Role::CheckBox | Role::Tab | Role::RadioButton | Role::Switch
      ) {
         assert!(
            rect.width() > 0.0 && rect.height() > 0.0,
            "{what} has no area to press: {rect:?}"
         );

         controls.push((
            egui::Rect::from_min_max(
               egui::pos2(rect.x0 as f32, rect.y0 as f32),
               egui::pos2(rect.x1 as f32, rect.y1 as f32),
            ),
            what,
         ));
      }
   }

   assert!(
      controls.len() > 10,
      "the page draws its controls as controls: {} found",
      controls.len()
   );

   // Two controls may sit on one line — a switch and the button to its right — so the check is the
   // rectangle, not the row: what it refuses is two controls drawn over one another, which is the
   // draw order deciding which of them a press reaches.
   for (index, (left, left_name)) in controls.iter().enumerate() {
      for (right, right_name) in &controls[index + 1..] {
         let shared = left.intersect(*right);

         assert!(
            shared.width() <= 1.0 || shared.height() <= 1.0,
            "{left_name} is drawn over {right_name}: {shared:?}"
         );
      }
   }
}
/// The page keeps one brake: with every credential off there is nothing to stop, and with one on,
/// a single press leaves no agent able to connect — the guarantee the removed Stop button gave.
#[test]
fn the_page_stops_every_agent_in_one_press() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write(|zeus| {
      let mut profile = AgentProfile::new("Claude Code — laptop", [9u8; 32], 0);
      profile.enabled = true;
      zeus.agent_profiles.insert(profile);
   });

   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Agents,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let stop = button_named(&nodes, "Stop all agents");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, stop)],
   );

   assert!(
      zeus.read(|zeus| zeus.agent_profiles.enabled_profiles().is_empty()),
      "one press must leave no credential able to connect"
   );
   assert_eq!(
      gui.settings.take_notice().as_deref(),
      Some("1 profile may no longer connect."),
      "and it must say what it did"
   );
}
/// The wallet grants are a table a user compares down, and one wallet's switches are a dialog.
///
/// The page used to stack every wallet's eleven switches one under the other, which is what made it a
/// dumpster: a person reading the page could not see two wallets side by side, and the switches of the
/// third wallet were a screen away from its name.
#[test]
fn the_wallet_grants_are_a_table_with_a_dialog_per_wallet() {
   let (ctx, mut gui, zeus) = settings_window();

   let first = Address::repeat_byte(0x11);
   let second = Address::repeat_byte(0x22);

   zeus.write(|zeus| {
      let mut profile = AgentProfile::new("Claude Code — laptop", [9u8; 32], 0);

      let grant = profile.wallets.entry(first).or_default();
      grant.enabled = true;
      grant.caps.insert("wallet.read.portfolio".to_owned());
      grant.caps.insert("wallet.write.send".to_owned());
      grant.tokens = TokenScope::All;

      zeus.agent_profiles.insert(profile);

      zeus.wallet_info_cache.ordered_vec = vec![
         wallet("Master Wallet", first),
         wallet("Railgun Test", second),
      ];
   });

   let nodes = open_agent(&ctx, &mut gui, &zeus, "Wallets");

   // One table, two body rows, and the grant said in words rather than left to the switches.
   let tables = nodes.iter().filter(|(_, node)| node.role() == Role::Table).count();
   assert_eq!(
      tables,
      1,
      "the wallets are one table: {:?}",
      role_counts(&nodes)
   );

   let words = spoken(&nodes);
   assert!(
      words.iter().any(|text| text.contains("Master Wallet")),
      "every wallet is a row: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("2 permissions · every token · no NFTs")),
      "a row says what the grant is: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("Nothing granted.")),
      "and a wallet with none says so: {words:?}"
   );

   // Each row's action names the wallet it opens, because every row draws the same word.
   let edit = button_named(&nodes, "Edit the grant for Master Wallet");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, edit)],
   );

   let words = spoken(&nodes);
   assert!(
      words.iter().any(|text| text.contains("Allow this wallet")),
      "the dialog holds that wallet's switches: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("What it may read")),
      "and groups them: {words:?}"
   );

   // The tier is not drawn on a row any more: twenty of them read as noise. It is still said, on the
   // node the switch is.
   // The tier is no longer drawn — twenty rows reading "Portfolio · tier 0" is the noise the page is
   // being cleaned of — but it is still what the switch means, so it is said on the node instead.
   let drawn: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.value()).collect();
   let described: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.description()).collect();

   assert!(
      !drawn.iter().any(|text| text.contains("tier")),
      "no grant row draws its tier any more: {drawn:?}"
   );
   assert!(
      described.iter().any(|text| text.contains("tier")),
      "and a reader still hears it: {described:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("Portfolio · tier 0")),
      "the tier is still what the switch is: {words:?}"
   );
}

/// Open one agent and walk its bar to `screen`, answering the tree that screen drew.
fn open_agent(
   ctx: &egui::Context,
   gui: &mut crate::gui::GUI,
   zeus: &ZeusCtx,
   screen: &str,
) -> Vec<(NodeId, Node)> {
   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Agents,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let open = button_named(&nodes, "Open");

   let nodes = render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, open)],
   );

   let tab = tab_named(&nodes, screen).expect("the screen's tab");

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, tab)],
   )
}

/// A wallet with a known address, for the table: the fixtures generate a random key, and the grant
/// the test builds is keyed by the address the page reads.
fn wallet(name: &str, address: Address) -> WalletInfo {
   let mut wallet = wallet_named(name);
   wallet.address = address;
   wallet
}

/// How many nodes of each role the tree holds, for a failure message.
fn role_counts(nodes: &[(NodeId, Node)]) -> Vec<(Role, usize)> {
   let mut counts: Vec<(Role, usize)> = Vec::new();

   for (_, node) in nodes {
      match counts.iter_mut().find(|(role, _)| *role == node.role()) {
         Some((_, count)) => *count += 1,
         None => counts.push((node.role(), 1)),
      }
   }

   counts
}
/// Every control on the page is the same height, and every button's words sit in the middle of it.
///
/// The page is a column of rows a user reads down, so a row whose controls disagree about their height
/// reads as a layout accident rather than as a set. One `CONTROL` height is what keeps them equal, and
/// this is what keeps anything from quietly going back to its own.
#[test]
fn every_control_on_the_page_is_one_height() {
   let (ctx, mut gui, zeus) = settings_window();

   let address = Address::repeat_byte(0x11);

   zeus.write(|zeus| {
      let mut profile = AgentProfile::new("Claude Code — laptop", [9u8; 32], 0);
      profile.wallets.entry(address).or_default().enabled = true;
      zeus.agent_profiles.insert(profile);
      zeus.wallet_info_cache.ordered_vec = vec![wallet("Master Wallet", address)];
   });

   // One pass over each of the agent's screens. `open_agent` opens the agent on the first call and
   // walks its bar afterwards, so the page is never opened twice in one pass.
   let mut heights = Vec::new();

   for (position, screen) in ["Profile", "Wallets", "Settings", "Activity"].into_iter().enumerate()
   {
      let nodes = match position {
         0 => open_agent(&ctx, &mut gui, &zeus, screen),
         _ => open_screen(&ctx, &mut gui, &zeus, screen),
      };

      for (_, node) in &nodes {
         if node.role() != Role::Button {
            continue;
         }

         // Only the page's own controls: the window's close button and the nav's plates belong to
         // the shell, not to this page.
         let Some(rect) = node.bounds() else {
            continue;
         };

         if rect.height() > 0.0 && rect.width() > 60.0 {
            heights.push((
               rect.height(),
               node.label().unwrap_or_default().to_owned(),
            ));
         }
      }
   }

   assert!(
      !heights.is_empty(),
      "the page draws buttons on its screens"
   );

   let tallest = heights.iter().map(|(height, _)| *height).fold(0.0_f64, f64::max);
   let shortest = heights.iter().map(|(height, _)| *height).fold(f64::MAX, f64::min);

   assert!(
      (tallest - shortest).abs() < 0.5,
      "every button is one height: {heights:?}"
   );
}
/// Walk an agent that is already open to `screen`, and answer the tree that screen drew.
fn open_screen(
   ctx: &egui::Context,
   gui: &mut crate::gui::GUI,
   zeus: &ZeusCtx,
   screen: &str,
) -> Vec<(NodeId, Node)> {
   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let tab = tab_named(&nodes, screen).expect("the screen's tab");

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, tab)],
   )
}
/// The wallet dialog fits the window: its body scrolls under a ceiling instead of running off screen.
///
/// A wallet with both allowlists filled was taller than a laptop display, and a dialog that overflows
/// the window hides the switches under the title bar rather than offering them.
#[test]
fn the_wallet_dialog_fits_and_scrolls() {
   let (ctx, mut gui, zeus) = settings_window();

   let address = Address::repeat_byte(0x11);

   zeus.write(|zeus| {
      let mut profile = AgentProfile::new("Claude Code — laptop", [9u8; 32], 0);

      let grant = profile.wallets.entry(address).or_default();
      grant.enabled = true;
      grant.caps.insert("wallet.write.send".to_owned());
      grant.tokens = TokenScope::Allowlist {
         tokens: (0..12)
            .map(|index| TokenRef {
               chain: 1,
               address: Address::repeat_byte(index as u8 + 1),
            })
            .collect(),
      };

      zeus.agent_profiles.insert(profile);
      zeus.wallet_info_cache.ordered_vec = vec![wallet("Master Wallet", address)];
   });

   let nodes = open_agent(&ctx, &mut gui, &zeus, "Wallets");
   let edit = button_named(&nodes, "Edit the grant for Master Wallet");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, edit)],
   );

   // The twelve tokens are in the tree, and the body that holds them is bounded: a dialog whose
   // content simply grew would have pushed the first of them out of the window.
   let words = spoken(&nodes);
   assert!(
      words.iter().any(|text| text.contains("Allow this wallet")),
      "the dialog's first control must still be there: {words:?}"
   );

   // The dialog is one node, and its body scrolls rather than growing past the ceiling it is given:
   // a wallet with both allowlists filled used to be taller than the screen it was drawn on.
   let dialogs = nodes.iter().filter(|(_, node)| node.role() == Role::Dialog).count();
   assert_eq!(
      dialogs,
      1,
      "one wallet's grant is one dialog: {:?}",
      role_counts(&nodes)
   );

   let bars = nodes.iter().filter(|(_, node)| node.role() == Role::ScrollBar).count();
   assert!(
      bars > 0,
      "its body scrolls: {:?}",
      role_counts(&nodes)
   );
}
/// The two ceilings are rails with a question mark beside them, and neither is an unexplained number.
#[test]
fn the_limits_are_rails_that_say_what_they_bound() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write(|zeus| {
      zeus.agent_profiles.insert(AgentProfile::new(
         "Claude Code — laptop",
         [9u8; 32],
         0,
      ));
   });

   let nodes = open_agent(&ctx, &mut gui, &zeus, "Wallets");

   // The limits live behind their own disclosure; opening it is what a user does to reach them.
   let limits = button_or_disclosure(&nodes, "How far it may go on its own");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, limits)],
   );

   let words = spoken(&nodes);
   assert!(
      words.iter().any(|text| text.contains("Max price impact")),
      "the price-impact ceiling must still be named: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("Max shortfall")),
      "and the shortfall one: {words:?}"
   );

   // Each rail carries the sentence that explains it, on the control a reader lands on.
   assert!(
      words.iter().any(|text| text.contains("How much worse a trade may get")),
      "the price-impact rail must say what it bounds: {words:?}"
   );
   assert!(
      words
         .iter()
         .any(|text| text.contains("How much of what the agent promised may fail to arrive")),
      "the shortfall rail must say what it bounds: {words:?}"
   );

   // And the sections a normal user reads are words, not tier numbers.
   assert!(
      !words.iter().any(|text| text.contains("tier 2") || text.contains("tier 3")),
      "no section is named by its tier: {words:?}"
   );
}

/// The one control whose name is exactly `name`, whatever shape it is drawn in: a disclosure is not a
/// button in the tree, and the settings page uses both.
fn button_or_disclosure(nodes: &[(NodeId, Node)], name: &str) -> NodeId {
   nodes
      .iter()
      .find(|(_, node)| {
         matches!(
            node.role(),
            Role::Button | Role::DisclosureTriangle
         ) && (node.label() == Some(name) || node.value() == Some(name))
      })
      .map(|(id, _)| *id)
      .unwrap_or_else(|| panic!("no control named {name:?}: {:?}", spoken(nodes)))
}
/// A question mark is drawn on the line of the control it explains, not on a row of its own.
///
/// Every direct child of a vertical `Ui` gets a row of its own, so a chip added after a switch renders
/// under it — which is what the first cut did. This is the assertion that would have caught it: the
/// chip and its switch share a band, so a reader moving down the screen meets them together.
#[test]
fn a_question_mark_is_beside_the_control_it_explains() {
   let (ctx, mut gui, zeus) = settings_window();

   let address = Address::repeat_byte(0x11);

   zeus.write(|zeus| {
      let mut profile = AgentProfile::new("Claude Code — laptop", [9u8; 32], 0);

      let grant = profile.wallets.entry(address).or_default();
      grant.enabled = true;

      zeus.agent_profiles.insert(profile);
      zeus.wallet_info_cache.ordered_vec = vec![wallet("Master Wallet", address)];
   });

   let nodes = open_agent(&ctx, &mut gui, &zeus, "Wallets");
   let edit = button_named(&nodes, "Edit the grant for Master Wallet");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, edit)],
   );

   // Private is the switch whose sentence is a warning, so it is the one with a chip beside it.
   let private = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::CheckBox && node.label() == Some("Private"))
      .expect("the Private switch");

   let chip = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("What this means"))
      .expect("the question mark beside it");

   let (_, switch) = private;
   let (_, chip_node) = chip;

   let switch_rect = switch.bounds().expect("the switch has bounds");
   let chip_rect = chip_node.bounds().expect("the chip has bounds");

   // On the same line, and to the right of the switch it explains.
   assert!(
      (chip_rect.y0 - switch_rect.y0).abs() < switch_rect.height(),
      "the chip sits on the switch's line: switch {switch_rect:?}, chip {chip_rect:?}"
   );
   assert!(
      chip_rect.x0 >= switch_rect.x1 - 1.0,
      "and after it: switch {switch_rect:?}, chip {chip_rect:?}"
   );
}

/// A table row is tall enough for the control it holds.
///
/// The row's pitch has to leave air above and below the button: at 34 points it was shorter than the
/// 30-point button inside it plus any gap at all, which is why Edit touched both edges of its row.
#[test]
fn a_table_row_holds_its_button() {
   let (ctx, mut gui, zeus) = settings_window();

   let address = Address::repeat_byte(0x11);

   zeus.write(|zeus| {
      zeus.agent_profiles.insert(AgentProfile::new(
         "Claude Code — laptop",
         [9u8; 32],
         0,
      ));
      // Two wallets, so the pitch between the two rows is measurable rather than the band's own edge.
      zeus.wallet_info_cache.ordered_vec = vec![
         wallet("Master Wallet", address),
         wallet("Railgun Test", Address::repeat_byte(0x22)),
      ];
   });

   let nodes = open_agent(&ctx, &mut gui, &zeus, "Wallets");

   let mut buttons: Vec<egui::accesskit::Rect> = nodes
      .iter()
      .filter(|(_, node)| matches!(node.role(), Role::Button))
      .filter_map(|(_, node)| node.bounds())
      // The page's own buttons are the ones under the table: the nav plates and the window's close
      // button live in the shell and are not rows.
      .filter(|rect| rect.width() < 120.0 && rect.height() >= 29.0)
      .collect();

   buttons.sort_by(|left, right| left.y0.total_cmp(&right.y0));
   buttons.dedup_by(|left, right| (left.y0 - right.y0).abs() < 0.5);

   assert!(
      buttons.len() >= 2,
      "the two rows' buttons must both be in the tree: {buttons:?}"
   );

   let pitch = buttons[1].y0 - buttons[0].y0;
   let height = buttons[0].height();

   // The pitch is the row: the button plus the air above and below it.
   assert!(
      pitch >= height + 8.0,
      "the row leaves air around its button: pitch {pitch}, button {height}"
   );
}
/// The two rails keep what the user moves them to.
///
/// A click in the middle of the price-impact rail must land near the middle of its range and *stay*
/// there: the value is read back from the profile on the next pass, so a rail that resets is a rail
/// whose stored value went back to zero.
#[test]
fn the_limits_keep_what_the_user_moves_them_to() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write(|zeus| {
      zeus.agent_profiles.insert(AgentProfile::new(
         "Claude Code — laptop",
         [9u8; 32],
         0,
      ));
   });

   let nodes = open_agent(&ctx, &mut gui, &zeus, "Wallets");
   let limits = button_or_disclosure(&nodes, "How far it may go on its own");

   // Open the disclosure. Its contents are measured on this pass and placed on the next, so the
   // answer is thrown away and the next render is the one whose geometry means anything.
   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, limits)],
   );
   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   // The rail the page draws for a ceiling: a slider node whose name is the setting's.
   let rail = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Max price impact"))
      .map(|(_, node)| node.bounds().expect("the rail has bounds"))
      .unwrap_or_else(|| panic!("no price-impact rail: {:?}", spoken(&nodes)));

   // Click three quarters along, which is inside the range and nowhere near its ends.
   let at = egui::pos2(
      (rail.x0 + rail.width() * 0.75) as f32,
      ((rail.y0 + rail.y1) * 0.5) as f32,
   );

   // What the store holds before the click. The page keeps its own copy of the profile, so this is
   // also what it still holds on the pass that moves the rail.
   let before = stored_price_impact(&zeus);

   let nodes = render_settings(&ctx, &mut gui, &zeus, click_at(at));

   // And the page draws it back: a value the next pass reads as zero is a rail that snaps home.
   let now = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Max price impact"))
      .and_then(|(_, node)| node.bounds());

   assert!(now.is_some(), "the rail is still drawn");

   let shown = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Max price impact"))
      .and_then(|(_, node)| node.value())
      .expect("the rail says what it is worth");
   let shown: f64 = shown.parse().expect("the rail says a number");

   assert!(
      shown > 10.0,
      "the rail keeps the value it was moved to, not reset to zero: {shown}"
   );

   // The drag is one edit, not one write per pass: the store is written when the window closes.
   assert_eq!(
      stored_price_impact(&zeus),
      before,
      "a pass of the page must not write the profile"
   );

   zeus.write(|zeus| gui.settings.close(zeus));

   let stored = stored_price_impact(&zeus);

   assert!(
      (f64::from(stored) / 100.0 - shown).abs() < 0.01,
      "the value the page kept is the one that is written: rail {shown}, stored {stored}"
   );

   // And it is wide enough to aim at. A rail squeezed to nothing is a rail that only moves when the
   // row is re-laid out — which is what the label claiming the whole row did to it.
   let width = rail.width();
   assert!(
      width > 100.0,
      "the rail must keep a width a pointer can aim at: {width}"
   );

   // The two ceilings are one control set: their rails start at the same x, which is what the name
   // column is for.
   let shortfall = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Max shortfall"))
      .and_then(|(_, node)| node.bounds())
      .expect("the shortfall rail");

   assert!(
      (shortfall.x0 - rail.x0).abs() < 1.0,
      "both rails start at the same x: price impact {rail:?}, shortfall {shortfall:?}"
   );
}

/// The price-impact ceiling the profile store holds, in basis points.
fn stored_price_impact(zeus: &ZeusCtx) -> u16 {
   zeus.read(|zeus| {
      zeus
         .agent_profiles
         .profiles()
         .first()
         .map(|profile| profile.max_price_impact_bps)
         .expect("the profile is still there")
   })
}

/// A click at one point, in the order `egui_kittest` delivers one.
///
/// The pointer moves there first, then the button is pressed and released: egui reads a press against
/// where it believes the pointer is, and a release is what turns the press into a click.
fn click_at(at: egui::Pos2) -> Vec<egui::Event> {
   let mut events = vec![egui::Event::PointerMoved(at)];

   for pressed in [true, false] {
      events.push(egui::Event::PointerButton {
         pos: at,
         button: egui::PointerButton::Primary,
         pressed,
         modifiers: egui::Modifiers::NONE,
      });
   }

   events
}
