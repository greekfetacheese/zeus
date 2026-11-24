//! UI that allows the user to discover and derive child wallets from a master wallet (BIP32 HD)

use crate::assets::Icons;
use crate::core::context::handle::MutexHandle;
use crate::core::{DiscoveredWallets, WalletPortfolio, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::{chain_label, name_text};
use crate::utils::malloc_trim;
use eframe::egui::{
   Align2, FontId, Id, Margin, Order, Response, RichText, Role, Stroke, Ui, WidgetInfo, vec2,
};
use egui_elements::{
   Button, ChipRole, Frame, Modal, SecureTextEdit, Spinner, Theme, Toast,
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

/// The wallets one page holds: what the dialog shows without scrolling.
///
/// egui places no row the table's viewport cannot show, and a row that is never placed cannot take
/// the keyboard, so a page holds the rows it shows and every one of them is reachable.
const WALLETS_PER_PAGE: usize = 5;

/// How many wallets one `Generate` press derives. It is not the page size: a short page is a
/// reading choice, and deriving five at a time would only make discovery slower.
const GENERATE_BATCH: usize = 10;

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
   /// Whether the name field still has to take the focus the window's close button is given when it
   /// opens.
   focus_name_field: bool,
   index_to_add: u32,
   wallet_name: String,
   pub current_page: usize,
   size: (f32, f32),
   child_value_cache: HashMap<Address, ChildValueCache>,
   /// The one sentence the flow wants said. A dialog cannot announce from inside its own draw, so
   /// the window above drains this after the flow is drawn.
   notice: Option<String>,
   /// The sentence waiting for a pass egui keeps. The results table asks for a discard on the pass
   /// it measures, and a pass that is thrown away reaches no reader, so what the flow raised waits
   /// here until a pass that is drawn.
   pending_notice: Option<String>,
   /// The last count said, so a count that did not change is not repeated.
   spoken_count: Option<String>,
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
         focus_name_field: false,
         index_to_add: 0,
         wallet_name: String::new(),
         current_page: 0,
         size: (600.0, 450.0),
         child_value_cache: HashMap::new(),
         notice: None,
         pending_notice: None,
         spoken_count: None,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
      self.loading = true;
      // A sentence the flow that ended before left behind is not about the list this one loads.
      self.pending_notice = None;

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
      // A sentence raised before the flow closed is not about the list the user comes back to.
      self.notice = None;
      self.pending_notice = None;
      // The count is said again when the flow is opened on a list.
      self.spoken_count = None;
   }

   /// The sentence the flow wants said, for the page's announcement channel.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   fn open_add_wallet_window(&mut self, index_to_add: u32) {
      self.index_to_add = index_to_add;
      self.add_wallet_window = true;
      // The field asks for the focus the window's close button is given on the pass it opens, so a
      // reader lands in the box that names the wallet.
      self.focus_name_field = true;
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

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.add_wallet(theme, ui);

      let was_open = self.open;
      let mut is_open = self.open;

      let frame = theme.window_frame.fill(theme.colors.bg);
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

                  let spinner = ui.add(Spinner::new().size(15.0));
                  spinner.widget_info(|| {
                     WidgetInfo::labeled(
                        Role::ProgressIndicator,
                        ui.is_enabled(),
                        "Loading discovered wallets",
                     )
                  });

                  return;
               }

               let len = self.discovered_wallets.wallets.len();

               let total_pages = len.div_ceil(WALLETS_PER_PAGE);

               let start = self.current_page * WALLETS_PER_PAGE;
               let end = (start + WALLETS_PER_PAGE).min(len);
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

               // The count is the answer a page press or a generate gets, so it is said when the
               // page or the number of wallets changes, and once per change.
               let count = count_sentence(start, end, len);

               if self.spoken_count.as_deref() != Some(count.as_str()) {
                  self.spoken_count = Some(count.clone());
                  self.pending_notice = Some(count);
               }

               ui.add_space(10.0);

               let batch_size = GENERATE_BATCH;

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
                        self.pending_notice = Some(format!("Generating {batch_size} wallets."));
                        self.generate_wallets(batch_size);
                     }

                     if self.syncing {
                        let spinner = ui.add(Spinner::new().size(20.0));
                        spinner.widget_info(|| {
                           WidgetInfo::labeled(
                              Role::ProgressIndicator,
                              ui.is_enabled(),
                              "Syncing balances",
                           )
                        });
                     } else {
                        if ui.add(sync_button).clicked() {
                           self.pending_notice = Some("Syncing balances.".to_string());
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

      // A pass egui is going to throw away reaches no reader, and the results table asks for one on
      // the pass it measures, so the sentence waits for one that is kept.
      let kept = !ui.ctx().will_discard();

      if kept && let Some(text) = self.pending_notice.take() {
         self.notice = Some(text);
      }
   }

   fn refresh_balance(&mut self, start: usize, end: usize) {
      let slice = &self.discovered_wallets.wallets[start..end];
      let addresses = slice.iter().map(|w| w.address).collect::<Vec<_>>();

      let concurrency = self.discovered_wallets.concurrency;
      self.syncing = true;

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let synced = sync_wallets_balance(ctx, addresses, concurrency).await;

         if let Err(e) = &synced {
            tracing::error!("Error syncing wallets: {:?}", e);
         }

         SHARED_GUI.write(|gui| {
            let ui = &mut gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui;
            ui.syncing = false;
            // The spinner going away says nothing on its own, so the end of the work is a sentence.
            ui.pending_notice = Some(balance_sync_sentence(synced.is_ok()).to_string());
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

      // A selectable label hands its drawn text to the tree as a `TextRun` under its own node, and
      // a reader reads both: the row said its index and then spelled the whole path, and went on
      // through every row spelling the addresses. The cells name themselves here, so the glyphs
      // stay on screen and out of the reading.
      let selectable_labels = ui.style().interaction.selectable_labels;
      ui.style_mut().interaction.selectable_labels = false;

      // The row is a card, not a control: an interactive frame is a focusable plate with no name,
      // and a Tab that lands on one strands the keyboard in that row — the next Tab re-enters it
      // at its first cell. The row has nothing to click, so it asks for no interaction at all.
      let row_frame = Frame::new()
         .inner_margin(theme.inner_margin)
         .corner_radius(theme.corner_radius)
         .visuals(theme.frame1_visuals());

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
         .row_frame(row_frame)
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

               // A row's place on its page repeats on every page, so the name uses the number the
               // derivation path ends with: "Add wallet 7" for m/44'/60'/0'/0/7.
               let wallet_number = child_index.saturating_sub(BIP32_HARDEN);
               let add_name = format!("Add wallet {wallet_number}");

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  let drawn = ui.label(RichText::new(&path).size(theme.typography.small));

                  // A derivation path is read one digit and apostrophe at a time; the row is named
                  // by the index it ends with, the number its button adds.
                  name_text(ui, &drawn, wallet_number.to_string());
               });

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  let drawn = ui.hyperlink_to(
                     RichText::new(&address_short)
                        .size(theme.typography.small)
                        .color(theme.colors.info),
                     &link,
                  );

                  // The glyphs are a shortening nobody can follow by ear, so the link says where
                  // it leads and which wallet it is about; the address is never read out.
                  name_text(
                     ui,
                     &drawn,
                     format!("Wallet {wallet_number} address on the block explorer"),
                  );
               });

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  ui.spacing_mut().item_spacing.x = theme.spacing.xs;
                  for chain in &chains {
                     let icon = icons.chain_icon(*chain, tint).fit_to_exact_size(vec2(16.0, 16.0));
                     let icon = ui.add(icon);
                     let label = chain_label(*chain);
                     icon.widget_info(|| WidgetInfo::labeled(Role::Image, ui.is_enabled(), &label));
                  }

                  let value_text = format!("${:.10}", value.abbreviated());
                  let drawn = ui.label(
                     RichText::new(&value_text)
                        .color(theme.colors.text_muted)
                        .size(theme.typography.small),
                  );

                  // The figure is one wallet's holdings, and a bare amount does not say whose.
                  name_text(ui, &drawn, format!("Value {value_text}"));
               });

               r.cell(|ui| {
                  if !enabled {
                     ui.disable();
                  }
                  let text = RichText::new("Add").size(theme.typography.normal);
                  let button = Button::new(text).visuals(button_visuals).min_size(vec2(80.0, 32.0));
                  let spinner = Spinner::new().size(20.0);

                  if wallet_is_being_added {
                     ui.add(spinner);
                  } else {
                     let add = ui.add(button);

                     // Every row draws the same word, so the name says which wallet it adds; the
                     // address behind it is not read out.
                     add.widget_info(|| {
                        WidgetInfo::labeled(Role::Button, ui.is_enabled(), &add_name)
                     });

                     if add.clicked() {
                        add_wallet_clicked = true;
                        index_to_add = child_index;
                     }
                  }
               });
            });
         });

      ui.style_mut().interaction.selectable_labels = selectable_labels;

      if add_wallet_clicked {
         self.open_add_wallet_window(index_to_add);
      }
   }

   fn add_wallet(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.add_wallet_window {
         return;
      }

      let mut open = self.add_wallet_window;
      let mut name_field: Option<Response> = None;

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
               let drawn = ui.label(text);

               // The field carries this instruction as its name, so the drawn label is not a
               // second stop.
               ui.ctx().accesskit_node_builder(drawn.id, |node| node.set_hidden());

               // The parentheses of the drawn label are read aloud, so the field is named without
               // them.
               let field = SecureTextEdit::singleline(&mut self.wallet_name)
                  .accessible_name("Wallet name, optional")
                  .visuals(text_edit_visuals)
                  .font(FontId::proportional(theme.typography.normal))
                  .margin(Margin::same(10))
                  .min_size(vec2(ui.available_width() * 0.9, 25.0))
                  .show(ui);

               name_field = Some(field.response);

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

      // The window spends its first passes sizing its body, where nothing is visible and a focus
      // request is dropped, so the field asks on the first pass that is drawn — after the window
      // has drawn.
      if let Some(field) = name_field.filter(|field| field.visible() && self.focus_name_field) {
         ui.ctx().memory_mut(|memory| memory.request_focus(field.id));
         self.focus_name_field = false;
      }

      if !open {
         self.close_add_wallet_window();
         self.wallet_name.clear();
      }
   }
}

/// The sentence that says which wallets of the list are on this page, and that there are none.
fn count_sentence(start: usize, end: usize, total: usize) -> String {
   match total {
      0 => "No wallets found.".to_string(),
      total => format!("Wallets {} to {} of {}.", start + 1, end, total),
   }
}

/// How the end of a balance sync is said: the spinner going away on its own tells a reader
/// nothing.
fn balance_sync_sentence(succeeded: bool) -> &'static str {
   match succeeded {
      true => "Balances synced.",
      false => "Balance sync failed.",
   }
}

#[cfg(test)]
impl DiscoverChildWallets {
   /// Open the flow on `discovered_wallets` without the worker its own `open` runs, for a tree
   /// test that has no vault behind it.
   pub(crate) fn for_test(&mut self, discovered_wallets: DiscoveredWallets) {
      self.open = true;
      self.loading = false;
      self.current_page = 0;
      self.set_discovered_wallets(discovered_wallets);
   }

   /// Turn the page the way the `Next` button does.
   pub(crate) fn next_page_for_test(&mut self) {
      self.current_page += 1;
   }

   /// Show a balance sync in flight, the state the worker leaves it in while it runs.
   pub(crate) fn set_syncing_for_test(&mut self) {
      self.syncing = true;
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

#[cfg(test)]
mod tests {
   use super::*;

   /// The count says which wallets of the list the page holds, and that the list has none.
   #[test]
   fn the_count_says_which_wallets_the_page_holds() {
      assert_eq!(count_sentence(0, 0, 0), "No wallets found.");
      assert_eq!(count_sentence(0, 3, 3), "Wallets 1 to 3 of 3.");
      assert_eq!(
         count_sentence(0, 10, 43),
         "Wallets 1 to 10 of 43."
      );
      assert_eq!(
         count_sentence(40, 43, 43),
         "Wallets 41 to 43 of 43."
      );
   }

   /// The end of a balance sync is a sentence, not the spinner disappearing.
   #[test]
   fn a_finished_balance_sync_says_how_it_ended() {
      assert_eq!(balance_sync_sentence(true), "Balances synced.");
      assert_eq!(
         balance_sync_sentence(false),
         "Balance sync failed."
      );
   }
}
