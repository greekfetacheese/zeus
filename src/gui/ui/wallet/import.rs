//! UI that allows the user to import a wallet from a private key or a seed phrase

use crate::gui::ui::common;
use crate::gui::{GUI, SHARED_GUI};
use crate::utils::malloc_trim;
use eframe::egui::{FontId, Id, Key, Margin, Order, Response, RichText, Ui, vec2};
use egui_elements::{Button, Modal, SecureInputField, SecureTextEdit, Theme};
use secure_types::SecureString;
use zeus_eth::types::SUPPORTED_CHAINS;

#[derive(PartialEq, Eq)]
pub enum ImportWalletType {
   PrivateKey,
   MnemonicPhrase,
}

pub struct ImportWallet {
   open: bool,
   import_key_or_phrase: ImportWalletType,
   input_field: SecureInputField,
   wallet_name: String,
   /// Whether the name field still has to take the focus the dialog's Close button is given when
   /// it opens.
   focus_name_field: bool,
   size: (f32, f32),
}

impl ImportWallet {
   pub fn new() -> Self {
      Self {
         open: false,
         import_key_or_phrase: ImportWalletType::PrivateKey,
         input_field: SecureInputField::new("Import Wallet", true, true),
         wallet_name: String::new(),
         focus_name_field: false,
         size: (450.0, 250.0),
      }
   }

   /// Draw or drop the field controls that only a sighted user can use.
   pub fn set_field_controls(&mut self, shown: bool) {
      common::field_controls::secure_field(&mut self.input_field, shown);
   }

   pub fn erase(&mut self) {
      self.input_field.erase();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, import_type: ImportWalletType) {
      self.open = true;
      self.import_key_or_phrase = import_type;
      self.focus_name_field = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let was_open = self.open;
      let mut is_open = self.open;
      let mut clicked = false;
      let mut name_field: Option<Response> = None;

      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new("Import Wallet").size(theme.typography.heading);
      let id = Id::unique("import_wallet_window");

      Modal::new(id, &mut is_open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;
               let size = vec2(ui.available_width() * 0.5, 40.0);
               ui.add_space(20.0);

               // Wallet Name
               let drawn =
                  ui.label(RichText::new("Wallet Name (Optional)").size(theme.typography.large));

               // The field carries this instruction as its name, so the drawn label is not a
               // second stop.
               ui.ctx().accesskit_node_builder(drawn.id, |node| node.set_hidden());

               // The parentheses of the drawn label are read aloud, so the field is named without
               // them.
               let field = ui.add(
                  SecureTextEdit::singleline(&mut self.wallet_name)
                     .accessible_name("Wallet name, optional")
                     .visuals(text_edit_visuals)
                     .font(FontId::proportional(theme.typography.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               // A single-line edit gives its focus up on Enter instead of taking the key as a
               // character, which leaves Enter free to press Import from here.
               if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                  clicked = true;
               }

               name_field = Some(field);

               // Private Key or Phrase
               let text = if self.import_key_or_phrase == ImportWalletType::PrivateKey {
                  "Private Key"
               } else {
                  "Seed Phrase"
               };

               // Input Field
               let size = vec2(ui.available_width() * 0.7, 40.0);
               self.input_field.set_min_size(size);
               self.input_field.set_id(text);

               let secret_field = ui
                  .scope(|ui| {
                     ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs);
                     self.input_field.show(ui)
                  })
                  .inner;

               if let Some(field) = &secret_field {
                  // The count is taken from the plaintext `unlock_str` hands back rather than
                  // kept: the field is the only owner of the secret, and a copy that outlived this
                  // pass would be one more place it lives.
                  let description = self
                     .input_field
                     .text()
                     .unlock_str(|typed| secret_count(&self.import_key_or_phrase, typed));

                  ui.ctx().accesskit_node_builder(field.response.id, |node| {
                     node.set_description(description.as_str());
                  });

                  // The same free key as the name field: Enter presses Import.
                  if field.response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
                  {
                     clicked = true;
                  }
               }

               // Import Button
               let text = RichText::new("Import").size(theme.typography.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      // The dialog spends its first passes sizing, where its body is invisible and cannot hold the
      // focus, so the name field asks for it on the first pass that is drawn. It asks after the
      // dialog has drawn, so the dialog's own opening grab — which moves focus to its Close button —
      // cannot take it back.
      if let Some(field) = name_field.filter(|field| field.visible() && self.focus_name_field) {
         ui.ctx().memory_mut(|memory| memory.request_focus(field.id));
         self.focus_name_field = false;
      }

      if clicked {
         let name = self.wallet_name.clone();
         let key_or_phrase = self.input_field.text();
         let from_key = self.import_key_or_phrase == ImportWalletType::PrivateKey;
         on_import_wallet(name, from_key, key_or_phrase);
      }

      if !is_open {
         self.close();
      }

      if was_open && !self.open {
         self.input_field.erase();
         on_reopen_add_wallet();
      }
   }
}

#[cfg(test)]
impl ImportWallet {
   /// Put a secret into the form the way typing would, for a tree test that cannot reach the field.
   pub(crate) fn set_secret_for_test(&mut self, text: &str) {
      self.input_field.set_text(SecureString::from(text));
   }
}

/// How much the secret field holds: a phrase is counted in words, a key in characters.
fn secret_count(kind: &ImportWalletType, text: &str) -> String {
   match kind {
      ImportWalletType::MnemonicPhrase => count_with_noun(text.split_whitespace().count(), "word"),
      ImportWalletType::PrivateKey => count_with_noun(text.chars().count(), "character"),
   }
}

/// A count with the noun it counts, singular when there is one of it: "1 word", "12 words".
fn count_with_noun(count: usize, noun: &str) -> String {
   match count {
      1 => format!("1 {noun}"),
      count => format!("{count} {noun}s"),
   }
}

/// Say a failed import and keep what was typed: the message window carries the crate's reason, and
/// the field still holds the phrase or key, so one wrong word can be corrected instead of the whole
/// secret being typed again.
fn import_failed(gui: &mut GUI, message: String) {
   gui.open_msg_window(message);
   gui.request_repaint();
}

fn on_import_wallet(name: String, from_key: bool, key_or_phrase: SecureString) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let mut new_vault = ctx.get_vault();

      // Import the wallet
      let new_wallet_address =
         match new_vault.new_wallet_from_key_or_phrase(name, from_key, key_or_phrase) {
            Ok(address) => address,
            Err(e) => {
               let message = format!("Failed to import wallet: {}", e);
               SHARED_GUI.write(|gui| import_failed(gui, message));
               return;
            }
         };

      SHARED_GUI.write(|gui| {
         gui.loading_window.open("Encrypting account...");
         gui.request_repaint();
      });

      // Encrypt the account
      match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               gui.loading_window.reset();
               gui.open_msg_window("Wallet imported successfully");
               gui.wallet_ui.add_wallet_ui.import_wallet.input_field.erase();
               gui.wallet_ui.add_wallet_ui.import_wallet.wallet_name.clear();
               gui.request_repaint();
            });
         }
         Err(e) => {
            let message = format!("Failed to encrypt account: {}", e);
            SHARED_GUI.write(|gui| {
               gui.loading_window.reset();
               import_failed(gui, message);
            });
            return;
         }
      };

      ctx.set_vault(new_vault);
      ctx.build_wallet_info_cache();

      let ctx_clone = ctx.clone();
      crate::runtime::rt().spawn(async move {
         for chain in SUPPORTED_CHAINS {
            if let Err(e) = ctx_clone.register_railgun_signers(chain, false).await {
               tracing::error!("Error registering Railgun signers: {:?}", e);
            }

            if let Err(e) = ctx_clone.sync_railgun(chain, false).await {
               tracing::error!("Error syncing Railgun: {:?}", e);
            }

            ctx_clone.update_private_data(chain, new_wallet_address).await;
         }

         // Free memory
         malloc_trim();
      });

      // Recalculate the wallets
      SHARED_GUI.write(|gui| {
         gui.wallet_ui.calc_wallet_value();
      });

      // Fetch the balance for the new wallet across all chains and add it to the portfolio db
      crate::runtime::rt().spawn(async move {
         let manager = ctx.balance_manager();
         for chain in SUPPORTED_CHAINS {
            if ctx.is_chain_disabled(chain) {
               continue;
            }

            match manager
               .update_eth_balance(
                  ctx.clone(),
                  chain,
                  vec![new_wallet_address],
                  false,
               )
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Failed to update ETH balance: {}", e);
               }
            }

            // Portfolio is created and saved here
            ctx.update_public_data(chain, new_wallet_address);
         }
      });
   });
}

fn on_reopen_add_wallet() {
   crate::runtime::rt().spawn_blocking(move || {
      SHARED_GUI.write(|gui| {
         gui.wallet_ui.add_wallet_ui.open();
      });
   });
}

#[cfg(test)]
mod tests {
   use super::*;

   /// A failed import keeps what was typed: one wrong word in a phrase has to be correctable, not a
   /// reason to type the whole secret again.
   #[test]
   fn a_failed_import_keeps_what_was_typed() {
      let mut gui = GUI::default();
      let phrase = "legal winner thank year wave sausage worth useful legal winner thank yellow";

      gui.wallet_ui
         .add_wallet_ui
         .import_wallet
         .input_field
         .set_text(SecureString::from(phrase));

      import_failed(
         &mut gui,
         "Failed to import wallet: invalid phrase".to_string(),
      );

      assert!(
         gui.msg_window.is_open(),
         "the failure must still be said"
      );

      assert_eq!(
         gui.wallet_ui
            .add_wallet_ui
            .import_wallet
            .input_field
            .text()
            .unlock_str(|text| text.to_string()),
         phrase,
         "the phrase must still be in the field"
      );
   }

   /// The count names words for a phrase and characters for a key, singular when there is one.
   #[test]
   fn the_secret_count_names_words_or_characters() {
      assert_eq!(
         secret_count(
            &ImportWalletType::MnemonicPhrase,
            "legal winner thank"
         ),
         "3 words"
      );
      assert_eq!(
         secret_count(&ImportWalletType::MnemonicPhrase, "legal"),
         "1 word"
      );
      assert_eq!(
         secret_count(&ImportWalletType::MnemonicPhrase, ""),
         "0 words"
      );
      assert_eq!(
         secret_count(&ImportWalletType::PrivateKey, "abcd1234"),
         "8 characters"
      );
      assert_eq!(
         secret_count(&ImportWalletType::PrivateKey, "a"),
         "1 character"
      );
   }
}
