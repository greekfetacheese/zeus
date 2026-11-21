//! QR code window for the selected wallet
//!
//! Shows the public (EVM) or the private (zk) address of the wallet as a QR
//! code, depending on the app privacy mode.

use crate::core::{WalletInfo, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::{address_for_speech, name_address, name_text, spoken_wallet_name};
use egui::{Order, RichText, Ui, vec2};
use egui_elements::{Button, Modal, QrImage, Spinner, Theme, components::QrEncoding};

const QR_IMAGE_SIZE: u32 = 250;

/// What a reader hears in place of a symbol: the address above, for another device to scan.
const QR_ALT: &str = "QR code of the address above, for another device to scan.";

pub struct QrWindow {
   open: bool,
   wallet: Option<WalletInfo>,
   evm_address_qr: QrImage,
   zk_address_qr: QrImage,
   size: (f32, f32),
   /// The words this window wants said, drained by the panel that draws it.
   notice: Option<String>,
}

impl QrWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         wallet: None,
         evm_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
         zk_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
         size: (450.0, 400.0),
         notice: None,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, wallet: WalletInfo) {
      let wallet_clone = wallet.clone();

      crate::runtime::rt().spawn_blocking(move || {
         let encoding = QrEncoding {
            target_px: QR_IMAGE_SIZE,
            ..Default::default()
         };

         let data = wallet.address.to_string();
         let uri = format!("bytes://receive-{}.png", wallet.address);
         let evm_address_qr = QrImage::with_encoding(&data, uri, encoding).alt(QR_ALT);

         let zk_address_qr = if let Some(railgun_address) = &wallet.railgun_address {
            let data = railgun_address.address.to_string();
            let uri = format!("bytes://receive-{}.png", railgun_address.address);
            QrImage::with_encoding(&data, uri, encoding).alt(QR_ALT)
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

   /// The words this window wants said, drained by the panel that draws it.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let privacy_mode = ctx.privacy_mode;
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let mut open = self.open;

      // The mode is the window's own name, and the only thing that tells the public address from
      // the private one on a surface that draws both of them the same way.
      let title = match privacy_mode {
         false => "Public address (EVM)",
         true => "Private address (zk)",
      };

      // The window is drawn from the panel, which holds the only announcement channel, so the
      // words it wants said are left here for the panel to drain.
      let mut notice = None;

      Modal::new("QR Code Window", &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(RichText::new(title).size(theme.typography.heading))
         // The window draws its own footer, and that Close button is the exit: a header cross
         // would be a second way to do the same thing.
         .closable(false)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.sm);
               ui.spacing_mut().button_padding = theme.button_padding;

               if self.wallet.is_none() {
                  ui.label(
                     RichText::new(
                        "No wallet selected. Close this window and open it from the account menu.",
                     )
                     .size(theme.typography.normal),
                  );
                  ui.add(Spinner::new().size(17.0));
                  self.close_button(theme, ui);
                  return;
               }

               let frame = theme.frame2;

               // Wallet Name and Address
               if let Some(wallet) = self.wallet.as_ref() {
                  frame.show(ui, |ui| {
                     ui.set_max_width(ui.available_width() * 0.95);

                     let name = ui.label(
                        RichText::new(wallet.name_with_source().as_str())
                           .size(theme.typography.large),
                     );
                     name_text(ui, &name, spoken_wallet_name(wallet));

                     let address = match privacy_mode {
                        false => wallet.address.to_string(),
                        true => wallet.zk_address(),
                     };

                     if !address.is_empty() {
                        // The drawn text is the whole address, which a reader cannot check by ear
                        // as a run: the node names it grouped and keeps Zeus's shortened form as
                        // its description, so the two forms stay one control.
                        let spoken = address_for_speech(None, &address);
                        let address_text =
                           RichText::new(address.clone()).size(theme.typography.normal);
                        let button = Button::selectable(false, address_text)
                           .visuals(theme.button_visuals())
                           .wrap();

                        let response = ui.add(button);
                        name_address(ui, &response, &spoken);

                        // Nothing on screen changes when the address is copied, so the window has
                        // to say that it happened.
                        if response.clicked() {
                           ui.ctx().copy_text(address);
                           notice = Some("Address copied.".to_string());
                        }
                     }
                  });
               }

               ui.add_space(10.0);

               if !privacy_mode && let Some(error) = self.evm_address_qr.error() {
                  ui.label(RichText::new(error.to_string()).size(theme.typography.large));
               }

               // QR Code. The symbol carries the sentence it is for (`QrImage::alt`) and draws
               // itself with it: the node is named by that sentence, and egui's own account of the
               // load state — `Loading bytes://…` — never reaches the tree.
               if !privacy_mode {
                  self.evm_address_qr.show(ui);
               } else {
                  self.zk_address_qr.show(ui);
               }

               ui.add_space(20.0);

               self.close_button(theme, ui);
            });
         });

      // A dismissal is the Close button's exit: the codes the window encoded go with it.
      if !open {
         self.close_and_clear(ui.ctx());
         return;
      }

      if let Some(text) = notice {
         self.notice = Some(text);
      }
   }

   /// The window's one exit: the codes it encoded go with it, and it forgets the wallet.
   ///
   /// The Close button and a dismissal both come through here, so nothing the window encoded
   /// outlives the window that drew it.
   fn close_and_clear(&mut self, ctx: &egui::Context) {
      self.evm_address_qr.clear(ctx);
      self.zk_address_qr.clear(ctx);
      self.reset();
   }

   fn close_button(&mut self, theme: &Theme, ui: &mut Ui) {
      let size = vec2(ui.available_width() * 0.9, 45.0);
      let text = RichText::new("Close").size(theme.typography.large);
      let button = Button::new(text).min_size(size);

      if ui.add(button).clicked() {
         self.close_and_clear(ui.ctx());
      }
   }
}

#[cfg(test)]
impl QrWindow {
   /// Open the window on `wallet` without encoding either code: the encoding runs on a worker that
   /// writes the frame's global, and a tree test has no frame for it to land in.
   pub(crate) fn open_for_test(&mut self, wallet: Option<WalletInfo>) {
      self.open = true;
      self.wallet = wallet;
   }
}
