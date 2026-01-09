use crate::assets::Icons;
use crate::core::{DiscoveredWallets, Portfolio, ZeusCtx};
use crate::gui::{SHARED_GUI, ui::REFRESH};
use crate::utils::RT;
use eframe::egui::{
   Align, Align2, FontId, Frame, Id, Layout, Margin, Order, RichText, ScrollArea, Spinner, Ui,
   Vec2, Window, vec2,
};
use zeus_bip32::BIP32_HARDEN;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, NativeCurrency},
   types::SUPPORTED_CHAINS,
   utils::{NumericValue, batch, truncate_address},
};
use zeus_theme::{OverlayManager, Theme};
use zeus_ui_components::SecureInputField;
use zeus_wallet::SecureHDWallet;
use zeus_widgets::{Button, SecureTextEdit};

use std::sync::Arc;
use tokio::{sync::Semaphore, task::JoinHandle};

#[derive(PartialEq, Eq)]
enum ImportWalletType {
   PrivateKey,
   MnemonicPhrase,
}

pub struct ImportWallet {
   open: bool,
   overlay: OverlayManager,
   import_key_or_phrase: ImportWalletType,
   input_field: SecureInputField,
   wallet_name: String,
   size: (f32, f32),
}

impl ImportWallet {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         import_key_or_phrase: ImportWalletType::PrivateKey,
         input_field: SecureInputField::new("Import Wallet", true, true),
         wallet_name: String::new(),
         size: (450.0, 250.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let was_open = self.open;
      let mut is_open = self.open;
      let mut clicked = false;

      Window::new(RichText::new("Import Wallet").size(theme.text_sizes.heading))
         .open(&mut is_open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let size = vec2(ui.available_width() * 0.5, 20.0);
               ui.add_space(20.0);

               // Wallet Name
               ui.label(RichText::new("Wallet Name (Optional)").size(theme.text_sizes.large));
               ui.add(
                  SecureTextEdit::singleline(&mut self.wallet_name)
                     .visuals(text_edit_visuals)
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

               // Input Field
               self.input_field.set_min_size(size);
               self.input_field.set_id(text);

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                  self.input_field.show(theme, ui);
               });

               // Import Button
               let text = RichText::new("Import").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  clicked = true;
               }
            });
         });

      if clicked {
         let name = self.wallet_name.clone();
         let key_or_phrase = self.input_field.text();
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
                     gui.wallet_ui.add_wallet_ui.import_wallet.input_field.erase();
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
            // Recalculate the wallets
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.open(ctx.clone());
            });

            // Fetch the balance for the new wallet across all chains and add it to the portfolio db
            RT.spawn(async move {
               let manager = ctx.balance_manager();
               for chain in SUPPORTED_CHAINS {
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
                  ctx.calculate_portfolio_value(chain, new_wallet_address);
               }
               ctx.save_balance_manager();
               ctx.save_portfolio_db();
            });
         });
      }

      if !is_open {
         self.close();
      }

      if was_open && !self.open {
         self.input_field.erase();
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.open();
            });
         });
      }
   }
}

pub struct AddWalletUi {
   open: bool,
   overlay: OverlayManager,
   import_wallet: ImportWallet,
   discover_child_wallets_ui: DiscoverChildWallets,
   #[allow(dead_code)]
   generate_wallet: bool,
   #[allow(dead_code)]
   wallet_name: String,
   size: (f32, f32),
}

impl AddWalletUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay: overlay.clone(),
         import_wallet: ImportWallet::new(overlay.clone()),
         discover_child_wallets_ui: DiscoverChildWallets::new(overlay),
         generate_wallet: false,
         wallet_name: String::new(),
         size: (450.0, 250.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      self.main_ui(ctx.clone(), theme, ui);
      self.import_wallet.show(ctx.clone(), theme, ui);
      self.discover_child_wallets_ui.show(ctx.clone(), theme, icons, ui);
      // self.derive_child_wallet_ui(ctx.clone(), theme, ui);
      // self.generate_wallet_ui(ctx.clone(), theme, ui);
   }

   fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let mut derive_clicked = false;
      let mut import_from_pk_clicked = false;
      let mut import_from_seed_clicked = false;

      Window::new(RichText::new("Add a new Wallet").size(theme.text_sizes.heading))
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.add_space(30.0);
               let size = vec2(ui.available_width() * 0.9, 50.0);

               // Derive a new child wallet from the master wallet
               let text = RichText::new("Derive from Master Wallet").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  derive_clicked = true;
               }

               // From private key
               let text = RichText::new("Import from a Private Key").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  import_from_pk_clicked = true;
               }

               // From seed phrase
               let text = RichText::new("Import from a Seed Phrase").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals).min_size(size);

               if ui.add(button).clicked() {
                  import_from_seed_clicked = true;
               }
            });
         });

      if derive_clicked {
         open = false;
         self.discover_child_wallets_ui.open();

         let vault = ctx.get_vault();

         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.loading = true;
            });

            let master = ctx.master_wallet_address();

            let mut discovered_wallets = match DiscoveredWallets::load_from_file() {
               Ok(wallets) => wallets,
               Err(e) => {
                  tracing::error!("Error loading discovered wallets: {:?}", e);
                  let mut discovered_wallets = DiscoveredWallets::new();
                  discovered_wallets.master_wallet_address = Some(master);
                  discovered_wallets
               }
            };

            // If wallets are empty, set master wallet address
            if discovered_wallets.wallets.is_empty() {
               discovered_wallets.master_wallet_address = Some(master);
            }

            // If master address is different, reset
            if let Some(master_wallet_address) = discovered_wallets.master_wallet_address {
               if master_wallet_address != master {
                  discovered_wallets = DiscoveredWallets::new();
                  discovered_wallets.master_wallet_address = Some(master);
               }
            }

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
                  .set_discovered_wallets(discovered_wallets);
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.current_page = 0;
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.open = true;
               gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.loading = false;
            });
         });
      }

      if import_from_pk_clicked {
         self.import_wallet.open();
         self.import_wallet.import_key_or_phrase = ImportWalletType::PrivateKey;
         open = false;
      }

      if import_from_seed_clicked {
         self.import_wallet.open();
         self.import_wallet.import_key_or_phrase = ImportWalletType::MnemonicPhrase;
         open = false;
      }

      if !open {
         self.close();
      }
   }

   fn _generate_wallet_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.generate_wallet {
         return;
      }

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

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let size = vec2(ui.available_width() * 0.5, 20.0);
               ui.add_space(20.0);

               // Wallet Name
               ui.label(RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal));
               ui.add(
                  SecureTextEdit::singleline(&mut self.wallet_name)
                     .visuals(text_edit_visuals)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .min_size(size),
               );

               // Generate Button
               let text = RichText::new("Generate").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

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

/// A UI for discovering and derive child wallets from a master wallet (BIP32 HD)
///
/// `discovery_wallets` stores the wallets that were discovered (No private keys)
/// Even if the json file is maliciously modified, the wallets will be derived from the current master wallet
/// so it's still safe to use
pub struct DiscoverChildWallets {
   open: bool,
   overlay: OverlayManager,
   hd_wallet: SecureHDWallet,
   /// A clone of the HD Wallet just to discover wallets
   discovery_wallet: SecureHDWallet,
   discovered_wallets: DiscoveredWallets,
   syncing: bool,
   loading: bool,
   add_wallet_window: bool,
   index_to_add: u32,
   wallet_name: String,
   current_page: usize,
   items_per_page: usize,
   size: (f32, f32),
}

impl DiscoverChildWallets {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         hd_wallet: SecureHDWallet::random(),
         discovery_wallet: SecureHDWallet::random(),
         discovered_wallets: DiscoveredWallets::new(),
         syncing: false,
         loading: false,
         add_wallet_window: false,
         index_to_add: 0,
         wallet_name: String::new(),
         current_page: 0,
         items_per_page: 20,
         size: (600.0, 450.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   fn open_add_wallet_window(&mut self, index_to_add: u32) {
      self.overlay.window_opened();
      self.index_to_add = index_to_add;
      self.add_wallet_window = true;
   }

   fn close_add_wallet_window(&mut self) {
      self.overlay.window_closed();
      self.add_wallet_window = false;
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
      self.close();
      *self = Self::new(self.overlay.clone());
   }

   fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

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

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.add_space(15.0);

               if self.loading {
                  ui.label(RichText::new("Loading...").size(theme.text_sizes.normal));
                  ui.add(Spinner::new().size(15.0).color(theme.colors.text));
                  return;
               }

               let len = self.discovered_wallets.wallets.len();
               let items_per_page = self.items_per_page;

               let total_pages = if items_per_page == 0 {
                  0
               } else {
                  (len + items_per_page - 1) / items_per_page
               };

               let start = self.current_page * items_per_page;
               let end = (start + items_per_page).min(len);
               let text = if len == 0 {
                  "No wallets found".to_string()
               } else {
                  format!(
                     "Showing {}-{} of {} wallets (Page {} of {})",
                     start + 1,
                     end,
                     len,
                     self.current_page + 1,
                     total_pages
                  )
               };

               ui.label(RichText::new(text).size(theme.text_sizes.normal));

               let batch_size = self.discovered_wallets.batch_size;

               let text = format!("Generate next {} wallets", batch_size);
               let text = RichText::new(text).size(theme.text_sizes.normal);
               let gen_button = Button::new(text).visuals(button_visuals);

               let text = RichText::new(REFRESH).size(theme.text_sizes.normal);
               let refresh_button = Button::new(text).visuals(button_visuals);

               let size = vec2(ui.available_width() * 0.4, 45.0);

               ui.vertical_centered(|ui| {
                  ui.allocate_ui(size, |ui| {
                     ui.horizontal(|ui| {
                        if ui.add(gen_button).clicked() {
                           self.generate_wallets(ctx.clone(), batch_size);
                        }

                        if ui.add(refresh_button).clicked() {
                           self.refresh_balance(ctx.clone(), start, end);
                        }

                        if self.syncing {
                           ui.add(Spinner::new().size(15.0).color(theme.colors.text));
                        }
                     });
                  });
               });

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
                           start,
                           end,
                           ui,
                        );
                     });
               });

               ui.add_space(10.0);

               let size = vec2(ui.available_width() * 0.5, 45.0);

               ui.vertical_centered(|ui| {
                  ui.allocate_ui(size, |ui| {
                     ui.horizontal(|ui| {
                        ui.add_enabled_ui(self.current_page > 0, |ui| {
                           let prev_text = RichText::new("Previous").size(theme.text_sizes.normal);
                           let button = Button::new(prev_text).visuals(button_visuals);

                           if ui.add(button).clicked() {
                              self.current_page -= 1;
                           }
                        });

                        let page_text = RichText::new(format!(
                           "Page {} of {}",
                           self.current_page + 1,
                           total_pages
                        ))
                        .size(theme.text_sizes.normal);

                        ui.label(page_text);
                        ui.add_enabled_ui(self.current_page + 1 < total_pages, |ui| {
                           let next_text = RichText::new("Next").size(theme.text_sizes.normal);
                           let button = Button::new(next_text).visuals(button_visuals);

                           if ui.add(button).clicked() {
                              self.current_page += 1;
                           }
                        });
                     });
                  });
               });
            });
         });

      if !is_open {
         self.close();
      }

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
               gui.wallet_ui.add_wallet_ui.open();
            });
         });
         self.reset();
      }
   }

   fn refresh_balance(&mut self, ctx: ZeusCtx, start: usize, end: usize) {
      let slice = &self.discovered_wallets.wallets[start..end];
      let addresses = slice.iter().map(|w| w.address).collect::<Vec<_>>();

      let concurrency = self.discovered_wallets.concurrency;
      let ctx_clone = ctx.clone();
      self.syncing = true;

      RT.spawn(async move {
         match sync_wallets_balance(ctx_clone.clone(), addresses, concurrency).await {
            Ok(_) => {}
            Err(e) => {
               tracing::error!("Error syncing wallets: {:?}", e);
            }
         }

         SHARED_GUI.write(|gui| {
            gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.syncing = false;
         });
      });
   }

   fn generate_wallets(&mut self, ctx: ZeusCtx, batch_size: usize) {
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

   fn show_wallets(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      column_widths: &[f32],
      start: usize,
      end: usize,
      ui: &mut Ui,
   ) {
      let tint = theme.image_tint_recommended;
      let button_visuals = theme.button_visuals();

      let mut add_wallet_clicked = false;
      let mut index_to_add = 0;

      let slice = &self.discovered_wallets.wallets[start..end];
      for child in slice.iter() {
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
                        RichText::new(text).size(theme.text_sizes.small).color(theme.colors.info),
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
                        let icon = icons.chain_icon_x16(chain, tint);
                        ui.add(icon);
                     }
                  });

                  ui.label(
                     RichText::new(format!("${}", value.abbreviated()))
                        .color(theme.colors.text_muted)
                        .size(theme.text_sizes.small),
                  );
               });

               ui.scope(|ui| {
                  ui.set_width(column_widths[3]);
                  let text = RichText::new("Add").size(theme.text_sizes.normal);
                  let button = Button::new(text).visuals(button_visuals);

                  if ui.add(button).clicked() {
                     add_wallet_clicked = true;
                     index_to_add = child.index;
                  }
               });
            });
         });
      }

      if add_wallet_clicked {
         self.open_add_wallet_window(index_to_add);
      }
   }

   fn add_wallet(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.add_wallet_window {
         return;
      }

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

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               let text = RichText::new("Wallet Name (Optional)").size(theme.text_sizes.normal);
               ui.label(text);

               SecureTextEdit::singleline(&mut self.wallet_name)
                  .visuals(text_edit_visuals)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let text = RichText::new("Add Wallet").size(theme.text_sizes.large);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  let index = self.index_to_add;
                  let name = self.wallet_name.clone();
                  let balances = self.discovered_wallets.balances.clone();

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Encrypting vault...");
                     });

                     let mut new_vault = ctx.get_vault();
                     let res = new_vault.derive_child_wallet_at_mut(name, index);

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

                     // On success save the vault and update the hd wallet in the Ui
                     // If this op fails we revert the changes
                     match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
                        Ok(_) => {
                           let hd_wallet = new_vault.get_hd_wallet();

                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .close_add_wallet_window();

                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.wallet_name =
                                 String::new();

                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .set_hd_wallet(hd_wallet);

                              gui.loading_window.reset();
                              gui.open_msg_window("Wallet Added", "");
                           });
                        }
                        Err(e) => {
                           let hd_wallet = ctx.get_vault().get_hd_wallet();

                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .close_add_wallet_window();

                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.wallet_name =
                                 String::new();

                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .set_hd_wallet(hd_wallet);

                              gui.loading_window.reset();
                              gui.open_msg_window("Failed to encrypt vault", e.to_string());
                           });
                        }
                     }
                     // Update the Vault in the ZeusCtx
                     ctx.set_vault(new_vault);
                     // Calculate the wallets again in the UI
                     SHARED_GUI.write(|gui| {
                        gui.wallet_ui.open(ctx.clone());
                     });
                  });
               }
            });
         });

      if !open {
         self.close_add_wallet_window();
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
         let _permit = semaphore.acquire().await?;
         let z_client = ctx.get_zeus_client();

         let balances = z_client
            .request(chain, |client| {
               let addresses = addresses.clone();
               async move { batch::get_eth_balances(client, chain, None, addresses).await }
            })
            .await?;

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
