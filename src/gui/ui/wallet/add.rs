use crate::core::ZeusCtx;
use crate::gui::{
   self, SHARED_GUI,
   ui::{button, rich_text},
};
use eframe::egui::{Align2, FontId, Frame, Margin, Order, TextEdit, Ui, Vec2, Window, vec2};
use egui_theme::{Theme, utils::*};
use secure_types::SecureString;

#[derive(PartialEq, Eq)]
pub enum ImportWalletType {
   PrivateKey,
   MnemonicPhrase,
}

pub struct ImportWallet {
   pub open: bool,
   pub import_key_or_phrase: ImportWalletType,
   pub key_or_phrase: SecureString,
   pub wallet_name: String,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl ImportWallet {
   pub fn new() -> Self {
      Self {
         open: false,
         import_key_or_phrase: ImportWalletType::PrivateKey,
         key_or_phrase: SecureString::from(""),
         wallet_name: String::new(),
         size: (450.0, 250.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;
      let mut clicked = false;
      Window::new(rich_text("Import Wallet").size(theme.text_sizes.large))
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
               let size = vec2(ui.available_width() * 0.5, 20.0);
               ui.add_space(20.0);

               // Wallet Name
               ui.label(rich_text("Wallet Name (Optional)").size(theme.text_sizes.normal));
               ui.add(
                  TextEdit::singleline(&mut self.wallet_name)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               // Private Key or Phrase
               let text = if self.import_key_or_phrase == ImportWalletType::PrivateKey {
                  "Private Key"
               } else {
                  "Seed Phrase"
               };

               ui.label(rich_text(text).size(theme.text_sizes.normal));
               // ! Key still remains in the buffer
               self.key_or_phrase.secure_mut(|imported_key| {
                  let text_edit = TextEdit::singleline(imported_key)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size)
                     .password(true);
                  let mut output = text_edit.show(ui);
                  output.state.clear_undoer();
               });

               // Import Button
               let button = button(rich_text("Import").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();
         let key_or_phrase = self.key_or_phrase.clone();
         let from_key = self.import_key_or_phrase == ImportWalletType::PrivateKey;

         std::thread::spawn(move || {
            let mut account = ctx.account();
            gui::utils::new_wallet_from_key_or_phrase(&mut account, name, from_key, key_or_phrase);
            let dir = gui::utils::get_account_dir();
            let info = gui::utils::get_encrypted_info(&dir);
            gui::utils::open_loading("Encrypting account...".to_string());

            match account.encrypt_and_save(&dir, info.argon2_params) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                  gui.wallet_ui
                     .add_wallet_ui
                     .import_wallet
                     .key_or_phrase
                     .erase();
                  gui.wallet_ui
                     .add_wallet_ui
                     .import_wallet
                     .wallet_name
                     .clear();
                  gui.loading_window.open = false;
                  gui.open_msg_window("Wallet imported successfully", "");
                  });
                  ctx.write(|ctx| {
                     ctx.account = account;
                  })
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                  gui.loading_window.open = false;
                  gui.open_msg_window("Failed to save account", e.to_string());
                  });
                  return;
               }
            }
         });
      }

      self.open = open;
      if !self.open {
         self.key_or_phrase.erase();
      }
   }
}

pub struct AddWalletUi {
   pub open: bool,
   pub main_ui: bool,
   pub import_wallet: ImportWallet,
   pub generate_wallet: bool,
   pub wallet_name: String,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl AddWalletUi {
   pub fn new(size: (f32, f32), offset: Vec2, align: Align2) -> Self {
      Self {
         open: false,
         main_ui: true,
         import_wallet: ImportWallet::new(),
         generate_wallet: false,
         wallet_name: String::new(),
         size,
         anchor: (align, offset),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(theme, ui);
      self.import_wallet.show(ctx.clone(), theme, ui);
      self.generate_wallet_ui(ctx.clone(), theme, ui);
   }

   pub fn main_ui(&mut self, theme: &Theme, ui: &mut Ui) {
      let mut open = self.main_ui;
      let mut clicked1 = false;
      let mut clicked2 = false;
      let mut clicked3 = false;

      Window::new(rich_text("Add a new Wallet").size(theme.text_sizes.large))
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
               ui.add_space(30.0);
               let size = vec2(ui.available_width() * 0.9, 50.0);
               widget_visuals(ui, theme.get_button_visuals(theme.colors.bg_color));

               // From private key
               let button1 = button(rich_text("From Private Key").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(button1).clicked() {
                  clicked1 = true;
               }

               // From seed phrase
               let button2 = button(rich_text("From Seed Phrase").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(button2).clicked() {
                  clicked2 = true;
               }

               // Generate new wallet
               let button2 = button(rich_text("Generate New Wallet").size(theme.text_sizes.large))
                  .corner_radius(5)
                  .min_size(size);
               if ui.add(button2).clicked() {
                  clicked3 = true;
               }
            });
         });

      if clicked1 {
         self.import_wallet.open = true;
         self.import_wallet.import_key_or_phrase = ImportWalletType::PrivateKey;
         open = false;
      }

      if clicked2 {
         self.import_wallet.open = true;
         self.import_wallet.import_key_or_phrase = ImportWalletType::MnemonicPhrase;
         open = false;
      }

      if clicked3 {
         self.generate_wallet = true;
         open = false;
      }

      self.main_ui = open;
   }

   pub fn generate_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.generate_wallet;
      let mut clicked = false;
      Window::new(rich_text("Generate Wallet").size(theme.text_sizes.large))
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
               let size = vec2(ui.available_width() * 0.5, 20.0);
               ui.add_space(20.0);

               // Wallet Name
               ui.label(rich_text("Wallet Name (Optional)").size(theme.text_sizes.normal));
               ui.add(
                  TextEdit::singleline(&mut self.wallet_name)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               // Generate Button
               let button = button(rich_text("Generate").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();

         std::thread::spawn(move || {
            let mut account = ctx.account();
            gui::utils::new_wallet_rng(&mut account, name);
            let dir = gui::utils::get_account_dir();
            let info = gui::utils::get_encrypted_info(&dir);
            gui::utils::open_loading("Encrypting account...".to_string());

            match account.encrypt_and_save(&dir, info.argon2_params) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                  gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                  gui.loading_window.open = false;
                  gui.open_msg_window("Wallet generated successfully", "");
                  });
                  ctx.write(|ctx| {
                     ctx.account = account;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                  gui.loading_window.open = false;
                  gui.open_msg_window("Failed to save account", e.to_string());
                  });
                  return;
               }
            }
         });
      }
      self.generate_wallet = open;
   }
}
