//! Settings → Agents: the switch, the credential list, and the token shown once.
//!
//! The page is drawn headlessly like every other settings page, so what it says is checked on the
//! tree a reader is handed rather than on the pixels.

use super::app::{render_settings, settings_window};
use super::harness::{accesskit_action, button_named, spoken, spoken_text};
use crate::core::agent::{AgentProfile, AuditDecision, AuditEntry};
use crate::gui::Page;
use crate::gui::ui::settings::SettingsPage;
use egui::accesskit::{Action, Node, NodeId, Role};
use zeus_eth::alloy_primitives::{Address, TxHash};

/// The switch names the action it will take, the credential list names its profile, and both are
/// controls in the tree rather than prose a reader has to find.
#[test]
fn the_agents_page_offers_the_switch_and_the_credentials() {
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
      names.contains(&"Start server"),
      "the switch must be there: {names:?}"
   );
   assert!(
      names.contains(&"New agent"),
      "the create button must be there: {names:?}"
   );
   assert!(
      names.contains(&"Open"),
      "a profile must be openable: {names:?}"
   );
   assert!(
      names.contains(&"Delete"),
      "a profile must be deletable: {names:?}"
   );

   // The name rides on the Enabled checkbox rather than on a label of its own, so every word a
   // reader can reach is the right set to search.
   let words = spoken(&nodes);
   assert!(
      words.iter().any(|text| text.contains("Claude Code — laptop")),
      "the profile's name must be in the tree: {words:?}"
   );
   assert!(
      words.iter().any(|text| text.contains("Stopped")),
      "the page must say the endpoint is stopped: {words:?}"
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
         .any(|text| text.contains("Start the agent server to see the address")),
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

   // The activity is inside the section of the credential that is open, so one has to be opened.
   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let open = button_named(&nodes, "Open");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, open)],
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

/// The id of the one button whose name contains `text`: a row's link names its transaction, so the
/// row a test wants is the one whose hash it recognises.
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
