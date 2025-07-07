use crate::assets::icons::Icons;
use crate::core::{WalletInfo, ZeusCtx, utils::RT};
use crate::gui::{SHARED_GUI, ui::CredentialsForm};
use eframe::egui::{Align2, Button, Frame, Id, Order, RichText, Ui, Vec2, Window, vec2};
use egui_theme::Theme;
use std::sync::Arc;

const VIEW_KEY_MSG: &str =
   "The key has been copied! In 60 seconds it will be cleared from the clipboard.";
const CLIPBOARD_EXPIRY: u64 = 60;

pub struct KeyExporter {
   pub wallet: Option<WalletInfo>,
   /// When the key was copied to the clipboard
   pub key_copied_time: Option<std::time::Instant>,

   /// How much time before we force clear the clipboard
   clipboard_clear_delay: std::time::Duration,
}

impl KeyExporter {
   pub fn new() -> Self {
      Self {
         wallet: None,
         key_copied_time: None,
         clipboard_clear_delay: std::time::Duration::from_secs(CLIPBOARD_EXPIRY),
      }
   }

   pub fn export_key(&mut self, zeus_ctx: ZeusCtx, ctx: egui::Context) {
      let info = self.wallet.take().unwrap();
      let wallet = zeus_ctx.get_wallet(info.address).unwrap();
      let key_string = wallet.key_string().str_scope(|key| key.to_string());
      ctx.copy_text(key_string);
      self.key_copied_time = Some(std::time::Instant::now());
      tracing::info!("Key copied to clipboard");
   }

   pub fn update(&mut self, theme: &Theme, ctx: egui::Context, ui: &mut egui::Ui) {
      // Check if we need to clear the clipboard
      if let Some(copy_time) = self.key_copied_time {
         let elapsed = copy_time.elapsed();
         if elapsed >= self.clipboard_clear_delay {
            ctx.copy_text("".to_string()); // Overwrite with empty string
            self.key_copied_time = None; // Reset timer
            tracing::info!("Key cleared from clipboard");
         } else {
            let remaining = self.clipboard_clear_delay - elapsed;
            let text = RichText::new(format!(
               "Clipboard will clear in {} seconds",
               remaining.as_secs()
            ))
            .size(theme.text_sizes.normal);
            ui.label(text);
         }
      }
   }
}

pub struct ExportKeyUi {
   pub open: bool,
   pub credentials_form: CredentialsForm,
   pub verified_credentials: bool,
   pub exporter: KeyExporter,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl ExportKeyUi {
   pub fn new() -> Self {
      Self {
         open: false,
         credentials_form: CredentialsForm::new(),
         verified_credentials: false,
         exporter: KeyExporter::new(),
         size: (550.0, 350.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.credentials_form.erase();
      self.credentials_form.open = false;
      self.verified_credentials = false;
      tracing::info!("ViewKeyUi reset");
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.verify_credentials_ui(ctx, theme, icons, ui);
   }

   pub fn verify_credentials_ui(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let mut open = self.credentials_form.open;
      let mut clicked = false;

      let id = Id::new("verify_credentials_view_key_ui");
      Window::new(RichText::new("Verify Credentials").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               self.credentials_form.show(theme, icons, ui);

               let button = Button::new(RichText::new("Confrim").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let mut account = ctx.get_account();
         account.set_credentials(self.credentials_form.credentials.clone());
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Decrypting account...");
            });

            // Verify the credentials by just decrypting the account
            match account.decrypt(None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     let egui_ctx = gui.egui_ctx.clone();
                     gui.wallet_ui
                        .export_key_ui
                        .exporter
                        .export_key(ctx.clone(), egui_ctx);
                     gui.wallet_ui.export_key_ui.reset();
                     gui.open_msg_window("", VIEW_KEY_MSG);
                     gui.loading_window.open = false;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to decrypt account", e.to_string());
                     gui.loading_window.open = false;
                  });
               }
            }
         });
      }

      self.credentials_form.open = open;
      if !self.credentials_form.open {
         self.credentials_form.erase();
      }
   }
}

pub struct DeleteWalletUi {
   pub open: bool,
   pub credentials_form: CredentialsForm,
   pub verified_credentials: bool,
   pub wallet_to_delete: Option<WalletInfo>,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl DeleteWalletUi {
   pub fn new() -> Self {
      Self {
         open: false,
         credentials_form: CredentialsForm::new(),
         verified_credentials: false,
         wallet_to_delete: None,
         size: (550.0, 350.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.verify_credentials_ui(ctx.clone(), theme, icons, ui);
      self.delete_wallet_ui(ctx, theme, ui);
   }

   pub fn verify_credentials_ui(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let mut open = self.credentials_form.open;
      let mut clicked = false;

      let id = Id::new("verify_credentials_delete_wallet_ui");
      Window::new(RichText::new("Verify Credentials").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               self.credentials_form.show(theme, icons, ui);

               let button = Button::new(RichText::new("Confrim").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let mut account = ctx.get_account();
         account.set_credentials(self.credentials_form.credentials.clone());
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Decrypting account...");
            });

            // Verify the credentials by just decrypting the account
            match account.decrypt(None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     // credentials are verified
                     gui.wallet_ui.delete_wallet_ui.verified_credentials = true;

                     // close the verify credentials ui
                     gui.wallet_ui.delete_wallet_ui.credentials_form.open = false;

                     // open the delete wallet ui
                     gui.wallet_ui.delete_wallet_ui.open = true;

                     // erase the credentials form
                     gui.wallet_ui.delete_wallet_ui.credentials_form.erase();
                     gui.loading_window.open = false;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to decrypt account", e.to_string());
                     gui.loading_window.open = false;
                  });
               }
            }
         });
      }

      self.credentials_form.open = open;
      if !self.credentials_form.open {
         self.credentials_form.erase();
      }
   }

   pub fn delete_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.verified_credentials {
         return;
      }
      let mut open = self.open;
      let mut clicked = false;

      let wallet = self.wallet_to_delete.clone();
      if wallet.is_none() {
         return;
      }
      let wallet = wallet.unwrap();

      let id = Id::new("delete_wallet_ui_delete_wallet");
      Window::new(RichText::new("Delete this wallet?").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(self.anchor.0, self.anchor.1)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               ui.label(RichText::new(wallet.name.clone()).size(theme.text_sizes.normal));
               ui.label(RichText::new(wallet.address_string()).size(theme.text_sizes.normal));

               let value = ctx.get_portfolio_value_all_chains(wallet.address);
               ui.label(
                  RichText::new(format!("Value ${}", value.formatted()))
                     .size(theme.text_sizes.normal),
               );

               if ui
                  .add(Button::new(
                     RichText::new("Yes").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  clicked = true;
               }
            });
         });

      if clicked {
         open = false;
         let mut new_account = ctx.get_account();

         RT.spawn_blocking(move || {
            new_account.remove_wallet(&wallet);

            // update the current wallet to the first available
            let wallets = new_account.wallets();
            if let Some(wallet_info) = wallets.first().map(|wallet| wallet.info.clone()) {
               new_account.set_current_wallet(wallet_info.clone());

               SHARED_GUI.write(|gui| {
                  gui.wallet_selection.wallet_select.wallet = wallet_info;
               });
            }

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting account...");
            });

            // Encrypt the account
            match ctx.encrypt_and_save_account(Some(new_account.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.wallet_ui.delete_wallet_ui.wallet_to_delete = None;
                     gui.wallet_ui.delete_wallet_ui.verified_credentials = false;
                     gui.open_msg_window("Wallet Deleted", "");
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.open_msg_window("Failed to encrypt wallet", e.to_string());
                  });
                  return;
               }
            };

            ctx.set_account(new_account);
         });
      }
      self.open = open;
   }
}
