//! Account picker shown when an app asks to connect.
//!
//! Walletbeat's "app isolation" attribute wants each app to get its own account
//! by default, so apps cannot correlate the user's activity across sites. The
//! prompt therefore offers a fresh, app-specific account as the default and lets
//! the user fall back to an existing account when they want composability.
//! Reconnecting an app defaults to the account it was connected with before.

use crate::core::WalletInfo;
use crate::gui::ui::common::delayed_action_label;
use egui::{Id, Order, RichText, ScrollArea, Sense, Ui, vec2};
use egui_elements::{Button, Label, Modal, Theme};
use std::time::{Duration, Instant};
use zeus_eth::alloy_primitives::Address;

/// The account the user chose for an app connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DappAccountChoice {
   /// Derive a child account dedicated to this app.
   NewAccount,
   /// Expose an existing account.
   Existing(Address),
}

/// Outcome of the app-connection prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DappConnectResult {
   Approved(DappAccountChoice),
   Rejected,
}

/// Tallest the wallet list box grows; it shrinks to fit a short list.
const WALLET_LIST_MAX_HEIGHT: f32 = 240.0;

/// Prompt shown when an app requests a connection.
///
/// Mirrors [`ConfirmWindow`](crate::gui::ConfirmWindow)'s contract: the server
/// opens it, polls [`get_result`](Self::get_result), then resets it.
pub struct ConnectDappWindow {
   open: bool,
   origin: String,
   /// `true` when the default "create a new account" option is selected.
   use_new_account: bool,
   wallets: Vec<WalletInfo>,
   selected: Option<Address>,
   /// Every account this app has been given. Each one is marked in the list, so
   /// the user can see which wallets the app already knows about.
   seen_accounts: Vec<Address>,
   pub result: Option<DappConnectResult>,
   /// When the prompt became visible. Drives the Confirm-style delay so a
   /// focus-steal click cannot approve the connection.
   opened_at: Option<Instant>,
   size: (f32, f32),
}

impl ConnectDappWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         origin: String::new(),
         use_new_account: true,
         wallets: Vec::new(),
         selected: None,
         seen_accounts: Vec::new(),
         result: None,
         opened_at: None,
         size: (460.0, 620.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// `true` while a connection is being decided: the prompt is visible, or a
   /// decision is waiting for the server to collect it.
   ///
   /// The server refuses a second connect request while busy — one shared
   /// prompt cannot belong to two origins at once.
   pub fn is_busy(&self) -> bool {
      self.open || self.result.is_some()
   }

   /// Open the prompt for `origin`.
   ///
   /// `remembered` is the account this app was last connected with; new apps
   /// default to a fresh app-specific account, known apps default to the
   /// remembered one so the app keeps seeing the same account. `seen` is every
   /// account the app has been given, marked in the list because the app still
   /// knows those accounts.
   pub fn open(
      &mut self,
      origin: String,
      remembered: Option<Address>,
      seen: Vec<Address>,
      wallets: Vec<WalletInfo>,
   ) {
      self.open = true;
      self.use_new_account = remembered.is_none();
      self.selected = remembered.or_else(|| wallets.first().map(|w| w.address));
      self.seen_accounts = seen;
      self.origin = origin;
      self.wallets = wallets;
      self.result = None;
      self.opened_at = Some(Instant::now());
   }

   pub fn get_result(&self) -> Option<DappConnectResult> {
      self.result
   }

   /// Close and answer the server with a rejection. Used when auto-lock fires,
   /// so a connection prompt cannot be approved while locked.
   pub fn reject(&mut self) {
      self.result = Some(DappConnectResult::Rejected);
      self.close();
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      self.origin.clear();
      self.wallets.clear();
      self.selected = None;
      self.seen_accounts.clear();
      self.use_new_account = true;
      self.result = None;
      self.opened_at = None;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let normal = theme.typography.normal;
      let large = theme.typography.large;
      let very_large = theme.typography.very_large;

      let title = RichText::new("Connect to App").size(very_large);
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let frame2 = theme.frame2;
      let button_visuals = theme.button_visuals();
      let mut open = self.open;

      // A selectable option row, shared by the two choices.
      let option_row = |ui: &mut Ui, text: &str, selected: bool| -> bool {
         ui.spacing_mut().button_padding = theme.button_padding;

         let label = Label::new(RichText::new(text).size(large), None)
            .fill_width(true)
            .interactive(true)
            .expand(Some(4.0))
            .selected(selected)
            .sense(Sense::click());
         ui.add(label).clicked()
      };

      Modal::new(Id::new("connect_dapp_window"), &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(false)
         .frame(frame)
         .max_width(self.size.0)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);
            ui.spacing_mut().item_spacing.y = theme.spacing.md;
            ui.spacing_mut().button_padding = theme.button_padding;

            ui.vertical_centered(|ui| {
               ui.label(RichText::new(&self.origin).size(large));
            });

            // Left-aligned body so wrapped copy and the option rows share one edge.
            let content_width = ui.available_width();
            ui.horizontal(|ui| {
               ui.add_space(theme.spacing.md);
               ui.vertical(|ui| {
                  ui.set_width(content_width - theme.spacing.md * 2.0);
                  ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                  let text = "Apps can see your onchain history. Zeus can create an \
                              account dedicated to this app, so the app cannot link \
                              your activity across sites.";
                  ui.add(
                     Label::new(RichText::new(text).size(normal), None)
                        .wrap()
                        .fill_width(true)
                        .interactive(false),
                  );

                  ui.add_space(theme.spacing.xs);

                  if option_row(
                     ui,
                     "Create a new account for this app",
                     self.use_new_account,
                  ) {
                     self.use_new_account = true;
                  }

                  ui.add_space(theme.spacing.lg);

                  if option_row(
                     ui,
                     "Use an existing account",
                     !self.use_new_account,
                  ) {
                     self.use_new_account = false;
                  }

                  ui.add_space(theme.spacing.lg);

                  if !self.use_new_account {
                     let mut picked = None;

                     // Reserve the box explicitly: a bare `ScrollArea` inside this
                     // nested layout only ever gets the leftover height, so it
                     // clips the list instead of scrolling it. `auto_shrink.y`
                     // keeps the box snug while the list is short.
                     let list_size = vec2(ui.available_width(), WALLET_LIST_MAX_HEIGHT);

                     ui.allocate_ui(list_size, |ui| {
                        ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                           ui.spacing_mut().item_spacing.y = theme.spacing.lg;

                           frame2.show(ui, |ui| {
                              for wallet in &self.wallets {
                                 let is_selected = self.selected == Some(wallet.address);

                                 let mut parts =
                                    vec![RichText::new(wallet.name_with_id_short()).size(normal)];

                                 // Explains why this row is the one already selected, and tells
                                 // the user the app already knows this account.
                                 if self.seen_accounts.contains(&wallet.address) {
                                    let muted = theme.colors.text_muted;
                                    let small = theme.typography.small;

                                    parts.push(RichText::new(" · ").size(small).color(muted));
                                    parts.push(
                                       RichText::new("Previously connected")
                                          .size(small)
                                          .color(muted),
                                    );
                                 }

                                 let row = Label::sections(parts, None)
                                    .fill_width(true)
                                    .interactive(true)
                                    .selected(is_selected)
                                    .expand(Some(6.0))
                                    .sense(Sense::click());

                                 if ui.add(row).clicked() {
                                    picked = Some(wallet.address);
                                 }
                              }
                           });
                        });
                     });

                     if let Some(address) = picked {
                        self.selected = Some(address);
                     }
                  }
               });
            });

            ui.add_space(theme.spacing.md);

            ui.vertical_centered(|ui| {
               let button_size = vec2(
                  (ui.available_width() - theme.spacing.xl) * 0.5,
                  45.0,
               );

               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.xl;

                  let (ready, label) = delayed_action_label(self.opened_at, "Connect");
                  if !ready {
                     ui.ctx().request_repaint_after(Duration::from_millis(100));
                  }

                  let connect = Button::new(RichText::new(label).size(theme.typography.normal))
                     .visuals(button_visuals)
                     .min_size(button_size);

                  if ui.add_enabled(ready, connect).clicked() {
                     let choice = if self.use_new_account {
                        DappAccountChoice::NewAccount
                     } else {
                        self
                           .selected
                           .map(DappAccountChoice::Existing)
                           .unwrap_or(DappAccountChoice::NewAccount)
                     };
                     self.result = Some(DappConnectResult::Approved(choice));
                     self.close();
                  }

                  let reject = Button::new(RichText::new("Reject").size(theme.typography.normal))
                     .visuals(button_visuals)
                     .min_size(button_size);

                  if ui.add(reject).clicked() {
                     self.result = Some(DappConnectResult::Rejected);
                     self.close();
                  }
               });
            });
         });
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   fn address(byte: u8) -> Address {
      Address::from([byte; 20])
   }

   fn wallet(address: Address) -> WalletInfo {
      let mut wallet = WalletInfo::default();
      wallet.address = address;
      wallet
   }

   /// A known app defaults to the account it was last connected with, and every
   /// account it has been given is marked — not only that one.
   #[test]
   fn known_app_preselects_last_account_and_marks_every_seen_one() {
      let (last_used, earlier, untouched) = (address(1), address(2), address(3));

      let mut window = ConnectDappWindow::new();
      window.open(
         "https://app.example".to_string(),
         Some(last_used),
         vec![earlier, last_used],
         vec![wallet(last_used), wallet(earlier), wallet(untouched)],
      );

      assert!(!window.use_new_account);
      assert_eq!(window.selected, Some(last_used));
      assert_eq!(window.seen_accounts, vec![earlier, last_used]);
      assert!(!window.seen_accounts.contains(&untouched));

      window.reset();
      assert!(window.seen_accounts.is_empty());
   }

   /// A new app marks nothing as previously connected; the list's preselection
   /// must not leak into the default choice.
   #[test]
   fn new_app_marks_nothing_previously_connected() {
      let first = address(1);

      let mut window = ConnectDappWindow::new();
      window.open(
         "https://app.example".to_string(),
         None,
         Vec::new(),
         vec![wallet(first)],
      );

      assert!(window.use_new_account);
      assert!(window.seen_accounts.is_empty());
      // Selected only so the "use an existing account" branch opens on a row.
      assert_eq!(window.selected, Some(first));
   }

   /// A prompt is busy while it is visible and until its decision is collected,
   /// so a second connect request cannot overwrite it.
   #[test]
   fn prompt_is_busy_from_open_until_reset() {
      let mut window = ConnectDappWindow::new();
      assert!(!window.is_busy());

      window.open(
         "https://app.example".to_string(),
         None,
         Vec::new(),
         Vec::new(),
      );
      assert!(window.is_busy());

      // A decision was taken but the server has not collected it yet: the
      // prompt is closed, still busy.
      window.result = Some(DappConnectResult::Rejected);
      window.close();
      assert!(window.is_busy());

      window.reset();
      assert!(!window.is_busy());
   }

   /// Auto-lock cancels a pending connection prompt: it is answered "rejected"
   /// so the server stops waiting, and cannot be approved from a locked UI.
   #[test]
   fn reject_records_a_rejection_and_closes() {
      let mut window = ConnectDappWindow::new();
      window.open(
         "https://app.example".to_string(),
         None,
         Vec::new(),
         vec![wallet(address(1))],
      );

      window.reject();

      assert!(!window.is_open());
      assert_eq!(window.result, Some(DappConnectResult::Rejected));
      // Still busy until the server collects the decision.
      assert!(window.is_busy());
   }
}
