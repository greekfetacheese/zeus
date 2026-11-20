//! UI that allows the user to delete a wallet

use crate::core::types::CredentialCheck;
use crate::core::{WalletInfo, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::spoken_wallet_name;
use eframe::egui::{Id, Key, Order, Response, RichText, Role, Ui, WidgetInfo, vec2};
use egui_elements::{Button, CredentialsForm, Modal, Theme};
use ncrypt_me::Credentials;

/// The consequence of a delete in one sentence: drawn as the paragraph and said as the alert, so a
/// reader is told what the wallet takes with it instead of walking past a coloured line.
const DELETION_CONSEQUENCE: &str = "Deleting this wallet will also delete all its transaction history and token approval data next time Zeus starts.";

pub struct DeleteWalletUi {
   open: bool,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   wallet_to_delete: Option<WalletInfo>,
   /// Whether the safe way out still has to take the focus, so Enter keeps the wallet instead of
   /// deleting it.
   focus_keep: bool,
   /// The one sentence the dialog wants said. A dialog cannot announce from inside its own draw,
   /// so the page drains this after it is drawn.
   notice: Option<String>,
   size: (f32, f32),
}

impl DeleteWalletUi {
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form =
         CredentialsForm::new().with_min_size(form_size).with_enabled_virtual_keyboard();
      Self {
         open: false,
         credentials_form,
         verified_credentials: false,
         wallet_to_delete: None,
         focus_keep: false,
         notice: None,
         size: (550.0, 350.0),
      }
   }

   pub fn erase(&mut self) {
      self.credentials_form.erase();
   }

   /// Drop every secret this UI holds and close it.
   ///
   /// Used when Zeus auto-locks: the verified-credentials flag is what authorizes
   /// the delete (`delete_wallet_ui` refuses to show without it and
   /// `verified_credentials` is set once the user has proved it), so a wallet whose
   /// credentials were verified before the lock must not be deletable after unlock
   /// without proving them again. The carried `wallet_to_delete` goes with it.
   pub fn lock(&mut self) {
      self.erase();
      self.close();
      self.verified_credentials = false;
      self.wallet_to_delete = None;
      // A sentence raised before the lock is not about the flow the user comes back to.
      self.notice = None;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, wallet: WalletInfo) {
      self.open = true;
      self.wallet_to_delete = Some(wallet);
      // The confirmation hands the focus to its safe exit the first time it paints, so Enter keeps
      // the wallet instead of deleting it.
      self.focus_keep = true;
      self.credentials_form.open();
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new();
   }

   /// The sentence the dialog wants said, for the page's announcement channel.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Take the safe way out: the wallet is kept, and the dialog says so — going away on its own
   /// would not tell a listener which of the two ways out was taken.
   fn keep_wallet(&mut self) {
      self.reset();
      // Raised after the reset: it builds a fresh flow, and the sentence belongs to the one that
      // just ended.
      self.notice = Some("Wallet not deleted.".to_string());
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      self.verify_credentials_ui(theme, ui);
      self.delete_wallet_ui(ctx, theme, ui);
   }

   fn verify_credentials_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.credentials_form.is_open() || !self.open {
         return;
      }

      let button_visuals = theme.button_visuals();
      let mut open = self.credentials_form.is_open();
      let mut clicked = false;

      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new("Verify Credentials").size(theme.typography.heading);
      let id = Id::unique("verify_credentials_delete_wallet_ui");

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

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;

               // The same form opens before an export, so the dialog says here which of the two it
               // is asking for.
               ui.label(
                  RichText::new(
                     "This verifies your vault credentials before the wallet is deleted.",
                  )
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

   fn delete_wallet_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.verified_credentials || !self.open {
         return;
      }

      let Some(wallet) = self.wallet_to_delete.clone() else {
         return;
      };

      let mut open = self.open;
      let mut clicked = false;
      let mut cancel = false;
      let mut escape = false;
      let mut keep_button: Option<Response> = None;

      // The dialog is named after the wallet it is about and the question it asks, so a reader is
      // asked something instead of landing in an unnamed window.
      let spoken_wallet = spoken_wallet_name(&wallet);
      let title = RichText::new(format!("Delete {spoken_wallet}?")).size(theme.typography.heading);
      let delete_name = format!("Delete {spoken_wallet}");

      let id = Id::unique("delete_wallet_ui_delete_wallet");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(false)
         .show(ui.ctx(), |ui| {
            // Escape is the dialog's own way out: `closable(false)` keeps the backdrop from
            // dismissing it, so the key is read here and answered as the cancel.
            escape = ui.input(|input| input.key_pressed(Key::Escape));

            ui.set_width(self.size.0);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().button_padding = theme.button_padding;

               let name =
                  ui.label(RichText::new(wallet.name_with_source()).size(theme.typography.large));

               // The heading already says which wallet this is, so the drawn name repeats it and
               // its label is left out of the reading.
               ui.ctx().accesskit_node_builder(name.id, |node| node.set_hidden());

               ui.label(
                  RichText::new(wallet.address.to_string())
                     .size(theme.typography.small)
                     .color(theme.colors.text_muted)
                     .monospace(),
               );

               let include_testnets = ctx.chain.is_testnet();
               let value = ctx.get_total_value(wallet.address, include_testnets);

               let size = vec2(ui.available_width() * 0.4, 30.0);
               let frame = theme.frame2;

               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     frame.show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing.xl;
                        ui.label(
                           RichText::new(format!(
                              "Public ${:.10}",
                              value.for_mode(false).abbreviated()
                           ))
                           .size(theme.typography.normal)
                           .strong(),
                        );
                        ui.separator();
                        ui.label(
                           RichText::new(format!(
                              "Railgun ${:.10}",
                              value.for_mode(true).abbreviated()
                           ))
                           .size(theme.typography.normal)
                           .strong(),
                        );
                     });
                  });
               });

               // The consequence is a sentence, not a colour: it is announced as an alert when the
               // dialog opens, so a reader is told what the delete costs.
               let warning = ui.label(
                  RichText::new(DELETION_CONSEQUENCE)
                     .size(theme.typography.normal)
                     .color(theme.colors.warning),
               );
               warning.widget_info(|| {
                  WidgetInfo::labeled(Role::Alert, ui.is_enabled(), DELETION_CONSEQUENCE)
               });

               ui.label(
                  RichText::new("Are you sure you want to continue?")
                     .size(theme.typography.normal)
                     .color(theme.colors.warning)
                     .strong(),
               );

               let content_width = ui.available_width() * 0.9;
               let button_size = vec2((content_width - theme.spacing.sm) / 2.0, 45.0);

               ui.allocate_ui(vec2(content_width, 45.0), |ui| {
                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = theme.spacing.md;

                     let text = RichText::new("Keep this wallet").size(theme.typography.normal);
                     let button = Button::new(text).visuals(button_visuals).min_size(button_size);
                     let keep = ui.add(button);

                     if keep.clicked() {
                        cancel = true;
                     }

                     keep_button = Some(keep);

                     let text = RichText::new("Delete").size(theme.typography.normal);
                     let button = Button::new(text).visuals(button_visuals).min_size(button_size);
                     let delete = ui.add(button);

                     // The label stays the one word; the name says what it removes.
                     delete.widget_info(|| {
                        WidgetInfo::labeled(Role::Button, ui.is_enabled(), &delete_name)
                     });

                     if delete.clicked() {
                        clicked = true;
                     }
                  });
               });
            });
         });

      // The dialog spends its first passes sizing its body, where nothing is visible and a focus
      // request is dropped, so the safe exit asks on the first pass that is drawn — after the
      // dialog has drawn, so its own opening grab cannot take the focus back. Enter then keeps the
      // wallet instead of deleting it.
      if let Some(keep) = keep_button.filter(|keep| keep.visible() && self.focus_keep) {
         ui.ctx().memory_mut(|memory| memory.request_focus(keep.id));
         self.focus_keep = false;
      }

      // Escape is the cancel, and the dialog has no other way out.
      if escape {
         cancel = true;
      }

      if cancel {
         self.keep_wallet();
         return;
      }

      if clicked {
         let is_current = ctx.is_current_wallet(wallet.address);
         on_delete_wallet(wallet, is_current);
         self.reset();
         return;
      }

      if !open {
         self.keep_wallet();
      }
   }
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
            SHARED_GUI.write(|gui| {
               // Mark the credentials as verified
               gui.wallet_ui.delete_wallet_ui.verified_credentials = true;
               // Close the verify credentials ui
               gui.wallet_ui.delete_wallet_ui.credentials_form.close();
               // Open the delete wallet ui
               gui.wallet_ui.delete_wallet_ui.open = true;
               // Erase the credentials form
               gui.wallet_ui.delete_wallet_ui.credentials_form.erase();
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

fn on_delete_wallet(wallet: WalletInfo, is_current: bool) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let mut new_vault = ctx.get_vault();
      new_vault.remove_wallet(wallet.address);

      // Set the master wallet as selected to avoid state inconsistencies
      if is_current {
         let master_wallet = new_vault.get_master_wallet();
         let master_info = WalletInfo::from_wallet(&master_wallet, true);
         ctx.write(|ctx| {
            ctx.current_wallet = master_info.clone();
         });
         SHARED_GUI.write(|gui| {
            gui.account_panel.set_current_wallet(master_info);
         });
      }

      SHARED_GUI.write(|gui| {
         gui.loading_window.open("Encrypting vault...");
         gui.request_repaint();
      });

      // Encrypt the vault
      match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               gui.loading_window.reset();
               gui.wallet_ui.delete_wallet_ui.wallet_to_delete = None;
               gui.wallet_ui.delete_wallet_ui.verified_credentials = false;
               gui.open_msg_window("Wallet Deleted");
               gui.request_repaint();
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               gui.loading_window.reset();
               gui.open_msg_window(format!("Failed to encrypt vault: {}", e));
               gui.request_repaint();
            });
            return;
         }
      };

      ctx.set_vault(new_vault);
      ctx.build_wallet_info_cache();

      // Recalculate the wallets
      SHARED_GUI.write(|gui| {
         gui.wallet_ui.calc_wallet_value();
      });
   });
}

#[cfg(test)]
impl DeleteWalletUi {
   /// Open the confirmation on `wallet` with its credentials already verified — the state
   /// `on_verify_credentials` leaves behind — so a tree test needs no vault, runtime or worker.
   pub(crate) fn for_test(&mut self, wallet: WalletInfo) {
      self.open(wallet);
      self.verified_credentials = true;
      self.credentials_form.close();
   }

   /// Whether the flow still authorizes a delete: the verified credentials and the wallet they
   /// were proved for, which a cancel must drop.
   pub(crate) fn holds_authorization_for_test(&self) -> bool {
      self.verified_credentials && self.wallet_to_delete.is_some()
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use secure_types::SecureString;

   /// The state `on_verify_credentials` leaves behind: the verification flag set, the
   /// wallet carried, the credentials still typed into the form.
   fn verified_form() -> DeleteWalletUi {
      let mut ui = DeleteWalletUi::new();
      ui.open(WalletInfo::default());
      ui.verified_credentials = true;
      ui.credentials_form.set_username_text(SecureString::from("user"));
      ui.credentials_form.set_password_text(SecureString::from("pass"));
      ui
   }

   /// `lock` is the auto-lock path: it has to drop the authorization bit as well as
   /// the form, or a wallet verified before the lock stays deletable after unlock.
   #[test]
   fn lock_drops_the_verified_delete_form() {
      let mut ui = verified_form();
      assert!(ui.open);
      assert!(ui.verified_credentials);
      assert!(ui.wallet_to_delete.is_some());

      ui.lock();

      assert!(!ui.open);
      assert!(!ui.verified_credentials);
      assert!(ui.wallet_to_delete.is_none());
      assert!(ui.credentials_form.username().is_empty());
      assert!(ui.credentials_form.password().is_empty());
   }

   /// The way out that is not a delete says so, once: the dialog going away does not tell a
   /// listener whether the wallet was removed or kept.
   #[test]
   fn a_kept_wallet_is_said_once() {
      let mut ui = verified_form();

      ui.keep_wallet();

      assert!(!ui.is_open());
      assert!(!ui.verified_credentials);
      assert!(ui.wallet_to_delete.is_none());
      assert_eq!(
         ui.take_notice().as_deref(),
         Some("Wallet not deleted."),
         "the safe way out is one of the dialog's sentences"
      );
      assert_eq!(ui.take_notice(), None, "the frame says it once");
   }
}
