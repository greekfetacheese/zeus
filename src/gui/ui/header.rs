//! UI component that we show at the top left of the window
//!
//! It allows the user to:
//! - select a chain
//! - select a wallet
//! - show the QR code for the selected wallet
//! - delegate to a smart contract
//! - delegate status of the current wallet (Green if not delegated, Red if delegated)

use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusContext, delegate_to};
use crate::gui::{
   SHARED_GUI,
   ui::{ChainSelect, WalletSelect, common::dots_button, dapps::railgun::RailgunMode},
};
use crate::utils::{RT, data_to_qr, truncate_address};
use egui::{
   Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Image, ImageSource, Layout,
   Margin, OpenUrl, Order, RichText, Spinner, Ui, Window, load::Bytes, vec2,
};
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, NativeCurrency},
   types::ChainId,
};

use zeus_wallet::Wallet;
use zeus_widgets::{Button, SecureTextEdit};

use elegance::{
   Badge, BadgeTone, Indicator, IndicatorState, Menu, MenuItem, TabBar, Theme as EleganceTheme,
};
use zeus_theme::{ButtonVisuals, OverlayManager, Theme};

const DELEGATE_TIP1: &str = "This wallet has been temporarily upgraded to a smart contract";
const DELEGATE_TIP2: &str = "This wallet is not upgraded to a smart contract";

/// The `ctx.data` key elegance widgets read their theme from. Mirrors the
/// private `Theme::storage_id()` in `egui-elegance` so we can inject a
/// Zeus-derived theme without calling `Theme::install()`.
fn elegance_theme_key() -> egui::Id {
   egui::Id::new("elegance::theme")
}

/// Ui component that we show at the top left of the window
///
/// It allows the user to:
/// - select a chain
/// - select a wallet
/// - show the QR code for the selected wallet
/// - delegate to a smart contract
/// - delegate status of the current wallet (Green if not delegated, Red if delegated)
pub struct Header {
   open: bool,
   overlay: OverlayManager,
   overview_size: (f32, f32),
   services_size: (f32, f32),
   chain_select: ChainSelect,
   wallet_select: WalletSelect,
   wallet_info: WalletInfo,
   qrcode_window: QRCodeWindow,
   delegate_window_open: bool,
   delegate_to: String,
   syncing: bool,

   /// Active header tab: 0 = Overview, 1 = Services.
   tab: usize,
   /// Cached elegance theme + signature so we only re-inject it when the Zeus theme changes.
   elegance_theme_cache: Option<(bool, Color32, EleganceTheme)>,
}

impl Header {
   pub fn new(overlay: OverlayManager) -> Self {
      let overview_size = (230.0, 240.0);
      let services_size = (330.0, 240.0);

      let chain_select = ChainSelect::new("main_chain_select", 1).size(vec2(overview_size.0, 20.0));
      let wallet_select = WalletSelect::new("main_wallet_select").size(vec2(overview_size.0, 20.0));

      Self {
         open: false,
         overlay: overlay.clone(),
         overview_size,
         services_size,
         chain_select,
         wallet_select,
         wallet_info: WalletInfo::default(),
         qrcode_window: QRCodeWindow::new(overlay),
         delegate_window_open: false,
         delegate_to: String::new(),
         syncing: false,

         tab: 0,
         elegance_theme_cache: None,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn open_delegate_window(&mut self) {
      if !self.delegate_window_open {
         self.overlay.window_opened();
      }
      self.delegate_window_open = true;
   }

   pub fn close_delegate_window(&mut self) {
      self.overlay.window_closed();
      self.delegate_window_open = false;
   }

   pub fn set_wallet_info(&mut self, wallet_info: WalletInfo) {
      self.wallet_info = wallet_info;
   }

   pub fn set_current_wallet(&mut self, wallet: Wallet) {
      let wallet_info = WalletInfo::from_wallet(&wallet, true);
      self.wallet_select.wallet = wallet;
      self.wallet_info = wallet_info;
   }

   pub fn set_current_chain(&mut self, chain: ChainId) {
      self.chain_select.chain = chain;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
      ui.spacing_mut().button_padding = vec2(4.0, 4.0);

      let chain = ctx.chain;
      let privacy_mode = ctx.privacy_mode;
      let tint = theme.image_tint_recommended;
      let button_visuals = theme.button_visuals();

      let frame = theme.frame1.outer_margin(Margin::same(10));
      let evm_addr = self.wallet_info.address;

      self.show_deleg_settings_window(ctx, theme, icons.clone(), evm_addr, ui);

      self.qrcode_window.show(ctx, theme, ui);

      self.inject_elegance_theme(ui.ctx(), theme);

      frame.show(ui, |ui| {
         let (width, height) = if self.tab == 0 {
            (self.overview_size.0, self.overview_size.1)
         } else {
            (self.services_size.0, self.services_size.1)
         };

         ui.set_width(width);
         ui.set_height(height);

         ui.vertical(|ui| {
            // Tab strip: Overview (wallet/chain) and Services (background tasks).
            ui.add(TabBar::new(
               &mut self.tab,
               ["Overview", "Services"],
            ));

            ui.add_space(5.0);

            match self.tab {
               0 => self.show_overview(
                  ctx,
                  theme,
                  &icons,
                  &tint,
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

   // TODO: Put it somewhere so it can be runned independently from this UI
   /// Inject an elegance [`Theme`] built from the active Zeus theme into
   /// `ctx.data` under the key elegance reads, so elegance widgets
   /// (`TabBar`, `Card`, `StatusPill`, `Indicator`) take Zeus's colours and
   /// respect light/dark without disturbing the rest of the UI.
   fn inject_elegance_theme(&mut self, ctx: &egui::Context, theme: &Theme) {
      let dark = theme.dark_mode;
      let accent = theme.colors.accent;
      if let Some((cached_dark, cached_accent, cached)) = &self.elegance_theme_cache {
         if *cached_dark == dark && *cached_accent == accent {
            ctx.data_mut(|d| d.insert_temp(elegance_theme_key(), cached.clone()));
            return;
         }
      }

      let c = &theme.colors;
      let mut pal = if theme.dark_mode {
         elegance::Palette::charcoal()
      } else {
         elegance::Palette::frost()
      };

      // Map Zeus colours onto elegance's palette so the tab underline, borders
      // and status dots match the rest of the wallet.
      pal.is_dark = theme.dark_mode;
      pal.bg = c.bg;
      pal.card = c.widget_bg;
      pal.input_bg = c.widget_bg;
      pal.border = c.border;
      pal.text = c.text;
      pal.text_muted = c.text_muted;
      pal.text_faint = c.text_muted;
      pal.focus = c.accent;
      pal.blue = c.info;
      pal.green = c.success;
      pal.green_hover = c.success;
      pal.red = c.error;
      pal.red_hover = c.error;
      pal.amber = c.warning;
      pal.amber_hover = c.warning;
      pal.purple = c.accent;
      pal.purple_hover = c.accent;
      pal.success = c.success;
      pal.danger = c.error;
      pal.warning = c.warning;

      let elegance_theme = EleganceTheme {
         palette: pal,
         ..EleganceTheme::slate()
      };

      ctx.data_mut(|d| d.insert_temp(elegance_theme_key(), elegance_theme.clone()));
      self.elegance_theme_cache = Some((dark, accent, elegance_theme));
   }

   /// Overview tab
   fn show_overview(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Arc<Icons>,
      tint: &bool,
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

         let address_text = RichText::new(address).size(theme.text_sizes.normal);
         let label = Button::selectable(false, address_text).visuals(button_visuals.clone());

         if ui.add(label).clicked() {
            ui.ctx().copy_text(full_address);
         }

         ui.add_space(7.0);

         let icon = match theme.dark_mode {
            true => icons.qrcode_white_x18(*tint),
            false => icons.qrcode_dark_x18(*tint),
         };

         let button = Button::image(icon).visuals(button_visuals.clone());
         let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

         // QR Code Window
         if res.clicked() {
            self.qrcode_window.open(ctx, wallet.clone());
         }

         ui.add_space(10.0);

         // Block explorer link
         let block_explorer = chain.block_explorer();
         let link = format!("{}/address/{}", block_explorer, wallet.address);
         let icon = match theme.dark_mode {
            true => icons.external_link_white_x18(*tint),
            false => icons.external_link_dark_x18(*tint),
         };

         let button = Button::image(icon).visuals(button_visuals.clone());
         let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

         if res.clicked() {
            let url = OpenUrl::new_tab(link);
            ui.ctx().open_url(url);
         }
      });

      // Wallet delegated status
      let deleg_addr = ctx.delegated_wallets.get(chain.id(), wallet.address);
      ui.horizontal(|ui| {
         // ui.set_width(self.overview_size.0);

         let text = match deleg_addr.is_some() {
            true => RichText::new("Delegated").size(theme.text_sizes.normal),
            false => RichText::new("Not Delegated").size(theme.text_sizes.normal),
         };

         let tip = if deleg_addr.is_some() {
            DELEGATE_TIP1
         } else {
            DELEGATE_TIP2
         };

         let tip_text = RichText::new(tip).size(theme.text_sizes.normal);

         let tone = match deleg_addr.is_some() {
            true => BadgeTone::Warning,
            false => BadgeTone::Ok,
         };

         let badge = Badge::new(text, tone);
         ui.add(badge).on_hover_text(tip_text);

         ui.add_space(10.0);

         let more = dots_button(theme, ui);

         if more.clicked() {
            if !self.delegate_window_open {
               self.open_delegate_window();
            }
         }
      });

      // Privacy mode switch button
      ui.scope(|ui| {
         ui.spacing_mut().button_padding = vec2(8.0, 8.0);

         let text = format!(
            "Switch to {} mode",
            if privacy_mode { "Public" } else { "Privacy" }
         );
         let rich_text = RichText::new(text).size(theme.text_sizes.normal);
         let button = Button::new(rich_text).visuals(button_visuals.clone());

         if ui.add(button).clicked() {
            let privacy_mode = !privacy_mode;

            ctx.privacy_mode = privacy_mode;

            RT.spawn_blocking(move || {
               let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
               let chain = ctx.chain();
               let owner = ctx.current_wallet_info().address;

               let new_mode = match privacy_mode {
                  false => RailgunMode::Shield,
                  true => RailgunMode::Unshield,
               };
               SHARED_GUI.write(|gui| {
                  gui.shield_ui.set_mode(new_mode);
                  gui.shield_ui.default_currency(chain.id());
                  gui.token_selection.process_currencies(privacy_mode, chain.id(), owner);
               });
            });
         }
      });
   }

   /// Services tab
   fn show_services(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let chain = ctx.chain;
      let railgun_is_supported = ctx.railgun_is_supported(chain);

      if !railgun_is_supported {
         let text =
            RichText::new("Railgun is not supported on this chain").size(theme.text_sizes.normal);
         ui.label(text);
         return;
      }

      ui.spacing_mut().item_spacing.y = 0.0;

      let frame = theme.frame2;
      let frame_height = 40.0;

      // Railgun Status
      frame.show(ui, |ui| {
         ui.set_height(frame_height);

         ui.horizontal(|ui| {
            let railgun_synced = ctx.railgun_status().synced(chain.id());
            let sync_state = match railgun_synced {
               true => IndicatorState::On,
               false => IndicatorState::Off,
            };

            ui.add(Indicator::new(sync_state));

            ui.add_space(20.0);

            let label = RichText::new("Railgun").size(theme.text_sizes.small);
            ui.label(label);

            ui.add_space(40.0);

            ui.vertical(|ui| {
               ui.spacing_mut().item_spacing.y = 0.0;
               let text = RichText::new("Synced block")
                  .size(theme.text_sizes.small)
                  .color(theme.colors.text_muted);
               ui.label(text);

               let block = ctx.railgun_status().synced_block(chain.id());
               let text = RichText::new(format!("{}", block)).size(theme.text_sizes.small);
               ui.label(text);
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let more = dots_button(theme, ui);
               Menu::new(("svc_menu", "railgun_id")).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("View last error").shortcut("⌘ E")).clicked() {
                     let error_opt = ctx.railgun_status().sync_error(chain.id());
                     let error = error_opt.map_or(
                        "No errors for now everything looks good".to_string(),
                        |e| e,
                     );

                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.msg_window.open("Railgun Last Error", error);
                           gui.request_repaint();
                        });
                     });
                  }

                  if ui.add(MenuItem::new("Settings").shortcut("⌘ S")).clicked() {
                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.msg_window.open("Not implement yet", "");
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
         ui.set_height(frame_height);

         ui.horizontal(|ui| {
            let running = ctx.server_running;
            let state = match running {
               true => IndicatorState::On,
               false => IndicatorState::Connecting,
            };

            ui.add(Indicator::new(state));

            ui.add_space(20.0);

            let label = RichText::new("Wallet Connector").size(theme.text_sizes.small);
            ui.label(label);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let more = dots_button(theme, ui);
               Menu::new(("svc_menu", "wallet_connector_id")).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("Settings").shortcut("⌘ S")).clicked() {
                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.msg_window.open("Not implement yet", "");
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
         if ctx.tx_confirm_window_open {
            ui.disable();
         }

         if ctx.sign_msg_window_open {
            ui.disable();
         }

         let clicked = self.chain_select.show(ctx, &[0], theme, icons.clone(), ui);
         if clicked {
            let new_chain = self.chain_select.chain;

            ctx.chain = new_chain;

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
                  gui.shield_ui.default_currency(new_chain.id());
                  // gui.uniswap.create_position_ui.default_currency0(new_chain.id());
                  // gui.uniswap.create_position_ui.default_currency1(new_chain.id());
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
         if ctx.tx_confirm_window_open {
            ui.disable();
         }

         if ctx.sign_msg_window_open {
            ui.disable();
         }

         let clicked = self.wallet_select.show(theme, ctx, icons.clone(), ui);
         if clicked {
            ctx.current_wallet = self.wallet_select.wallet.clone();

            RT.spawn(async move {
               let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
               let current_wallet = ctx.current_wallet_info();
               let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);
               let owner = current_wallet.address;
               let chain_id = ctx.chain().id();

               SHARED_GUI.write(|gui| {
                  gui.header.set_wallet_info(current_wallet);

                  if gui.token_selection.is_open() {
                     gui.token_selection.process_currencies(privacy_mode, chain_id, owner);
                  }
               });
            });
         }
      });
   }

   fn show_deleg_settings_window(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      wallet: Address,
      ui: &mut Ui,
   ) {
      if !self.delegate_window_open {
         return;
      }

      Window::new("Delegation_settings")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(350.0);
            ui.set_height(200.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let button_visuals = theme.button_visuals();

            let chain = ctx.chain;
            let delegated = ctx.delegated_wallets.get(chain.id(), wallet);

            ui.horizontal(|ui| {
               let size = vec2(ui.available_width(), 20.0);

               ui.allocate_ui(size, |ui| {
                  ui.vertical_centered(|ui| {
                     if let Some(delegated_adrress) = delegated {
                        self.undelegate_ui(ctx, theme, wallet, delegated_adrress, ui);
                     } else {
                        self.delegate_ui(ctx, theme, wallet, ui);
                     }

                     let text = RichText::new("Close").size(theme.text_sizes.normal);
                     let button = Button::new(text).visuals(button_visuals);
                     if ui.add(button).clicked() {
                        self.close_delegate_window();
                     }
                  });
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  self.refresh(theme, icons, wallet, ui);
               });
            });
         });
   }

   fn refresh(&mut self, theme: &Theme, icons: Arc<Icons>, wallet: Address, ui: &mut Ui) {
      ui.spacing_mut().button_padding = vec2(4.0, 4.0);

      let button_visuals = theme.button_visuals();
      let tint = theme.image_tint_recommended;
      let icon = match theme.dark_mode {
         true => icons.refresh_white_x22(tint),
         false => icons.refresh_dark_x22(tint),
      };

      if !self.syncing {
         let mut visuals = ButtonVisuals::default();
         visuals.bg_hover = button_visuals.bg_hover;
         visuals.corner_radius = CornerRadius::same(25);
         let button = Button::image(icon).small().visuals(visuals);
         let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

         if res.clicked() {
            self.syncing = true;

            RT.spawn(async move {
               let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
               let chain = ctx.chain();
               match ctx.check_delegated_wallet_status(chain.id(), wallet).await {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.header.syncing = false;
                     });
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.open_msg_window(
                           "Error while checking smart account status",
                           e.to_string(),
                        );
                        gui.header.syncing = false;
                     });
                  }
               }
            });
         }
      } else {
         ui.add(Spinner::new().size(17.0).color(theme.colors.text));
      }
   }

   // TODO: Maybe ask for credentials before proceeding
   fn delegate_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, wallet: Address, ui: &mut Ui) {
      let text = RichText::new("Delegate to").size(theme.text_sizes.large);
      ui.label(text);

      let text_edit_visuals = theme.text_edit_visuals();
      let button_visuals = theme.button_visuals();

      let hint = RichText::new("Enter a smart contract address")
         .color(theme.colors.text_muted)
         .size(theme.text_sizes.normal);

      let text = SecureTextEdit::singleline(&mut self.delegate_to)
         .visuals(text_edit_visuals)
         .hint_text(hint)
         .font(FontId::proportional(theme.text_sizes.normal))
         .margin(Margin::same(10))
         .desired_width(ui.available_width() * 0.8);

      ui.add(text);

      let text = RichText::new("Delegate").size(theme.text_sizes.large);
      let button = Button::new(text).visuals(button_visuals);

      let clicked = ui.add(button).clicked();

      if clicked {
         let delegate_to_addr = self.delegate_to.clone();
         let chain = ctx.chain;
         RT.spawn(async move {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

            let delegate_address = match Address::from_str(&delegate_to_addr) {
               Ok(address) => address,
               Err(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window(
                        "Not a valid Ethereum address",
                        delegate_to_addr.clone(),
                     );
                  });
                  return;
               }
            };

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.header.close_delegate_window();
               gui.request_repaint();
            });

            match delegate_to(ctx, chain, wallet, delegate_address).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Error while delegating", e.to_string());
                     gui.loading_window.reset();
                     gui.header.open_delegate_window();
                     gui.notification.reset();
                  });
               }
            }
         });
      }
   }

   fn undelegate_ui(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      wallet: Address,
      delegated_address: Address,
      ui: &mut Ui,
   ) {
      let text = RichText::new("Currently delegated to").size(theme.text_sizes.normal);
      ui.label(text);

      let button_visuals = theme.button_visuals();

      let chain = ctx.chain;

      let address_short = truncate_address(delegated_address.to_string());
      let explorer = chain.block_explorer();
      let link = format!(
         "{}/address/{}",
         explorer,
         delegated_address.to_string()
      );
      let text = RichText::new(address_short)
         .size(theme.text_sizes.normal)
         .color(theme.colors.info);
      ui.hyperlink_to(text, link);

      let text = RichText::new("Undelegate").size(theme.text_sizes.normal);
      let button = Button::new(text).visuals(button_visuals).min_size(vec2(100.0, 30.0));

      let clicked = ui.add(button).clicked();
      if clicked {
         RT.spawn(async move {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.header.close_delegate_window();
               gui.request_repaint();
               gui.ctx.clone()
            });

            match delegate_to(ctx, chain, wallet, Address::ZERO).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Error while undelegating", e.to_string());
                     gui.loading_window.reset();
                     gui.header.open_delegate_window();
                     gui.notification.reset();
                  });
               }
            }
         });
      }
   }
}

pub struct QRCodeWindow {
   open: bool,
   overlay: OverlayManager,
   wallet: Option<WalletInfo>,
   image_uri: Option<String>,
   error: Option<String>,
   size: (f32, f32),
}

impl QRCodeWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         wallet: None,
         image_uri: None,
         error: None,
         size: (400.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, ctx: &mut ZeusContext, wallet: WalletInfo) {
      let png_bytes_res = data_to_qr(&wallet.address.to_string().as_str());

      match png_bytes_res {
         Ok(png_bytes) => {
            ctx.set_qr_image_data(png_bytes);

            let uri = format!("bytes://receive-{}.png", &wallet.address);

            self.image_uri = Some(uri);
            self.error = None;
         }
         Err(e) => {
            self.image_uri = None;
            self.error = Some(format!("Failed to generate QR Code: {}", e));
         }
      }

      if !self.open {
         self.overlay.window_opened();
      }

      self.open = true;
      self.wallet = Some(wallet);
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new(self.overlay.clone());
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("QR Code Window")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               if self.wallet.is_none() {
                  ui.label(
                     RichText::new("No wallet found, this is a bug").size(theme.text_sizes.normal),
                  );
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  self.close_button(ctx, theme, ui);
                  return;
               }

               // Wallet Info
               if let Some(wallet) = self.wallet.as_ref() {
                  let text = RichText::new("Wallet Name").size(theme.text_sizes.large);
                  ui.label(text);
                  ui.label(
                     RichText::new(wallet.name_with_source().as_str())
                        .size(theme.text_sizes.normal),
                  );

                  let text = RichText::new("Address").size(theme.text_sizes.large);
                  ui.label(text);
                  ui.label(RichText::new(wallet.address.to_string()).size(theme.text_sizes.normal));
               }

               ui.add_space(10.0);

               if self.error.is_some() {
                  ui.label(
                     RichText::new(self.error.as_ref().unwrap()).size(theme.text_sizes.large),
                  );
               }

               // QR Code
               if let Some(image_uri) = self.image_uri.clone() {
                  let data = ctx.qr_image_data.clone();
                  let image = Image::new(ImageSource::Bytes {
                     uri: image_uri.into(),
                     bytes: Bytes::Shared(data),
                  })
                  .fit_to_exact_size(vec2(250.0, 250.0));
                  ui.add(image);
               }

               ui.add_space(10.0);

               self.close_button(ctx, theme, ui);
            });
         });
   }

   fn close_button(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let text = RichText::new("Close").size(theme.text_sizes.normal);
      let button = Button::new(text).visuals(theme.button_visuals());
      if ui.add(button).clicked() {
         if let Some(uri) = &self.image_uri {
            ui.ctx().forget_image(uri);
         }
         self.reset();
         ctx.erase_qr_image_data();
      }
   }
}
