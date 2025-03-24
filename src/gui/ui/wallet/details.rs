use crate::core::{Wallet, ZeusCtx};
use crate::gui::{
   self, SHARED_GUI,
   ui::{CredentialsForm, button, rich_text},
};
use eframe::egui::{Align2, Frame, Id, Order, Ui, Vec2, Window, vec2};
use egui_theme::Theme;

const VIEW_KEY_MSG: &str = "The key has been copied! In 30 seconds it will be cleared from the clipboard.";


pub struct KeyExporter {
   pub wallet: Option<Wallet>,
   /// When the key was copied to the clipboard
   pub key_copied_time: Option<std::time::Instant>,

   /// How much time before we force clear the clipboard
   clipboard_clear_delay: std::time::Duration
}

impl KeyExporter {
   pub fn new() -> Self {
      Self {
         wallet: None,
         key_copied_time: None,
         clipboard_clear_delay: std::time::Duration::from_secs(30),
      }
   }

   pub fn export_key(&mut self, ctx: egui::Context) {
      let key = self.wallet.take().unwrap().key_string();
      ctx.copy_text(key.to_string());
      self.key_copied_time = Some(std::time::Instant::now());
      self.wallet = None;
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
              let text = rich_text(format!("Clipboard will clear in {} seconds", remaining.as_secs())).size(theme.text_sizes.normal);
              ui.label(text);
          }
      }
}
}

pub struct ViewKeyUi {
   pub open: bool,
   pub credentials_form: CredentialsForm,
   pub verified_credentials: bool,
   pub exporter: KeyExporter,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl ViewKeyUi {
   pub fn new() -> Self {
      Self {
         open: false,
         credentials_form: CredentialsForm::new(),
         verified_credentials: false,
         exporter: KeyExporter::new(),
         size: (400.0, 300.0),
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      self.verify_credentials_ui(ctx, theme, ui);
   }

   pub fn verify_credentials_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.credentials_form.open;
      let mut clicked = false;

      let id = Id::new("verify_credentials_view_key_ui");
      Window::new(rich_text("Verify Credentials").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               self.credentials_form.show(theme, ui);

               let button = button(rich_text("Confrim").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let mut account = ctx.account();
         account.credentials = self.credentials_form.credentials.clone();
         std::thread::spawn(move || {
            let ok = gui::utils::verify_credentials(&mut account);

            // All good copy the key to the clipboard and show a msg
            if ok {
               let mut gui = SHARED_GUI.write().unwrap();
               let ctx = gui.egui_ctx.clone();
               gui.wallet_ui.view_key_ui.exporter.export_key(ctx);
               gui.wallet_ui.view_key_ui.reset();
               gui.open_msg_window("", VIEW_KEY_MSG);
            } else {
               let mut gui = SHARED_GUI.write().unwrap();
               gui.open_msg_window(
                  "Failed to verify credentials",
                  "Please try again".to_string(),
               );
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
   pub wallet_to_delete: Option<Wallet>,
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
         size: (400.0, 300.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      self.verify_credentials_ui(ctx.clone(), theme, ui);
      self.delete_wallet_ui(ctx, theme, ui);
   }

   pub fn verify_credentials_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.credentials_form.open;
      let mut clicked = false;

      let id = Id::new("verify_credentials_delete_wallet_ui");
      Window::new(rich_text("Verify Credentials").size(theme.text_sizes.large))
         .id(id)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               self.credentials_form.show(theme, ui);

               let button = button(rich_text("Confrim").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let mut account = ctx.account();
         account.credentials = self.credentials_form.credentials.clone();
         std::thread::spawn(move || {
            let ok = gui::utils::verify_credentials(&mut account);

            if ok {
               let mut gui = SHARED_GUI.write().unwrap();
               // credentials are verified
               gui.wallet_ui.delete_wallet_ui.verified_credentials = true;

               // close the verify credentials ui
               gui.wallet_ui.delete_wallet_ui.credentials_form.open = false;

               // open the delete wallet ui
               gui.wallet_ui.delete_wallet_ui.open = true;

               // erase the credentials form
               gui.wallet_ui.delete_wallet_ui.credentials_form.erase();
            } else {
               let mut gui = SHARED_GUI.write().unwrap();
               gui.open_msg_window(
                  "Failed to verify credentials",
                  "Please try again".to_string(),
               );
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

      let id = Id::new("delete_wallet_ui_delete_wallet");
      Window::new(rich_text("Delete this wallet?").size(theme.text_sizes.large))
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

               // should not happen
               if wallet.is_none() {
                  ui.label(rich_text("No wallet to delete"));
               } else {
                  let wallet = wallet.clone().unwrap();
                  ui.label(rich_text(wallet.name.clone()).size(theme.text_sizes.normal));
                  ui.label(rich_text(wallet.address_string()).size(theme.text_sizes.normal));

                  let value = ctx.get_portfolio_value_all_chains(wallet.key.borrow().address());
                  ui.label(rich_text(format!("Value ${}", value.formatted())).size(theme.text_sizes.normal));

                  if ui
                     .add(button(rich_text("Yes").size(theme.text_sizes.normal)))
                     .clicked()
                  {
                     clicked = true;
                  }
               }
            });
         });

      if clicked {
         open = false;

         let mut account = ctx.clone().account();
         let wallet = wallet.unwrap();
         std::thread::spawn(move || {
            account.remove_wallet(wallet);

            let dir = gui::utils::get_account_dir();
            let params = gui::utils::get_encrypted_info(&dir);
            gui::utils::open_loading("Encrypting account...".to_string());
            match account.encrypt_and_save(&dir, params.argon2_params) {
               Ok(_) => {
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.loading_window.open = false;
                  gui.wallet_ui.delete_wallet_ui.wallet_to_delete = None;
                  gui.wallet_ui.delete_wallet_ui.verified_credentials = false;
                  gui.open_msg_window("Wallet Deleted", "");
               }
               Err(e) => {
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.loading_window.open = false;
                  gui.open_msg_window("Failed to delete wallet", e.to_string());
                  return;
               }
            }

            ctx.write(|ctx| {
               ctx.account = account;
            });
         });
      }
      self.open = open;
   }
}
