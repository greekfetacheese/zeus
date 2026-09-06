//! UI that allows the user to delete a wallet

use crate::core::{WalletInfo, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{Id, Order, RichText, Ui, vec2};
use egui_elements::{Button, CredentialsForm, Modal, Theme};
use ncrypt_me::Credentials;

pub struct DeleteWalletUi {
   open: bool,
   credentials_form: CredentialsForm,
   verified_credentials: bool,
   wallet_to_delete: Option<WalletInfo>,
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
         size: (550.0, 350.0),
      }
   }

   pub fn erase(&mut self) {
      self.credentials_form.erase();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, wallet: WalletInfo) {
      self.open = true;
      self.wallet_to_delete = Some(wallet);
      self.credentials_form.open();
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new();
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
      let id = Id::new("verify_credentials_delete_wallet_ui");

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

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Decrypting vault...");
               gui.request_repaint();
               gui.ctx.clone()
            });

            let creds_match = ctx.read_vault(|vault| vault.credentials_match(&credentials));

            // Verify the credentials by just decrypting the vault
            match creds_match {
               true => {
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
               false => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Credentials do not match");
                     gui.loading_window.reset();
                     gui.request_repaint();
                  });
               }
            }
         });
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

      let id = Id::new("delete_wallet_ui_delete_wallet");

      Modal::new(id, &mut open)
      .backdrop_order(Order::Middle)
      .content_order(Order::Foreground)
      .closable(false)
      .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().button_padding = theme.button_padding;

               ui.label(
                  RichText::new(wallet.name_with_source()).size(theme.typography.large),
               );

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
                        "Public ${}",
                        value.for_mode(false).abbreviated()
                     ))
                     .size(theme.typography.normal)
                     .strong(),
                  );
                  ui.label(
                     RichText::new(format!(
                        "Railgun ${}",
                        value.for_mode(true).abbreviated()
                     ))
                     .size(theme.typography.normal)
                     .strong(),
                  );
               });
               });
            });

               ui.label(
                  RichText::new(
                     "Deleting this wallet will also delete all its transaction history and token approval data next time Zeus starts.",
                  )
                  .size(theme.typography.normal)
                  .color(theme.colors.warning),
               );

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

                  let text = RichText::new("Changed my mind").size(theme.typography.normal);
                  let button = Button::new(text).visuals(button_visuals).min_size(button_size);

                  if ui.add(button).clicked() {
                     cancel = true;
                  }

                  let text = RichText::new("Delete").size(theme.typography.normal);
                  let button = Button::new(text).visuals(button_visuals).min_size(button_size);

                  if ui.add(button).clicked() {
                     clicked = true;
                  }
               });
            });
            });
         });

      if cancel {
         self.reset();
         return;
      }

      if clicked {
         let is_current = ctx.is_current_wallet(wallet.address);

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            let mut new_vault = ctx.get_vault();
            new_vault.remove_wallet(wallet.address);

            // Set the master wallet as selected to avoid state inconsistencies
            if is_current {
               let master_wallet = new_vault.get_master_wallet();
               ctx.write(|ctx| {
                  ctx.current_wallet = master_wallet.clone();
               });
               SHARED_GUI.write(|gui| {
                  gui.header.set_current_wallet(master_wallet);
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
                     gui.open_msg_window(format!(
                        "Failed to encrypt vault: {}",
                        e.to_string()
                     ));
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

         self.reset();
         return;
      }

      if !open {
         self.reset();
      }
   }
}
