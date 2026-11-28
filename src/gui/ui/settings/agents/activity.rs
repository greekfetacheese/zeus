//! The Activity view: what this credential did, page by page.
//!
//! The ring is the only record an autonomous action leaves, and the one place a user who cannot
//! watch the screen can read afterwards. Read as a run of sentences it stops being readable after the
//! first screen, so an entry is a row of one table — when, what came of it, which flow, what it said
//! — and a row that produced a transaction offers the step to the history page, because the audit
//! says what was asked and the history says what happened.
//!
//! A ring holds thousands of entries, so the view never loads it: the pager divides the count the
//! store reports, and a page reads only the rows it draws.

use super::CONTROL;
use crate::core::ZeusContext;
use crate::core::agent::{
   AUDIT_RING_CAPACITY, AgentProfile, AgentProfileId, AuditDecision, AuditEntry,
};
use crate::gui::Page;
use crate::gui::ui::common::{describe, section_heading};
use crate::utils::TimeStamp;
use crate::utils::truncate_address;
use egui::{Align, Layout, Response, RichText, TextWrapMode, Ui, vec2};
use egui_elements::{Button, Chip, ChipRole, Column, Label, Table, TableScroll, Theme};
use zeus_eth::alloy_primitives::B256;

/// How many entries a page draws.
const PER_PAGE: usize = 20;

/// The height of one row: the tallest control a row carries — its button — plus the air around it,
/// so a row of text and a row with a button are drawn at one pitch.
const ROW: f32 = CONTROL + 16.0;

/// The viewport the rows scroll in. A page holds more rows than this shows, so the list scrolls
/// inside the page and the pager is a jump over twenty entries rather than the only way down.
const VIEWPORT: f32 = 400.0;

/// The least room the rows are given when the window is short: three rows, so the table is still a
/// table the pager can be read over rather than one line it disappears behind.
const FLOOR: f32 = ROW * 3.0;

/// The gap between two columns: wide enough that a date, an outcome and a flow read as three fields
/// rather than as one run of text.
const COLUMN_GAP: f32 = 20.0;

/// The Activity section of the Agents page.
///
/// It holds the page it last read and little else: the ring is the store the actions wrote, so the
/// rows are read again whenever that ring moves, and nothing here can be a stale copy of the log.
#[derive(Default)]
pub struct Activity {
   /// The page in front, once one has been read.
   loaded: Option<Loaded>,
}

impl Activity {
   /// Draw this profile's entries newest first, twenty to a page, and answer the page a row asked
   /// for.
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

      // The ring's length is the cheap read: a frame that draws the page already in front reads it
      // and nothing else, because a page is read again only when that length moved or the user
      // turned the page.
      let ring = ctx.agent_audit.len();
      self.refresh(ctx, profile, ring);

      if self.loaded.as_ref().is_none_or(|loaded| loaded.total == 0) {
         ui.add_space(theme.spacing.sm);
         ui.label(RichText::new("Nothing yet.").size(theme.typography.normal));
         return None;
      }

      self.pager(ctx, profile, ring, theme, ui);

      let Some(loaded) = &self.loaded else {
         return None;
      };

      let mut page = None;

      Table::new(("agent_activity", &loaded.profile), &loaded.rows)
         .columns([
            Column::Fixed(110.0), // When
            Column::Fixed(130.0), // Outcome
            Column::Fr(1.3),      // Tool
            Column::Fr(2.7),      // What happened
            Column::Fixed(170.0), // Transaction
         ])
         .gap(COLUMN_GAP)
         .row_gap(theme.spacing.sm)
         .row_height(ROW)
         .row_divider(true)
         .header_rule(true)
         // The header stays outside the scroll area, so the column a row is read under is still
         // there when the last of the page is in view.
         .scroll(
            TableScroll::new()
               .max_height(ui.available_height().clamp(FLOOR, VIEWPORT))
               .auto_shrink([false, true]),
         )
         .key((loaded.index, loaded.ring))
         .show(ui, |table| {
            table.header(|header| {
               for name in ["When", "Outcome", "Tool", "What happened", "Transaction"] {
                  header.cell_named(name, |ui| header_cell(ui, theme, name));
               }
            });

            table.rows(|row, entry| {
               let age = entry.age();

               row.cell_named(age.clone(), |ui| {
                  let response = text_cell(ui, theme, &age);
                  // An age alone cannot be checked against anything, so the instant it stands for
                  // rides on the cell: a reader hears it, and so does a pointer.
                  describe(ui, &response, entry.at.as_str());
                  response.on_hover_text(entry.at.as_str());
               });

               row.cell_named(entry.outcome.words(), |ui| {
                  let text = RichText::new(entry.outcome.words()).size(theme.typography.small);
                  ui.add(Chip::new(text).role(entry.outcome.role()));
               });

               row.cell_named(entry.tool.clone(), |ui| {
                  let response = text_cell(ui, theme, &entry.tool);
                  // The flow is the request's name; the capability is the grant it needed, which
                  // the column has no room for and a reader still needs to hear.
                  let needed = format!("Needed the {} capability", entry.capability);
                  describe(ui, &response, needed.as_str());
                  response.on_hover_text(needed);
               });

               row.cell_named(entry.what.clone(), |ui| {
                  let response = text_cell(ui, theme, &entry.what);
                  // The cell's name and the label's own text carry the whole sentence; the cut is
                  // only what is drawn, so the hover is what a pointer has to ask for.
                  response.on_hover_text(entry.what.as_str());
               });

               let Some(tx) = &entry.tx else {
                  row.cell_named("No transaction", |_ui| {});
                  return;
               };

               // The hash is in the button's name, not beside it: a reader Tabbing the table hears
               // which transaction each step is about, and the name tells the rows apart.
               let name = format!("Show {} in the history", tx.short);

               row.cell_named(name.clone(), |ui| {
                  let label = RichText::new("Show in history").size(theme.typography.normal);

                  let show =
                     Button::new(label).min_size(vec2(150.0, CONTROL)).label_align(Align::Center);

                  let response = ui.add(show).accessible_name(name);
                  describe(ui, &response, tx.full.as_str());

                  if response.clicked() {
                     page = Some(Page::Transactions);
                  }
               });
            });
         });

      page
   }

   /// The page controls: two steps and the count the ring holds for this agent.
   ///
   /// A press turns the page in the frame it is read, so the rows under the controls and the number
   /// between them agree on that frame.
   fn pager(
      &mut self,
      ctx: &ZeusContext,
      profile: &AgentProfile,
      ring: usize,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      let Some(loaded) = &self.loaded else {
         return;
      };

      let (index, total) = (loaded.index, loaded.total);
      let pages = page_count(total);
      let mut step = 0;

      ui.add_space(theme.spacing.sm);

      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;

         let newer = Button::new(RichText::new("Newer").size(theme.typography.normal))
            .min_size(vec2(90.0, CONTROL))
            .label_align(Align::Center);

         if ui.add_enabled(index > 0, newer).clicked() {
            step = -1;
         }

         ui.label(
            RichText::new(format!("Page {} of {}", index + 1, pages))
               .size(theme.typography.normal)
               .color(theme.colors.text),
         );

         let older = Button::new(RichText::new("Older").size(theme.typography.normal))
            .min_size(vec2(90.0, CONTROL))
            .label_align(Align::Center);

         if ui.add_enabled(index + 1 < pages, older).clicked() {
            step = 1;
         }

         // What the page is a page of, and — once the ring is at its ceiling — why the oldest
         // actions are not in it: the ring keeps a fixed number and drops the tail.
         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing.xs;

            let mut said = format!("{total} {}", noun(total));

            if ring >= AUDIT_RING_CAPACITY {
               said = format!("{said} · the log keeps the newest {AUDIT_RING_CAPACITY}");
            }

            ui.label(
               RichText::new(said).size(theme.typography.small).color(theme.colors.text_muted),
            );
         });

         if step != 0 {
            self.turn(ctx, profile, step, ring);
         }
      });
   }

   /// Read the page in front again when it can have moved: another agent is open, the ring gained or
   /// lost an entry, or the page in front is not the one the last read left.
   fn refresh(&mut self, ctx: &ZeusContext, profile: &AgentProfile, ring: usize) {
      let index = match &self.loaded {
         Some(loaded) if loaded.profile == profile.id => loaded.index,
         _ => 0,
      };

      let fresh = self.loaded.as_ref().is_some_and(|loaded| {
         loaded.profile == profile.id && loaded.index == index && loaded.ring == ring
      });

      if !fresh {
         self.read(ctx, profile, index, ring);
      }
   }

   /// Move the page in front by `step`, clamped to the pages the ring has.
   fn turn(&mut self, ctx: &ZeusContext, profile: &AgentProfile, step: isize, ring: usize) {
      let Some(loaded) = &self.loaded else {
         return;
      };

      let last = page_count(loaded.total).saturating_sub(1) as isize;
      let index = (loaded.index as isize + step).clamp(0, last) as usize;

      if index != loaded.index {
         self.read(ctx, profile, index, ring);
      }
   }

   /// Read one page of this profile's ring, and the count the pager divides by.
   ///
   /// This is the only read of the log: one walk for the count and a clone of the twenty rows the
   /// page draws, so a ring of thousands costs a page rather than its whole length.
   fn read(&mut self, ctx: &ZeusContext, profile: &AgentProfile, index: usize, ring: usize) {
      let page_at = |index: usize| {
         ctx.agent_audit.page(index * PER_PAGE, PER_PAGE, |entry| {
            entry.profile == profile.id
         })
      };

      let page = page_at(index);
      let pages = page_count(page.matching);

      // The ring can have forgotten the page under the cursor — its entries fell off the ceiling
      // while the page was open — so the last page left is the one to land on.
      let (index, page) = match index >= pages {
         true => (pages - 1, page_at(pages - 1)),
         false => (index, page),
      };

      self.loaded = Some(Loaded {
         profile: profile.id.clone(),
         index,
         total: page.matching,
         ring,
         rows: page.entries.iter().map(Row::of).collect(),
      });
   }
}

/// One page as the last read left it.
struct Loaded {
   /// The agent the rows belong to: another agent's page is another log.
   profile: AgentProfileId,
   /// Which page this is, counting from the newest.
   index: usize,
   /// How many entries the profile has at any age, which is what the pager divides by.
   total: usize,
   /// How many entries the ring held when this page was read: a recorded action moves it, and a
   /// moved ring is the one thing that can make the page stale without the user asking.
   ring: usize,
   /// The rows on this page, newest first.
   rows: Vec<Row>,
}

/// One entry as a row: the words the table draws, and the transaction the row can offer.
struct Row {
   /// Unix seconds. The age is drawn from this frame's clock, not from the frame the page was read,
   /// so a row read a minute ago does not go on claiming that minute.
   ts: u64,
   /// The instant in full, for the hover and for a reader that asks what the age was.
   at: String,
   /// What came of it, as the chip in its own column says.
   outcome: Outcome,
   /// The flow the request came through.
   tool: String,
   /// The capability it needed, whether or not the grant carried it.
   capability: String,
   /// What the user was told, or the flow's own name when it said nothing.
   what: String,
   tx: Option<Tx>,
}

impl Row {
   /// One entry as the table draws it.
   fn of(entry: &AuditEntry) -> Self {
      Self {
         ts: entry.ts,
         at: TimeStamp::Seconds(entry.ts).to_date_string(),
         outcome: Outcome::of(entry.decision),
         tool: entry.tool.clone(),
         capability: entry.capability.clone(),
         what: entry.detail.clone().unwrap_or_else(|| entry.tool.clone()),
         tx: entry.tx_hash.map(Tx::of),
      }
   }

   /// The age this row shows in this frame.
   fn age(&self) -> String {
      TimeStamp::Seconds(self.ts).to_relative()
   }
}

/// The transaction an entry produced, in the two forms the row uses.
struct Tx {
   /// The short form the row names the step with, so a reader can tell two rows apart.
   short: String,
   /// The whole hash, which is what the history page keys the transaction by.
   full: String,
}

impl Tx {
   /// The short and the whole form of the transaction `hash` names.
   fn of(hash: B256) -> Self {
      let full = format!("{hash:#x}");
      let short = truncate_address(full.clone());

      Self { short, full }
   }
}

/// What the policy decided about one attempt, as a row shows it.
#[derive(Clone, Copy, Debug)]
enum Outcome {
   /// Ran without asking.
   Allowed,
   /// Ran, but only after the profile's own mode raised the in-app prompt.
   Confirmed,
   /// Refused.
   Denied,
   /// Handed back to the human, who may or may not have finished it.
   Waiting,
}

impl Outcome {
   fn of(decision: AuditDecision) -> Self {
      match decision {
         AuditDecision::Allowed => Self::Allowed,
         AuditDecision::Prompted => Self::Confirmed,
         AuditDecision::Denied => Self::Denied,
         AuditDecision::NeedsUser => Self::Waiting,
      }
   }

   /// The words the chip carries.
   ///
   /// They are the words the sentence used to carry — "Denied", "after you confirmed", "Waiting for
   /// you" — so the column adds structure to the record rather than a second wording of it.
   fn words(self) -> &'static str {
      match self {
         Self::Allowed => "Allowed",
         Self::Confirmed => "You confirmed",
         Self::Denied => "Denied",
         Self::Waiting => "Waiting for you",
      }
   }

   /// The colour the chip is tinted with: a refusal and a wait are not the same thing to read.
   fn role(self) -> ChipRole {
      match self {
         Self::Allowed => ChipRole::Success,
         Self::Confirmed => ChipRole::Info,
         Self::Denied => ChipRole::Error,
         Self::Waiting => ChipRole::Warning,
      }
   }
}

/// One cell of plain text, cut at the column edge rather than wrapped: a row is one line, and the
/// whole of the value is on the cell's node and in its hover.
fn text_cell(ui: &mut Ui, theme: &Theme, text: &str) -> Response {
   ui.add(
      Label::new(
         RichText::new(text).size(theme.typography.normal).color(theme.colors.text),
         None,
      )
      .wrap_mode(TextWrapMode::Truncate)
      .visuals(theme.label_visuals()),
   )
}

/// One cell of the header line: the column's name, smaller than the words under it so a row is read
/// before the line that names it.
fn header_cell(ui: &mut Ui, theme: &Theme, text: &str) {
   ui.label(RichText::new(text).size(theme.typography.small).color(theme.colors.text_muted));
}

/// How many pages `total` entries take. An empty log still has one — the empty page the view draws.
fn page_count(total: usize) -> usize {
   total.div_ceil(PER_PAGE).max(1)
}

/// The word for a count of entries.
fn noun(count: usize) -> &'static str {
   match count {
      1 => "entry",
      _ => "entries",
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The pager divides the count the store walks: every page but the last is full, and a log with
   /// nothing in it is the one empty page the view draws.
   #[test]
   fn the_page_count_is_the_ceiling_of_the_entries() {
      assert_eq!(page_count(0), 1);
      assert_eq!(page_count(1), 1);
      assert_eq!(page_count(PER_PAGE), 1);
      assert_eq!(page_count(PER_PAGE + 1), 2);
      assert_eq!(page_count(PER_PAGE * 7), 7);
   }
}
