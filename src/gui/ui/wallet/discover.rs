//! UI that allows the user to discover and derive child wallets from a master wallet (BIP32 HD)

use crate::assets::Icons;
use crate::core::context::handle::MutexHandle;
use crate::core::{DiscoveredWallets, WalletPortfolio, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::malloc_trim;
use eframe::egui::{Align2, FontId, Id, Margin, Order, RichText, Stroke, Ui, vec2};
use egui_elements::{
   Button, ChipRole, Modal, SecureTextEdit, Spinner, Theme, Toast,
   layout::{Column, Table, TableScroll},
   widgets::Window,
};
use egui_lucide::Lucide;

use zeus_bip32::BIP32_HARDEN;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, NativeCurrency},
   types::SUPPORTED_CHAINS,
   utils::{NumericValue, batch, truncate_address},
};
use zeus_wallet::SecureHDWallet;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::{sync::Semaphore, task::JoinHandle};

const CHILD_VALUE_TTL: Duration = Duration::from_secs(1);

struct ChildValueCache {
   chains: Vec<u64>,
   total_value: f64,
   updated_at: Instant,
}

/// A UI for discovering and derive child wallets from a master wallet (BIP32 HD)
///
/// Discovery state ([DiscoveredWallets]) is kept in the encrypted vault and
/// updated in memory, it is written when the vault is saved (shutdown / vault ops).
pub struct DiscoverChildWallets {
   open: bool,
   hd_wallet: MutexHandle<SecureHDWallet>,
   /// A clone of the HD Wallet just to discover wallets
   discovery_wallet: MutexHandle<SecureHDWallet>,
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
   child_value_cache: HashMap<Address, ChildValueCache>,
}

impl DiscoverChildWallets {
   pub fn new() -> Self {
      Self {
         open: false,
         hd_wallet: MutexHandle::new(SecureHDWallet::random()),
         discovery_wallet: MutexHandle::new(SecureHDWallet::random()),
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
         child_value_cache: HashMap::new(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
      self.loading = true;

      crate::runtime::rt().spawn_blocking(move || {
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
      self.child_value_cache.clear();
   }

   pub fn set_hd_wallet(&mut self, hd_wallet: SecureHDWallet) {
      self.hd_wallet.write(|stored| *stored = hd_wallet);
   }

   pub fn set_discovery_wallet(&mut self, discovery_wallet: SecureHDWallet) {
      self.discovery_wallet.write(|stored| *stored = discovery_wallet);
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new();
   }

   pub fn erase(&mut self) {
      self.hd_wallet.write(|hd_wallet| hd_wallet.erase());
      self.discovery_wallet.write(|discovery_wallet| discovery_wallet.erase());
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
      let id = Id::unique("discover_wallets_window");

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
                  len.div_ceil(items_per_page)
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

               self.show_wallets(ctx, theme, icons.clone(), start, end, ui);

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
         crate::runtime::rt().spawn_blocking(move || {
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

      crate::runtime::rt().spawn(async move {
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
      let discovery_wallet = self.discovery_wallet.read(|wallet| wallet.clone());
      let mut discovered_wallets = self.discovered_wallets.clone();

      crate::runtime::rt().spawn(async move {
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

   fn cached_child_value(&mut self, ctx: &ZeusContext, address: Address) -> (Vec<u64>, f64) {
      let now = Instant::now();
      if let Some(cached) = self.child_value_cache.get(&address)
         && now.duration_since(cached.updated_at) < CHILD_VALUE_TTL
      {
         return (cached.chains.clone(), cached.total_value);
      }

      let mut chains = Vec::with_capacity(SUPPORTED_CHAINS.len());
      let mut total_value = 0.0;

      for chain in SUPPORTED_CHAINS {
         if ctx.is_chain_disabled(chain) {
            continue;
         }

         let key = (chain, address);
         if let Some(balance) = self.discovered_wallets.balances.get(&key)
            && !balance.is_zero()
         {
            chains.push(chain);

            let native = Currency::from(NativeCurrency::from(chain));
            let balance = NumericValue::currency_balance(*balance, native.decimals());
            let value = ctx.get_currency_value_for_amount(balance.f64(), &native);
            total_value += value.f64();
         }
      }

      self.child_value_cache.insert(
         address,
         ChildValueCache {
            chains: chains.clone(),
            total_value,
            updated_at: now,
         },
      );

      (chains, total_value)
   }

   /// The discovered wallets table: its derivation path, address, holdings, and `Add`.
   ///
   /// The page is cloned out of the discovery store first: resolving a row's value and adding it both
   /// need `&mut self`, which a borrow of `self.discovered_wallets` would not allow.
   fn show_wallets(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      start: usize,
      end: usize,
      ui: &mut Ui,
   ) {
      let tint = theme.image_tint_recommended;
      let button_visuals = theme.button_visuals();
      let current_chain = ctx.chain;

      let page = self.discovered_wallets.wallets.get(start..end).unwrap_or_default().to_vec();

      let mut add_wallet_clicked = false;
      let mut index_to_add = 0;

      Table::new("discover_wallets", &page)
         .columns([
            Column::Fr(3.4),     // Derivation Path
            Column::Fr(3.4),     // Address
            Column::Fr(3.2),     // Value
            Column::Fixed(80.0), // Add
         ])
         .gap(theme.spacing.md)
         // 12 pt, not xs: a themed `Button` paints ~6 pt above and below its band.
         .row_gap(theme.spacing.md)
         .scroll(TableScroll::new().auto_shrink([false; 2]).content_margin(15))
         .row_divider(true)
         .header_rule(true)
         .key((self.current_page, page.len()))
         .show(ui, |t| {
            t.header(|h| {
               for text in ["Derivation Path", "Address", "Value", ""] {
                  h.cell(|ui| {
                     if !text.is_empty() {
                        ui.label(
                           RichText::new(text)
                              .strong()
                              .size(theme.typography.normal)
                              .color(theme.colors.text),
                        );
                     }
                  });
               }
            });

            t.rows(|r, wallet| {
               let child_address = wallet.address;
               let child_index = wallet.index;
               let path = wallet.path.derivation_string();

               // A child that is already in the wallet is displayed as disabled in the Ui.
               let enabled =
                  !self.hd_wallet.read(|hd_wallet| hd_wallet.contains_child(child_address));
               let wallet_is_being_added = self.adding_wallet.contains(&child_address);
               let (chains, total_value) = self.cached_child_value(ctx, child_address);

               let value = if !enabled {
                  NumericValue::from_f64(total_value)
               } else {
                  let include_testnets = ctx.chain.is_testnet();
                  ctx.get_total_value(child_address, include_testnets).public
               };

               let address = child_address.to_string();
               let address_short = truncate_address(&address, 20);
               let explorer = current_chain.block_explorer();
               let link = format!("{}/address/{}", explorer, address);

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  ui.label(RichText::new(&path).size(theme.typography.small));
               });

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  ui.hyperlink_to(
                     RichText::new(&address_short)
                        .size(theme.typography.small)
                        .color(theme.colors.info),
                     &link,
                  );
               });

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  ui.spacing_mut().item_spacing.x = theme.spacing.xs;
                  for chain in &chains {
                     let icon = icons.chain_icon(*chain, tint).fit_to_exact_size(vec2(16.0, 16.0));
                     ui.add(icon);
                  }
                  ui.label(
                     RichText::new(format!("${:.10}", value.abbreviated()))
                        .color(theme.colors.text_muted)
                        .size(theme.typography.small),
                  );
               });

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  let text = RichText::new("Add").size(theme.typography.normal);
                  let button = Button::new(text).visuals(button_visuals).min_size(vec2(80.0, 32.0));
                  let spinner = Spinner::new().size(20.0).color(theme.colors.text);

                  if wallet_is_being_added {
                     ui.add(spinner);
                  } else if ui.add(button).clicked() {
                     add_wallet_clicked = true;
                     index_to_add = child_index;
                  }
               });
            });
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
         .id(Id::unique("discover_wallets_add_wallet_window"))
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

                  crate::runtime::rt().spawn_blocking(move || {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

                     let mut new_vault = ctx.get_vault();
                     let res = new_vault.derive_child_wallet_at_mut(name, index);

                     let address = match res {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window(format!("Failed to add wallet: {}", e));
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
                                 .tone(ChipRole::Success)
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
                                 .tone(ChipRole::Error)
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
                     crate::runtime::rt().spawn(async move {
                        for chain in SUPPORTED_CHAINS {
                           if let Err(e) = ctx_clone.register_railgun_signers(chain, false).await {
                              tracing::error!("Error registering Railgun signers: {:?}", e);
                           }

                           if let Err(e) = ctx_clone.sync_railgun(chain, false).await {
                              tracing::error!("Error syncing Railgun: {:?}", e);
                           }

                           ctx.update_private_data(chain, address).await;
                        }

                        // Free memory
                        malloc_trim();
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

      let task = crate::runtime::rt().spawn(async move {
         let _permit = semaphore.acquire().await?;
         let z_client = ctx.get_client_manager();

         let balances = z_client
            .request_with(
               chain,
               addresses.clone(),
               |client, addresses| async move {
                  batch::get_eth_balances(client, chain, None, addresses).await
               },
            )
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
            let ui = &mut gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui;
            ui.discovered_wallets.balances = balance_map;
            ui.child_value_cache.clear();
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
