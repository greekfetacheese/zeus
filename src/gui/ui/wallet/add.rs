use crate::assets::Icons;
use crate::core::{BIP32_HARDEN, DiscoveredWallets, Portfolio, SecureHDWallet, ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use eframe::egui::{
   Align, Align2, Button, FontId, Frame, Id, Layout, Margin, Order, RichText, ScrollArea, Spinner,
   TextEdit, Ui, Vec2, Window, vec2,
};
use egui_theme::Theme;
use egui_widgets::SecureTextEdit;
use secure_types::SecureString;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, NativeCurrency},
   types::SUPPORTED_CHAINS,
   utils::{NumericValue, batch, truncate_address},
};

use std::sync::Arc;
use tokio::{sync::Semaphore, task::JoinHandle};

#[derive(PartialEq, Eq)]
enum ImportWalletType {
   PrivateKey,
   MnemonicPhrase,
}

pub struct ImportWallet {
   open: bool,
   import_key_or_phrase: ImportWalletType,
   key_or_phrase: SecureString,
   wallet_name: String,
   size: (f32, f32),
}

impl ImportWallet {
   pub fn new() -> Self {
      Self {
         open: false,
         import_key_or_phrase: ImportWalletType::PrivateKey,
         key_or_phrase: SecureString::from(""),
         wallet_name: String::new(),
         size: (450.0, 250.0),
      }
   }

   fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let was_open = self.open;
      let mut is_open = self.open;
      let mut clicked = false;

      Window::new(RichText::new("Import Wallet").size(theme.text_sizes.large))
         .open(&mut is_open)
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
               self.key_or_phrase.unlock_mut(|imported_key| {
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
                     gui.loading_window.reset();
                     gui.open_msg_window("Wallet imported successfully", "");
                     gui.wallet_ui.add_wallet_ui.import_wallet.key_or_phrase.erase();
                     gui.wallet_ui.add_wallet_ui.import_wallet.wallet_name.clear();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
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
                  match manager.update_eth_balance(ctx.clone(), chain, vec![new_wallet_address]).await {
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

      self.open = is_open;
      if was_open && !self.open {
         self.key_or_phrase.erase();
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.main_ui = true;
            });
         });
      }
   }
}

pub struct AddWalletUi {
   open: bool,
   main_ui: bool,
   import_wallet: ImportWallet,
   discover_child_wallets_ui: DiscoverChildWallets,
   #[allow(dead_code)]
   generate_wallet: bool,
   #[allow(dead_code)]
   derive_child_wallet: bool,
   #[allow(dead_code)]
   wallet_name: String,
   size: (f32, f32),
}

impl AddWalletUi {
   pub fn new() -> Self {
      Self {
         open: false,
         main_ui: true,
         import_wallet: ImportWallet::new(),
         discover_child_wallets_ui: DiscoverChildWallets::new(),
         generate_wallet: false,
         derive_child_wallet: false,
         wallet_name: String::new(),
         size: (450.0, 250.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn is_main_ui_open(&self) -> bool {
      self.main_ui
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn open_main_ui(&mut self) {
      self.main_ui = true;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(ctx.clone(), theme, ui);
      self.import_wallet.show(ctx.clone(), theme, ui);
      self.discover_child_wallets_ui.show(ctx.clone(), theme, icons, ui);
      // self.derive_child_wallet_ui(ctx.clone(), theme, ui);
      // self.generate_wallet_ui(ctx.clone(), theme, ui);
   }

   fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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
               let button =
                  Button::new(RichText::new("Add a new Child Wallet").size(theme.text_sizes.large))
                     .corner_radius(5)
                     .min_size(size);

               if ui.add(button).clicked() {
                  clicked1 = true;
               }

               // From private key
               let button = Button::new(
                  RichText::new("Import from a Private Key").size(theme.text_sizes.large),
               )
               .corner_radius(5)
               .min_size(size);
               if ui.add(button).clicked() {
                  clicked2 = true;
               }

               // From seed phrase
               let button = Button::new(
                  RichText::new("Import from a  Seed Phrase").size(theme.text_sizes.large),
               )
               .corner_radius(5)
               .min_size(size);
               if ui.add(button).clicked() {
                  clicked3 = true;
               }
            });
         });

      if clicked1 {
         open = false;

         let vault = ctx.get_vault();

         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.loading = true;
            });

            let wallets = match DiscoveredWallets::load_from_file() {
               Ok(wallets) => wallets,
               Err(e) => {
                  tracing::error!("Error loading discovered wallets: {:?}", e);
                  DiscoveredWallets::new()
               }
            };

            SHARED_GUI.write(|gui| {
               gui.wallet_ui
                  .add_wallet_ui
                  .discover_child_wallets_ui
                  .set_hd_wallet(vault.get_hd_wallet());
               gui.wallet_ui
                  .add_wallet_ui
                  .discover_child_wallets_ui
                  .set_discovery_wallet(vault.get_hd_wallet());
               gui.wallet_ui
                  .add_wallet_ui
                  .discover_child_wallets_ui
                  .set_discovered_wallets(wallets);
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.open = true;
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.loading = false;
            });
         });
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

      self.main_ui = open;
   }

   fn _derive_child_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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
                     gui.loading_window.reset();
                     gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                     gui.open_msg_window("Wallet generated successfully", "");
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
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

   fn _generate_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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
                     gui.loading_window.reset();
                     gui.wallet_ui.add_wallet_ui.wallet_name.clear();
                     gui.open_msg_window("Wallet generated successfully", "");
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
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

/// Still WIP
/// 
/// If the user generates a lot of wallets the Ui can lag
pub struct DiscoverChildWallets {
   open: bool,
   hd_wallet: SecureHDWallet,
   /// A clone of the HD Wallet just to discover wallets
   discovery_wallet: SecureHDWallet,
   discovered_wallets: DiscoveredWallets,
   syncing: bool,
   loading: bool,
   add_wallet_window: bool,
   index_to_add: u32,
   wallet_name: String,
   size: (f32, f32),
}

impl DiscoverChildWallets {
   pub fn new() -> Self {
      Self {
         open: false,
         hd_wallet: SecureHDWallet::random(),
         discovery_wallet: SecureHDWallet::random(),
         discovered_wallets: DiscoveredWallets::new(),
         syncing: false,
         loading: false,
         add_wallet_window: false,
         index_to_add: 0,
         wallet_name: String::new(),
         size: (600.0, 450.0),
      }
   }

   fn set_discovered_wallets(&mut self, discovered_wallets: DiscoveredWallets) {
      self.discovered_wallets = discovered_wallets;
   }

   fn set_hd_wallet(&mut self, hd_wallet: SecureHDWallet) {
      self.hd_wallet = hd_wallet;
   }

   fn set_discovery_wallet(&mut self, discovery_wallet: SecureHDWallet) {
      self.discovery_wallet = discovery_wallet;
   }

   fn reset(&mut self) {
      *self = Self::new();
   }

   fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.add_wallet(ctx.clone(), theme, ui);

      let was_open = self.open;
      let mut is_open = self.open;

      let title = RichText::new("Discover Wallets").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut is_open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = Vec2::new(20.0, 15.0);
            ui.spacing_mut().button_padding = Vec2::new(10.0, 8.0);
            ui.vertical_centered(|ui| {
               ui.add_space(15.0);

               if self.loading {
                  ui.label(RichText::new("Loading...").size(theme.text_sizes.normal));
                  ui.add(Spinner::new().size(15.0));
                  return;
               }

               let len = self.discovered_wallets.wallets.len();
               let text = format!("Showing {} wallets", len);
               ui.label(RichText::new(text).size(theme.text_sizes.normal));

               let batch_size = self.discovered_wallets.batch_size;

               let text = format!("Generate next {} wallets", batch_size);
               let text = RichText::new(text).size(theme.text_sizes.normal);
               if ui.add(Button::new(text)).clicked() {
                  self.syncing = true;
                  let ctx_clone = ctx.clone();
                  let mut addresses = Vec::new();
                  let concurrency = self.discovered_wallets.concurrency;
                  let discovery_wallet = self.discovery_wallet.clone();
                  let mut discovered_wallets = self.discovered_wallets.clone();

                  RT.spawn(async move {
                     for _ in 0..batch_size {
                        let mut index = discovered_wallets.index;
                        discovered_wallets.index += 1;

                        if index < BIP32_HARDEN {
                           index += BIP32_HARDEN;
                        }

                        if let Ok(wallet) = discovery_wallet.derive_child_at("".into(), index) {
                           discovered_wallets.add_wallet(
                              wallet.address(),
                              wallet.derivation_path(),
                              wallet.index(),
                           );

                           // Do not fetch the balance for already existing wallets
                           if ctx_clone.wallet_exists(wallet.address()) {
                              continue;
                           }

                           addresses.push(wallet.address());
                        }
                     }

                     SHARED_GUI.write(|gui| {
                        gui.wallet_ui
                           .add_wallet_ui
                           .discover_child_wallets_ui
                           .set_discovery_wallet(discovery_wallet);
                        gui.wallet_ui
                           .add_wallet_ui
                           .discover_child_wallets_ui
                           .set_discovered_wallets(discovered_wallets);
                     });

                     match sync_wallets_balance(ctx_clone, addresses, concurrency).await {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.syncing = false;
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.syncing = false;
                           });
                           tracing::error!("Error syncing wallets: {:?}", e);
                        }
                     }
                  });
               }

               if self.syncing {
                  ui.add(Spinner::new().size(15.0));
               }

               let column_widths = [
                  ui.available_width() * 0.20, // Derivation Path
                  ui.available_width() * 0.20, // Address
                  ui.available_width() * 0.20, // Value
                  ui.available_width() * 0.05, // Import Button
               ];

               Frame::new().inner_margin(20).show(ui, |ui| {
                  // Header
                  ui.horizontal(|ui| {
                     ui.scope(|ui| {
                        ui.set_width(column_widths[0]);
                        let text = RichText::new("Derivation Path").size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                     ui.scope(|ui| {
                        ui.set_width(column_widths[1]);
                        let text = RichText::new("Address").size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                     ui.scope(|ui| {
                        ui.set_width(column_widths[2]);
                        let text = RichText::new("Value").size(theme.text_sizes.normal);
                        ui.label(text);
                     });
                     // Just occupy space
                     ui.scope(|ui| {
                        ui.set_width(column_widths[3]);
                     });
                  });

                  ScrollArea::vertical()
                     .id_salt("children_wallets_in_discovery")
                     .auto_shrink([false; 2])
                     .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        self.show_wallets(
                           ctx.clone(),
                           theme,
                           icons.clone(),
                           &column_widths,
                           ui,
                        );
                     });
               });
            });
         });

      self.open = is_open;
      if was_open && !self.open {
         let wallets = self.discovered_wallets.clone();
         RT.spawn_blocking(move || {
            match wallets.save() {
               Ok(_) => {
                  tracing::info!("Discovered wallets saved");
               }
               Err(e) => {
                  tracing::error!("Error saving discovered wallets: {:?}", e);
               }
            }

            SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.main_ui = true;
            });
         });
         self.reset();
      }
   }

   fn show_wallets(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      column_widths: &[f32],
      ui: &mut Ui,
   ) {
      for child in self.discovered_wallets.wallets.iter() {
         // If child already exists it will displayed as disabled in the Ui
         let exists = self.hd_wallet.contains_child(child.address);

         let mut chains = Vec::new();
         let mut total_value = 0.0;
         let ctx = ctx.clone();
         let current_chain = ctx.chain();

         // get the chains which the wallet has balance in
         for chain in SUPPORTED_CHAINS {
            let key = (chain, child.address);
            if let Some(balance) = self.discovered_wallets.balances.get(&key) {
               if !balance.is_zero() {
                  chains.push(chain);

                  let native = Currency::from(NativeCurrency::from(chain));
                  let balance = NumericValue::currency_balance(*balance, native.decimals());
                  let value = ctx.get_currency_value_for_amount(balance.f64(), &native);
                  total_value += value.f64();
               }
            }
         }

         ui.horizontal(|ui| {
            ui.add_enabled_ui(exists == false, |ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  // Derivation Path
                  ui.scope(|ui| {
                     ui.set_width(column_widths[0]);
                     let text = child.path.derivation_string();
                     let rich_text = RichText::new(text).size(theme.text_sizes.small);
                     ui.label(rich_text);
                  });

                  // Address
                  ui.scope(|ui| {
                     ui.set_width(column_widths[1]);
                     let address = child.address.to_string();
                     let text = truncate_address(&address, 20);
                     let explorer = current_chain.block_explorer();
                     let link = format!("{}/address/{}", explorer, address);
                     ui.hyperlink_to(
                        RichText::new(text)
                           .size(theme.text_sizes.small)
                           .color(theme.colors.hyperlink_color),
                        link,
                     );
                  });
               });

               ui.add_space(10.0);

               // Value
               ui.vertical(|ui| {
                  ui.set_width(column_widths[2]);
                  let value = if !exists {
                     NumericValue::from_f64(total_value)
                  } else {
                     ctx.get_portfolio_value_all_chains(child.address)
                  };

                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = 1.0;
                     for chain in SUPPORTED_CHAINS {
                        let icon = icons.chain_icon_x16(chain);
                        ui.add(icon);
                     }
                  });

                  ui.label(
                     RichText::new(format!("${}", value.format_abbreviated()))
                        .color(theme.colors.text_secondary)
                        .size(theme.text_sizes.small),
                  );
               });

               ui.scope(|ui| {
                  ui.set_width(column_widths[3]);
                  let text = RichText::new("Add").size(theme.text_sizes.small);
                  if ui.button(text).clicked() {
                     self.add_wallet_window = true;
                     self.index_to_add = child.index;
                  }
               });
            });
         });
      }
   }

   fn add_wallet(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.add_wallet_window;

      let title = RichText::new("Add Wallet").size(theme.text_sizes.heading);
      Window::new(title)
         .id(Id::new("discover_wallets_add_wallet_window"))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 20.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               let text = RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal);
               ui.label(text);

               TextEdit::singleline(&mut self.wallet_name)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let text = RichText::new("Add Wallet").size(theme.text_sizes.large);

               if ui.button(text).clicked() {
                  let index = self.index_to_add;
                  let name = self.wallet_name.clone();
                  let balances = self.discovered_wallets.balances.clone();

                  RT.spawn_blocking(move || {
                     let res =
                        ctx.write_vault(|vault| vault.derive_child_wallet_at_mut(name, index));

                     let address = match res {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Failed to add wallet", e.to_string());
                           });
                           return;
                        }
                     };

                     for chain in SUPPORTED_CHAINS {
                        let eth = NativeCurrency::from(chain);
                        let balance = balances.get(&(chain, address)).cloned().unwrap_or_default();
                        let balance_manager = ctx.balance_manager();
                        balance_manager.insert_eth_balance(chain, address, balance, &eth);

                        ctx.write(|ctx| {
                           ctx.portfolio_db.insert_portfolio(
                              chain,
                              address,
                              Portfolio::new(address, chain),
                           );
                        });
                     }

                     // Save the vault and update the hd wallet in the Ui
                     // If this op fails we revert the changes
                     match ctx.encrypt_and_save_vault(None, None) {
                        Ok(_) => {
                           let hd_wallet = ctx.get_vault().get_hd_wallet();
                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .add_wallet_window = false;
                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.wallet_name =
                                 String::new();
                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .set_hd_wallet(hd_wallet);
                              gui.open_msg_window("Wallet Added", "");
                           });
                        }
                        Err(e) => {
                           ctx.write_vault(|vault| {
                              vault.remove_child(address);
                           });

                           let hd_wallet = ctx.get_vault().get_hd_wallet();

                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .add_wallet_window = false;
                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.wallet_name =
                                 String::new();
                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .set_hd_wallet(hd_wallet);

                              gui.open_msg_window("Failed to encrypt vault", e.to_string());
                           });
                        }
                     }
                  });
               }
            });
         });

      self.add_wallet_window = open;
      if !self.add_wallet_window {
         self.wallet_name.clear();
      }
   }
}

async fn sync_wallets_balance(
   ctx: ZeusCtx,
   addresses: Vec<Address>,
   concurrency: usize,
) -> Result<(), anyhow::Error> {
   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let semaphore = Arc::new(Semaphore::new(concurrency));

   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      let semaphore = semaphore.clone();
      let addresses = addresses.clone();

      let task = RT.spawn(async move {
         let client = ctx.get_client(chain).await?;
         let _permit = semaphore.acquire().await?;
         let balances = batch::get_eth_balances(client.clone(), chain, None, addresses).await?;

         let mut balance_map = SHARED_GUI.read(|gui| {
            gui.wallet_ui
               .add_wallet_ui
               .discover_child_wallets_ui
               .discovered_wallets
               .balances
               .clone()
         });

         for balance in balances {
            balance_map.insert((chain, balance.owner), balance.balance);
         }

         SHARED_GUI.write(|gui| {
            gui.wallet_ui
               .add_wallet_ui
               .discover_child_wallets_ui
               .discovered_wallets
               .balances = balance_map;
         });

         Ok(())
      });
      tasks.push(task);
   }

   for task in tasks {
      match task.await {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Error syncing wallets balance: {:?}", e);
         }
      }
   }

   Ok(())
}
