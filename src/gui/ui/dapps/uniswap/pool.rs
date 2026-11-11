use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use egui::{Align, FontId, Grid, Id, Layout, Margin, Order, RichText, Sense, Ui, vec2};

use crate::assets::icons::Icons;
use crate::core::context::pool_manager::{
   CatalogLoadReport, V4CleanupConfig, V4CleanupReport, V4SyncConfig, V4SyncReport,
};
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::truncate_symbol_or_name;
use egui_elements::{
   Button, ChipRole, ComboBox, Label, Modal, SecureTextEdit, Spinner, Theme, Toast,
   layout::{Column, Table, TableScroll},
};
use zeus_eth::{
   alloy_primitives::Address,
   amm::uniswap::{
      AnyUniswapPool, DexKind, FEE_TIERS, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      UniswapV4Pool, v4::DYNAMIC_FEE_FLAG,
   },
   currency::{Currency, ERC20Token},
   utils::batch,
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Version {
   All,
   V2,
   V3,
   V4,
}

impl Version {
   fn all() -> Vec<Self> {
      vec![Self::All, Self::V2, Self::V3, Self::V4]
   }

   fn add_options() -> Vec<Self> {
      vec![Self::V2, Self::V3, Self::V4]
   }

   fn is_all(&self) -> bool {
      matches!(self, Self::All)
   }

   fn is_v2(&self) -> bool {
      matches!(self, Self::V2)
   }

   fn is_v3(&self) -> bool {
      matches!(self, Self::V3)
   }

   fn is_v4(&self) -> bool {
      matches!(self, Self::V4)
   }

   fn to_str(self) -> &'static str {
      match self {
         Self::All => "All",
         Self::V2 => "V2",
         Self::V3 => "V3",
         Self::V4 => "V4",
      }
   }
}

struct PoolRow {
   token0: String,
   token1: String,
   icon0: egui::Image<'static>,
   icon1: egui::Image<'static>,
   protocol: &'static str,
   fee: String,
}

/// What the add-pool form asks the background task to build.
#[derive(Clone, Copy)]
enum AddPoolRequest {
   V2 {
      token_a: Address,
      token_b: Address,
   },
   V3 {
      token_a: Address,
      token_b: Address,
      fee: u32,
   },
   V4 {
      token_a: Address,
      token_b: Address,
      fee: u32,
      tick_spacing: i32,
      hooks: Address,
   },
}

struct AddPoolForm {
   open: bool,
   version: Version,
   token_a: String,
   token_b: String,
   fee: u32,
   v4_custom_fee: bool,
   custom_fee: String,
   tick_spacing: String,
   hooks: String,
   adding: bool,
   error: Option<String>,
}

impl Default for AddPoolForm {
   fn default() -> Self {
      Self {
         open: false,
         version: Version::V3,
         token_a: String::new(),
         token_b: String::new(),
         fee: FeeAmount::MEDIUM.fee(),
         v4_custom_fee: false,
         custom_fee: String::new(),
         tick_spacing: String::new(),
         hooks: String::new(),
         adding: false,
         error: None,
      }
   }
}

impl AddPoolForm {
   fn open(&mut self) {
      *self = Self {
         open: true,
         ..Self::default()
      };
   }

   fn reset(&mut self) {
      self.token_a.clear();
      self.token_b.clear();
      self.custom_fee.clear();
      self.v4_custom_fee = false;
      self.tick_spacing.clear();
      self.hooks.clear();
      self.error = None;
      self.adding = false;
   }

   /// The fee for a V4 pool: the selected standard tier, or the custom value.
   fn v4_fee(&self) -> Result<u32, String> {
      if !self.v4_custom_fee {
         return Ok(self.fee);
      }

      let fee = self
         .custom_fee
         .trim()
         .parse::<u32>()
         .map_err(|_| "Custom fee must be a whole number".to_string())?;

      // A static fee of 0 is legal in V4: `LPFeeLibrary.isValid` only bounds the fee
      // above, so a zero-fee pool must stay addable by hand.
      if fee > 0x00FF_FFFF {
         return Err("Custom fee must be between 0 and 16777215".to_string());
      }

      Ok(fee)
   }

   fn request(&self) -> Result<AddPoolRequest, String> {
      if self.version == Version::All {
         return Err("Choose a pool version".to_string());
      }

      let token_a = parse_token_address(&self.token_a, "token A address")?;
      let token_b = parse_token_address(&self.token_b, "token B address")?;

      if token_a == token_b {
         return Err("Token A and Token B must be different".to_string());
      }

      match self.version {
         Version::V2 => Ok(AddPoolRequest::V2 { token_a, token_b }),
         Version::V3 => Ok(AddPoolRequest::V3 {
            token_a,
            token_b,
            fee: self.fee,
         }),
         Version::V4 => {
            let fee = self.v4_fee()?;

            let tick_spacing = match self.tick_spacing.trim() {
               "" => 0,
               text => text
                  .parse::<i32>()
                  .map_err(|_| "Tick spacing must be a whole number".to_string())?,
            };

            if fee == DYNAMIC_FEE_FLAG && tick_spacing == 0 {
               return Err("Dynamic fee pools need an explicit tick spacing".to_string());
            }

            let hooks = match self.hooks.trim() {
               "" => Address::ZERO,
               text => parse_address(text, "hooks address")?,
            };

            Ok(AddPoolRequest::V4 {
               token_a,
               token_b,
               fee,
               tick_spacing,
               hooks,
            })
         }
         Version::All => unreachable!(),
      }
   }
}

pub struct PoolsUi {
   pub open: bool,
   pub search_query: String,
   version: Option<Version>,
   add_form: AddPoolForm,
   pub size: (f32, f32),
   /// Chains with a V4 log sync running, keyed by chain.
   ///
   /// Per chain on purpose: a run takes minutes, and switching chain while it works must not show
   /// the other chain as busy (or let a second run start on the same one).
   v4_syncing: HashMap<u64, bool>,

   /// Chains with a V4 cleanup running, keyed by chain.
   ///
   /// Keyed per chain like the sync flag: a run deletes rows for one chain and switching chain must
   /// not show the other one as busy.
   v4_cleaning: HashMap<u64, bool>,

   /// Chains with a catalog load running, keyed by chain.
   v4_loading: HashMap<u64, bool>,

   /// Chains whose running sync is a resync, keyed by chain: the label says which one it is.
   v4_resyncing: HashMap<u64, bool>,
}

impl PoolsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         search_query: String::new(),
         version: None,
         add_form: AddPoolForm::default(),
         size: (600.0, 500.0),
         v4_syncing: HashMap::new(),
         v4_cleaning: HashMap::new(),
         v4_loading: HashMap::new(),
         v4_resyncing: HashMap::new(),
      }
   }

   fn select_version(&mut self, theme: &Theme, ui: &mut Ui) {
      if let Some(version) = version_combo(
         theme,
         ui,
         "pool_explore_combobox",
         self.version,
         &Version::all(),
      ) {
         self.version = Some(version);
      }
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let chain = ctx.chain.id();

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
         ui.spacing_mut().button_padding = theme.button_padding;
         let ui_width = ui.available_width();

         // The version filter shares the first line with the search, so the second line has room for
         // the three run buttons without pushing either row past the card edge.
         ui.allocate_ui_with_layout(
            vec2(ui_width, 36.0),
            Layout::left_to_right(Align::Center),
            |ui| {
               self.select_version(theme, ui);
               ui.add_space(theme.spacing.sm);

               let hint = RichText::new("Search pools")
                  .color(theme.colors.text_muted)
                  .size(theme.typography.normal);

               SecureTextEdit::singleline(&mut self.search_query)
                  .visuals(theme.text_edit_visuals())
                  .hint_text(hint)
                  .desired_width(ui.available_width())
                  .margin(Margin::same(10))
                  .font(FontId::proportional(theme.typography.normal))
                  .show(ui);
            },
         );

         // The span lives in the catalog (the settings modal writes it), so the tooltip states what
         // the run will actually use.
         let block_range = ctx.pool_manager.v4_sync_block_range();

         // All five actions on one line: the card is 600 wide and the buttons come to ~577 at the
         // tighter gap, so the catalog's own accounting gets the row under them.
         ui.allocate_ui_with_layout(
            vec2(ui_width, 36.0),
            Layout::left_to_right(Align::Center),
            |ui| {
               let text = RichText::new("Add Pool").size(theme.typography.normal);
               let button =
                  Button::new(text).visuals(theme.button_visuals()).label_align(Align::Center);
               if ui.add(button).clicked() {
                  self.add_form.open();
               }

               ui.add_space(theme.spacing.xs);

               // The sync walks history from the stored checkpoint: minutes of RPC, one at a time,
               // and the label says so while it runs. Every run in this window touches the same
               // catalog, so all four disable each other.
               let syncing = self.is_v4_syncing(chain);
               let cleaning = self.is_v4_cleaning(chain);
               let loading = self.is_v4_loading(chain);
               let busy = syncing || cleaning || loading;
               let label = if syncing {
                  "Syncing..."
               } else {
                  "Sync V4 Pools"
               };
               let text = RichText::new(label).size(theme.typography.normal);
               let button =
                  Button::new(text).visuals(theme.button_visuals()).label_align(Align::Center);

               let clicked = ui
                  .add_enabled_ui(!busy, |ui| {
                     ui.add(button).on_hover_text(format!(
                        "Resumes from the last synced block, {block_range} blocks per log request."
                     ))
                  })
                  .inner
                  .clicked();

               if clicked {
                  self.start_v4_sync(ctx, chain, ui.ctx());
               }

               ui.add_space(theme.spacing.xs);

               // The way back from an over-eager cleanup: dropping the checkpoint makes the run walk
               // the whole history again, so a pool the catalog no longer holds is found again.
               let label = if self.is_v4_resyncing(chain) {
                  "Resyncing..."
               } else {
                  "Resync"
               };
               let text = RichText::new(label).size(theme.typography.normal);
               let button =
                  Button::new(text).visuals(theme.button_visuals()).label_align(Align::Center);

               let clicked = ui
                  .add_enabled_ui(!busy, |ui| {
                     ui.add(button).on_hover_text(
                        "Forgets the last synced block and scans this chain's V4 pools from the start, so pools an earlier cleanup deleted are discovered again. Takes as long as the first sync.",
                     )
                  })
                  .inner
                  .clicked();

               if clicked {
                  self.start_v4_resync(ctx, chain, ui.ctx());
               }

               ui.add_space(theme.spacing.xs);

               let label = if cleaning {
                  "Cleaning up..."
               } else {
                  "Cleanup Pools"
               };
               let text = RichText::new(label).size(theme.typography.normal);
               let button =
                  Button::new(text).visuals(theme.button_visuals()).label_align(Align::Center);

               let clicked = ui
                  .add_enabled_ui(!busy, |ui| {
                     ui.add(button).on_hover_text(format!(
                        "Permanently deletes V4 pools above {}% fee, above {}% when one side is ETH, WETH, USDC, USDT or DAI, pools with no base asset, pools with no in-range liquidity, and pools holding less than the minimum liquidity.",
                        ctx.pool_manager.v4_max_fee_percent(),
                        ctx.pool_manager.v4_max_base_fee_percent(),
                     ))
                  })
                  .inner
                  .clicked();

               if clicked {
                  self.start_v4_cleanup(ctx, chain);
               }

               ui.add_space(theme.spacing.xs);

               // The catalog's cold rows are the pools nobody can see yet. Reading them resolves the
               // tokens they hold, which is how a dead pair gets judged by eye instead of by rule.
               let label = if loading {
                  "Loading..."
               } else {
                  "Load All Pools"
               };
               let text = RichText::new(label).size(theme.typography.normal);
               let button =
                  Button::new(text).visuals(theme.button_visuals()).label_align(Align::Center);

               let clicked = ui
                  .add_enabled_ui(!busy, |ui| {
                     ui.add(button).on_hover_text(
                        "Reads every pool in this chain's catalog into memory, fetching the tokens it does not know yet so they can be inspected. The fetched tokens are not saved to the wallet.",
                     )
                  })
                  .inner
                  .clicked();

               if clicked {
                  self.start_load_catalog(ctx, chain);
               }
            },
         );

         // The accounting alone on its line now that the actions share the one above; the width is
         // pinned so it stays flush left instead of centring with its own text.
         ui.horizontal(|ui| {
            ui.set_width(ui_width);
            self.v4_sync_status(&*ctx, theme, ui);
         });

         ui.add_space(20.0);

         let selected_version = self.version.unwrap_or(Version::All);
         let tint = theme.image_tint_recommended;
         let query = self.search_query.as_str();
         let manager = ctx.pool_manager.clone();

         let rows: Vec<PoolRow> = manager.read(|manager| {
            let mut rows: Vec<PoolRow> = manager
               .pools
               .values()
               .filter(|pool| pool.chain_id() == chain)
               .filter(|pool| {
                  selected_version.is_all()
                     || selected_version.is_v2() && pool.dex_kind().is_v2()
                     || selected_version.is_v3() && pool.dex_kind().is_v3()
                     || selected_version.is_v4() && pool.dex_kind().is_v4()
               })
               .filter(|pool| valid_search(pool, query))
               .map(|pool| {
                  let token0 = pool.currency0();
                  let token1 = pool.currency1();

                  PoolRow {
                     token0: truncate_symbol_or_name(token0.symbol(), 10),
                     token1: truncate_symbol_or_name(token1.symbol(), 10),
                     icon0: icons.currency_icon_x32(token0, tint),
                     icon1: icons.currency_icon_x32(token1, tint),
                     protocol: protocol_str(pool),
                     fee: fee_label(pool.fee().fee()),
                  }
               })
               .collect();

            rows.sort_by(|a, b| a.token0.cmp(&b.token0).then_with(|| a.token1.cmp(&b.token1)));

            rows
         });

         if rows.is_empty() {
            ui.add_space(theme.spacing.lg);
            ui.label(
               RichText::new("No pools match this filter.")
                  .size(theme.typography.normal)
                  .color(theme.colors.text_muted),
            );
            return;
         }

         let table_height = (ui.available_height() - theme.spacing.sm).max(140.0);

         Table::new("pools_ui", &rows)
            .columns([
               Column::Fr(3.2), // Pool
               Column::Fr(1.0), // Protocol
               Column::Fr(1.0), // Fee
            ])
            .gap(20.0)
            .row_gap(theme.spacing.lg + theme.spacing.xs)
            .scroll(
               TableScroll::new()
                  .auto_shrink([false; 2])
                  .content_margin(15)
                  .max_height(table_height),
            )
            .row_divider(true)
            .header_rule(true)
            .key((chain, selected_version.to_str(), rows.len()))
            .show(ui, |t| {
               t.header(|h| {
                  for text in ["Pool", "Protocol", "Fee"] {
                     h.cell(|ui| {
                        ui.label(
                           RichText::new(text)
                              .strong()
                              .size(theme.typography.large)
                              .color(theme.colors.text),
                        );
                     });
                  }
               });

               t.rows(|r, row| {
                  r.cell(|ui| {
                     ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing.xs;

                        let label0 = Label::new(
                           RichText::new(&row.token0).size(theme.typography.normal),
                           Some(row.icon0.clone()),
                        )
                        .image_on_left()
                        .interactive(false);

                        let label1 = Label::new(
                           RichText::new(&row.token1).size(theme.typography.normal),
                           Some(row.icon1.clone()),
                        )
                        .image_on_left()
                        .interactive(false);

                        ui.add(label0);
                        ui.add(
                           Label::new(
                              RichText::new("/").size(theme.typography.normal),
                              None,
                           )
                           .interactive(false),
                        );
                        ui.add(label1);
                     });
                  });

                  r.cell(|ui| {
                     ui.label(RichText::new(row.protocol).size(theme.typography.normal));
                  });

                  r.cell(|ui| {
                     ui.label(RichText::new(&row.fee).size(theme.typography.normal));
                  });
               });
            });
      });

      self.add_pool_modal(chain, theme, ui);
   }

   fn add_pool_modal(&mut self, chain: u64, theme: &Theme, ui: &mut Ui) {
      if !self.add_form.open {
         return;
      }

      let mut open = self.add_form.open;
      let adding = self.add_form.adding;
      let form = &mut self.add_form;
      let mut submit: Option<Result<AddPoolRequest, String>> = None;

      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new("Add Pool").size(theme.typography.heading);
      let id = Id::unique("add_pool_modal");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(!adding)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
            ui.spacing_mut().button_padding = theme.button_padding;

            ui.horizontal(|ui| {
               ui.label(RichText::new("Pool version").size(theme.typography.large));

               if let Some(version) = version_combo(
                  theme,
                  ui,
                  "add_pool_version_combo",
                  Some(form.version),
                  &Version::add_options(),
               ) {
                  form.version = version;
               }
            });

            let field_width = 320.0;

            Grid::new("add_pool_fields")
               .num_columns(2)
               .spacing(vec2(theme.spacing.md, theme.spacing.sm))
               .show(ui, |ui| {
                  ui.label(RichText::new("Token A").size(theme.typography.large));
                  form_text_field(
                     theme,
                     ui,
                     &mut form.token_a,
                     "0x… (empty = native ETH)",
                     field_width,
                  );
                  ui.end_row();

                  ui.label(RichText::new("Token B").size(theme.typography.large));
                  form_text_field(
                     theme,
                     ui,
                     &mut form.token_b,
                     "0x… (empty = native ETH)",
                     field_width,
                  );
                  ui.end_row();

                  if !form.version.is_v2() {
                     let allow_custom = form.version.is_v4();

                     ui.label(RichText::new("Fee").size(theme.typography.large));
                     fee_combo(
                        theme,
                        ui,
                        &mut form.fee,
                        &mut form.v4_custom_fee,
                        &mut form.custom_fee,
                        allow_custom,
                     );
                     ui.end_row();

                     if allow_custom && form.v4_custom_fee {
                        ui.label(RichText::new("Custom fee").size(theme.typography.large));
                        form_text_field(
                           theme,
                           ui,
                           &mut form.custom_fee,
                           "Hundredths of a bip, e.g. 2500 = 0.25%, 0 = no fee",
                           field_width,
                        );
                        ui.end_row();
                     }
                  }

                  if form.version.is_v4() {
                     ui.label(RichText::new("Tick spacing").size(theme.typography.large));
                     form_text_field(
                        theme,
                        ui,
                        &mut form.tick_spacing,
                        "Optional, defaults to the fee tier",
                        field_width,
                     );
                     ui.end_row();

                     ui.label(RichText::new("Hooks").size(theme.typography.large));
                     form_text_field(
                        theme,
                        ui,
                        &mut form.hooks,
                        "Optional, defaults to no hooks",
                        field_width,
                     );
                     ui.end_row();
                  }
               });

            let note = match (form.version.is_v4(), form.v4_custom_fee) {
               (true, true) => {
                  "Custom fees are hundredths of a bip: 2500 = 0.25%, 0 = no fee, 8388608 = dynamic fee. Dynamic pools need their on-chain tick spacing."
               }
               (true, false) => {
                  "Leave a token field empty (or use the zero address) for native ETH. The pool is validated on chain before it is added."
               }
               (false, _) => "The pair is looked up on chain before it is added.",
            };

            ui.label(
               RichText::new(note)
                  .size(theme.typography.normal)
                  .color(theme.colors.text_muted),
            );

            if let Some(error) = &form.error {
               ui.label(
                  RichText::new(error)
                     .size(theme.typography.normal)
                     .color(theme.colors.error),
               );
            }

            ui.add_space(theme.spacing.sm);

            if adding {
               ui.vertical_centered(|ui| {
                  ui.add(Spinner::new().size(20.0).color(theme.colors.text));
                  ui.label(
                     RichText::new("Looking up the pool on chain…")
                        .size(theme.typography.normal)
                        .color(theme.colors.text_muted),
                  );
               });
            } else {
               let text = RichText::new("Add Pool").size(theme.typography.large);
               let button = Button::new(text)
                  .visuals(theme.button_visuals())
                  .min_size(vec2(field_width, 42.0))
                  .label_align(Align::Center);

               if ui.add(button).clicked() {
                  submit = Some(form.request());
               }
            }
         });

      self.add_form.open = open;

      if let Some(result) = submit {
         match result {
            Ok(request) => self.start_add(chain, request),
            Err(error) => self.add_form.error = Some(error),
         }
      }
   }

   /// Whether a V4 sync or resync is running on `chain`.
   fn is_v4_syncing(&self, chain: u64) -> bool {
      self.v4_syncing.get(&chain).copied().unwrap_or(false)
   }

   /// Whether the running V4 run is a resync rather than a checkpointed sync.
   fn is_v4_resyncing(&self, chain: u64) -> bool {
      self.v4_resyncing.get(&chain).copied().unwrap_or(false)
   }

   /// The archive check both V4 log runs share.
   ///
   /// Deliberate: they read historic log ranges, so without an archive endpoint they would fail
   /// inside the first step instead of explaining themselves here.
   fn v4_archive_ready(&self, ctx: &ZeusContext, chain: u64, egui_ctx: &egui::Context) -> bool {
      if ctx.client_manager.rpc_archive_available(chain) {
         return true;
      }

      Toast::new("No archive RPC available")
         .description(format!(
            "Syncing V4 pools reads past logs, so it needs an archive endpoint for chain {chain}."
         ))
         .tone(ChipRole::Error)
         .duration(Duration::from_secs(6))
         .show(egui_ctx);

      false
   }

   fn start_v4_sync(&mut self, ctx: &mut ZeusContext, chain: u64, egui_ctx: &egui::Context) {
      if self.is_v4_syncing(chain) {
         return;
      }

      if !self.v4_archive_ready(ctx, chain, egui_ctx) {
         return;
      }

      self.v4_syncing.insert(chain, true);

      // The top panel reads this flag as "this chain is syncing", and the wallet's own state sync
      // treats it as a lock. Claim it only when nobody else holds it, so a run of ours can never
      // clear the flag of a state sync that started first.
      let claimed = !ctx.state_sync.get(&chain).copied().unwrap_or(false);
      if claimed {
         ctx.state_sync.insert(chain, true);
      }

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let result = ctx
            .pool_manager()
            .sync_v4_pools(ctx.clone(), chain, V4SyncConfig::default())
            .await;

         if claimed {
            ctx.set_chain_syncing(chain, false);
         }

         SHARED_GUI.write(|gui| {
            let (title, message, tone) = gui.uniswap.pools_ui.finish_v4_sync(chain, false, result);

            Toast::new(title)
               .description(message)
               .tone(tone)
               .duration(Duration::from_secs(6))
               .show(&gui.egui_ctx);

            gui.request_repaint();
         });
      });
   }

   /// Re-sync `chain` from its V4 creation block, so pools an earlier cleanup deleted can come back.
   ///
   /// The checkpoint is dropped before the scan, which is what makes this "from scratch": every pool
   /// the chain ever emitted is discovered again and judged by the current cleanup rules.
   fn start_v4_resync(&mut self, ctx: &mut ZeusContext, chain: u64, egui_ctx: &egui::Context) {
      if self.is_v4_syncing(chain) || self.is_v4_cleaning(chain) || self.is_v4_loading(chain) {
         return;
      }

      if !self.v4_archive_ready(ctx, chain, egui_ctx) {
         return;
      }

      self.v4_syncing.insert(chain, true);
      self.v4_resyncing.insert(chain, true);

      // The same indicator and the same lock as a sync: this is a sync that starts further back.
      let claimed = !ctx.state_sync.get(&chain).copied().unwrap_or(false);
      if claimed {
         ctx.state_sync.insert(chain, true);
      }

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let result = ctx
            .pool_manager()
            .resync_v4_pools(ctx.clone(), chain, V4SyncConfig::default())
            .await;

         if claimed {
            ctx.set_chain_syncing(chain, false);
         }

         SHARED_GUI.write(|gui| {
            let (title, message, tone) = gui.uniswap.pools_ui.finish_v4_sync(chain, true, result);

            Toast::new(title)
               .description(message)
               .tone(tone)
               .duration(Duration::from_secs(6))
               .show(&gui.egui_ctx);

            gui.request_repaint();
         });
      });
   }

   fn finish_v4_sync(
      &mut self,
      chain: u64,
      resync: bool,
      result: Result<V4SyncReport, anyhow::Error>,
   ) -> (&'static str, String, ChipRole) {
      self.v4_syncing.insert(chain, false);
      self.v4_resyncing.insert(chain, false);

      let report = match result {
         Ok(report) => report,
         Err(e) => {
            let title = if resync {
               "V4 resync failed"
            } else {
               "V4 sync failed"
            };

            return (title, format!("{e:?}"), ChipRole::Error);
         }
      };

      let mut message = if resync {
         format!(
            "Scanned from block {} to {}: {} new pools ({} hot, {} cold)",
            report.from_block,
            report.to_block,
            report.hot + report.cold,
            report.hot,
            report.cold
         )
      } else {
         format!(
            "{} new pools ({} hot, {} cold) up to block {}",
            report.hot + report.cold,
            report.hot,
            report.cold,
            report.to_block
         )
      };

      // The cleanup runs inside the sync, so its result belongs in this toast: a pool that vanished
      // is explained here, not in the log.
      if let Some(error) = &report.cleanup_error {
         return (
            "V4 cleanup failed",
            format!("{message}; {error}"),
            ChipRole::Error,
         );
      }

      if let Some(cleanup) = &report.cleanup {
         message.push_str(&format!(
            "; {} of {} deleted ({} high fee, {} base-asset fee, {} no base asset, {} dead, {} illiquid), {} unjudged",
            cleanup.removed(),
            cleanup.scanned,
            cleanup.removed_by_fee,
            cleanup.removed_by_base_fee,
            cleanup.removed_no_base,
            cleanup.removed_dead,
            cleanup.removed_illiquid,
            cleanup.unjudged(),
         ));
      }

      let title = if resync {
         "V4 pools resynced"
      } else {
         "V4 pools synced"
      };

      (title, message, ChipRole::Success)
   }

   fn is_v4_cleaning(&self, chain: u64) -> bool {
      self.v4_cleaning.get(&chain).copied().unwrap_or(false)
   }

   /// Delete this chain's dead V4 pools, or say why the run cannot start.
   ///
   /// No archive check: the fee rules read the catalog and the liquidity rule reads current state, so
   /// the historic endpoint a sync needs is not required here.
   fn start_v4_cleanup(&mut self, ctx: &mut ZeusContext, chain: u64) {
      if self.is_v4_cleaning(chain) || self.is_v4_syncing(chain) {
         return;
      }

      self.v4_cleaning.insert(chain, true);

      // Same indicator as the sync: the top panel reads it as "this chain is busy". Claim it only
      // when nobody else holds it, so this run can never clear another run's flag.
      let claimed = !ctx.state_sync.get(&chain).copied().unwrap_or(false);
      if claimed {
         ctx.state_sync.insert(chain, true);
      }

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let result = ctx
            .pool_manager()
            .cleanup_v4_pools(ctx.clone(), chain, V4CleanupConfig::default())
            .await;

         if claimed {
            ctx.set_chain_syncing(chain, false);
         }

         SHARED_GUI.write(|gui| {
            let (title, message, tone) = gui.uniswap.pools_ui.finish_v4_cleanup(chain, result);

            Toast::new(title)
               .description(message)
               .tone(tone)
               .duration(Duration::from_secs(6))
               .show(&gui.egui_ctx);

            gui.request_repaint();
         });
      });
   }

   fn finish_v4_cleanup(
      &mut self,
      chain: u64,
      result: Result<V4CleanupReport, anyhow::Error>,
   ) -> (&'static str, String, ChipRole) {
      self.v4_cleaning.insert(chain, false);

      match result {
         Ok(report) => (
            "Pools cleaned",
            format!(
               "{} of {} V4 pools deleted ({} high fee, {} base-asset fee, {} no base asset, {} dead, {} illiquid), {} unjudged",
               report.removed(),
               report.scanned,
               report.removed_by_fee,
               report.removed_by_base_fee,
               report.removed_no_base,
               report.removed_dead,
               report.removed_illiquid,
               report.unjudged(),
            ),
            ChipRole::Success,
         ),
         Err(e) => (
            "Cleanup failed",
            format!("{e:?}"),
            ChipRole::Error,
         ),
      }
   }

   fn is_v4_loading(&self, chain: u64) -> bool {
      self.v4_loading.get(&chain).copied().unwrap_or(false)
   }

   /// Read this chain's cold catalog rows into memory, fetching the tokens they hold.
   fn start_load_catalog(&mut self, ctx: &mut ZeusContext, chain: u64) {
      if self.is_v4_loading(chain) || self.is_v4_syncing(chain) || self.is_v4_cleaning(chain) {
         return;
      }

      self.v4_loading.insert(chain, true);

      // Same indicator as the other two runs: the top panel reads it as "this chain is busy".
      let claimed = !ctx.state_sync.get(&chain).copied().unwrap_or(false);
      if claimed {
         ctx.state_sync.insert(chain, true);
      }

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let result = ctx.pool_manager().load_catalog_pools(ctx.clone(), chain).await;

         if claimed {
            ctx.set_chain_syncing(chain, false);
         }

         SHARED_GUI.write(|gui| {
            let (title, message, tone) = gui.uniswap.pools_ui.finish_load_catalog(chain, result);

            Toast::new(title)
               .description(message)
               .tone(tone)
               .duration(Duration::from_secs(6))
               .show(&gui.egui_ctx);

            gui.request_repaint();
         });
      });
   }

   fn finish_load_catalog(
      &mut self,
      chain: u64,
      result: Result<CatalogLoadReport, anyhow::Error>,
   ) -> (&'static str, String, ChipRole) {
      self.v4_loading.insert(chain, false);

      match result {
         Ok(report) => (
            "Pools loaded",
            format!(
               "{} cold pools now in memory ({} tokens fetched, {} unreadable)",
               report.loaded, report.tokens, report.failed
            ),
            ChipRole::Success,
         ),
         Err(e) => (
            "Loading failed",
            format!("{e:?}"),
            ChipRole::Error,
         ),
      }
   }

   /// Where the last V4 sync stopped, linked to that block on the chain's explorer, then what the
   /// catalog holds underneath it.
   ///
   /// The checkpoint is what the next run resumes from, so showing it is the only way to tell
   /// "already synced" from "never ran". The three lines below are the catalog's own accounting: a
   /// cold record is a 68-byte row on disk with an id in the index, a hot pool is a live struct.
   fn v4_sync_status(&self, ctx: &ZeusContext, theme: &Theme, ui: &mut Ui) {
      let chain = ctx.chain.id();
      let last_sync = ctx.pool_manager.read(|manager| {
         manager
            .checkpoints
            .get(&(chain, DexKind::UniswapV4))
            .map(|checkpoint| checkpoint.block)
      });
      let stats = ctx.pool_manager.v4_catalog_stats(chain);

      ui.vertical(|ui| {
         ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, 2.0);

         ui.horizontal(|ui| {
            ui.label(
               RichText::new("Last synced block:")
                  .size(theme.typography.small)
                  .color(theme.colors.text_muted),
            );

            match last_sync {
               Some(block) => {
                  // `block_explorer` carries a trailing slash for some chains.
                  let explorer = ctx.chain.block_explorer().trim_end_matches('/');
                  ui.hyperlink_to(
                     RichText::new(block.to_string())
                        .size(theme.typography.small)
                        .color(theme.colors.info),
                     format!("{explorer}/block/{block}"),
                  );
               }
               None => {
                  ui.label(
                     RichText::new("never")
                        .size(theme.typography.small)
                        .color(theme.colors.text_muted),
                  );
               }
            }
         });

         for text in [
            format!("{} V4 pools in the db", stats.total),
            format!(
               "{} cold ({} in memory)",
               stats.cold,
               human_bytes(stats.cold_bytes)
            ),
            format!(
               "{} hot ({} in memory)",
               stats.hot,
               human_bytes(stats.hot_bytes)
            ),
         ] {
            ui.label(
               RichText::new(text).size(theme.typography.small).color(theme.colors.text_muted),
            );
         }
      });
   }

   fn start_add(&mut self, chain: u64, request: AddPoolRequest) {
      self.add_form.adding = true;
      self.add_form.error = None;

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let result = insert_pool(ctx, chain, request).await;

         SHARED_GUI.write(|gui| {
            let toast = gui.uniswap.pools_ui.finish_add(result);

            if let Some(message) = toast {
               Toast::new("Pool added")
                  .description(message)
                  .tone(ChipRole::Success)
                  .duration(Duration::from_secs(5))
                  .show(&gui.egui_ctx);
            }

            gui.request_repaint();
         });
      });
   }

   fn finish_add(&mut self, result: Result<String, String>) -> Option<String> {
      self.add_form.adding = false;

      match result {
         Ok(message) => {
            self.add_form.open = false;
            self.add_form.reset();
            Some(message)
         }
         Err(error) => {
            self.add_form.error = Some(error);
            None
         }
      }
   }
}

/// A byte count the pools window can show: whole bytes, then one decimal above a kilobyte.
fn human_bytes(bytes: usize) -> String {
   const KB: f64 = 1024.0;
   let bytes = bytes as f64;

   if bytes < KB {
      format!("{bytes:.0} B")
   } else if bytes < KB * KB {
      format!("{:.1} KB", bytes / KB)
   } else {
      format!("{:.1} MB", bytes / (KB * KB))
   }
}

fn version_combo(
   theme: &Theme,
   ui: &mut Ui,
   id: &str,
   current: Option<Version>,
   versions: &[Version],
) -> Option<Version> {
   let selected_text = current.map(|v| v.to_str()).unwrap_or("Select Version");
   let selected_text = RichText::new(selected_text).size(theme.typography.normal);
   let selected_label = Label::new(selected_text, None).sense(Sense::click()).interactive(true);

   let mut selected = None;

   ComboBox::new(id, selected_label).width(200.0).show_ui(ui, |ui| {
      for version in versions {
         let text = RichText::new(version.to_str()).size(theme.typography.normal);
         let label = Label::new(text, None)
            .expand(Some(4.0))
            .fill_width(true)
            .visuals(theme.label_visuals())
            .sense(Sense::click())
            .interactive(true);

         if ui.add(label).clicked() {
            selected = Some(*version);
         }
      }
   });

   selected
}

fn form_text_field(theme: &Theme, ui: &mut Ui, value: &mut String, hint: &str, width: f32) {
   let hint = RichText::new(hint).color(theme.colors.text_muted).size(theme.typography.normal);
   let visuals = theme.text_edit_visuals();

   ui.allocate_ui(vec2(width, 40.0), |ui| {
      SecureTextEdit::singleline(value)
         .visuals(visuals)
         .hint_text(hint)
         .margin(Margin::same(10))
         .font(FontId::proportional(theme.typography.normal))
         .desired_width(ui.available_width())
         .show(ui);
   });
}

fn fee_combo(
   theme: &Theme,
   ui: &mut Ui,
   fee: &mut u32,
   custom: &mut bool,
   custom_fee: &mut String,
   allow_custom: bool,
) {
   let selected_text = if allow_custom && *custom {
      custom_fee_label(custom_fee)
   } else {
      fee_label(*fee)
   };
   let selected_text = RichText::new(selected_text).size(theme.typography.normal);
   let selected_label = Label::new(selected_text, None).sense(Sense::click()).interactive(true);

   ComboBox::new("add_pool_fee_combo", selected_label)
      .width(160.0)
      .show_ui(ui, |ui| {
         for tier in FEE_TIERS {
            let text = RichText::new(fee_label(tier)).size(theme.typography.normal);
            let label = Label::new(text, None)
               .expand(Some(4.0))
               .fill_width(true)
               .visuals(theme.label_visuals())
               .sense(Sense::click())
               .interactive(true);

            if ui.add(label).clicked() {
               *fee = tier;
               *custom = false;
            }
         }

         if allow_custom {
            let text = RichText::new("Custom").size(theme.typography.normal);
            let label = Label::new(text, None)
               .expand(Some(4.0))
               .fill_width(true)
               .visuals(theme.label_visuals())
               .sense(Sense::click())
               .interactive(true);

            if ui.add(label).clicked() {
               *custom = true;

               if custom_fee.trim().is_empty() {
                  *custom_fee = fee.to_string();
               }
            }
         }
      });
}

fn fee_label(fee: u32) -> String {
   if fee == DYNAMIC_FEE_FLAG {
      "Dynamic".to_string()
   } else {
      format!("{}%", FeeAmount::new(fee).fee_percent())
   }
}

fn custom_fee_label(text: &str) -> String {
   match text.trim().parse::<u32>() {
      Ok(fee) => format!("Custom ({})", fee_label(fee)),
      Err(_) => "Custom".to_string(),
   }
}

fn parse_address(text: &str, field: &str) -> Result<Address, String> {
   Address::from_str(text.trim()).map_err(|_| format!("Enter a valid {field}"))
}

/// Token fields treat an empty input as the native currency (the zero address).
fn parse_token_address(text: &str, field: &str) -> Result<Address, String> {
   if text.trim().is_empty() {
      Ok(Address::ZERO)
   } else {
      parse_address(text, field)
   }
}

fn protocol_str(pool: &AnyUniswapPool) -> &'static str {
   if pool.dex_kind().is_v2() {
      "V2"
   } else if pool.dex_kind().is_v3() {
      "V3"
   } else {
      "V4"
   }
}

async fn resolve_currency(ctx: &ZeusCtx, chain: u64, address: Address) -> Result<Currency, String> {
   if address.is_zero() {
      return Ok(Currency::native(chain));
   }

   let token = ctx.get_token(chain, address).await.map_err(|e| e.to_string())?;
   Ok(Currency::from(token))
}

/// Resolve both sides of a V2/V3 pair as ERC20 tokens.
///
/// Native ETH is wrapped, because V2/V3 pools hold WETH rather than the native asset.
async fn resolve_token_pair(
   ctx: &ZeusCtx,
   chain: u64,
   token_a: Address,
   token_b: Address,
) -> Result<(ERC20Token, ERC20Token), String> {
   let currency_a = resolve_currency(ctx, chain, token_a).await?;
   let currency_b = resolve_currency(ctx, chain, token_b).await?;
   let token_a = currency_a.to_erc20().into_owned();
   let token_b = currency_b.to_erc20().into_owned();

   if token_a.address == token_b.address {
      return Err("Token A and Token B must be different".to_string());
   }

   Ok((token_a, token_b))
}

async fn insert_pool(ctx: ZeusCtx, chain: u64, request: AddPoolRequest) -> Result<String, String> {
   let pool = match request {
      AddPoolRequest::V2 { token_a, token_b } => {
         let (token_a, token_b) = resolve_token_pair(&ctx, chain, token_a, token_b).await?;
         let client = ctx.get_client_manager();

         let pool = client
            .request(chain, |client| {
               let token_a = token_a.clone();
               let token_b = token_b.clone();

               async move {
                  UniswapV2Pool::from_components(
                     client,
                     chain,
                     token_a,
                     token_b,
                     DexKind::UniswapV2,
                  )
                  .await
               }
            })
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No V2 pool exists for this pair".to_string())?;

         AnyUniswapPool::from_pool(pool)
      }
      AddPoolRequest::V3 {
         token_a,
         token_b,
         fee,
      } => {
         let (token_a, token_b) = resolve_token_pair(&ctx, chain, token_a, token_b).await?;
         let client = ctx.get_client_manager();

         let pool = client
            .request(chain, |client| {
               let token_a = token_a.clone();
               let token_b = token_b.clone();

               async move {
                  UniswapV3Pool::from_components(
                     client,
                     chain,
                     fee,
                     token_a,
                     token_b,
                     DexKind::UniswapV3,
                  )
                  .await
               }
            })
            .await
            .map_err(|e| e.to_string())?;

         AnyUniswapPool::from_pool(pool)
      }
      AddPoolRequest::V4 {
         token_a,
         token_b,
         fee,
         tick_spacing,
         hooks,
      } => {
         let currency_a = resolve_currency(&ctx, chain, token_a).await?;
         let currency_b = resolve_currency(&ctx, chain, token_b).await?;

         if currency_a.address() == currency_b.address() {
            return Err("Token A and Token B must be different".to_string());
         }

         let fee = FeeAmount::new(fee);
         let tick_spacing = if tick_spacing == 0 {
            fee.tick_spacing_i32()
         } else {
            tick_spacing
         };

         if FeeAmount::i24_tick_spacing(tick_spacing).is_none() {
            return Err("Tick spacing is out of range".to_string());
         }

         let pool = UniswapV4Pool::from_components_with_spacing(
            chain,
            currency_a,
            currency_b,
            fee,
            tick_spacing,
            DexKind::UniswapV4,
            hooks,
         );

         let client = ctx.get_client_manager();
         let pool_id = pool.id();

         let valid = client
            .request(chain, |client| async move {
               batch::validate_v4_pools(client, chain, vec![pool_id]).await
            })
            .await
            .map_err(|e| e.to_string())?;

         if valid.is_empty() {
            return Err("The V4 pool is not initialized on chain".to_string());
         }

         AnyUniswapPool::V4(pool)
      }
   };

   let label = format!(
      "{}/{}",
      truncate_symbol_or_name(pool.currency0().symbol(), 10),
      truncate_symbol_or_name(pool.currency1().symbol(), 10)
   );

   let currencies = [pool.currency0().clone(), pool.currency1().clone()];
   let manager = ctx.pool_manager();

   manager
      .update_state_for_pools(ctx.clone(), chain, vec![pool])
      .await
      .map_err(|e| e.to_string())?;

   ctx.write(|ctx| {
      for currency in currencies {
         ctx.currency_db.insert_currency(chain, currency);
      }
   });

   ctx.save_pool_manager();

   Ok(format!("{label} added to the pool list"))
}

fn valid_search(pool: &AnyUniswapPool, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   if pool.currency0().symbol().to_lowercase().contains(&query) {
      return true;
   }

   if pool.currency1().symbol().to_lowercase().contains(&query) {
      return true;
   }

   if pool.dex_kind().is_v2() || pool.dex_kind().is_v3() {
      if pool.address().to_string().to_lowercase().contains(&query) {
         return true;
      }
   } else {
      if pool.id().to_string().to_lowercase().contains(&query) {
         return true;
      }
   };

   false
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn human_bytes_picks_a_unit() {
      assert_eq!(human_bytes(512), "512 B");
      assert_eq!(human_bytes(1536), "1.5 KB");
      assert_eq!(human_bytes(6 * 1024 * 1024), "6.0 MB");
   }

   const TOKEN_A: &str = "0x0000000000000000000000000000000000000001";
   const TOKEN_B: &str = "0x0000000000000000000000000000000000000002";

   fn form(version: Version) -> AddPoolForm {
      AddPoolForm {
         version,
         token_a: TOKEN_A.to_string(),
         token_b: TOKEN_B.to_string(),
         ..Default::default()
      }
   }

   #[test]
   fn add_pool_form_parses_v2_pair() {
      assert!(matches!(
         form(Version::V2).request(),
         Ok(AddPoolRequest::V2 { .. })
      ));
   }

   #[test]
   fn add_pool_form_parses_v4_pair() {
      assert!(matches!(
         form(Version::V4).request(),
         Ok(AddPoolRequest::V4 {
            tick_spacing: 0,
            hooks: Address::ZERO,
            ..
         })
      ));
   }

   #[test]
   fn add_pool_form_rejects_matching_tokens() {
      let mut form = form(Version::V3);
      form.token_b = TOKEN_A.to_string();
      assert!(form.request().is_err());
   }

   #[test]
   fn add_pool_form_treats_empty_token_as_native() {
      let mut form = form(Version::V4);
      form.token_a = String::new();

      assert!(matches!(
         form.request(),
         Ok(AddPoolRequest::V4 {
            token_a: Address::ZERO,
            ..
         })
      ));

      form.token_b = String::new();
      assert!(form.request().is_err());
   }

   #[test]
   fn add_pool_form_rejects_bad_tick_spacing() {
      let mut form = form(Version::V4);
      form.tick_spacing = "abc".to_string();
      assert!(form.request().is_err());
   }

   #[test]
   fn add_pool_form_parses_custom_v4_fee() {
      let mut form = form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = "2500".to_string();
      assert!(matches!(
         form.request(),
         Ok(AddPoolRequest::V4 { fee: 2500, .. })
      ));
   }

   #[test]
   fn add_pool_form_accepts_zero_v4_fee() {
      let mut form = form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = "0".to_string();
      form.tick_spacing = "60".to_string();
      assert!(matches!(
         form.request(),
         Ok(AddPoolRequest::V4 {
            fee: 0,
            tick_spacing: 60,
            ..
         })
      ));
   }

   #[test]
   fn add_pool_form_accepts_dynamic_v4_fee() {
      let mut form = form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = DYNAMIC_FEE_FLAG.to_string();
      form.tick_spacing = "60".to_string();
      assert!(matches!(
         form.request(),
         Ok(AddPoolRequest::V4 {
            fee: DYNAMIC_FEE_FLAG,
            ..
         })
      ));
   }

   #[test]
   fn add_pool_form_requires_spacing_for_dynamic_fee() {
      let mut form = form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = DYNAMIC_FEE_FLAG.to_string();
      assert!(form.request().is_err());

      form.tick_spacing = "60".to_string();
      assert!(form.request().is_ok());
   }

   #[test]
   fn add_pool_form_rejects_invalid_custom_fee() {
      for value in ["16777216", "abc"] {
         let mut form = form(Version::V4);
         form.v4_custom_fee = true;
         form.custom_fee = value.to_string();
         assert!(
            form.request().is_err(),
            "{value} should be rejected"
         );
      }
   }
}
