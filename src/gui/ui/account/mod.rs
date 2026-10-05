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
use crate::utils::RT;
use egui::{
   Align, CursorIcon, FontId, Layout, Margin, OpenUrl, Rect, RichText, ScrollArea, Shadow, Stroke,
   TextWrapMode, Ui, pos2, vec2,
};
use std::sync::Arc;
use zeus_eth::types::ChainId;

use egui_elements::{Button, Label, Theme};
use egui_lucide::Lucide;
use elegance::{Indicator, IndicatorState, Menu, MenuItem, TabBar};

const DELEGATE_TIP1: &str = "This wallet has been temporarily upgraded to a smart contract";
const DELEGATE_TIP2: &str = "This wallet is not upgraded to a smart contract";

/// `https://app.uniswap.org` → `app.uniswap.org`; the account panel is narrow.
fn short_origin(origin: &str) -> &str {
   let host = origin.split_once("://").map_or(origin, |(_, host)| host);
   host.trim_end_matches('/')
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
   /// Maximum size of the panel body. The body itself hugs its content; the space up to
   /// this height is what keeps the nav below the panel in one place.
   overview_size: (f32, f32),
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
}

impl AccountPanel {
   pub fn new() -> Self {
      let overview_size = (260.0, 336.0);

      let chain_select = ChainSelect::new("main_chain_select", 1).size(vec2(PANEL_ROW_WIDTH, 20.0));
      let wallet_select = WalletSelect::new("main_wallet_select").size(vec2(PANEL_ROW_WIDTH, 20.0));

      Self {
         open: false,
         overview_size,
         chain_select,
         wallet_select,
         wallet_info: WalletInfo::default(),
         qr_window: QrWindow::new(),
         delegate: DelegateUi::new(),
         tab: 0,
         body_heights: [0.0; 2],
         vitals: SystemVitals::default(),
      }
   }

   pub fn erase(&mut self) {
      self.delegate.erase();
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn set_wallet_info(&mut self, wallet_info: WalletInfo) {
      self.wallet_info = wallet_info;
   }

   pub fn set_current_wallet(&mut self, wallet: WalletInfo) {
      self.wallet_select.wallet = wallet.clone();
      self.wallet_info = wallet;
   }

   pub fn set_current_chain(&mut self, chain: ChainId) {
      self.chain_select.chain = chain;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
      ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs);

      let chain = ctx.chain;
      let privacy_mode = ctx.privacy_mode;

      let evm_addr = self.wallet_info.address;

      self.delegate.show(ctx, theme, evm_addr, ui);

      self.qr_window.show(ctx, theme, ui);

      let frame2 = theme.frame2.outer_margin(Margin::same(10));

      // The panel body hugs its content, but the space it may occupy is fixed: whatever
      // is left below its tallest state stays as gap, so the nav beneath the panel never
      // moves as apps connect or as the tab changes.
      let frame_margins = frame2.inner_margin.sum().y + frame2.outer_margin.sum().y;
      let footprint = self.overview_size.1 + frame_margins + ui.spacing().item_spacing.y;
      let panel_top = ui.cursor().top();

      // Slide to the tab's height instead of jumping. A tab that has not been laid out
      // yet has no height to slide to, so the first visit renders naturally.
      let target = self.body_heights[self.tab];
      let animate = target > 0.0;
      let height = if animate {
         ui.ctx().animate_value_with_time(
            ui.id().with("panel_body_height"),
            target,
            ui.style().animation_time,
         )
      } else {
         0.0
      };

      // While it moves, force the layout to the animated height and clip the painted
      // output to it: the forced height is what keeps a body that *shrinks* from
      // snapping, the clip is what keeps a body that *grows* from running ahead of the
      // animation (egui never clips a child on its own).
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
         ui.set_max_width(self.overview_size.0);
         if animate {
            ui.set_min_height(height);
         }

         ui.vertical(|ui| {
            // Tab strip: Overview (wallet/chain) and Diagnostics.
            ui.add(TabBar::new(
               &mut self.tab,
               ["Overview", "Diagnostics"],
            ));

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

      // Additional margin we can take
      let margin = 20.0;
      let spent = ui.cursor().top() - (panel_top - margin);
      ui.add_space((footprint - spent).max(0.0));
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

            if ui.add(label).clicked() {
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

               let button = Button::image(icon).visuals(btn_visuals).small();
               let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               if res.clicked() {
                  let url = OpenUrl::new_tab(link);
                  ui.ctx().open_url(url);
               }

               ui.separator();

               let icon = Lucide::QrCode.size(18.0).color(icon_color).image();

               let button = Button::image(icon).visuals(btn_visuals).small();
               let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               // QR Code Window
               if res.clicked() {
                  self.qr_window.open(wallet.clone());
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

            let indicator = Indicator::new(state).size(12.0);

            ui.add(indicator);
            ui.add(label).on_hover_text(tip_text);

            // Align this separator with the one above it.
            ui.add_space(1.2);

            ui.separator();

            let size = vec2(24.0, 12.0);
            let more = dots_button(theme, size, ui);

            if more.clicked() {
               if !self.delegate.is_open() {
                  self.delegate.open();
               }
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
      let frame_width = self.overview_size.0 - 50.0;

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

            ui.add(Indicator::new(sync_state));

            ui.add_space(10.0);

            let label = RichText::new("Railgun").size(theme.typography.small);
            ui.label(label);

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
               Menu::new(("svc_menu", "railgun_id")).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("View last error")).clicked() {
                     let error_opt = ctx.railgun_status().sync_error(chain.id());
                     let error = error_opt.map_or(
                        "No errors for now everything looks good".to_string(),
                        |e| e,
                     );

                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.msg_window.open(error);
                           gui.request_repaint();
                        });
                     });
                  }

                  if ui.add(MenuItem::new("Settings")).clicked() {
                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.ctx.clone().write(|ctx| {
                              gui.settings.open_page(SettingsPage::Railgun, ctx);
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

            ui.add(Indicator::new(state));

            ui.add_space(10.0);

            let label = RichText::new("Wallet Connector").size(theme.typography.small);
            ui.label(label);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let size = vec2(24.0, 12.0);
               let more = dots_button(theme, size, ui);
               Menu::new(("svc_menu", "wallet_connector_id")).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("Settings")).clicked() {
                     RT.spawn_blocking(move || {
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
            ctx.current_wallet = self.wallet_select.wallet.clone();

            // Update the state on wallet change
            RT.spawn(async move {
               let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
               let current_wallet = ctx.current_wallet_info();
               let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);
               let owner = current_wallet.address;
               let chain_id = ctx.chain().id();

               SHARED_GUI.write(|gui| {
                  gui.account_panel.set_wallet_info(current_wallet);

                  if gui.token_selection.is_open() {
                     gui.token_selection.process_currencies(privacy_mode, chain_id, owner);
                  }

                  // The send view is paired with a wallet too: a token — or an NFT — picked for the
                  // previous one cannot be sent from this one. Resetting to the new wallet's default
                  // clears the NFT selection along with the fungible one.
                  if gui.send_crypto.is_open() {
                     gui.send_crypto.default_currency(privacy_mode, chain_id);
                  }
               });
            });
         }
      });
   }
}
