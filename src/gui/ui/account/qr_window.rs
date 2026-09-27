//! QR code window for the selected wallet
//!
//! Shows the public (EVM) or the private (zk) address of the wallet as a QR
//! code, depending on the app privacy mode.

use crate::core::{WalletInfo, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{Order, RichText, Spinner, Ui, vec2};
use egui_elements::{Button, Modal, QrImage, Theme};

pub struct QrWindow {
   open: bool,
   wallet: Option<WalletInfo>,
   evm_address_qr: QrImage,
   zk_address_qr: QrImage,
   size: (f32, f32),
}

impl QrWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         wallet: None,
         evm_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
         zk_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
         size: (450.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, wallet: WalletInfo) {
      let wallet_clone = wallet.clone();

      RT.spawn_blocking(move || {
         let data = wallet.address.to_string();
         let uri = format!("bytes://receive-{}.png", &wallet.address);
         let evm_address_qr = QrImage::new(&data, uri);

         let zk_address_qr = if let Some(railgun_address) = &wallet.railgun_address {
            let data = railgun_address.address.to_string();
            let uri = format!("bytes://receive-{}.png", &railgun_address.address);
            QrImage::new(&data, uri)
         } else {
            QrImage::empty_with_error("No zkAddress available".to_string())
         };

         SHARED_GUI.write(|gui| {
            gui.account_panel.qr_window.evm_address_qr = evm_address_qr;
            gui.account_panel.qr_window.zk_address_qr = zk_address_qr;
         });
      });

      self.open = true;
      self.wallet = Some(wallet_clone);
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new();
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let privacy_mode = ctx.privacy_mode;
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let mut open = self.open;

      Modal::new("QR Code Window", &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.sm);
               ui.spacing_mut().button_padding = theme.button_padding;

               if self.wallet.is_none() {
                  ui.label(
                     RichText::new("No wallet found, this is a bug").size(theme.typography.normal),
                  );
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  self.close_button(theme, ui);
                  return;
               }

               let frame = theme.frame2;

               // Wallet Name and Address
               if let Some(wallet) = self.wallet.as_ref() {
                  frame.show(ui, |ui| {
                     ui.set_max_width(ui.available_width() * 0.95);

                     ui.label(
                        RichText::new(wallet.name_with_source().as_str())
                           .size(theme.typography.large),
                     );

                     let text = match privacy_mode {
                        false => "Public Address (EVM)",
                        true => "Private Address (zk)",
                     };

                     let rich_text = RichText::new(text).size(theme.typography.large);
                     ui.label(rich_text);

                     let address = match privacy_mode {
                        false => wallet.address.to_string(),
                        true => wallet.zk_address(),
                     };

                     if !address.is_empty() {
                        let address_text =
                           RichText::new(address.clone()).size(theme.typography.normal);
                        let label = Button::selectable(false, address_text)
                           .visuals(theme.button_visuals())
                           .wrap();

                        if ui.add(label).clicked() {
                           ui.ctx().copy_text(address);
                        }
                     }
                  });
               }

               ui.add_space(10.0);

               if !privacy_mode {
                  if let Some(error) = self.evm_address_qr.error() {
                     ui.label(RichText::new(error.to_string()).size(theme.typography.large));
                  }
               }

               // QR Code
               if !privacy_mode {
                  let image = self.evm_address_qr.image().fit_to_exact_size(vec2(250.0, 250.0));
                  ui.add(image);
               } else {
                  let image = self.zk_address_qr.image().fit_to_exact_size(vec2(250.0, 250.0));
                  ui.add(image);
               }

               ui.add_space(20.0);

               self.close_button(theme, ui);
            });
         });
   }

   fn close_button(&mut self, theme: &Theme, ui: &mut Ui) {
      let size = vec2(ui.available_width() * 0.9, 45.0);
      let text = RichText::new("Close").size(theme.typography.large);
      let button = Button::new(text).min_size(size);

      if ui.add(button).clicked() {
         self.evm_address_qr.clear(ui.ctx());
         self.zk_address_qr.clear(ui.ctx());
         self.reset();
      }
   }
}
