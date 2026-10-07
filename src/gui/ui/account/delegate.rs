//! Delegate / undelegate flow for the selected wallet
//!
//! Owns the delegate modal, the credentials verification modal, and the
//! delegate / undelegate transactions.

use crate::core::types::CredentialCheck;
use crate::core::{ZeusContext, delegate_to};
use crate::gui::{SHARED_GUI, ui::tx::address};
use crate::utils::RT;
use egui::{CursorIcon, FontId, Id, Margin, Order, RichText, Spinner, Ui, vec2};
use egui_elements::{Button, CredentialsForm, Modal, SecureTextEdit, Theme};
use egui_lucide::Lucide;
use ncrypt_me::Credentials;
use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;

/// Delegate / undelegate the selected wallet to a smart contract
pub struct DelegateUi {
   open: bool,
   to: String,
   credentials_form: CredentialsForm,
   syncing: bool,
}

impl DelegateUi {
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form =
         CredentialsForm::new().with_min_size(form_size).with_enabled_virtual_keyboard();

      Self {
         open: false,
         to: String::new(),
         credentials_form,
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
      self.credentials_form.close();
      self.credentials_form.erase();
   }

   pub fn erase(&mut self) {
      self.credentials_form.erase();
   }

   /// Show the delegate modal and the credentials modal
   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, wallet: Address, ui: &mut Ui) {
      self.show_deleg_settings_window(ctx, theme, wallet, ui);
      self.verify_credentials_ui(theme, wallet, ui);
   }

   fn show_deleg_settings_window(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      wallet: Address,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let chain = ctx.chain;
      let delegated = ctx.delegated_wallets.get(chain.id(), wallet);
      let heading = if delegated.is_some() {
         "Currently delegated"
      } else {
         "Delegate to"
      };
      let title = RichText::new(heading).size(theme.typography.heading);
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let id = Id::new("delegate_settings_window");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(450.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().button_padding = theme.button_padding;

               self.refresh(theme, wallet, ui);

               if let Some(delegated_address) = delegated {
                  self.undelegate_ui(ctx, theme, wallet, delegated_address, ui);
               } else {
                  self.delegate_ui(theme, ui);
               }
            });
         });

      self.open = open;

      if !open {
         self.to.clear();
         self.credentials_form.close();
         self.credentials_form.erase();
      }
   }

   fn refresh(&mut self, theme: &Theme, wallet: Address, ui: &mut Ui) {
      ui.spacing_mut().button_padding = theme.button_padding;

      let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();

      if !self.syncing {
         let text = RichText::new("Check Delegation Status").size(theme.typography.normal);
         let button = Button::image_and_text(icon, text);
         let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

         if res.clicked() {
            self.syncing = true;

            RT.spawn(async move {
               let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
               let chain = ctx.chain();
               match ctx.check_delegated_wallet_status(chain.id(), wallet).await {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.account_panel.delegate.syncing = false;
                     });
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        let msg = format!(
                           "Error while checking wallet delegation status: {}",
                           e
                        );
                        gui.open_msg_window(msg);
                        gui.account_panel.delegate.syncing = false;
                     });
                  }
               }
            });
         }
      } else {
         ui.add(Spinner::new().size(17.0).color(theme.colors.text));
      }
   }

   fn delegate_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.lg);

      let text_edit_visuals = theme.text_edit_visuals();
      let button_visuals = theme.button_visuals();
      let field_width = ui.available_width() * 0.9;
      let field_size = vec2(field_width, 45.0);

      let hint = RichText::new("Enter a smart contract address")
         .color(theme.colors.text_muted)
         .size(theme.typography.normal);

      ui.add_space(10.0);

      ui.allocate_ui(field_size, |ui| {
         let text = SecureTextEdit::singleline(&mut self.to)
            .visuals(text_edit_visuals)
            .hint_text(hint)
            .font(FontId::proportional(theme.typography.normal))
            .margin(Margin::same(10))
            .desired_width(ui.available_width());
         ui.add(text);
      });

      ui.add_space(10.0);

      let text = RichText::new("Delegate").size(theme.typography.large);
      let button = Button::new(text).visuals(button_visuals).min_size(field_size);

      if ui.add(button).clicked() {
         let delegate_to_addr = self.to.clone();
         if Address::from_str(&delegate_to_addr).is_err() {
            RT.spawn(async move {
               SHARED_GUI.write(|gui| {
                  let msg = format!(
                     "Not a valid Ethereum address: {}",
                     delegate_to_addr
                  );
                  gui.open_msg_window(msg);
                  gui.request_repaint();
               });
            });
            return;
         }

         self.credentials_form.open();
      }
   }

   fn verify_credentials_ui(&mut self, theme: &Theme, wallet: Address, ui: &mut Ui) {
      if !self.credentials_form.is_open() || !self.open {
         return;
      }

      let mut open = self.credentials_form.is_open();
      let mut clicked = false;
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new("Verify Credentials").size(theme.typography.heading);
      let id = Id::new("verify_credentials_delegate_ui");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(550.0, 350.0));

            let button_visuals = theme.button_visuals();

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
               gui.loading_window.open("Checking credentials...");
               gui.request_repaint();
               gui.ctx.clone()
            });

            match ctx.check_credentials(&credentials, false) {
               CredentialCheck::Matched => {
                  let (delegate_to_addr, chain) = SHARED_GUI.write(|gui| {
                     gui.account_panel.delegate.credentials_form.erase();
                     gui.account_panel.delegate.credentials_form.close();
                     (gui.account_panel.delegate.to.clone(), ctx.chain())
                  });

                  let delegate_address = match Address::from_str(&delegate_to_addr) {
                     Ok(address) => address,
                     Err(_) => {
                        SHARED_GUI.write(|gui| {
                           let msg = format!(
                              "Not a valid Ethereum address: {}",
                              delegate_to_addr
                           );
                           gui.open_msg_window(msg);
                           gui.loading_window.reset();
                           gui.request_repaint();
                        });
                        return;
                     }
                  };

                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open("Wait while magic happens");
                     gui.account_panel.delegate.close();
                     gui.request_repaint();
                  });

                  RT.spawn(async move {
                     let source_is_zeus = true;
                     match delegate_to(
                        ctx,
                        source_is_zeus,
                        chain,
                        wallet,
                        delegate_address,
                     )
                     .await
                     {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              gui.loading_window.reset();
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              let msg = format!("Error while delegating: {}", e);
                              gui.open_msg_window(msg);
                              gui.loading_window.reset();
                              gui.account_panel.delegate.open();
                              gui.notification.reset();
                           });
                        }
                     }
                  });
               }
               CredentialCheck::Mismatch => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Credentials do not match");
                     gui.loading_window.reset();
                     gui.request_repaint();
                  });
               }
               // The attempt cap was reached: Zeus is shutting down.
               CredentialCheck::LockedOut => {}
            }
         });
      }

      if !open {
         self.credentials_form.close();
         self.credentials_form.erase();
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
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.lg);

      let frame = theme.frame2;
      let chain = ctx.chain;
      let label = "Contract";

      ui.add_space(10.0);

      frame.show(ui, |ui| {
         address(ctx, chain, label, delegated_address, theme, ui);
      });

      ui.add_space(10.0);

      let text = RichText::new("Undelegate").size(theme.typography.large);
      let btn_size = vec2(ui.available_width() * 0.9, 45.0);
      let button = Button::new(text).min_size(btn_size);

      let clicked = ui.add(button).clicked();
      if clicked {
         RT.spawn(async move {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.account_panel.delegate.close();
               gui.request_repaint();
               gui.ctx.clone()
            });

            let source_is_zeus = true;

            match delegate_to(ctx, source_is_zeus, chain, wallet, Address::ZERO).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     let msg = format!("Error while undelegating: {}", e);
                     gui.open_msg_window(msg);
                     gui.loading_window.reset();
                     gui.account_panel.delegate.open();
                     gui.notification.reset();
                  });
               }
            }
         });
      }
   }
}
