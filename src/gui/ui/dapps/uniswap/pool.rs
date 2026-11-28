use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use egui::{
   Align, FontId, Grid, Id, Layout, Margin, Order, RichText, Role, Sense, Ui, WidgetInfo, vec2,
};

use crate::assets::icons::Icons;
use crate::core::{PoolError, PoolManagerHandle, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::{currency_for_speech, name_alert, name_picker, name_text};
use crate::utils::truncate_symbol_or_name;
use egui_elements::{
   Button, ChipRole, ComboBox, Label, Modal, SecureTextEdit, Spinner, Theme, Toast,
   layout::{Column, Table, TableScroll},
};
use zeus_eth::{
   alloy_primitives::Address,
   amm::uniswap::{
      AnyUniswapPool, DexKind, FEE_TIERS, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      UniswapV4Pool,
   },
   currency::{Currency, ERC20Token},
   utils::batch,
};

/// Width of every pool-version combobox, shared by the layouts that budget for it.
const VERSION_COMBO_WIDTH: f32 = 200.0;

/// Width of the pools search field. A full-width bar dwarfs the filter next to it.
const SEARCH_FIELD_WIDTH: f32 = 200.0;

/// Height of a row that holds an input field, so everything on it centres on one line.
const FIELD_ROW_HEIGHT: f32 = 40.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Version {
   /// The table's default filter: every pool version.
   #[default]
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

/// Fingerprint of what the pool table was last built from, so a rebuild only runs on a change.
///
/// The pool labels (symbols, kind, fee) never change once a pool exists, so the manager's size is
/// what notices an added or dropped pool.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PoolRowsKey {
   chain: u64,
   version: Version,
   query: String,
   pools: usize,
}

/// Uniswap V4 `LPFeeLibrary.DYNAMIC_FEE_FLAG`: the hook supplies the fee.
const DYNAMIC_FEE_FLAG: u32 = 0x80_0000;

struct PoolRow {
   token0: String,
   token1: String,
   /// The pair said in the tokens' own names, which is what a reader hears over the two tickers
   /// the cell draws.
   pair_for_speech: String,
   icon0: egui::Image<'static>,
   icon1: egui::Image<'static>,
   protocol: &'static str,
   fee: String,
}

/// What the add-pool form asks the background task to build.
#[derive(Clone, Copy)]
pub(crate) enum AddPoolRequest {
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
   pub search_query: String,
   version: Option<Version>,
   add_form: AddPoolForm,
   pub size: (f32, f32),
   /// The table's rows, built off the GUI thread by [`Self::refresh_rows`].
   rows: Vec<PoolRow>,
   /// The key the rows were last built for; a build whose key no longer matches is dropped.
   rows_key: PoolRowsKey,
   /// True while a background row build is in flight.
   rows_loading: bool,
   /// The row count a reader was last told about, so a moved count is said once.
   rows_announced: Option<usize>,
   /// The sentence the window wants said, drained by it right after the page has drawn.
   notice: Option<String>,
}

impl PoolsUi {
   pub fn new() -> Self {
      Self {
         search_query: String::new(),
         version: None,
         add_form: AddPoolForm::default(),
         size: (600.0, 500.0),
         rows: Vec::new(),
         rows_key: PoolRowsKey::default(),
         rows_loading: false,
         rows_announced: None,
         notice: None,
      }
   }

   /// The words the window should say, drained by it right after the page has drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Open the Add Pool form the way its button does, for a test that draws the dialog.
   #[cfg(test)]
   pub fn open_add_pool_for_test(&mut self) {
      self.add_form.open();
   }

   /// Stand the table up with rows already built, keyed for the chain and pool count the page is
   /// about to be drawn with, so it draws them instead of asking the worker for a set that would
   /// land in the global `GUI` rather than in this one.
   #[cfg(test)]
   pub fn seed_rows_for_test(
      &mut self,
      chain: u64,
      pools: usize,
      rows: Vec<(Currency, Currency, &'static str, String)>,
      icons: &Icons,
      tint: bool,
   ) {
      self.rows = rows
         .into_iter()
         .map(|(token0, token1, protocol, fee)| PoolRow {
            token0: truncate_symbol_or_name(token0.symbol(), 10),
            token1: truncate_symbol_or_name(token1.symbol(), 10),
            pair_for_speech: format!(
               "{} / {}",
               currency_for_speech(&token0).name,
               currency_for_speech(&token1).name
            ),
            icon0: icons.currency_icon_x32(&token0, tint),
            icon1: icons.currency_icon_x32(&token1, tint),
            protocol,
            fee,
         })
         .collect();

      self.rows_key = PoolRowsKey {
         chain,
         version: self.version.unwrap_or(Version::All),
         query: self.search_query.clone(),
         pools,
      };
   }

   fn select_version(&mut self, theme: &Theme, ui: &mut Ui) {
      if let Some(version) = version_combo(
         theme,
         ui,
         "pool_explore_combobox",
         "Pool version",
         self.version,
         &Version::all(),
      ) {
         self.version = Some(version);
      }
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let chain = ctx.chain.id();

      ui.vertical_centered(|ui| {
         // The frame owns the width now — pinning it here would defeat the width transition.
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
         ui.spacing_mut().button_padding = theme.button_padding;

         let hint = RichText::new("Search pools")
            .color(theme.colors.text_muted)
            .size(theme.typography.normal);

         // A bounded row: `horizontal_centered` alone would centre the widgets on the whole
         // page's remaining height, not on one line.
         ui.allocate_ui_with_layout(
            vec2(ui.available_width(), FIELD_ROW_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.sm;

               // The field's only label is the placeholder it draws while it is empty, which is a
               // hint rather than a name.
               SecureTextEdit::singleline(&mut self.search_query)
                  .accessible_name("Search pools")
                  .visuals(theme.text_edit_visuals())
                  .hint_text(hint)
                  .desired_width(SEARCH_FIELD_WIDTH)
                  .margin(Margin::same(10))
                  .font(FontId::proportional(theme.typography.normal))
                  .show(ui);

               self.select_version(theme, ui);
            },
         );

         ui.horizontal(|ui| {
            let text = RichText::new("Add Pool").size(theme.typography.normal);
            let button =
               Button::new(text).visuals(theme.button_visuals()).label_align(Align::Center);
            if ui.add(button).clicked() {
               self.add_form.open();
            }
         });

         ui.add_space(20.0);

         let selected_version = self.version.unwrap_or(Version::All);

         self.refresh_rows(
            chain,
            ctx.pool_manager.clone(),
            icons,
            theme.image_tint_recommended,
         );

         // The rows are rebuilt on a worker thread, so the count a reader hears is the one that has
         // arrived. It is said when it moves, and never while a build is still in flight — there is
         // no count yet to talk about.
         if !self.rows_loading && self.rows_announced != Some(self.rows.len()) {
            self.rows_announced = Some(self.rows.len());

            self.notice = Some(match self.rows.len() {
               0 => "No pools match this filter.".to_string(),
               1 => "1 pool matches this filter.".to_string(),
               count => format!("{count} pools match this filter."),
            });
         }

         if self.rows.is_empty() {
            ui.add_space(theme.spacing.lg);

            if self.rows_loading {
               let spinner = ui.add(Spinner::new().size(20.0));

               // The table is not there yet and the spinner is a picture with no words: it says
               // what is being fetched.
               spinner.widget_info(|| {
                  WidgetInfo::labeled(
                     Role::ProgressIndicator,
                     ui.is_enabled(),
                     "Loading pools",
                  )
               });
            } else {
               ui.label(
                  RichText::new("No pools match this filter.")
                     .size(theme.typography.normal)
                     .color(theme.colors.text_muted),
               );
            }

            return;
         }

         let table_height = (ui.available_height() - theme.spacing.sm).max(140.0);

         Table::new("pools_ui", &self.rows)
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
            .key((chain, selected_version.to_str(), self.rows.len()))
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
                     let pair = ui.horizontal(|ui| {
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

                        let token0 = ui.add(label0);
                        let slash = ui.add(
                           Label::new(
                              RichText::new("/").size(theme.typography.normal),
                              None,
                           )
                           .interactive(false),
                        );
                        let token1 = ui.add(label1);

                        (token0.id, slash.id, token1.id)
                     });

                     // The cell draws the pair as three separate words, which reads as three
                     // unrelated items: the row says the pair once and the pieces leave the tree.
                     name_text(ui, &pair.response, &row.pair_for_speech);

                     for id in [pair.inner.0, pair.inner.1, pair.inner.2] {
                        ui.ctx().accesskit_node_builder(id, |node| node.set_hidden());
                     }
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

            // A bounded row, so the label centres on the cross axis against the taller combobox
            // instead of on a one-line band the combobox overflows.
            ui.allocate_ui_with_layout(
               vec2(ui.available_width(), FIELD_ROW_HEIGHT),
               Layout::left_to_right(Align::Center),
               |ui| {
                  let caption = ui.label(RichText::new("Pool version").size(theme.typography.large));
                  ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());

                  if let Some(version) = version_combo(
                     theme,
                     ui,
                     "add_pool_version_combo",
                     "Pool version",
                     Some(form.version),
                     &Version::add_options(),
                  ) {
                     form.version = version;
                  }
               },
            );

            let field_width = 320.0;

            Grid::new("add_pool_fields")
               .num_columns(2)
               .spacing(vec2(theme.spacing.md, theme.spacing.sm))
               .show(ui, |ui| {
                  let caption = ui.label(RichText::new("Token A").size(theme.typography.large));
                  ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
                  form_text_field(
                     theme,
                     ui,
                     &mut form.token_a,
                     "Token A",
                     "0x… (empty = native ETH)",
                     field_width,
                  );
                  ui.end_row();

                  let caption = ui.label(RichText::new("Token B").size(theme.typography.large));
                  ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
                  form_text_field(
                     theme,
                     ui,
                     &mut form.token_b,
                     "Token B",
                     "0x… (empty = native ETH)",
                     field_width,
                  );
                  ui.end_row();

                  if !form.version.is_v2() {
                     let allow_custom = form.version.is_v4();

                     let caption = ui.label(RichText::new("Fee").size(theme.typography.large));
                     ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
                     fee_combo(
                        theme,
                        ui,
                        "Fee tier",
                        &mut form.fee,
                        &mut form.v4_custom_fee,
                        &mut form.custom_fee,
                        allow_custom,
                     );
                     ui.end_row();

                     if allow_custom && form.v4_custom_fee {
                        let caption =
                           ui.label(RichText::new("Custom fee").size(theme.typography.large));
                        ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
                        form_text_field(
                           theme,
                           ui,
                           &mut form.custom_fee,
                           "Custom fee",
                           "Hundredths of a bip, e.g. 2500 = 0.25%, 0 = no fee",
                           field_width,
                        );
                        ui.end_row();
                     }
                  }

                  if form.version.is_v4() {
                     let caption =
                        ui.label(RichText::new("Tick spacing").size(theme.typography.large));
                     ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
                     form_text_field(
                        theme,
                        ui,
                        &mut form.tick_spacing,
                        "Tick spacing",
                        "Optional, defaults to the fee tier",
                        field_width,
                     );
                     ui.end_row();

                     let caption = ui.label(RichText::new("Hooks").size(theme.typography.large));
                     ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
                     form_text_field(
                        theme,
                        ui,
                        &mut form.hooks,
                        "Hooks",
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
               let alert = ui.label(
                  RichText::new(error)
                     .size(theme.typography.normal)
                     .color(theme.colors.error),
               );

               // The refusal is a sentence that appears, not a colour: it is an alert named with its
               // own words, so a reader is told why the form did not submit.
               name_alert(ui, &alert, error.as_str());
            }

            ui.add_space(theme.spacing.sm);

            if adding {
               ui.vertical_centered(|ui| {
                  let spinner = ui.add(Spinner::new().size(20.0));

                  // A spinner is a picture with no words: it says what the lookup is waiting for.
                  spinner.widget_info(|| {
                     WidgetInfo::labeled(
                        Role::ProgressIndicator,
                        ui.is_enabled(),
                        "Looking up the pool on chain",
                     )
                  });

                  ui.label(
                     RichText::new("Looking up the pool on chain…")
                        .size(theme.typography.normal)
                        .color(theme.colors.text_muted),
                  );
               });
            } else {
               ui.vertical_centered(|ui| {
                  let text = RichText::new("Add Pool").size(theme.typography.large);
                  let button = Button::new(text)
                     .visuals(theme.button_visuals())
                     .min_size(vec2(field_width, 42.0))
                     .label_align(Align::Center);

                  if ui.add(button).clicked() {
                     submit = Some(form.request());
                  }
               });
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

   /// Rebuild the table's rows off the GUI thread when the filter or the pool set moves.
   ///
   /// The rows are applied only if [`PoolRowsKey`] still matches, so a build the user has already
   /// filtered past cannot land on a newer one.
   fn refresh_rows(
      &mut self,
      chain: u64,
      manager: PoolManagerHandle,
      icons: Arc<Icons>,
      tint: bool,
   ) {
      let key = PoolRowsKey {
         chain,
         version: self.version.unwrap_or(Version::All),
         query: self.search_query.clone(),
         pools: manager.read(|manager| manager.pools.len()),
      };

      if key == self.rows_key {
         return;
      }

      self.rows_key = key.clone();
      self.rows_loading = true;

      crate::runtime::rt().spawn_blocking(move || {
         let rows = build_rows(&manager, &icons, &key, tint);

         SHARED_GUI.write(|gui| {
            let pools_ui = &mut gui.uniswap.pools_ui;

            // A newer filter owns the table; the build it superseded must not land.
            if pools_ui.rows_key != key {
               return;
            }

            pools_ui.rows = rows;
            pools_ui.rows_loading = false;
            gui.request_repaint();
         });
      });
   }
}

/// Build the pool table's rows for `key`, under the pool manager's read lock.
///
/// Runs on a blocking thread so the GUI never builds the table while holding the lock.
fn build_rows(
   manager: &PoolManagerHandle,
   icons: &Icons,
   key: &PoolRowsKey,
   tint: bool,
) -> Vec<PoolRow> {
   let selected_version = key.version;
   let query = key.query.as_str();

   manager.read(|manager| {
      let mut rows: Vec<PoolRow> = manager
         .pools
         .values()
         .filter(|pool| pool.chain_id() == key.chain)
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
               pair_for_speech: format!(
                  "{} / {}",
                  currency_for_speech(token0).name,
                  currency_for_speech(token1).name
               ),
               icon0: icons.currency_icon_x32(token0, tint),
               icon1: icons.currency_icon_x32(token1, tint),
               protocol: protocol_str(pool),
               fee: fee_label(pool.fee().fee()),
            }
         })
         .collect();

      rows.sort_by(|a, b| a.token0.cmp(&b.token0).then_with(|| a.token1.cmp(&b.token1)));

      rows
   })
}

fn version_combo(
   theme: &Theme,
   ui: &mut Ui,
   id: &str,
   caption: &str,
   current: Option<Version>,
   versions: &[Version],
) -> Option<Version> {
   let selected_text = current.map(|v| v.to_str()).unwrap_or("Select Version");
   let selected_text = RichText::new(selected_text).size(theme.typography.normal);
   let selected_label = Label::new(selected_text, None).sense(Sense::click()).interactive(true);

   // The trigger's node is the next widget this ui allocates, and the crate keeps the popup under
   // an id it takes before the list is drawn, so the id is read here first.
   let trigger_id = ui.next_auto_id();
   let mut selected = None;

   ComboBox::new(id, selected_label).width(VERSION_COMBO_WIDTH).show_ui(ui, |ui| {
      for version in versions {
         let text = RichText::new(version.to_str()).size(theme.typography.normal);
         let label = Label::new(text, None)
            .expand(Some(4.0))
            .fill_width(true)
            .visuals(theme.label_visuals())
            .sense(Sense::click())
            .interactive(true);

         let is_selected = Some(*version) == current;
         let response = ui.add(label);

         // The rows are clickable labels that say nothing about the state they carry: a radio
         // button says which version this one is and whether it is the one in force.
         response.widget_info(|| {
            WidgetInfo::selected(
               Role::RadioButton,
               ui.is_enabled(),
               is_selected,
               version.to_str(),
            )
         });

         if response.clicked() {
            selected = Some(*version);
         }
      }
   });

   // The trigger draws the version alone, so two pickers on one form read the same: the caption the
   // row draws names it, and it is read after the popup so a version picked this frame is the one
   // named.
   let shown = selected.or(current).map(|v| v.to_str()).unwrap_or("Select Version");
   name_picker(ui, trigger_id, caption, shown);

   selected
}

/// One field of the add-pool grid, named after the caption the grid draws beside it.
///
/// The placeholder the field draws is a hint, not a name: it explains the format, and it is gone as
/// soon as there is text. The caption is what a reader needs, so it is taken here and put on the
/// field's own node.
fn form_text_field(
   theme: &Theme,
   ui: &mut Ui,
   value: &mut String,
   caption: &str,
   hint: &str,
   width: f32,
) {
   let hint = RichText::new(hint).color(theme.colors.text_muted).size(theme.typography.normal);
   let visuals = theme.text_edit_visuals();

   ui.allocate_ui(vec2(width, FIELD_ROW_HEIGHT), |ui| {
      SecureTextEdit::singleline(value)
         .accessible_name(caption)
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
   caption: &str,
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

   let trigger_id = ui.next_auto_id();

   ComboBox::new("add_pool_fee_combo", selected_label)
      .width(160.0)
      .show_ui(ui, |ui| {
         for tier in FEE_TIERS {
            let text = fee_label(tier);
            let is_selected = *fee == tier && !*custom;
            let label = Label::new(
               RichText::new(&text).size(theme.typography.normal),
               None,
            )
            .expand(Some(4.0))
            .fill_width(true)
            .visuals(theme.label_visuals())
            .sense(Sense::click())
            .interactive(true);

            let response = ui.add(label);

            // The rows are clickable labels that say nothing about the state they carry: a radio
            // button says which tier this one is and whether it is the one on the trigger.
            response.widget_info(|| {
               WidgetInfo::selected(
                  Role::RadioButton,
                  ui.is_enabled(),
                  is_selected,
                  &text,
               )
            });

            if response.clicked() {
               *fee = tier;
               *custom = false;
            }
         }

         if allow_custom {
            let text = "Custom";
            let is_selected = *custom;
            let label = Label::new(
               RichText::new(text).size(theme.typography.normal),
               None,
            )
            .expand(Some(4.0))
            .fill_width(true)
            .visuals(theme.label_visuals())
            .sense(Sense::click())
            .interactive(true);

            let response = ui.add(label);

            response.widget_info(|| {
               WidgetInfo::selected(
                  Role::RadioButton,
                  ui.is_enabled(),
                  is_selected,
                  text,
               )
            });

            if response.clicked() {
               *custom = true;

               if custom_fee.trim().is_empty() {
                  *custom_fee = fee.to_string();
               }
            }
         }
      });

   // The trigger draws the fee alone: the caption the row says names the node, and the value is read
   // after the popup so a tier picked this frame is the one named.
   let value = if allow_custom && *custom {
      custom_fee_label(custom_fee)
   } else {
      fee_label(*fee)
   };
   name_picker(ui, trigger_id, caption, &value);
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

/// Look one pool up on chain, proving the request describes a pool that exists.
///
/// Separate from [`store_pool`] because the two answer different questions: this one says whether such
/// a pool is there, and the store is what makes it tracked. The agent port reaches the pair directly, so
/// it can answer the row it added; the page only wants a sentence.
pub(crate) async fn build_pool(
   ctx: ZeusCtx,
   chain: u64,
   request: AddPoolRequest,
) -> Result<AnyUniswapPool, PoolError> {
   let pool = match request {
      AddPoolRequest::V2 { token_a, token_b } => {
         let (token_a, token_b) = resolve_token_pair(&ctx, chain, token_a, token_b)
            .await
            .map_err(PoolError::Refused)?;
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
            .map_err(|error| PoolError::Network(error.to_string()))?
            .ok_or_else(|| PoolError::Refused("No V2 pool exists for this pair".to_owned()))?;

         AnyUniswapPool::from_pool(pool)
      }
      AddPoolRequest::V3 {
         token_a,
         token_b,
         fee,
      } => {
         let (token_a, token_b) = resolve_token_pair(&ctx, chain, token_a, token_b)
            .await
            .map_err(PoolError::Refused)?;
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
            .map_err(|error| PoolError::Network(error.to_string()))?;

         AnyUniswapPool::from_pool(pool)
      }
      AddPoolRequest::V4 {
         token_a,
         token_b,
         fee,
         tick_spacing,
         hooks,
      } => {
         let currency_a =
            resolve_currency(&ctx, chain, token_a).await.map_err(PoolError::Refused)?;
         let currency_b =
            resolve_currency(&ctx, chain, token_b).await.map_err(PoolError::Refused)?;

         if currency_a.address() == currency_b.address() {
            return Err(PoolError::Refused(
               "Token A and Token B must be different".to_owned(),
            ));
         }

         let fee = FeeAmount::new(fee);
         let tick_spacing = if tick_spacing == 0 {
            fee.tick_spacing_i32()
         } else {
            tick_spacing
         };

         if FeeAmount::i24_tick_spacing(tick_spacing).is_none() {
            return Err(PoolError::Refused(
               "Tick spacing is out of range".to_owned(),
            ));
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
            .map_err(|error| PoolError::Network(error.to_string()))?;

         if valid.is_empty() {
            return Err(PoolError::Refused(
               "The V4 pool is not initialized on chain".to_owned(),
            ));
         }

         AnyUniswapPool::V4(pool)
      }
   };

   Ok(pool)
}

/// Make one pool tracked: sync its state, remember its currencies, and save the store.
pub(crate) async fn store_pool(
   ctx: ZeusCtx,
   chain: u64,
   pool: AnyUniswapPool,
) -> Result<(), PoolError> {
   let currencies = [pool.currency0().clone(), pool.currency1().clone()];
   let manager = ctx.pool_manager();

   manager
      .update_state_for_pools(ctx.clone(), chain, vec![pool])
      .await
      .map_err(|error| PoolError::Network(error.to_string()))?;

   ctx.write(|ctx| {
      for currency in currencies {
         ctx.currency_db.insert_currency(chain, currency);
      }
   });

   ctx.save_pool_manager();

   Ok(())
}

/// Look a pool up and track it, answering the sentence the page shows.
async fn insert_pool(ctx: ZeusCtx, chain: u64, request: AddPoolRequest) -> Result<String, String> {
   let pool = build_pool(ctx.clone(), chain, request)
      .await
      .map_err(|error| error.message().to_owned())?;

   // The name this pool goes by in words, for anything that says it out loud.
   let label = format!(
      "{}/{}",
      currency_for_speech(pool.currency0()).name,
      currency_for_speech(pool.currency1()).name
   );

   store_pool(ctx, chain, pool).await.map_err(|error| error.message().to_owned())?;

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

   const TOKEN_A: &str = "0x0000000000000000000000000000000000000001";
   const TOKEN_B: &str = "0x0000000000000000000000000000000000000002";

   fn base_form(version: Version) -> AddPoolForm {
      AddPoolForm {
         version,
         token_a: TOKEN_A.to_string(),
         token_b: TOKEN_B.to_string(),
         ..Default::default()
      }
   }

   #[test]
   fn add_pool_form_builds_a_request_for_what_it_can_parse_and_refuses_the_rest() {
      // Case: add_pool_form_parses_v2_pair
      assert!(
         matches!(
            base_form(Version::V2).request(),
            Ok(AddPoolRequest::V2 { .. })
         ),
         "a V2 pair builds a V2 request"
      );

      // Case: add_pool_form_parses_v4_pair
      assert!(
         matches!(
            base_form(Version::V4).request(),
            Ok(AddPoolRequest::V4 {
               tick_spacing: 0,
               hooks: Address::ZERO,
               ..
            })
         ),
         "a bare V4 form defaults to no tick spacing and no hooks"
      );

      // Case: add_pool_form_rejects_matching_tokens
      let mut form = base_form(Version::V3);
      form.token_b = TOKEN_A.to_string();
      assert!(
         form.request().is_err(),
         "a pool of one token is refused"
      );

      // Case: add_pool_form_treats_empty_token_as_native
      let mut form = base_form(Version::V4);
      form.token_a = String::new();

      assert!(
         matches!(
            form.request(),
            Ok(AddPoolRequest::V4 {
               token_a: Address::ZERO,
               ..
            })
         ),
         "an empty token field is native ETH"
      );

      form.token_b = String::new();
      assert!(
         form.request().is_err(),
         "both sides native is not a pool"
      );

      // Case: add_pool_form_rejects_bad_tick_spacing
      let mut form = base_form(Version::V4);
      form.tick_spacing = "abc".to_string();
      assert!(
         form.request().is_err(),
         "a non-numeric tick spacing is refused"
      );

      // Case: add_pool_form_parses_custom_v4_fee
      let mut form = base_form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = "2500".to_string();
      assert!(
         matches!(
            form.request(),
            Ok(AddPoolRequest::V4 { fee: 2500, .. })
         ),
         "a custom fee goes into the request"
      );

      // Case: add_pool_form_accepts_zero_v4_fee
      let mut form = base_form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = "0".to_string();
      form.tick_spacing = "60".to_string();
      assert!(
         matches!(
            form.request(),
            Ok(AddPoolRequest::V4 {
               fee: 0,
               tick_spacing: 60,
               ..
            })
         ),
         "a zero-fee pool stays addable"
      );

      // Case: add_pool_form_accepts_dynamic_v4_fee
      let mut form = base_form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = DYNAMIC_FEE_FLAG.to_string();
      form.tick_spacing = "60".to_string();
      assert!(
         matches!(
            form.request(),
            Ok(AddPoolRequest::V4 {
               fee: DYNAMIC_FEE_FLAG,
               ..
            })
         ),
         "the dynamic-fee flag goes into the request"
      );

      // Case: add_pool_form_requires_spacing_for_dynamic_fee
      let mut form = base_form(Version::V4);
      form.v4_custom_fee = true;
      form.custom_fee = DYNAMIC_FEE_FLAG.to_string();
      assert!(
         form.request().is_err(),
         "a dynamic fee without tick spacing is refused"
      );

      form.tick_spacing = "60".to_string();
      assert!(
         form.request().is_ok(),
         "the same pool is addable once the spacing is given"
      );

      // Case: add_pool_form_rejects_invalid_custom_fee
      for value in ["16777216", "abc"] {
         let mut form = base_form(Version::V4);
         form.v4_custom_fee = true;
         form.custom_fee = value.to_string();
         assert!(
            form.request().is_err(),
            "{value} should be rejected"
         );
      }
   }
}
