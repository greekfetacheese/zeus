use crate::core::{ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use eframe::egui::{
   Align2, Button, FontId, Frame, Margin, Order, RichText, TextEdit, Ui, Vec2, Window, vec2,
};
use egui_theme::Theme;
use egui_widgets::SecureTextEdit;
use secure_types::SecureString;
use zeus_eth::types::SUPPORTED_CHAINS;

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
      Window::new(RichText::new("Import Wallet").size(theme.text_sizes.large))
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
               ui.label(RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal));
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

               ui.label(RichText::new(text).size(theme.text_sizes.normal));
               self.key_or_phrase.mut_scope(|imported_key| {
                  let text_edit = SecureTextEdit::singleline(imported_key)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size)
                     .password(true);
                  text_edit.show(ui);
               });

               // Import Button
               let button = Button::new(RichText::new("Import").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();
         let key_or_phrase = self.key_or_phrase.clone();
         let from_key = self.import_key_or_phrase == ImportWalletType::PrivateKey;

         RT.spawn_blocking(move || {
            let mut new_vault = ctx.get_vault();

            // Import the wallet
            let new_wallet_address =
               match new_vault.new_wallet_from_key_or_phrase(name, from_key, key_or_phrase) {
                  Ok(address) => address,
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.open_msg_window("Failed to import wallet", e.to_string());
                     });
                     return;
                  }
               };

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting account...");
            });

            // Encrypt the account
            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.open_msg_window("Wallet imported successfully", "");
                     gui.wallet_ui.add_wallet_ui.import_wallet.key_or_phrase.erase();
                     gui.wallet_ui.add_wallet_ui.import_wallet.wallet_name.clear();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.open_msg_window("Failed to encrypt account", e.to_string());
                  });
                  return;
               }
            };

            ctx.set_vault(new_vault);

            // Fetch the balance for the new wallet across all chains and add it to the portfolio db
            RT.spawn(async move {
               let manager = ctx.balance_manager();
               for chain in SUPPORTED_CHAINS {
                  match manager.update_eth_balance(ctx.clone(), chain, new_wallet_address).await {
                     Ok(_) => {}
                     Err(e) => {
                        tracing::error!("Failed to update ETH balance: {}", e);
                     }
                  }

                  // Portfolio is created and saved here
                  ctx.calculate_portfolio_value(chain, new_wallet_address);
               }
               ctx.save_balance_manager();
               ctx.save_portfolio_db();
            });
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
   pub derive_child_wallet: bool,
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
         derive_child_wallet: false,
         wallet_name: String::new(),
         size,
         anchor: (align, offset),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(ctx.clone(), theme, ui);
      self.import_wallet.show(ctx.clone(), theme, ui);
      self.derive_child_wallet_ui(ctx.clone(), theme, ui);
      // self.generate_wallet_ui(ctx.clone(), theme, ui);
   }

   pub fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.main_ui;
      let mut clicked1 = false;
      let mut clicked2 = false;
      let mut clicked3 = false;

      Window::new(RichText::new("Add a new Wallet").size(theme.text_sizes.large))
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

               // Derive a new child wallet from the master wallet
               let button1 = Button::new(
                  RichText::new("Create a new Child Wallet").size(theme.text_sizes.large),
               )
               .corner_radius(5)
               .min_size(size);

               let enabled = !ctx.wallet_discovery_in_progress();
               if ui.add_enabled(enabled, button1).clicked() {
                  clicked1 = true;
               }

               // From private key
               let button2 = Button::new(
                  RichText::new("Import from a Private Key").size(theme.text_sizes.large),
               )
               .corner_radius(5)
               .min_size(size);
               if ui.add(button2).clicked() {
                  clicked2 = true;
               }

               // From seed phrase
               let button3 = Button::new(
                  RichText::new("Import from a  Seed Phrase").size(theme.text_sizes.large),
               )
               .corner_radius(5)
               .min_size(size);
               if ui.add(button3).clicked() {
                  clicked3 = true;
               }

               // Generate new wallet
               /*
               let button2 =
                  Button::new(RichText::new("Generate New Wallet").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);
               if ui.add(button2).clicked() {
                  clicked3 = true;
               }
               */
            });
         });

      if clicked1 {
         self.derive_child_wallet = true;
         open = false;
      }

      if clicked2 {
         self.import_wallet.open = true;
         self.import_wallet.import_key_or_phrase = ImportWalletType::PrivateKey;
         open = false;
      }

      if clicked3 {
         self.import_wallet.open = true;
         self.import_wallet.import_key_or_phrase = ImportWalletType::MnemonicPhrase;
         open = false;
      }

      /*
      if clicked3 {
         self.generate_wallet = true;
         open = false;
      }
      */

      self.main_ui = open;
   }

   fn derive_child_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.derive_child_wallet;
      let mut clicked = false;

      Window::new(RichText::new("Derive Child Wallet").size(theme.text_sizes.large))
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
               ui.label(RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal));
               ui.add(
                  TextEdit::singleline(&mut self.wallet_name)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               let button = Button::new(RichText::new("Create").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let wallet_name = self.wallet_name.clone();

         RT.spawn_blocking(move || {
            let mut new_vault = ctx.get_vault();

            match new_vault.derive_child_wallet(wallet_name) {
               Ok(_) => {}
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to create wallet", e.to_string());
                  });
                  return;
               }
            }

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting vault...");
            });

            // Encrypt the vault
            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                     gui.open_msg_window("Wallet generated successfully", "");
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.open_msg_window("Failed to encrypt vault", e.to_string());
                  });
                  return;
               }
            };

            ctx.set_vault(new_vault);
         });
      }
      self.derive_child_wallet = open;
   }

   pub fn _generate_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.generate_wallet;
      let mut clicked = false;
      Window::new(RichText::new("Generate Wallet").size(theme.text_sizes.large))
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
               ui.label(RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal));
               ui.add(
                  TextEdit::singleline(&mut self.wallet_name)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               // Generate Button
               let button = Button::new(RichText::new("Generate").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();

         RT.spawn_blocking(move || {
            let mut new_vault = ctx.get_vault();

            match new_vault.new_wallet_rng(name) {
               Ok(_) => {}
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to generate wallet", e.to_string());
                  });
                  return;
               }
            }

            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Encrypting vault...");
            });

            // Encrypt the vault
            match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                     gui.open_msg_window("Wallet generated successfully", "");
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.open = false;
                     gui.open_msg_window("Failed to encrypt vault", e.to_string());
                  });
                  return;
               }
            };

            ctx.set_vault(new_vault);
         });
      }
      self.generate_wallet = open;
   }
}
