//! The wallet grants: one row per wallet, and the switches for one wallet in a dialog.
//!
//! A grant is per wallet, so the shape a user needs is a comparison — which wallets may act, and how much
//! of each — and a comparison is a table. The page therefore lists the wallets as rows of one grid, with a
//! single action per row; the eleven switches a wallet can hold are a dialog, opened from its row.
//!
//! Nothing here decides anything: the switch that flips a capability is the same one the dialog always
//! had, so what a row says and what the dialog does cannot drift apart.

use super::CONTROL;
use super::profile_editor::{Edited, ProfileEditor};
use crate::core::WalletInfo;
use crate::core::agent::AgentProfile;
use crate::gui::ui::common::section_heading;
use egui::{Id, RichText, Ui, vec2};
use egui_elements::{Button, Column, Label, Table, TableScroll, Theme};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use zeus_eth::alloy_primitives::Address;

/// The height of a table row: its cell padding above and below the one control the row holds.
///
/// A row is as tall as the button in it plus the air around it — 34 was less than `CONTROL` itself, so
/// the Edit button was drawn touching both edges of its row.
const ROW: f32 = CONTROL + 16.0;

/// The gap between two columns: wide enough that a wallet's name and its address read as two fields
/// rather than as one run of text.
const COLUMN_GAP: f32 = 24.0;

/// The dialog's width, and the ceiling its body scrolls under.
const MODAL_WIDTH: f32 = 560.0;
const MODAL_BODY_HEIGHT: f32 = 460.0;

/// What a wallet's row says the grant is, in one line.
struct Row {
   name: String,
   address: Address,
   short: String,
   grant: String,
}

/// The Wallets screen's table, and the dialog it opens.
#[derive(Default)]
pub struct WalletGrants {
   /// The wallet the dialog is open on, and the profile it belongs to.
   editing: Option<(Address, super::AgentProfileId)>,
}

impl WalletGrants {
   /// Close the dialog, which is what leaving the agent or the window does.
   pub fn close(&mut self) {
      self.editing = None;
   }

   /// Draw the table, then the dialog when a row asked for it.
   pub fn show(
      &mut self,
      profile: &mut AgentProfile,
      wallets: &[WalletInfo],
      theme: &Theme,
      ui: &mut Ui,
   ) -> Edited {
      let mut edited = Edited::default();

      section_heading(ui, theme.typography.large, "Wallets");

      if wallets.is_empty() {
         let text = RichText::new("There is no wallet to grant anything on yet.")
            .size(theme.typography.normal);
         let label = Label::text(text).wrap();
         ui.add(label);

         return edited;
      }

      let text = "What this agent may do on each wallet. Nothing is granted until you open a wallet \
          and turn a switch on.";
      let label = Label::text(RichText::new(text).size(theme.typography.normal)).wrap();
      ui.add(label);

      ui.add_space(theme.spacing.sm);

      let rows: Vec<Row> = wallets
         .iter()
         .map(|wallet| Row {
            name: wallet.name().to_owned(),
            address: wallet.address,
            short: short(wallet.address),
            grant: row_summary(profile, wallet.address),
         })
         .collect();

      // The table is measured once and placed on every frame, so its key is the row count: a wallet
      // added or removed is the only thing that changes what a row occupies.
      let key = hash(&profile.id);
      let mut open = None;

      Table::new(("agent_wallet_grants", key), &rows)
         .columns([
            Column::Fixed(200.0),
            Column::Fixed(170.0),
            Column::Fr(1.0),
            Column::Fixed(90.0),
         ])
         .gap(COLUMN_GAP)
         .row_gap(theme.spacing.xs)
         .row_height(ROW)
         .row_divider(true)
         .header_rule(true)
         .scroll(TableScroll::new().max_height(320.0).auto_shrink([false, true]))
         .show(ui, |table| {
            table.header(|header| {
               header.cell_named("Wallet", |ui| {
                  ui.label(RichText::new("Wallet").size(theme.typography.normal));
               });

               header.cell_named("Address", |ui| {
                  ui.label(RichText::new("Address").size(theme.typography.normal));
               });

               header.cell_named("Grant", |ui| {
                  ui.label(RichText::new("Grant").size(theme.typography.normal));
               });

               header.cell_named("Action", |ui| {
                  ui.label(RichText::new("Action").size(theme.typography.normal));
               });
            });

            table.rows(|row, wallet| {
               row.cell_named(wallet.name.clone(), |ui| {
                  ui.add(
                     egui::Label::new(RichText::new(&wallet.name).size(theme.typography.normal))
                        .wrap(),
                  );
               });

               row.cell_named(wallet.short.clone(), |ui| {
                  ui.add(
                     egui::Label::new(
                        RichText::new(&wallet.short).monospace().size(theme.typography.small),
                     )
                     .wrap(),
                  );
               });

               let grant = wallet.grant.clone();

               row.cell_named(grant.clone(), |ui| {
                  ui.add(
                     egui::Label::new(RichText::new(grant).size(theme.typography.small)).wrap(),
                  );
               });

               row.cell_named("Edit", |ui| {
                  let button = Button::new(RichText::new("Edit").size(theme.typography.normal))
                     .min_size(vec2(80.0, CONTROL))
                     .label_align(egui::Align::Center);

                  let response = ui.add(button);

                  // Every row draws the same word, so the button is named after the wallet it opens.
                  response.widget_info(|| {
                     egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        ui.is_enabled(),
                        format!("Edit the grant for {}", wallet.name),
                     )
                  });

                  if response.clicked() {
                     open = Some(wallet.address);
                  }
               });
            });
         });

      if let Some(address) = open {
         self.editing = Some((address, profile.id.clone()));
      }

      self.dialog(ui.ctx(), profile, wallets, theme, &mut edited);

      edited
   }

   /// The switches for one wallet, in a dialog over the table.
   fn dialog(
      &mut self,
      egui_ctx: &egui::Context,
      profile: &mut AgentProfile,
      wallets: &[WalletInfo],
      theme: &Theme,
      edited: &mut Edited,
   ) {
      let Some((address, owner)) = self.editing.clone() else {
         return;
      };

      // A dialog belongs to one agent: switching agents, or losing the wallet, closes it rather than
      // leaving the previous one's switches on screen.
      if owner != profile.id {
         self.editing = None;

         return;
      }

      let Some(wallet) = wallets.iter().find(|wallet| wallet.address == address) else {
         self.editing = None;

         return;
      };

      let mut open = true;
      let id = Id::unique(("agent_wallet_grant", address));
      let heading = RichText::new(wallet.name()).size(theme.typography.heading);
      let subtitle = RichText::new(short(address)).size(theme.typography.small);
      let frame = theme.window_frame.fill(theme.frame1.fill);

      egui_elements::Modal::new(id, &mut open)
         .backdrop_order(egui::Order::Middle)
         .content_order(egui::Order::Foreground)
         .heading(heading)
         .subtitle(subtitle)
         .accessible_name(format!("Grant for {}", wallet.name()))
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .close_on_backdrop(true)
         .close_on_escape(true)
         .width(MODAL_WIDTH)
         .frame(frame)
         .show(egui_ctx, |ui| {
            ui.set_width(MODAL_WIDTH);

            // One wallet's switches are taller than a laptop screen once both allowlists have
            // entries, so the body scrolls inside a ceiling rather than growing past the window.
            egui::ScrollArea::vertical()
               .id_salt("agent_wallet_grant_body")
               .max_height(MODAL_BODY_HEIGHT)
               .auto_shrink([false, true])
               .show(ui, |ui| {
                  ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.sm);

                  let mut editor = ProfileEditor::default();

                  let pass = editor.wallet(profile, wallet, theme, ui);

                  if pass.changed {
                     edited.changed = true;
                  }

                  if let Some(sentence) = pass.notice {
                     edited.notice = Some(sentence);
                  }
               });
         });

      if !open {
         self.editing = None;
      }
   }
}

/// What one wallet's grant reads as, in the three states it can be in.
///
/// No grant and a grant switched off are different things, and this column is the only place a user can
/// tell them apart while scanning: "Nothing granted" is a wallet nobody has set up, "Off" is one that was
/// set up and then taken away.
fn row_summary(profile: &AgentProfile, address: Address) -> String {
   let Some(grant) = profile.wallets.get(&address) else {
      return "Nothing granted.".to_owned();
   };

   if !grant.enabled {
      return if grant.caps.is_empty() {
         "Nothing granted.".to_owned()
      } else {
         "Off".to_owned()
      };
   }

   if grant.caps.is_empty() {
      return "Nothing granted.".to_owned();
   }

   let mut parts = vec![format!(
      "{} {}",
      grant.caps.len(),
      if grant.caps.len() == 1 {
         "permission"
      } else {
         "permissions"
      }
   )];

   let tokens = match &grant.tokens {
      crate::core::agent::TokenScope::None => "no tokens",
      crate::core::agent::TokenScope::All => "every token",
      crate::core::agent::TokenScope::Allowlist { .. } => "some tokens",
   };

   let nfts = match &grant.nfts {
      crate::core::agent::NftScope::None => "no NFTs",
      crate::core::agent::NftScope::All => "every NFT",
      crate::core::agent::NftScope::Allowlist { .. } => "some NFTs",
   };

   parts.push(tokens.to_owned());
   parts.push(nfts.to_owned());

   parts.join(" · ")
}

fn short(address: Address) -> String {
   let text = format!("{address:#x}");
   format!("{}…{}", &text[..8], &text[text.len() - 6..])
}

/// A stable key for the table, from anything the caller already has.
fn hash(value: impl Hash) -> u64 {
   let mut hasher = DefaultHasher::new();
   value.hash(&mut hasher);
   hasher.finish()
}
