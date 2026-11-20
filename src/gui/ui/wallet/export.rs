//! UI that allows the user to export a private key

use crate::core::types::CredentialCheck;
use crate::core::{WalletInfo, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::spoken_wallet_name;
use eframe::egui::{Context, Id, Order, Response, RichText, Role, Ui, WidgetInfo, vec2};
use egui_elements::{Button, CredentialsForm, Modal, QrImage, Theme, components::QrEncoding};
use ncrypt_me::Credentials;
use zeus_wallet::Wallet;

const MASTER_WALLET_WARNING: &str = "This is your master wallet, if this key gets exposed any child wallet you generated with this key will be compromised\n
You don't have to export this key unless something broke in Zeus and you cannot move your funds to other wallets";

const QR_IMAGE_SIZE: u32 = 250;

pub struct ExportKeyUi {
   open: bool,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   wallet_to_export: Option<Wallet>,
   /// The wallet this dialog is about, as speech: "Main, master wallet". The buttons are named
   /// from it, so a reader hears whose key is at stake. It never leaves the struct.
   spoken_wallet: String,
   private_key_qr: QrImage,
   show_key: bool,
   show_key_qrcode: bool,
   show_warning: bool,
   /// Whether the warning's safe exit still has to take the focus, so Enter backs out of the
   /// export instead of revealing the key.
   focus_dismiss: bool,
   /// Whether the body's first copy button still has to take the focus, once the warning is gone.
   focus_copy_key: bool,
   /// The one sentence the dialog wants said. A dialog cannot announce from inside its own draw,
   /// so the page drains this after it is drawn.
   notice: Option<String>,
   size: (f32, f32),
}

impl ExportKeyUi {
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form =
         CredentialsForm::new().with_min_size(form_size).with_enabled_virtual_keyboard();
      Self {
         open: false,
         credentials_form,
         verified_credentials: false,
         wallet_to_export: None,
         spoken_wallet: String::new(),
         private_key_qr: QrImage::empty_with_error("No QR code found".to_string()),
         show_key: false,
         show_key_qrcode: false,
         show_warning: false,
         focus_dismiss: false,
         focus_copy_key: false,
         notice: None,
         size: (550.0, 350.0),
      }
   }

   pub fn open(&mut self, wallet: WalletInfo) {
      let spoken_wallet = spoken_wallet_name(&wallet);
      let show_warning = wallet.is_master();

      crate::runtime::rt().spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let wallet_full = ctx.get_wallet(wallet.address);

         SHARED_GUI.write(|gui| {
            let export_ui = &mut gui.wallet_ui.export_key_ui;
            export_ui.wallet_to_export = wallet_full;
            export_ui.spoken_wallet = spoken_wallet;
            export_ui.show_warning = show_warning;
            // The safe exit takes the focus when the warning paints, so Enter backs out instead
            // of revealing the key.
            export_ui.focus_dismiss = show_warning;
            export_ui.open = true;
            export_ui.credentials_form.open();
         });
      });
   }

   pub fn close(&mut self) {
      self.open = false;
      self.wallet_to_export = None;
   }

   fn reset(&mut self) {
      self.close();
      *self = Self::new();
   }

   pub fn erase(&mut self, ctx: &Context) {
      self.credentials_form.erase();
      self.private_key_qr.clear(ctx);
      self.wallet_to_export = None;
   }

   /// Drop every secret this UI holds and close it.
   ///
   /// Used when Zeus auto-locks: a private key or seed shown before the lock
   /// must not resurface after unlock, so the verification state and the
   /// shown key/QR flags are cleared, not just the window flag.
   pub fn lock(&mut self, ctx: &Context) {
      self.erase(ctx);
      self.close();
      self.verified_credentials = false;
      self.show_key = false;
      self.show_key_qrcode = false;
      // A copy said before the lock is not about the vault the user comes back to.
      self.notice = None;
   }

   /// The sentence the dialog wants said, for the page's announcement channel.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      self.verify_credentials_ui(theme, ui);
      self.show_key(ctx, theme, ui);
   }

   /// Draw the master-wallet warning, if it is due.
   ///
   /// Returns the safe exit, so the dialog can hand it the focus once the warning has painted.
   fn show_warning(&mut self, theme: &Theme, ui: &mut Ui) -> Option<Response> {
      if !self.show_warning {
         return None;
      }

      ui.label(
         RichText::new("WARNING!")
            .size(theme.typography.very_large)
            .color(theme.colors.warning)
            .strong(),
      );

      // The paragraph is the whole danger, and a colour is not a sentence: it is an alert named
      // with the same words said in one breath, so a reader is told when the dialog opens instead
      // of walking past a red paragraph.
      let warning = ui.label(
         RichText::new(MASTER_WALLET_WARNING)
            .size(theme.typography.normal)
            .color(theme.colors.warning),
      );
      let sentence = warning_sentence();
      warning.widget_info(|| WidgetInfo::labeled(Role::Alert, ui.is_enabled(), sentence.clone()));

      let button_visuals = theme.button_visuals();
      let button_size = vec2(ui.available_width() * 0.9, 45.0);

      let text = RichText::new("I understand the risks").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals).min_size(button_size);
      if ui.add(button).clicked() {
         self.show_warning = false;
         // The body replaces the warning from here on, and its first copy button takes the focus.
         self.focus_dismiss = false;
         self.focus_copy_key = true;
      }

      let text = RichText::new("Changed my mind").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals).min_size(button_size);
      let changed_mind = ui.add(button);
      if changed_mind.clicked() {
         let erased = self.private_key_qr.clear(ui.ctx());
         self.reset();
         debug_assert!(erased);
      }

      Some(changed_mind)
   }

   fn show_key(&mut self, _ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.show_key || !self.verified_credentials {
         return;
      }

      let mut open = self.show_key;
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new("Export Key").size(theme.typography.heading);
      let id = Id::unique("show_key_export_ui");

      // The buttons are named for the wallet the dialog was opened on, so a reader hears whose key
      // it is about; the words they draw do not change.
      let copy_key_name = format!("Copy key for {}", self.spoken_wallet);
      let copy_seed_name = format!("Copy seed phrase for {}", self.spoken_wallet);
      let show_qr_name = format!("Show QR code for {}", self.spoken_wallet);

      // The controls that ask for the focus once the dialog has painted: the warning's safe exit
      // first, then the body's first copy button.
      let mut warning_exit: Option<Response> = None;
      let mut first_copy: Option<Response> = None;

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().button_padding = theme.button_padding;

               warning_exit = self.show_warning(theme, ui);
               if self.show_warning {
                  return;
               }

               let Some(wallet) = self.wallet_to_export.as_ref() else {
                  ui.label(
                     RichText::new("No wallet found, this is a bug").size(theme.typography.normal),
                  );
                  return;
               };

               ui.label(
                  RichText::new("Make sure to save this key in a safe place!")
                     .size(theme.typography.large)
                     .color(theme.colors.warning),
               );

               let has_seed = wallet.seed_phrase.is_some();
               let n = if has_seed { 3.0 } else { 2.0 };
               let content_width = ui.available_width() * 0.9;
               let gap = theme.spacing.sm;
               let button_size = vec2((content_width - gap * (n - 1.0)) / n, 45.0);

               ui.allocate_ui(vec2(content_width, 45.0), |ui| {
                  ui.spacing_mut().item_spacing.x = gap;
                  ui.horizontal(|ui| {
                     let text = RichText::new("Copy Key").size(theme.typography.normal);
                     let button = Button::new(text).visuals(button_visuals).min_size(button_size);
                     let copy_key = ui.add(button);

                     copy_key.widget_info(|| {
                        WidgetInfo::labeled(Role::Button, ui.is_enabled(), &copy_key_name)
                     });

                     if copy_key.clicked() {
                        ui.ctx().copy_text(wallet.key_string().unlock_str(|key| key.to_string()));
                        // Nothing changes on screen when a copy happens, so it has to be said.
                        self.notice = Some("Private key copied.".to_string());
                     }

                     // The first button of the body is the one the dialog hands the focus to once
                     // the warning is out of the way.
                     first_copy = Some(copy_key);

                     if let Some(seed_phrase) = &wallet.seed_phrase {
                        let text = RichText::new("Copy Seed Phrase").size(theme.typography.normal);
                        let button =
                           Button::new(text).visuals(button_visuals).min_size(button_size);
                        let copy_seed = ui.add(button);

                        copy_seed.widget_info(|| {
                           WidgetInfo::labeled(Role::Button, ui.is_enabled(), &copy_seed_name)
                        });

                        if copy_seed.clicked() {
                           let seed =
                              seed_phrase.lock(|phrase| phrase.unlock_str(|seed| seed.to_string()));
                           ui.ctx().copy_text(seed);
                           self.notice = Some("Seed phrase copied.".to_string());
                        }
                     }

                     let text = RichText::new("Show QR Code").size(theme.typography.normal);
                     let button = Button::new(text).visuals(button_visuals).min_size(button_size);
                     let show_qr = ui.add(button);

                     show_qr.widget_info(|| {
                        WidgetInfo::labeled(Role::Button, ui.is_enabled(), &show_qr_name)
                     });

                     if show_qr.clicked() {
                        self.show_key_qrcode = true;
                     }
                  });
               });

               ui.add_space(10.0);

               // This does not erase the clipboard
               // The key could still remain in cliboard history
               let clear_size = vec2(content_width, 45.0);
               let text = RichText::new("Clear Clipboard").size(theme.typography.normal);
               let button = Button::new(text).min_size(clear_size);
               if ui.add(button).clicked() {
                  ui.ctx().copy_text(String::new());
                  self.notice = Some("Clipboard cleared.".to_string());
               }

               if self.show_key_qrcode {
                  if let Some(error) = self.private_key_qr.error() {
                     ui.label(RichText::new(error.to_string()).size(theme.typography.large));
                  }

                  // The QR is the key itself. It is drawn as a picture and stays unnamed: no
                  // label, no description, nothing for a reader to read out.
                  let size = self.private_key_qr.image_size_pt(ui.ctx().pixels_per_point());
                  let image = self.private_key_qr.image().fit_to_exact_size(size);
                  ui.add(image);
               }
            });
         });

      // A dialog spends its first passes sizing its body, where nothing is visible and a focus
      // request is dropped, so a control asks for the focus on the first pass that is drawn. It
      // asks after the dialog has drawn, so the dialog's own opening grab — its Close button —
      // cannot take it back.
      let body_only = warning_exit.is_none();

      if let Some(exit) = warning_exit.filter(|exit| exit.visible() && self.focus_dismiss) {
         ui.ctx().memory_mut(|memory| memory.request_focus(exit.id));
         self.focus_dismiss = false;
      }

      // The pass that dismisses the warning still draws the warning first, and the ids of the body
      // drawn after it are not the ids of the next pass. The copy button therefore asks for the
      // focus on the first pass that draws the body alone.
      if let Some(copy) =
         first_copy.filter(|copy| body_only && copy.visible() && self.focus_copy_key)
      {
         ui.ctx().memory_mut(|memory| memory.request_focus(copy.id));
         self.focus_copy_key = false;
      }

      if !open {
         let erased = self.private_key_qr.clear(ui.ctx());
         self.reset();
         debug_assert!(erased);
      }
   }

   fn verify_credentials_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.credentials_form.is_open() || !self.open {
         return;
      }

      let mut open = self.credentials_form.is_open();
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new("Verify Credentials").size(theme.typography.heading);
      let id = Id::unique("verify_credentials_window");
      let mut clicked = false;

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;
               ui.add_space(20.0);

               // The same form opens before a delete, so the dialog says which of the two it is
               // asking for.
               ui.label(
                  RichText::new("This verifies your vault credentials before the key is shown.")
                     .size(theme.typography.normal),
               );

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs);
                  self.credentials_form.show(ui);
               });

               let text = RichText::new("Confirm").size(theme.typography.normal);
               let button = Button::new(text)
                  .visuals(button_visuals)
                  .min_size(vec2(ui.available_width() * 0.8, 45.0));

               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let username = self.credentials_form.username();
         let password = self.credentials_form.password();
         let confirm_password = self.credentials_form.confirm_password();
         let credentials = Credentials::new(username, password, confirm_password);
         on_verify_credentials(credentials);
      }

      if !open {
         self.close();
         self.credentials_form.erase();
      }
   }

   /// Open the body on the key: its QR built, the key shown and the credentials form gone.
   fn reveal_key(&mut self, qr: QrImage) {
      self.private_key_qr = qr;
      // Allow the user to export the key
      self.show_key = true;
      // Mark the credentials as verified
      self.verified_credentials = true;
      // Erase the credentials form
      self.credentials_form.erase();
      // Close the credentials form
      self.credentials_form.close();
   }
}

/// The warning as one spoken sentence.
///
/// The drawn paragraph breaks between its two sentences; a reader hears them in one breath, so the
/// alert's name is the same words joined by a space.
fn warning_sentence() -> String {
   MASTER_WALLET_WARNING.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The QR of `key`, encoded for this dialog's size.
fn key_qr(key: &str, uri: String) -> QrImage {
   let encoding = QrEncoding {
      target_px: QR_IMAGE_SIZE,
      ..Default::default()
   };

   QrImage::with_encoding(key, uri, encoding)
}

fn on_verify_credentials(credentials: Credentials) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.write(|gui| {
         gui.loading_window.open("Checking credentials...");
         gui.request_repaint();
         gui.ctx.clone()
      });

      match ctx.check_credentials(&credentials, false) {
         CredentialCheck::Matched => {
            let key_data = SHARED_GUI.read(|gui| {
               gui.wallet_ui.export_key_ui.wallet_to_export.as_ref().map(|wallet| {
                  (
                     wallet.key_string(),
                     format!("bytes://key-{}.png", wallet.address()),
                  )
               })
            });

            // The QR is encoded after the global lock is released: it is the one expensive step
            // here, and holding the read lock through it would stall the frame.
            let qr_image = if let Some((key_data, uri)) = key_data {
               key_data.unlock_str(|key| key_qr(key, uri))
            } else {
               QrImage::empty_with_error("No wallet found".to_string())
            };

            SHARED_GUI.write(|gui| {
               gui.wallet_ui.export_key_ui.reveal_key(qr_image);
               gui.loading_window.reset();
               gui.request_repaint();
            });
         }
         CredentialCheck::Mismatch => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window("Credentials do not match");
               gui.loading_window.reset();
               gui.request_repaint();
            });
         }
         // The attempt cap was reached: `check_credentials` has shown why and is
         // shutting Zeus down.
         CredentialCheck::LockedOut => {}
      }
   });
}

#[cfg(test)]
impl ExportKeyUi {
   /// Open the dialog on `wallet` the way [`Self::open`] leaves it once its worker has read the
   /// full wallet out of the vault, so a tree test needs no vault, runtime or worker.
   pub(crate) fn for_test(&mut self, info: &WalletInfo, wallet: Wallet) {
      self.spoken_wallet = spoken_wallet_name(info);
      self.show_warning = info.is_master();
      self.focus_dismiss = info.is_master();
      self.wallet_to_export = Some(wallet);
      self.open = true;
      self.credentials_form.open();
   }

   /// Show the key and build its QR: the state a matched credential leaves behind.
   pub(crate) fn reveal_key_for_test(&mut self) {
      let wallet = self.wallet_to_export.as_ref().expect("the dialog must be open on a wallet");
      let uri = format!("bytes://key-{}.png", wallet.address());
      let qr = wallet.key_string().unlock_str(|key| key_qr(key, uri));

      self.reveal_key(qr);
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The alert is one line: a reader is told the danger in one breath, not asked to interpret the
   /// line break the paragraph is drawn with.
   #[test]
   fn the_warning_sentence_is_one_line() {
      let sentence = warning_sentence();

      assert!(!sentence.contains('\n'));

      assert!(
         sentence.starts_with("This is your master wallet"),
         "the sentence starts with the paragraph's first words"
      );

      assert!(
         sentence.ends_with("move your funds to other wallets"),
         "and ends with its last"
      );
   }
}
