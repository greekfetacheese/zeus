//! UI that allows the user to discover and derive child wallets from a master wallet (BIP32 HD)

use crate::assets::Icons;
use crate::core::{DiscoveredWallets, WalletPortfolio, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use eframe::egui::{
   Align, Align2, FontId, Id, Layout, Margin, Order, RichText, ScrollArea, Sense, Spinner, Stroke,
   TextWrapMode, Ui, UiBuilder, vec2,
};
use egui_elements::{Button, Modal, SecureTextEdit, Theme, widgets::Window};
use egui_lucide::Lucide;
use elegance::{BadgeTone, Toast};

use zeus_bip32::BIP32_HARDEN;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, NativeCurrency},
   types::SUPPORTED_CHAINS,
   utils::{NumericValue, batch, truncate_address},
};
use zeus_wallet::SecureHDWallet;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::{sync::Semaphore, task::JoinHandle};

/// A UI for discovering and derive child wallets from a master wallet (BIP32 HD)
///
/// Discovery state ([DiscoveredWallets]) is kept in the encrypted vault and
/// updated in memory, it is written when the vault is saved (shutdown / vault ops).
pub struct DiscoverChildWallets {
   open: bool,
   hd_wallet: SecureHDWallet,
   /// A clone of the HD Wallet just to discover wallets
   discovery_wallet: SecureHDWallet,
   discovered_wallets: DiscoveredWallets,
   syncing: bool,
   adding_wallet: HashSet<Address>,
   pub loading: bool,
   add_wallet_window: bool,
   index_to_add: u32,
   wallet_name: String,
   pub current_page: usize,
   items_per_page: usize,
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
         adding_wallet: HashSet::new(),
         loading: false,
         add_wallet_window: false,
         index_to_add: 0,
         wallet_name: String::new(),
         current_page: 0,
         items_per_page: 10,
         size: (600.0, 450.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
      self.loading = true;

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let vault = ctx.get_vault();
         let master = ctx.master_wallet_address();
         let hd_wallet = vault.get_hd_wallet();

         let mut discovered_wallets = ctx.read_wallet_state(|ws| ws.discovered_wallets.clone());

         if discovered_wallets.master_wallet_address.is_none() {
            discovered_wallets.master_wallet_address = Some(master);
         } else if discovered_wallets.master_wallet_address != Some(master) {
            tracing::warn!("Discovered wallets master address mismatch, resetting");
            discovered_wallets = DiscoveredWallets::new();
            discovered_wallets.master_wallet_address = Some(master);
         } else if discovered_wallets.is_corrupted() {
            tracing::warn!("Discovered wallets index is corrupted, resetting");
            discovered_wallets = DiscoveredWallets::new();
            discovered_wallets.master_wallet_address = Some(master);
         } else {
            discovered_wallets.rediscover_wallets(hd_wallet.clone());
         }

         SHARED_GUI.write(|gui| {
            let ui = &mut gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui;
            ui.set_hd_wallet(hd_wallet.clone());
            ui.set_discovery_wallet(hd_wallet);
            ui.set_discovered_wallets(discovered_wallets);
            ui.current_page = 0;
            ui.loading = false;
         });
      });
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   fn open_add_wallet_window(&mut self, index_to_add: u32) {
      self.index_to_add = index_to_add;
      self.add_wallet_window = true;
   }

   fn close_add_wallet_window(&mut self) {
      self.add_wallet_window = false;
   }

   pub fn set_discovered_wallets(&mut self, discovered_wallets: DiscoveredWallets) {
      self.discovered_wallets = discovered_wallets;
   }

   pub fn set_hd_wallet(&mut self, hd_wallet: SecureHDWallet) {
      self.hd_wallet = hd_wallet;
   }

   pub fn set_discovery_wallet(&mut self, discovery_wallet: SecureHDWallet) {
      self.discovery_wallet = discovery_wallet;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new();
   }

   /// Fixed-size cell. The parent always advances by `width` even if a label
   /// wants more space — otherwise path/address shove later columns.
   fn row_cell(ui: &mut Ui, width: f32, height: f32, add_contents: impl FnOnce(&mut Ui)) {
      let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
      let mut child =
         ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
      child.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
      add_contents(&mut child);
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.add_wallet(theme, ui);

      let was_open = self.open;
      let mut is_open = self.open;

      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new("Discover Wallets").size(theme.typography.heading);
      let id = Id::new("discover_wallets_window");

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
            ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
            ui.spacing_mut().button_padding = theme.button_padding;

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               if self.loading {
                  ui.label(RichText::new("Loading...").size(theme.typography.normal));
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

               ui.label(RichText::new(text).size(theme.typography.normal));

               ui.add_space(10.0);

               let batch_size = self.items_per_page;

               let n = 2.0;
               let content_width = ui.available_width() * 0.9;
               let gap = theme.spacing.sm;
               let button_size = vec2((content_width - gap * (n - 1.0)) / n, 45.0);

               let text = format!("Generate next {} wallets", batch_size);
               let text = RichText::new(text).size(theme.typography.normal);
               let gen_button = Button::new(text).min_size(button_size);

               let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();
               let text = RichText::new("Sync ETH Balances").size(theme.typography.normal);
               let sync_button = Button::image_and_text(icon, text).min_size(button_size);

               ui.allocate_ui(vec2(content_width, 45.0), |ui| {
                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = theme.spacing.md;
                     if ui.add(gen_button).clicked() {
                        self.generate_wallets(batch_size);
                     }

                     if self.syncing {
                        ui.add(Spinner::new().size(20.0).color(theme.colors.text));
                     } else {
                        if ui.add(sync_button).clicked() {
                           self.refresh_balance(start, end);
                        }
                     }
                  });
               });

               ui.add_space(10.0);

               let row_height = 40.0;
               let col_spacing = theme.spacing.md;
               let n_cols = 4.0;
               let row_frame = theme.frame2.outer_margin(Margin::ZERO);
               let inner_left = row_frame.inner_margin.leftf();
               let inner_right = row_frame.inner_margin.rightf();
               let inner_y = row_frame.inner_margin.topf() + row_frame.inner_margin.bottomf();
               let row_width = ui.available_width();
               let inner_width = (row_width - inner_left - inner_right).max(0.0);
               let usable = (inner_width - col_spacing * (n_cols - 1.0)).max(0.0);
               let add_w = 80.0_f32.min(usable);
               let rest = (usable - add_w).max(0.0);
               let column_widths = [
                  rest * 0.34, // Derivation Path
                  rest * 0.34, // Address
                  rest * 0.32, // Value
                  add_w,       // Add
               ];

               ui.horizontal(|ui| {
                  ui.add_space((ui.available_width() - row_width).max(0.0) / 2.0 + inner_left);
                  ui.spacing_mut().item_spacing.x = col_spacing;
                  for (i, header) in
                     ["Derivation Path", "Address", "Value", ""].into_iter().enumerate()
                  {
                     Self::row_cell(ui, column_widths[i], 28.0, |ui| {
                        if !header.is_empty() {
                           ui.label(
                              RichText::new(header)
                                 .strong()
                                 .size(theme.typography.normal)
                                 .color(theme.colors.text),
                           );
                        }
                     });
                  }
               });

               ScrollArea::vertical()
                  .id_salt("children_wallets_in_discovery")
                  .auto_shrink([false; 2])
                  .content_margin(5)
                  .show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     self.show_wallets(
                        ctx,
                        theme,
                        icons.clone(),
                        &column_widths,
                        row_width,
                        inner_width,
                        col_spacing,
                        row_height,
                        inner_y,
                        start,
                        end,
                        ui,
                     );
                  });

               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.sm;

                  ui.add_enabled_ui(self.current_page > 0, |ui| {
                     let prev_text = RichText::new("Previous").size(theme.typography.normal);
                     let button = Button::new(prev_text).visuals(button_visuals);
                     if ui.add(button).clicked() {
                        self.current_page -= 1;
                     }
                  });

                  ui.label(
                     RichText::new(format!(
                        "Page {} of {}",
                        self.current_page + 1,
                        total_pages.max(1)
                     ))
                     .size(theme.typography.normal),
                  );

                  ui.add_enabled_ui(self.current_page + 1 < total_pages, |ui| {
                     let next_text = RichText::new("Next").size(theme.typography.normal);
                     let button = Button::new(next_text).visuals(button_visuals);
                     if ui.add(button).clicked() {
                        self.current_page += 1;
                     }
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
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            ctx.write_wallet_state(|ws| ws.discovered_wallets = wallets);

            tracing::debug!("Discovered wallets updated");

            SHARED_GUI.write(|gui| {
               gui.wallet_ui.add_wallet_ui.open();
            });
         });
         self.reset();
      }
   }

   fn refresh_balance(&mut self, start: usize, end: usize) {
      let slice = &self.discovered_wallets.wallets[start..end];
      let addresses = slice.iter().map(|w| w.address).collect::<Vec<_>>();

      let concurrency = self.discovered_wallets.concurrency;
      self.syncing = true;

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         match sync_wallets_balance(ctx, addresses, concurrency).await {
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

   fn generate_wallets(&mut self, batch_size: usize) {
      self.syncing = true;
      let mut addresses = Vec::new();
      let concurrency = self.discovered_wallets.concurrency;
      let discovery_wallet = self.discovery_wallet.clone();
      let mut discovered_wallets = self.discovered_wallets.clone();

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let vault = ctx.get_vault();

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
               if vault.wallet_address_exists(wallet.address()) {
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

         match sync_wallets_balance(ctx, addresses, concurrency).await {
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
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      column_widths: &[f32],
      row_width: f32,
      inner_width: f32,
      col_spacing: f32,
      row_height: f32,
      inner_y: f32,
      start: usize,
      end: usize,
      ui: &mut Ui,
   ) {
      let tint = theme.image_tint_recommended;
      let button_visuals = theme.button_visuals();
      let row_frame = theme.frame2.outer_margin(Margin::ZERO);

      let mut add_wallet_clicked = false;
      let mut index_to_add = 0;

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = theme.spacing.sm;

         let wallets = &self.discovered_wallets.wallets[start..end];
         for child in wallets {
            // If child already exists it will displayed as disabled in the Ui
            let exists = self.hd_wallet.contains_child(child.address);
            let wallet_is_beign_added = self.adding_wallet.contains(&child.address);

            let mut chains = Vec::new();
            let mut total_value = 0.0;
            let current_chain = ctx.chain;

            // get the chains which the wallet has balance in
            for chain in SUPPORTED_CHAINS {
               if ctx.is_chain_disabled(chain) {
                  continue;
               }

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

            let value = if !exists {
               NumericValue::from_f64(total_value)
            } else {
               let include_testnets = ctx.chain.is_testnet();
               ctx.get_total_value(child.address, include_testnets).public
            };

            let path = child.path.derivation_string();
            let address = child.address.to_string();
            let address_short = truncate_address(&address, 20);
            let explorer = current_chain.block_explorer();
            let link = format!("{}/address/{}", explorer, address);
            let child_index = child.index;

            ui.allocate_ui(vec2(row_width, row_height + inner_y), |ui| {
               row_frame.show(ui, |ui| {
                  ui.set_width(inner_width);
                  ui.spacing_mut().item_spacing.x = col_spacing;

                  ui.add_enabled_ui(!exists, |ui| {
                     ui.horizontal(|ui| {
                        Self::row_cell(ui, column_widths[0], row_height, |ui| {
                           ui.label(RichText::new(&path).size(theme.typography.small));
                        });

                        Self::row_cell(ui, column_widths[1], row_height, |ui| {
                           ui.hyperlink_to(
                              RichText::new(&address_short)
                                 .size(theme.typography.small)
                                 .color(theme.colors.info),
                              &link,
                           );
                        });

                        Self::row_cell(ui, column_widths[2], row_height, |ui| {
                           ui.spacing_mut().item_spacing.x = theme.spacing.xs;
                           for chain in &chains {
                              let icon =
                                 icons.chain_icon(*chain, tint).fit_to_exact_size(vec2(16.0, 16.0));
                              ui.add(icon);
                           }
                           ui.label(
                              RichText::new(format!("${}", value.abbreviated()))
                                 .color(theme.colors.text_muted)
                                 .size(theme.typography.small),
                           );
                        });

                        Self::row_cell(ui, column_widths[3], row_height, |ui| {
                           let text = RichText::new("Add").size(theme.typography.normal);
                           let button = Button::new(text)
                              .visuals(button_visuals)
                              .min_size(vec2(column_widths[3], 32.0));

                           let spinner = Spinner::new().size(20.0).color(theme.colors.text);

                           if !wallet_is_beign_added {
                              if ui.add(button).clicked() {
                                 add_wallet_clicked = true;
                                 index_to_add = child_index;
                              }
                           } else {
                              ui.add(spinner);
                           }
                        });
                     });
                  });
               });
            });
         }
      });

      if add_wallet_clicked {
         self.open_add_wallet_window(index_to_add);
      }
   }

   fn add_wallet(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.add_wallet_window {
         return;
      }

      let mut open = self.add_wallet_window;

      let title = RichText::new("Add Wallet").size(theme.typography.heading);
      let window_frame = theme.window_frame;
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(title)
         .id(Id::new("discover_wallets_add_wallet_window"))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.xl);
            ui.spacing_mut().button_padding = theme.button_padding;

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               let text = RichText::new("Wallet Name (Optional)").size(theme.typography.large);
               ui.label(text);

               SecureTextEdit::singleline(&mut self.wallet_name)
                  .visuals(text_edit_visuals)
                  .font(FontId::proportional(theme.typography.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               let text = RichText::new("Add Wallet").size(theme.typography.large);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  let index = self.index_to_add;
                  let name = self.wallet_name.clone();
                  let balances = self.discovered_wallets.balances.clone();

                  RT.spawn_blocking(move || {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

                     let mut new_vault = ctx.get_vault();
                     let res = new_vault.derive_child_wallet_at_mut(name, index);

                     let address = match res {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window(format!(
                                 "Failed to add wallet: {}",
                                 e.to_string()
                              ));
                           });
                           return;
                        }
                     };

                     for chain in SUPPORTED_CHAINS {
                        if ctx.is_chain_disabled(chain) {
                           continue;
                        }

                        let eth = NativeCurrency::from(chain);
                        let balance = balances.get(&(chain, address)).cloned().unwrap_or_default();
                        let balance_manager = ctx.balance_manager();
                        balance_manager.insert_eth_balance(chain, address, balance, &eth);

                        ctx.write_wallet_state(|ws| {
                           ws.portfolio_db.insert_portfolio(
                              chain,
                              address,
                              WalletPortfolio::new(address, chain),
                           );
                        });
                     }

                     // Dont open the loading window here, just show a toast
                     // Safety: The wallet is only added if the op is successful

                     SHARED_GUI.write(|gui| {
                        gui.wallet_ui
                           .add_wallet_ui
                           .discover_child_wallets_ui
                           .adding_wallet
                           .insert(address);

                        gui.wallet_ui
                           .add_wallet_ui
                           .discover_child_wallets_ui
                           .close_add_wallet_window();
                     });

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

                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .adding_wallet
                                 .remove(&address);

                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.wallet_name =
                                 String::new();

                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .set_hd_wallet(hd_wallet);

                              Toast::new("Wallet Added")
                                 .tone(BadgeTone::Ok)
                                 .description("Wallet added successfully")
                                 .duration(Duration::from_secs(5))
                                 .show(&gui.egui_ctx);
                           });
                        }
                        Err(e) => {
                           let hd_wallet = ctx.get_vault().get_hd_wallet();

                           SHARED_GUI.write(|gui| {
                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .close_add_wallet_window();

                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .adding_wallet
                                 .remove(&address);

                              gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.wallet_name =
                                 String::new();

                              gui.wallet_ui
                                 .add_wallet_ui
                                 .discover_child_wallets_ui
                                 .set_hd_wallet(hd_wallet);

                              Toast::new("Failed to encrypt vault")
                                 .tone(BadgeTone::Danger)
                                 .description(e.to_string())
                                 .duration(Duration::from_secs(5))
                                 .show(&gui.egui_ctx);
                           });
                           return;
                        }
                     }
                     // Update the Vault in the ZeusCtx
                     ctx.set_vault(new_vault);
                     ctx.build_wallet_info_cache();

                     let ctx_clone = ctx.clone();
                     RT.spawn(async move {
                        for chain in SUPPORTED_CHAINS {
                           if let Err(e) = ctx_clone.register_railgun_signers(chain, false).await {
                              tracing::error!("Error registering Railgun signers: {:?}", e);
                           }
                        }
                     });

                     // Calculate the wallets again in the UI
                     SHARED_GUI.write(|gui| {
                        gui.wallet_ui.calc_wallet_value();
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
      if ctx.is_chain_disabled(chain) {
         continue;
      }

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
