//! The account panel, shown at the top of the left sidebar
//!
//! It allows the user to:
//! - select a chain
//! - select a wallet
//! - show the QR code for the selected wallet
//! - delegate to a smart contract
//! - delegate status of the current wallet (Green if not delegated, Red if delegated)

pub mod delegate;
pub mod qr_window;
pub mod vitals;

pub use delegate::DelegateUi;
pub use qr_window::QrWindow;
pub use vitals::SystemVitals;

use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusContext};
use crate::gui::{
   SHARED_GUI, SettingsPage,
   ui::{ChainSelect, WalletSelect, common::*},
};
use egui::{
   Align, CursorIcon, FontId, Layout, Margin, OpenUrl, Rect, Response, RichText, Role, ScrollArea,
   Shadow, Stroke, TextWrapMode, Ui, WidgetInfo, pos2, vec2,
};
use std::sync::Arc;
use zeus_eth::types::ChainId;

use egui_elements::{
   Button, Indicator, IndicatorState, Label, Menu, MenuItem, Tabs, TabsStyle, Theme,
};
use egui_lucide::Lucide;

const DELEGATE_TIP1: &str = "This wallet has been temporarily upgraded to a smart contract";
const DELEGATE_TIP2: &str = "This wallet is not upgraded to a smart contract";

/// `https://app.uniswap.org` → `app.uniswap.org`; the account panel is narrow.
fn short_origin(origin: &str) -> &str {
   let host = origin.split_once("://").map_or(origin, |(_, host)| host);
   host.trim_end_matches('/')
}

/// What the Railgun row says to a reader. The dot is hidden from the tree, so the row's label
/// carries the state: the service first, then where its sync stands.
fn railgun_row_name(state: IndicatorState) -> String {
   let word = match state {
      IndicatorState::On => "synced",
      IndicatorState::Off => "off",
      IndicatorState::Connecting => "syncing",
   };

   format!("Railgun, {word}")
}

/// What the Wallet Connector row says to a reader, in the Railgun row's shape: the dot is hidden
/// and the label names the service and its state.
fn wallet_connector_row_name(state: IndicatorState) -> String {
   let word = match state {
      IndicatorState::On => "running",
      IndicatorState::Off => "off",
      IndicatorState::Connecting => "starting",
   };

   format!("Wallet Connector, {word}")
}

/// Put a service row's one sentence on the node a reader hears.
///
/// AccessKit names a `Role::Label` node from the node's value rather than its label, so the
/// sentence replaces the drawn word there. The label gets the same sentence, so whichever of the
/// two a consumer reads, the row says exactly one thing.
pub(crate) fn name_service_row(ui: &Ui, label: Response, name: String) {
   ui.ctx().accesskit_node_builder(label.id, |node| {
      node.set_value(name.clone());
      node.set_label(name);
   });
}

/// Width of the panel's rows. The selectors set it; everything else matches it so
/// no row can stretch the panel past the sidebar.
const PANEL_ROW_WIDTH: f32 = 220.0;

/// The app list's slot. It is capped *and* floored at this height: capped so a long
/// list scrolls instead of growing the panel, floored because egui's default
/// `min_scrolled_height` (64) would otherwise win and make the list taller than the
/// panel's budget. A shorter list still shrinks to fit.
const DAPP_LIST_MAX_HEIGHT: f32 = 40.0;

/// Slack under the height clip for the card's shadow, so the clip never bites into the
/// panel once the animation has settled on the body's measured height.
const PANEL_CLIP_SLACK: f32 = 8.0;

/// The account panel, shown at the top of the left sidebar
///
/// It allows the user to:
/// - select a chain
/// - select a wallet
/// - show the QR code for the selected wallet
/// - delegate to a smart contract
/// - delegate status of the current wallet (Green if not delegated, Red if delegated)
/// - toggle the privacy mode
/// - check the status of the background services (Railgun, wallet connector)
pub struct AccountPanel {
   open: bool,
   /// Width the panel body may fill (the sidebar is 260 wide).
   body_width: f32,
   chain_select: ChainSelect,
   wallet_select: WalletSelect,
   wallet_info: WalletInfo,
   pub qr_window: QrWindow,
   pub delegate: DelegateUi,
   /// Active tab: 0 = Overview, 1 = Services.
   tab: usize,
   /// Natural body height of each tab (indexed by `tab`), measured while it is laid out.
   /// The height animation slides towards it; a tab that has never been shown has none.
   body_heights: [f32; 2],
   /// Machine vitals for the Diagnostics tab, refreshed off the frame path.
   vitals: SystemVitals,
   /// The words the panel itself wants said, drained by the left panel after the panel draws.
   notice: Option<String>,
   /// Whether the address row still has to take the focus a region shortcut asked for.
   focus_address_pending: bool,
}

impl AccountPanel {
   pub fn new() -> Self {
      let body_width = 260.0;

      let chain_select = ChainSelect::new("main_chain_select", 1).size(vec2(PANEL_ROW_WIDTH, 20.0));
      let wallet_select = WalletSelect::new("main_wallet_select").size(vec2(PANEL_ROW_WIDTH, 20.0));

      Self {
         open: false,
         body_width,
         chain_select,
         wallet_select,
         wallet_info: WalletInfo::default(),
         qr_window: QrWindow::new(),
         delegate: DelegateUi::new(),
         tab: 0,
         body_heights: [0.0; 2],
         vitals: SystemVitals::default(),
         notice: None,
         focus_address_pending: false,
      }
   }

   /// Draw or drop the field controls that only a sighted user can use.
   pub fn set_field_controls(&mut self, shown: bool) {
      self.delegate.set_field_controls(shown);
   }

   pub fn erase(&mut self) {
      self.delegate.erase();
   }

   /// Drop the panel's dialogs' secrets on auto-lock.
   ///
   /// The delegate form holds a typed target and a credentials form; the shutdown
   /// path erases it through [`Self::erase`], the lock path has to reach it too.
   pub fn lock(&mut self) {
      self.delegate.lock();
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Show the account overview and ask its address row to take the focus when the card draws.
   ///
   /// The row lives on the overview tab, so a region shortcut lands where the address is.
   pub fn focus_address(&mut self) {
      if !self.open {
         return;
      }

      self.tab = 0;
      self.focus_address_pending = true;
   }

   pub fn set_wallet_info(&mut self, wallet_info: WalletInfo) {
      self.wallet_info = wallet_info;
   }

   /// The address the card hands over, for the shortcut that copies it from another page.
   ///
   /// It is the string the card's own button copies — the EVM address, or the zk one while privacy
   /// mode is on — so a reader gets the same words whichever way they asked. `None` when privacy
   /// mode is on and the wallet has no zk address: the card draws a note rather than an address
   /// then, and there is nothing it could honestly hand over.
   pub fn address_to_copy(&self, privacy_mode: bool) -> Option<String> {
      match privacy_mode {
         false => Some(self.wallet_info.address.to_string()),
         true => self.wallet_info.has_zk_address().then(|| self.wallet_info.zk_address()),
      }
   }

   pub fn set_current_wallet(&mut self, wallet: WalletInfo) {
      self.wallet_select.wallet = wallet.clone();
      self.wallet_info = wallet;
   }

   pub fn set_current_chain(&mut self, chain: ChainId) {
      self.chain_select.chain = chain;
   }

   /// The words the panel wants said, drained by the left panel right after the panel draws.
   ///
   /// Neither the receive window nor the header's address copy can announce on its own: the frame
   /// holds the only channel, so each leaves its sentence here for the panel to hand up.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take().or_else(|| self.qr_window.take_notice())
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let chain = ctx.chain;
      let privacy_mode = ctx.privacy_mode;

      let evm_addr = self.wallet_info.address;

      self.delegate.show(ctx, theme, evm_addr, ui);

      self.qr_window.show(ctx, theme, ui);

      let frame2 = theme.frame2.outer_margin(Margin::same(10));

      // The nav below the panel rides the panel's height, so what the panel takes has to be
      // the *animated* height of its body and not the body's own.
      let frame_margins = frame2.inner_margin.sum().y + frame2.outer_margin.sum().y;
      let panel_top = ui.cursor().top();

      // Slide to the tab's height instead of jumping. A tab that has not been laid out
      // yet has no height to slide to, so the first visit renders naturally.
      let target = self.body_heights[self.tab];
      let animate = target > 0.0;
      let height = if animate {
         ui.ctx().animate_value_with_time(
            ui.scope_id().with("panel_body_height"),
            target,
            ui.style().animation_time,
         )
      } else {
         0.0
      };

      // While it moves, hold a shrinking body at the animated height and clip the painted
      // output to it: the forced minimum is what keeps a shrinking body from snapping, the
      // clip is what keeps a growing one from painting past the animation (egui never clips a
      // child on its own). The nav below is pinned to the same height at the end of `show`.
      let clip = ui.clip_rect();
      if animate {
         let bottom = (panel_top + height + frame_margins + PANEL_CLIP_SLACK).min(clip.max.y);
         ui.set_clip_rect(Rect::from_min_max(
            clip.min,
            pos2(clip.max.x, bottom),
         ));
      }

      let mut measured = 0.0;

      frame2.show(ui, |ui| {
         // The card's rows own the space between them; the ui the card is shown in is only
         // the gap between the panel and the nav below it (set by the panel's host).
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
         ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs);

         ui.set_max_width(self.body_width);
         if animate {
            ui.set_min_height(height);
         }

         ui.vertical(|ui| {
            // Tab strip: Overview (wallet/chain) and Diagnostics.
            Tabs::new(
               "account_tabs",
               &mut self.tab,
               ["Overview", "Diagnostics"],
            )
            .style(TabsStyle::Underline)
            .fill_width(false)
            .show(ui);

            ui.add_space(5.0);

            match self.tab {
               0 => self.show_overview(ctx, theme, &icons, privacy_mode, chain, ui),
               1 => self.show_services(ctx, theme, ui),
               _ => {}
            }

            // Where this tab's body naturally ends, for the animation to slide towards.
            measured = ui.min_rect().height();
         });
      });

      if animate {
         ui.set_clip_rect(clip);
      }

      self.body_heights[self.tab] = measured;

      // Pin the space the panel consumed to the animated height, so the nav below rides the
      // same slide. A body *shorter* than the animated height is already held by
      // `set_min_height`; a *taller* one takes the nav down with it — egui grows a parent
      // along with its child — and the nav would move ahead of the animation.
      if animate {
         let mut consumed = ui.min_rect();
         consumed.max.y = panel_top + height + frame_margins;
         ui.advance_cursor_after_rect(consumed);
      }
   }

   /// Overview tab
   fn show_overview(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Arc<Icons>,
      privacy_mode: bool,
      chain: ChainId,
      ui: &mut Ui,
   ) {
      let focus_address = std::mem::take(&mut self.focus_address_pending);

      ui.horizontal(|ui| {
         self.show_chain_select(ctx, theme, icons.clone(), ui);
      });

      ui.horizontal(|ui| {
         self.show_wallet_select(ctx, theme, icons.clone(), ui);
      });

      let wallet = &self.wallet_info;
      let icon_color = theme.colors.text;

      let mut btn_visuals = theme.button_visuals();
      btn_visuals.shadow = Shadow::NONE;
      btn_visuals.border_hover = Stroke::NONE;

      let normal = theme.typography.normal;

      let frame2 = theme.frame2.inner_margin(5);

      // `set_width` sizes the frame's *content* box, and the frame paints its inner
      // margin outside of it, so a row has to ask for one `inner_margin` less than
      // `PANEL_ROW_WIDTH` (the selectors' total width) to line up with its siblings.
      let row_width = PANEL_ROW_WIDTH - frame2.inner_margin.sum().x;

      // Wallet address, on click copy it to the clipboard
      frame2.show(ui, |ui| {
         ui.set_width(row_width);

         ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing.xs;

            let address = match privacy_mode {
               false => wallet.evm_address_truncated(),
               true => wallet.zk_address_truncated(),
            };

            let full_address = match privacy_mode {
               false => wallet.address.to_string(),
               true => wallet.zk_address(),
            };

            // Privacy mode on a wallet we cannot derive a zk address for: the row is
            // a muted note. The copy / QR / explorer actions would act on the note
            // instead of an address, so they are hidden.
            if privacy_mode && !wallet.has_zk_address() {
               let text = RichText::new(address).size(normal).color(theme.colors.text_muted);

               // Keep the clickable label's height so hiding the actions does not
               // shift the rows below.
               let text_height =
                  ui.ctx().fonts_mut(|f| f.row_height(&FontId::proportional(normal)));
               let height = (text_height + 2.0 * ui.spacing().button_padding.y)
                  .max(ui.spacing().interact_size.y);

               ui.allocate_ui_with_layout(
                  vec2(ui.available_width(), height),
                  Layout::left_to_right(Align::Center),
                  |ui| {
                     // Same inset as the clickable label's button padding.
                     ui.add_space(ui.spacing().button_padding.x);
                     ui.add(Label::new(text, None).interactive(false));
                  },
               );

               return;
            }

            let address_text = RichText::new(address).size(normal);
            let label = Button::selectable(false, address_text);

            let address_button = ui.add(label);
            if focus_address {
               address_button.request_focus();
            }

            if address_button.clicked() {
               // The words name the address: a listener has no other way to check what landed on
               // the clipboard.
               self.notice = Some(copied_words(&full_address));
               ui.ctx().copy_text(full_address);
            }

            // The QR / block-explorer actions, and the separators before them, are
            // anchored to the row's right edge: laid out after the text they would
            // follow its width, and the zk address is shorter than the evm one, so
            // they would slide out of line with the rows below. Right-to-left, so
            // they are added rightmost first.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               // Block explorer link
               let block_explorer = chain.block_explorer();
               let link = format!("{}/address/{}", block_explorer, wallet.address);
               let icon = Lucide::ExternalLink.size(18.0).color(icon_color).image();

               // A picture of a link: the text alternative says where pressing it goes.
               let button = Button::image(icon)
                  .visuals(btn_visuals)
                  .small()
                  .alt("Open the address in the block explorer");
               let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               if res.clicked() {
                  let url = OpenUrl::new_tab(link);
                  ui.ctx().open_url(url);
               }

               ui.separator();

               let icon = Lucide::QrCode.size(18.0).color(icon_color).image();

               // A picture of a QR code: the text alternative names what pressing it opens, in
               // the words the dialog's own button carries.
               let button = Button::image(icon).visuals(btn_visuals).small().alt("Show QR Code");
               let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               // QR Code Window
               if res.clicked() {
                  self.qr_window.open(wallet.clone(), Some(res.id));
               }

               ui.separator();
            });
         });
      });

      // Wallet delegated status
      frame2.show(ui, |ui| {
         ui.set_width(row_width);

         ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing.sm;

            let deleg_addr = ctx.delegated_wallets.get(chain.id(), wallet.address);

            let text = match deleg_addr.is_some() {
               true => RichText::new("Delegated").size(normal),
               false => RichText::new("Not Delegated").size(normal),
            };

            let label = Label::new(text, None).interactive(false);

            let tip = if deleg_addr.is_some() {
               DELEGATE_TIP1
            } else {
               DELEGATE_TIP2
            };

            let tip_text = RichText::new(tip).size(normal);

            let state = match deleg_addr.is_some() {
               true => IndicatorState::Off,
               false => IndicatorState::On,
            };

            // The dot is Off precisely when the wallet is delegated, and it tells a reader
            // "status: off", the opposite of the word beside it. The word is the row's only node.
            let indicator = ui.add(Indicator::new(state).size(12.0));
            ui.ctx().accesskit_node_builder(indicator.id, |node| node.set_hidden());

            ui.add(label).on_hover_text(tip_text);

            // Align this separator with the one above it.
            ui.add_space(1.2);

            ui.separator();

            let size = vec2(24.0, 12.0);
            let more = dots_button(theme, size, ui);
            let options = "Delegation options";
            more.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), options));

            if more.clicked() && !self.delegate.is_open() {
               self.delegate.open();
            }
         });
      });

      frame2.show(ui, |ui| {
         ui.set_width(row_width);
         privacy_mode_switch(ctx, theme, ui);
      });

      // The apps this account is exposed to, last so it never pushes the fixed
      // rows around. A dedicated account stops being self-explanatory once it is
      // no longer the selected one, and the panel is narrow, so the list scrolls
      // rather than growing the panel.
      let used_by: Vec<String> = ctx
         .connected_dapps()
         .into_iter()
         .filter(|origin| ctx.dapp_account(origin) == Some(wallet.address))
         .collect();

      if !used_by.is_empty() {
         // Box width must match the panel's other rows: filling the panel's max
         // width instead widens the frame past the sidebar.
         let list_size = vec2(PANEL_ROW_WIDTH, DAPP_LIST_MAX_HEIGHT);

         ui.allocate_ui(list_size, |ui| {
            let text = RichText::new("Connected dApps (Public Mode)").size(theme.typography.small);
            ui.label(text);

            // Capped and floored at the slot height, so the body's height cannot depend
            // on how many apps are connected.
            let list = ScrollArea::vertical()
               .auto_shrink([false, true])
               .max_height(DAPP_LIST_MAX_HEIGHT)
               .min_scrolled_height(DAPP_LIST_MAX_HEIGHT);

            list.show(ui, |ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.xs;

               for origin in &used_by {
                  let text = RichText::new(short_origin(origin))
                     .size(theme.typography.small)
                     .color(theme.colors.text_muted);
                  let icon = Lucide::Link.size(14.0).color(theme.colors.text_muted).image();

                  // Truncate rather than wrap: a long domain must not widen the row.
                  ui.add(
                     Label::new(text, Some(icon))
                        .image_on_left()
                        .wrap_mode(TextWrapMode::Truncate)
                        .interactive(false),
                  )
                  .on_hover_text(RichText::new(origin).size(theme.typography.small));
               }
            });
         });
      }
   }

   /// Services tab
   fn show_services(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let chain = ctx.chain;
      let railgun_is_supported = ctx.railgun_is_supported(chain);

      ui.spacing_mut().item_spacing.y = theme.spacing.sm;

      let frame = theme.frame1.inner_margin(Margin::same(5));
      let frame_height = 40.0;
      let frame_width = self.body_width - 50.0;

      // Railgun Status
      frame.show(ui, |ui| {
         ui.set_max_width(frame_width);
         ui.set_height(frame_height);

         ui.horizontal(|ui| {
            let railgun_synced = ctx.railgun_status().synced(chain.id());
            let mut sync_state = match railgun_synced {
               true => IndicatorState::On,
               false => IndicatorState::Connecting,
            };

            if !railgun_is_supported || !ctx.is_railgun_enabled(chain.id()) {
               sync_state = IndicatorState::Off;
            }

            let indicator = ui.add(Indicator::new(sync_state));
            ui.ctx().accesskit_node_builder(indicator.id, |node| node.set_hidden());

            ui.add_space(10.0);

            let label = RichText::new("Railgun").size(theme.typography.small);
            let label = ui.label(label);
            name_service_row(ui, label, railgun_row_name(sync_state));

            ui.add_space(10.0);

            ui.vertical(|ui| {
               ui.spacing_mut().item_spacing.y = 0.0;
               let text = RichText::new("Synced block")
                  .size(theme.typography.small)
                  .color(theme.colors.text_muted);
               ui.label(text);

               let block = ctx.railgun_status().synced_block(chain.id());
               let text = RichText::new(format!("{}", block)).size(theme.typography.small);
               ui.label(text);
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let size = vec2(24.0, 12.0);
               let more = dots_button(theme, size, ui);
               let options = "Railgun options";
               more.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), options));
               Menu::new(("svc_menu", "railgun_id")).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("View last error")).clicked() {
                     let error_opt = ctx.railgun_status().sync_error(chain.id());
                     let error =
                        error_opt.unwrap_or("No errors for now everything looks good".to_string());

                     crate::runtime::rt().spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.msg_window.open(error);
                           gui.request_repaint();
                        });
                     });
                  }

                  if ui.add(MenuItem::new("Settings")).clicked() {
                     crate::runtime::rt().spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.ctx.clone().write(|ctx| {
                              gui.settings.open_page(
                                 SettingsPage::Railgun,
                                 ctx,
                                 &mut gui.appearance_settings,
                              );
                           });
                           gui.request_repaint();
                        });
                     });
                  }
               });
            });
         });
      });

      // Wallet Connector Status
      frame.show(ui, |ui| {
         ui.set_max_width(frame_width);
         ui.set_height(frame_height);

         ui.horizontal(|ui| {
            let running = ctx.server_running;
            let state = match running {
               true => IndicatorState::On,
               false => IndicatorState::Connecting,
            };

            let indicator = ui.add(Indicator::new(state));
            ui.ctx().accesskit_node_builder(indicator.id, |node| node.set_hidden());

            ui.add_space(10.0);

            let label = RichText::new("Wallet Connector").size(theme.typography.small);
            let label = ui.label(label);
            name_service_row(ui, label, wallet_connector_row_name(state));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let size = vec2(24.0, 12.0);
               let more = dots_button(theme, size, ui);
               let options = "Wallet Connector options";
               more.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), options));
               Menu::new(("svc_menu", "wallet_connector_id")).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("Settings")).clicked() {
                     crate::runtime::rt().spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.msg_window.open("Not implemented yet");
                           gui.request_repaint();
                        });
                     });
                  }
               });
            });
         });
      });

      // Local vitals
      self.vitals.refresh_if_stale();
      vitals::card(theme, ui, &self.vitals);
   }

   fn show_chain_select(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      ui.vertical(|ui| {
         let clicked = self.chain_select.show(ctx, &[0], theme, icons.clone(), ui);
         if clicked {
            switch_chain(ctx, self.chain_select.chain);
         }
      });
   }

   fn show_wallet_select(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      ui.vertical(|ui| {
         let clicked = self.wallet_select.show(theme, ctx, icons.clone(), ui);
         if clicked {
            // The selector already holds the picked wallet; the switch hook moves the rest of the
            // app onto it.
            switch_wallet(ctx, self.wallet_select.wallet.clone());
         }
      });
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// A service row's dot is hidden from the tree, so its one node is the label: the service and
   /// its state, in reading order, never the dot's "status: on".
   #[test]
   fn the_railgun_row_says_the_service_and_its_state() {
      assert_eq!(
         railgun_row_name(IndicatorState::On),
         "Railgun, synced"
      );
      assert_eq!(
         railgun_row_name(IndicatorState::Connecting),
         "Railgun, syncing"
      );
      assert_eq!(
         railgun_row_name(IndicatorState::Off),
         "Railgun, off"
      );
   }

   /// The Wallet Connector row follows the same shape, with the words that connector uses.
   #[test]
   fn the_wallet_connector_row_says_the_service_and_its_state() {
      assert_eq!(
         wallet_connector_row_name(IndicatorState::On),
         "Wallet Connector, running"
      );
      assert_eq!(
         wallet_connector_row_name(IndicatorState::Connecting),
         "Wallet Connector, starting"
      );
      assert_eq!(
         wallet_connector_row_name(IndicatorState::Off),
         "Wallet Connector, off"
      );
   }
}
