//! The Activity view: what this credential did, and the way to what it produced.
//!
//! The ring is the only record an autonomous action leaves, and the one place a user who cannot
//! watch the screen can read afterwards. A row that produced a transaction offers the step to the
//! history page, because the audit says what was asked and the history says what happened.

use super::ago;
use crate::core::ZeusContext;
use crate::core::agent::{AgentProfile, AuditDecision, AuditEntry};
use crate::gui::Page;
use crate::gui::ui::common::section_heading;
use crate::utils::truncate_address;
use egui::{RichText, Ui, vec2};
use egui_elements::{Button, Theme};

/// How many rows the view lists: the ring holds thousands, and a page shows the recent ones.
const ROWS: usize = 20;

/// The Activity section of the Agents page.
///
/// It carries no state of its own: every pass reads the ring, which is the store the actions wrote,
/// so nothing here can be a stale copy of what an agent did.
#[derive(Default)]
pub struct Activity;

impl Activity {
   /// Draw this profile's entries, newest first, and answer the page a row asked for.
   pub fn show(
      &mut self,
      ctx: &ZeusContext,
      profile: &AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) -> Option<Page> {
      section_heading(ui, theme.typography.very_large, "Activity");

      ui.label(
         RichText::new(format!(
            "What {} did, newest first. Every granted action is here, whether or not it asked you \
             first.",
            profile.label
         ))
         .size(theme.typography.normal),
      );

      let recent = ctx.agent_audit.recent(ROWS, |entry| entry.profile == profile.id);

      if recent.entries.is_empty() {
         ui.add_space(theme.spacing.sm);
         ui.label(RichText::new("Nothing yet.").size(theme.typography.normal));
         return None;
      }

      let mut page = None;

      for entry in &recent.entries {
         ui.add_space(theme.spacing.sm);
         ui.label(RichText::new(sentence(entry)).size(theme.typography.normal));

         let Some(hash) = entry.tx_hash else {
            continue;
         };

         // The hash is in the button's name, not beside it: a reader Tabbing the list hears which
         // transaction each step is about, and the name tells the rows apart.
         let label = format!(
            "Show {} in the history",
            truncate_address(format!("{hash:#x}"))
         );

         let button = Button::new(RichText::new(label).size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(vec2(260.0, 26.0));

         if ui.add(button).clicked() {
            page = Some(Page::Transactions);
         }
      }

      if recent.matching > recent.entries.len() {
         ui.add_space(theme.spacing.sm);
         ui.label(
            RichText::new(format!(
               "The {} most recent of {} entries.",
               recent.entries.len(),
               recent.matching
            ))
            .size(theme.typography.normal),
         );
      }

      page
   }
}

/// One entry as a sentence: when it happened, what came of it, and in whose words.
///
/// A granted action's own sentence ("Agent sent 1.5 USDC to 0x1234...abcd") is the one the app said
/// out loud, so the page repeats it rather than inventing a second wording for the same event. A
/// refusal has no such sentence — it never reached a flow — so it is named for what it was.
fn sentence(entry: &AuditEntry) -> String {
   let words = entry.detail.as_deref().unwrap_or(&entry.tool);

   let what = match entry.decision {
      AuditDecision::Allowed => words.to_owned(),
      AuditDecision::Prompted => format!("{words} (after you confirmed)"),
      AuditDecision::Denied => format!("Denied: {words}"),
      AuditDecision::NeedsUser => format!("Waiting for you: {words}"),
   };

   format!("{}, {what}", ago(entry.ts))
}
