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

pub use delegate::DelegateUi;
pub use qr_window::QrWindow;

use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusContext};
use crate::gui::{
   SHARED_GUI, SettingsPage,
   ui::{ChainSelect, WalletSelect, common::*},
};
use crate::utils::RT;
use egui::{Align, CursorIcon, Layout, Margin, OpenUrl, RichText, Ui, vec2};
use std::sync::Arc;
use zeus_eth::{
   currency::{Currency, NativeCurrency},
   types::ChainId,
};

use egui_elements::{Button, Theme, visuals::ButtonVisuals};
use egui_lucide::Lucide;
use elegance::{Badge, BadgeTone, Indicator, IndicatorState, Menu, MenuItem, TabBar};

const DELEGATE_TIP1: &str = "This wallet has been temporarily upgraded to a smart contract";
const DELEGATE_TIP2: &str = "This wallet is not upgraded to a smart contract";

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
   overview_size: (f32, f32),
   chain_select: ChainSelect,
   wallet_select: WalletSelect,
   wallet_info: WalletInfo,
   pub qr_window: QrWindow,
   pub delegate: DelegateUi,
   /// Active tab: 0 = Overview, 1 = Services.
   tab: usize,
}

impl AccountPanel {
   pub fn new() -> Self {
      let overview_size = (260.0, 250.0);

      let chain_select = ChainSelect::new("main_chain_select", 1).size(vec2(220.0, 20.0));
      let wallet_select = WalletSelect::new("main_wallet_select").size(vec2(220.0, 20.0));

      Self {
         open: false,
         overview_size,
         chain_select,
         wallet_select,
         wallet_info: WalletInfo::default(),
         qr_window: QrWindow::new(),
         delegate: DelegateUi::new(),
         tab: 0,
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
      let button_visuals = theme.button_visuals();

      let evm_addr = self.wallet_info.address;

      self.delegate.show(ctx, theme, evm_addr, ui);

      self.qr_window.show(ctx, theme, ui);

      let frame2 = theme.frame2.outer_margin(Margin::same(10));

      frame2.show(ui, |ui| {
         ui.set_max_width(self.overview_size.0);
         ui.set_height(self.overview_size.1);

         ui.vertical(|ui| {
            // Tab strip: Overview (wallet/chain) and Diagnostics.
            ui.add(TabBar::new(
               &mut self.tab,
               ["Overview", "Diagnostics"],
            ));

            ui.add_space(5.0);

            match self.tab {
               0 => self.show_overview(
                  ctx,
                  theme,
                  &icons,
                  &button_visuals,
                  privacy_mode,
                  chain,
                  ui,
               ),
               1 => self.show_services(ctx, theme, ui),
               _ => {}
            }
         });
      });
   }

   /// Overview tab
   fn show_overview(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Arc<Icons>,
      button_visuals: &ButtonVisuals,
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

      // Wallet address, on click copy it to the clipboard
      ui.horizontal(|ui| {
         let address = match privacy_mode {
            false => wallet.evm_address_truncated(),
            true => wallet.zk_address_truncated(),
         };

         let full_address = match privacy_mode {
            false => wallet.address.to_string(),
            true => wallet.zk_address(),
         };

         let address_text = RichText::new(address).size(theme.typography.normal);
         let label = Button::selectable(false, address_text).visuals(button_visuals.clone());

         if ui.add(label).clicked() {
            ui.ctx().copy_text(full_address);
         }

         ui.add_space(7.0);

         let icon = Lucide::QrCode.size(16.0).color(icon_color).image();

         let button = Button::image(icon);
         let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

         // QR Code Window
         if res.clicked() {
            self.qr_window.open(wallet.clone());
         }

         ui.add_space(10.0);

         // Block explorer link
         let block_explorer = chain.block_explorer();
         let link = format!("{}/address/{}", block_explorer, wallet.address);
         let icon = Lucide::ExternalLink.size(16.0).color(icon_color).image();

         let button = Button::image(icon);
         let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

         if res.clicked() {
            let url = OpenUrl::new_tab(link);
            ui.ctx().open_url(url);
         }
      });

      // Wallet delegated status
      let deleg_addr = ctx.delegated_wallets.get(chain.id(), wallet.address);
      ui.horizontal(|ui| {
         let text = match deleg_addr.is_some() {
            true => RichText::new("Delegated").size(theme.typography.normal),
            false => RichText::new("Not Delegated").size(theme.typography.normal),
         };

         let tip = if deleg_addr.is_some() {
            DELEGATE_TIP1
         } else {
            DELEGATE_TIP2
         };

         let tip_text = RichText::new(tip).size(theme.typography.normal);

         let tone = match deleg_addr.is_some() {
            true => BadgeTone::Warning,
            false => BadgeTone::Ok,
         };

         let badge = Badge::new(text, tone);
         ui.add(badge).on_hover_text(tip_text);

         ui.add_space(10.0);

         let more = dots_button(theme, ui);

         if more.clicked() {
            if !self.delegate.is_open() {
               self.delegate.open();
            }
         }
      });

      privacy_mode_switch(ctx, theme, ui);
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
               let more = dots_button(theme, ui);
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
               let more = dots_button(theme, ui);
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
            let new_chain = self.chain_select.chain;

            ctx.chain = new_chain;

            // Update the state on chain change
            RT.spawn(async move {
               let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
               let owner = ctx.current_wallet_info().address;
               let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);

               SHARED_GUI.write(|gui| {
                  let currency: Currency = NativeCurrency::from(new_chain.id()).into();
                  gui.send_crypto.set_currency(currency.clone());

                  if gui.token_selection.is_open() {
                     gui.token_selection.process_currencies(privacy_mode, new_chain.id(), owner);
                  }

                  gui.uniswap.swap_ui.default_currency_in(new_chain.id());
                  gui.uniswap.swap_ui.default_currency_out(new_chain.id());
                  gui.send_crypto.default_currency(privacy_mode, new_chain.id());
                  gui.shield_ui.default_currency(new_chain.id());
                  gui.wallet_ui.calc_wallet_value();
                  gui.recipient_selection.calc_wallet_value();
               });
            });
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
               });
            });
         }
      });
   }
}
