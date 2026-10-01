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
         result: None,
         opened_at: None,
         size: (460.0, 620.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Open the prompt for `origin`.
   ///
   /// `remembered` is the account this app was last connected with. New apps
   /// default to a fresh app-specific account; known apps default to the
   /// remembered one so the app keeps seeing the same account.
   pub fn open(&mut self, origin: String, remembered: Option<Address>, wallets: Vec<WalletInfo>) {
      self.open = true;
      self.use_new_account = remembered.is_none();
      self.selected = remembered.or_else(|| wallets.first().map(|w| w.address));
      self.origin = origin;
      self.wallets = wallets;
      self.result = None;
      self.opened_at = Some(Instant::now());
   }

   pub fn get_result(&self) -> Option<DappConnectResult> {
      self.result
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      self.origin.clear();
      self.wallets.clear();
      self.selected = None;
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
                                 let text = RichText::new(wallet.name_with_id_short()).size(normal);

                                 let row = Label::new(text, None)
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
