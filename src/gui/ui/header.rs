use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::{
   SHARED_GUI,
   ui::{ChainSelect, WalletSelect},
};
use crate::utils::{RT, data_to_qr, truncate_address, tx::delegate_to};
use egui::{
   Align, Align2, Button, CursorIcon, FontId, Frame, Image, ImageSource, Layout, Margin, OpenUrl,
   Order, RichText, Spinner, TextEdit, Ui, Window, vec2,
};
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, NativeCurrency},
   types::ChainId,
};
use zeus_wallet::Wallet;
use zeus_widgets::Label;

use zeus_theme::{OverlayManager, Theme};

const DELEGATE_TIP1: &str = "This wallet has been temporarily upgraded to a smart contract";
const DELEGATE_TIP2: &str = "This wallet is not upgraded to a smart contract";

/// Show some of current state of Zeus like the current chain, wallet, etc.
pub struct Header {
   open: bool,
   overlay: OverlayManager,
   size: (f32, f32),
   chain_select: ChainSelect,
   wallet_select: WalletSelect,
   qrcode_window: QRCodeWindow,
   delegate_window_open: bool,
   delegate_to: String,
   syncing: bool,
}

impl Header {
   pub fn new(overlay: OverlayManager) -> Self {
      let size = (230.0, 200.0);
      let chain_select = ChainSelect::new("main_chain_select", 1).size(vec2(size.0, 20.0));
      let wallet_select = WalletSelect::new("main_wallet_select").size(vec2(size.0, 20.0));

      Self {
         open: false,
         overlay: overlay.clone(),
         size,
         chain_select,
         wallet_select,
         qrcode_window: QRCodeWindow::new(overlay),
         delegate_window_open: false,
         delegate_to: String::new(),
         syncing: false,
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
      self.overlay.window_opened();
      self.delegate_window_open = true;
   }

   pub fn close_delegate_window(&mut self) {
      self.overlay.window_closed();
      self.delegate_window_open = false;
   }

   pub fn set_current_wallet(&mut self, wallet: Wallet) {
      self.wallet_select.wallet = wallet;
   }

   pub fn set_current_chain(&mut self, chain: ChainId) {
      self.chain_select.chain = chain;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
      ui.spacing_mut().button_padding = vec2(4.0, 4.0);

      let chain = ctx.chain();
      let frame = theme.frame1;
      let wallet = ctx.current_wallet_info();
      let tint = theme.image_tint_recommended;

      self.show_deleg_settings_window(
         ctx.clone(),
         theme,
         icons.clone(),
         wallet.address,
         ui,
      );

      self.qrcode_window.show(theme, ui);

      frame.show(ui, |ui| {
         ui.vertical(|ui| {
            ui.horizontal(|ui| {
               self.show_chain_select(ctx.clone(), theme, icons.clone(), ui);
            });

            ui.horizontal(|ui| {
               self.show_wallet_select(ctx.clone(), theme, icons.clone(), ui);
            });

            // Wallet address, on click copy it to the clipboard
            ui.horizontal(|ui| {
               let address = wallet.address_truncated();

               let address_text = RichText::new(address).size(theme.text_sizes.normal);
               let button = Button::selectable(false, address_text);
               if ui.add(button).clicked() {
                  ui.ctx().copy_text(wallet.address.to_string());
               }

               ui.add_space(5.0);

               // QR Code
               let icon = match theme.dark_mode {
                  true => icons.qrcode_white_x18(tint),
                  false => icons.qrcode_dark_x18(tint),
               };

               let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

               if res.clicked() {
                  self.qrcode_window.open(wallet.clone());
               }

               ui.add_space(10.0);

               // Block explorer link
               let block_explorer = chain.block_explorer();
               let link = format!("{}/address/{}", block_explorer, wallet.address);
               let icon = match theme.dark_mode {
                  true => icons.external_link_white_x18(tint),
                  false => icons.external_link_dark_x18(tint),
               };

               let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

               if res.clicked() {
                  let url = OpenUrl::new_tab(link);
                  ui.ctx().open_url(url);
               }
            });

            // Wallet delegated status
            let deleg_addr = ctx.get_delegated_address(chain.id(), wallet.address);
            ui.horizontal(|ui| {
               ui.set_width(self.size.0);

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = match deleg_addr.is_some() {
                     true => RichText::new("Delegated").size(theme.text_sizes.normal),
                     false => RichText::new("Not delegated").size(theme.text_sizes.normal),
                  };

                  let icon = match deleg_addr.is_some() {
                     true => icons.orange_circle(tint),
                     false => icons.green_circle(tint),
                  };

                  let tip = if deleg_addr.is_some() {
                     DELEGATE_TIP1
                  } else {
                     DELEGATE_TIP2
                  };

                  let tip_text = RichText::new(tip).size(theme.text_sizes.normal);

                  let label = Label::new(text, Some(icon));
                  ui.add(label).on_hover_text(tip_text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let icon = match theme.dark_mode {
                     true => icons.gear_white_x24(tint),
                     false => icons.gear_dark_x24(tint),
                  };

                  let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

                  if res.clicked() {
                     self.open_delegate_window();
                  }
               });
            });
         });
      });
   }

   fn show_chain_select(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.vertical(|ui| {
         if ctx.tx_confirm_window_open() {
            ui.disable();
         }

         if ctx.sign_msg_window_open() {
            ui.disable();
         }

         let clicked = self.chain_select.show(0, theme, icons.clone(), ui);
         if clicked {
            let new_chain = self.chain_select.chain;

            ctx.write(|ctx| {
               ctx.chain = new_chain;
            });

            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  let currency: Currency = NativeCurrency::from(new_chain.id()).into();
                  gui.send_crypto.set_currency(currency.clone());

                  let owner = ctx.current_wallet_info().address;
                  gui.token_selection.process_currencies(ctx, new_chain.id(), owner);

                  gui.uniswap.swap_ui.default_currency_in(new_chain.id());
                  gui.uniswap.swap_ui.default_currency_out(new_chain.id());
                  // gui.uniswap.create_position_ui.default_currency0(new_chain.id());
                  // gui.uniswap.create_position_ui.default_currency1(new_chain.id());
               });
            });
         }
      });
   }

   fn show_wallet_select(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.vertical(|ui| {
         if ctx.tx_confirm_window_open() {
            ui.disable();
         }

         if ctx.sign_msg_window_open() {
            ui.disable();
         }

         let clicked = self.wallet_select.show(theme, ctx.clone(), icons.clone(), ui);
         if clicked {
            ctx.write(|ctx| {
               ctx.current_wallet = self.wallet_select.wallet.clone();
            });

            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  let owner = ctx.current_wallet_info().address;
                  let chain_id = ctx.chain().id();
                  gui.token_selection.process_currencies(ctx, chain_id, owner);
               });
            });
         }
      });
   }

   fn show_deleg_settings_window(
      &mut self,
      ctx: ZeusCtx,
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
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(350.0);
            ui.set_height(200.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let chain = ctx.chain();
            let delegated = ctx.get_delegated_address(chain.id(), wallet);

            ui.horizontal(|ui| {
               let size = vec2(ui.available_width(), 20.0);

               ui.allocate_ui(size, |ui| {
                  ui.vertical_centered(|ui| {
                     if let Some(delegated_adrress) = delegated {
                        self.undelegate_ui(ctx.clone(), theme, wallet, delegated_adrress, ui);
                     } else {
                        self.delegate_ui(ctx.clone(), theme, wallet, ui);
                     }

                     let text = RichText::new("Close").size(theme.text_sizes.normal);
                     if ui.add(Button::new(text)).clicked() {
                        self.close_delegate_window();
                     }
                  });
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  self.refresh(ctx.clone(), theme, icons, wallet, ui);
               });
            });
         });
   }

   fn refresh(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      wallet: Address,
      ui: &mut Ui,
   ) {
      ui.spacing_mut().button_padding = vec2(4.0, 4.0);

      let tint = theme.image_tint_recommended;
      let icon = match theme.dark_mode {
         true => icons.refresh_white_x22(tint),
         false => icons.refresh_dark_x22(tint),
      };

      if !self.syncing {
         let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

         if res.clicked() {
            self.syncing = true;
            let ctx_clone = ctx.clone();
            RT.spawn(async move {
               match ctx_clone.check_delegated_wallet_status(ctx.chain().id(), wallet).await {
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
   fn delegate_ui(&mut self, ctx: ZeusCtx, theme: &Theme, wallet: Address, ui: &mut Ui) {
      let text = RichText::new("Delegate to").size(theme.text_sizes.large);
      ui.label(text);

      let hint = RichText::new("Enter a smart contract address")
         .color(theme.colors.text_muted)
         .size(theme.text_sizes.normal);

      let text = TextEdit::singleline(&mut self.delegate_to)
         .hint_text(hint)
         .font(FontId::proportional(theme.text_sizes.normal))
         .margin(Margin::same(10))
         .desired_width(ui.available_width() * 0.8);

      ui.add(text);

      let text = RichText::new("Delegate").size(theme.text_sizes.large);
      let button = Button::new(text);

      let clicked = ui.add(button).clicked();

      if clicked {
         let delegate_to_addr = self.delegate_to.clone();
         let ctx_clone = ctx.clone();
         let chain = ctx.chain();
         RT.spawn(async move {
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

            match delegate_to(ctx_clone, chain, wallet, delegate_address).await {
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
      ctx: ZeusCtx,
      theme: &Theme,
      wallet: Address,
      delegated_address: Address,
      ui: &mut Ui,
   ) {
      let text = RichText::new("Currently delegated to").size(theme.text_sizes.normal);
      ui.label(text);

      let chain = ctx.chain();

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
      let button = Button::new(text).min_size(vec2(100.0, 30.0));

      let clicked = ui.add(button).clicked();
      if clicked {
         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.header.close_delegate_window();
               gui.request_repaint();
            });

            match delegate_to(ctx.clone(), chain, wallet, Address::ZERO).await {
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
   image: Option<Image<'static>>,
   error: Option<String>,
   size: (f32, f32),
}

impl QRCodeWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         wallet: None,
         image: None,
         error: None,
         size: (400.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, wallet: WalletInfo) {
      let png_bytes_res = data_to_qr(&wallet.address.to_string().as_str());

      let (image, error) = if let Ok(png_bytes) = png_bytes_res {
         let uri = format!("bytes://receive-{}.png", &wallet.address);
         (
            Some(Image::new(ImageSource::Bytes {
               uri: uri.into(),
               bytes: png_bytes.into(),
            })),
            None,
         )
      } else {
         (
            None,
            Some(format!(
               "Failed to generate QR Code: {}",
               png_bytes_res.unwrap_err()
            )),
         )
      };
      self.overlay.window_opened();
      self.open = true;
      self.wallet = Some(wallet);
      self.image = image;
      self.error = error;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new(self.overlay.clone());
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
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
                  self.close_button(theme, ui);
                  return;
               }

               if self.error.is_some() {
                  ui.label(
                     RichText::new(self.error.as_ref().unwrap()).size(theme.text_sizes.large),
                  );
                  self.close_button(theme, ui);
                  return;
               }

               // Wallet Info
               if let Some(wallet) = self.wallet.as_ref() {
                  let text = RichText::new("Wallet Name").size(theme.text_sizes.large);
                  ui.label(text);
                  ui.label(RichText::new(wallet.name().as_str()).size(theme.text_sizes.normal));

                  let text = RichText::new("Address").size(theme.text_sizes.large);
                  ui.label(text);
                  ui.label(RichText::new(wallet.address.to_string()).size(theme.text_sizes.normal));
               }

               ui.add_space(10.0);

               // QR Code
               if let Some(image) = self.image.clone() {
                  ui.add(image);
               }

               ui.add_space(10.0);

               self.close_button(theme, ui);
            });
         });
   }

   fn close_button(&mut self, theme: &Theme, ui: &mut Ui) {
      let text = RichText::new("Close").size(theme.text_sizes.normal);
      if ui.add(Button::new(text)).clicked() {
         self.reset();
      }
   }
}
