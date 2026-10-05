//! A Window that allows the user to select a token

use eframe::egui::{
   Align, Color32, FontId, Id, Layout, Margin, OpenUrl, Order, RichText, ScrollArea, Sense,
   Spinner, Ui, emath::Vec2b, vec2,
};

use crate::assets::icons::Icons;
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::{SHARED_GUI, dots_button};
use crate::utils::{
   RT, nft_icon::start_nft_art_downloads, token_icon::spawn_fetch_token_icon, truncate_address,
   truncate_symbol_or_name,
};
use elegance::{Menu, MenuItem};
use std::{
   collections::{HashMap, HashSet},
   str::FromStr,
   sync::Arc,
   time::Duration,
};

use zeus_eth::{
   abi::erc165,
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   nft::{NftCollection, NftStandard, NftToken, collections_of},
   types::ChainId,
   utils::{
      NumericValue,
      batch::{NftRef, get_erc721_owners_and_uris},
   },
};

use anyhow::{anyhow, bail};
use egui_elements::{Button, Label, Modal, SecureTextEdit, Theme, utils::frame as frame_fn};
use elegance::{Badge, BadgeTone, Toast};

/// Currency direction for [`TokenSelectionWindow`].
///
/// Used by Swap (and similar) to know whether the user is picking the currency
/// to sell or buy.
#[derive(Copy, Clone, PartialEq)]
pub enum InOrOut {
   In,
   Out,
}

impl InOrOut {
   pub fn to_string(&self) -> String {
      (match self {
         Self::In => "Sell",
         Self::Out => "Buy",
      })
      .to_string()
   }
}

/// What the picker is listing.
///
/// An enum rather than the two `bool`s `recipient_selection.rs` uses for its tabs: a mode is exactly
/// one of these, so "neither" and "both" cannot be represented. `Fungible` is the default and what
/// `open` restores, which is what keeps the ERC-20 path unchanged.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PickerMode {
   Fungible,
   Nft,
}

/// One row of the NFT list.
///
/// The label data is resolved by the loader, not by the row: a row runs every frame for every visible
/// entry, and a collection lookup clones its `name`/`symbol` strings each time.
#[derive(Clone, Debug)]
struct NftRow {
   token: NftToken,
   /// 1 for ERC-721; the owned amount for an ERC-1155.
   balance: u64,
   /// Whether the wallet holds it, as the chain answered.
   ///
   /// `None` when the chain could not be asked at all, so the row claims nothing rather than claiming
   /// "not owned". The catalog deliberately keeps listing a token that has left the wallet — that is
   /// what it is for — so this flag is the only thing separating "tracked" from "held".
   owned: Option<bool>,
   /// Collection name, or the truncated collection address when no metadata was ever cached.
   name: String,
   /// Collection symbol, empty when the contract has none.
   symbol: String,
   /// Whether the wallet's portfolio lists this token. The picker lists the catalog, so a row is
   /// often a token that was discovered but never added — the row menu offers to change that.
   in_portfolio: bool,
}

impl NftRow {
   /// `"<collection> #<token id>"`.
   fn label(&self) -> String {
      format!("{} #{}", self.name, self.token.token_id)
   }

   /// The second line: the symbol and the standard, e.g. `BAYC · ERC-721`.
   fn subtitle(&self) -> String {
      let standard = match self.token.standard {
         NftStandard::Erc721 => "ERC-721",
         NftStandard::Erc1155 => "ERC-1155",
      };

      match self.symbol.is_empty() {
         true => standard.to_string(),
         false => format!("{} · {standard}", self.symbol),
      }
   }

   /// Whether the wallet holds this token, as a badge.
   fn ownership_badge(&self, ui: &mut Ui) {
      ownership_badge(ui, self.owned, self.balance);
   }
}

/// The ownership badge's text and tone, or `None` when nothing should be drawn.
///
/// `owned` is `None` when the chain could not be asked, and an absent badge is the honest answer
/// there: "not owned" is a claim, and a wrong one would invite the user to believe a token they hold
/// has gone. An ERC-1155's ownership is a quantity, so say how many; an ERC-721's is a flag.
fn ownership_label(owned: Option<bool>, amount: u64) -> Option<(String, BadgeTone)> {
   match (owned, amount) {
      (None, _) => None,
      (Some(true), amount) if amount > 1 => Some((format!("Owned ×{amount}"), BadgeTone::Ok)),
      (Some(true), _) => Some(("Owned".to_string(), BadgeTone::Ok)),
      (Some(false), _) => Some(("Not owned".to_string(), BadgeTone::Neutral)),
   }
}

/// Draw the ownership badge for a token, if we know the answer.
///
/// Shared with the portfolio: both lists mix tokens the wallet holds with tokens it only tracks, and
/// both have to say which is which.
pub(crate) fn ownership_badge(ui: &mut Ui, owned: Option<bool>, amount: u64) {
   if let Some((text, tone)) = ownership_label(owned, amount) {
      ui.add(Badge::new(text, tone));
   }
}

/// A simple window that allows the user to select a token
///
/// We can also use the search bar to search for a specific token either by its name or symbol.
///
/// If a valid address is passed to the search bar, we can fetch the token from the blockchain if it exists
pub struct TokenSelectionWindow {
   open: bool,
   loading: bool,
   syncing_balances: bool,
   title: String,
   pub size: (f32, f32),
   pub search_query: String,
   pub selected_currency: Option<Currency>,
   /// The NFT the user picked, if any.
   ///
   /// Its own field rather than a second meaning for `selected_currency`: they are different asset
   /// kinds, and the send flow reads them apart.
   pub selected_nft: Option<NftToken>,
   /// Did we fetched this token from the blockchain?
   pub token_fetched: bool,
   /// Currency direction, this only applies if we try to select a token from a SwapUi
   pub currency_direction: InOrOut,
   /// What the picker is listing: ERC-20 tokens or NFTs.
   mode: PickerMode,

   /// Cached and sorted list of currencies with their balances.
   ///
   /// (Currency, Balance, Value)
   processed_currencies: Vec<(Currency, NumericValue, NumericValue)>,

   /// The NFT rows: what the user tracks (`NftDB`) unioned with what the wallet holds (its portfolio),
   /// each with its balance and its collection label already resolved.
   ///
   /// ERC-721 has no amount — holding one is a 1 — so only ERC-1155 rows carry a real quantity.
   processed_nfts: Vec<NftRow>,
   /// Is the NFT list being fetched? Kept apart from `loading`, which is the ERC-20 balance fetch and
   /// hides the mode switch while it runs.
   nfts_loading: bool,
   /// Did that fetch finish? An empty list is a legitimate result, so `processed_nfts.is_empty()`
   /// cannot stand in for "never fetched".
   nfts_loaded: bool,
   /// Bumped whenever the list is invalidated ([`Self::clear_processed_nfts`]). A fetch captures it and
   /// writes only while it still matches, so a slow fetch for a previous `(chain, owner)` cannot land its
   /// rows — or latch `nfts_loaded` — over a newer one.
   nfts_generation: u64,
}

/// How wide the Tokens / NFTs pair renders, so the row that holds it can centre it.
///
/// A framed `egui_elements::Button` is `max(min_size, label + 2 * button_padding.x)`, and its default
/// `min_size` is zero, so measuring the two labels is enough. The gap between the buttons is
/// `theme.spacing.sm` **plus the row's `item_spacing.x` once**: egui advances by the item spacing after
/// each *widget*, and `add_space` is not one (it moves the cursor by its own amount and nothing more).
/// (The picker zeroes `item_spacing.x`, so in the real row the second term disappears — but the
/// arithmetic has to hold wherever the row is laid out.)
///
/// The measurement is needed because a `ui.horizontal` inside `vertical_centered` is a full-width
/// region whose children start at its left edge: nothing else will centre the pair.
fn mode_switch_width(theme: &Theme, ui: &Ui) -> f32 {
   const LABELS: [&str; 2] = ["Tokens", "NFTs"];

   let text = ui.ctx().fonts_mut(|fonts| {
      LABELS
         .iter()
         .map(|label| {
            fonts
               .layout_no_wrap(
                  (*label).to_owned(),
                  FontId::proportional(theme.typography.large),
                  Color32::PLACEHOLDER,
               )
               .size()
               .x
         })
         .sum::<f32>()
   });

   text
      + (LABELS.len() as f32 * 2.0 * ui.spacing().button_padding.x)
      + theme.spacing.sm
      + ui.spacing().item_spacing.x
}

impl TokenSelectionWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         syncing_balances: false,
         title: "Select Token".to_string(),
         size: (550.0, 500.0),
         search_query: String::new(),
         selected_currency: None,
         selected_nft: None,
         token_fetched: false,
         currency_direction: InOrOut::In,
         mode: PickerMode::Fungible,
         processed_currencies: Vec::new(),
         processed_nfts: Vec::new(),
         nfts_loading: false,
         nfts_loaded: false,
         nfts_generation: 0,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn is_loading(&self) -> bool {
      self.loading
   }

   pub fn open(&mut self, privacy_mode: bool, chain_id: u64, owner: Address) {
      self.open = true;
      // Always open on the ERC-20 list, so the existing flows see exactly what they saw before; a
      // caller that wants NFTs opts in with `set_mode`.
      self.mode = PickerMode::Fungible;
      // Both the tracked tokens and the wallet's holdings can have moved since last time;
      // `process_currencies` reloads the first and drops the second.
      self.process_currencies(privacy_mode, chain_id, owner);
   }

   pub fn reset(&mut self) {
      self.close();
      self.title = "Select Token".to_string();
      self.search_query.clear();
      self.selected_currency = None;
      self.selected_nft = None;
      self.token_fetched = false;
      self.currency_direction = InOrOut::In;
      self.mode = PickerMode::Fungible;
      self.processed_currencies = Vec::new();
      self.clear_processed_nfts();
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn set_title(&mut self, title: String) {
      self.title = title;
   }

   pub fn set_processed_currencies(
      &mut self,
      processed_currencies: Vec<(Currency, NumericValue, NumericValue)>,
   ) {
      self.processed_currencies = processed_currencies;
   }

   pub fn get_processed_currencies(&self) -> Vec<(Currency, NumericValue, NumericValue)> {
      self.processed_currencies.clone()
   }

   /// Get the selected currency if any
   pub fn get_selected_currency(&self) -> Option<&Currency> {
      self.selected_currency.as_ref()
   }

   /// Get the selected NFT if any
   pub fn get_selected_nft(&self) -> Option<&NftToken> {
      self.selected_nft.as_ref()
   }

   /// Start a reload of the picker's lists for `(privacy_mode, chain_id, owner)`.
   ///
   /// Drops the NFT list along with the fungible one: both are keyed on that same triple and
   /// [`Self::load_nfts`] early-returns while `nfts_loaded`, so a context change that reloaded only the
   /// fungibles would leave the NFT tab on the previous context's rows — and a row picked from there
   /// seeds a send with a foreign-chain token.
   ///
   /// Split from [`Self::process_currencies`] because that one spawns through `RT` + `SHARED_GUI` and
   /// cannot be driven from a test; this is the part that has to hold.
   fn begin_asset_reload(&mut self) {
      self.loading = true;
      self.clear_processed_nfts();
   }

   pub fn process_currencies(&mut self, privacy_mode: bool, chain_id: u64, owner: Address) {
      self.begin_asset_reload();

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         if !privacy_mode {
            let currencies = process_currencies(ctx.clone(), chain_id, owner);
            SHARED_GUI.write(|gui| {
               gui.token_selection.processed_currencies = currencies;
               gui.token_selection.loading = false;
            });
         } else {
            let portfolio = ctx.get_portfolio(chain_id, owner);
            let mut currencies = Vec::new();
            for (token, balance, value, _price) in portfolio.private_tokens() {
               let currency = Currency::from(token.clone());
               currencies.push((currency, balance.clone(), value.clone()));
            }

            SHARED_GUI.write(|gui| {
               gui.token_selection.processed_currencies = currencies;
               gui.token_selection.loading = false;
            });
         }
      });
   }

   pub fn clear_processed_currencies(&mut self) {
      self.processed_currencies.clear();
      self.processed_currencies.shrink_to_fit();
   }

   pub fn clear_processed_nfts(&mut self) {
      self.processed_nfts.clear();
      self.processed_nfts.shrink_to_fit();
      self.nfts_loading = false;
      self.nfts_loaded = false;
      // Invalidate a fetch still in flight: its rows belong to the list this call just forgot.
      self.nfts_generation = self.nfts_generation.wrapping_add(1);
   }

   /// Kick off the NFT list fetch, at most once per opening.
   ///
   /// Spawned, never inline: it reads the wallet's ERC-1155 balances over the network, and this is
   /// the frame path.
   fn load_nfts(&mut self, chain_id: u64, owner: Address, privacy_mode: bool) {
      if self.nfts_loaded || self.nfts_loading {
         return;
      }

      self.nfts_loading = true;
      let generation = self.nfts_generation;

      RT.spawn(async move {
         // Read on a worker: the handle is only reachable once the frame has dropped `SHARED_GUI`.
         let ctx = SHARED_GUI.write(|gui| gui.ctx.clone());

         // Privacy mode lists only what is actually shielded, exactly like the ERC-20 list, because only a
         // shielded NFT can be unshielded or privately transferred. There is nothing to fetch either way:
         // the portfolio already ran the private balance scan.
         let mut nfts = if privacy_mode {
            let portfolio = ctx.get_portfolio(chain_id, owner);
            private_nft_rows(
               portfolio.private_nfts(),
               &cached_collections(ctx.read(|ctx| ctx.nft_db.get_collections(chain_id))),
               portfolio.private_nft_amounts(),
            )
         } else {
            process_nfts(ctx, chain_id, owner).await
         };

         sort_owned_first(&mut nfts);

         // Art is fetched from the list's own metadata URIs, and this reads `SHARED_GUI`, so it belongs
         // here on the worker — never in the row loop, which runs inside the frame.
         start_nft_art_downloads(chain_id, nfts.iter().map(|row| &row.token));

         SHARED_GUI.write(|gui| {
            // A newer open, close or wallet change has already replaced this list — these rows are stale.
            if gui.token_selection.nfts_generation != generation {
               return;
            }
            gui.token_selection.processed_nfts = nfts;
            gui.token_selection.nfts_loading = false;
            gui.token_selection.nfts_loaded = true;
            gui.request_repaint();
         });
      });
   }

   pub fn set_currency_direction(&mut self, currency_direction: InOrOut) {
      self.currency_direction = currency_direction;
   }

   pub fn get_currency_direction(&self) -> &InOrOut {
      &self.currency_direction
   }

   /// What this picker is listing.
   pub fn get_mode(&self) -> PickerMode {
      self.mode
   }

   /// What this picker lists.
   ///
   /// Callers that open the picker for a specific asset kind (sending an NFT, say) set this in the
   /// same breath as `open`; the user can still switch mode from inside.
   pub fn set_mode(&mut self, mode: PickerMode) {
      self.mode = mode;
   }

   /// Show This [TokenSelectionWindow]
   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      chain_id: u64,
      owner: Address,
      ui: &mut Ui,
   ) {
      let mut open = self.open;

      if !open {
         return;
      }

      // Entering NFT mode loads the list once per opening; the switch row itself only flips `mode`.
      if self.mode == PickerMode::Nft {
         self.load_nfts(chain_id, owner, ctx.privacy_mode);
      }

      let mut close_window = false;
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let title = RichText::new(&self.title).size(theme.typography.heading);
      let id = Id::new("token_selection_window");

      Modal::new(id, &mut open)
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
            let ui_width = ui.available_width();

            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

               if self.loading {
                  ui.add(Spinner::new().size(25.0).color(theme.colors.text));
                  return;
               }

               self.show_mode_switch(theme, ui);

               // Both modes can sync; privacy mode lists what is already shielded, and there is nothing
               // on chain to check it against. The search bar below serves both modes.
               if !ctx.privacy_mode {
                  let text = RichText::new("Sync balances").size(theme.typography.normal);
                  let button = Button::new(text).min_size(vec2(70.0, 25.0));

                  let size = vec2(ui.available_width() * 0.25, 25.0);
                  let mut sync_clicked = false;

                  ui.allocate_ui(size, |ui| {
                     ui.spacing_mut().button_padding = theme.button_padding;

                     ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        sync_clicked = ui.add_enabled(!self.syncing_balances, button).clicked();
                        if self.syncing_balances {
                           ui.add_space(5.0);
                           ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                        }
                     });
                  });

                  if sync_clicked {
                     self.syncing_balances = true;
                     let chain = ctx.chain;
                     let owner = ctx.current_wallet_info().address;
                     let mode = self.mode;
                     RT.spawn(async move {
                        let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

                        match mode {
                           PickerMode::Fungible => {
                              sync_balances(ctx.clone(), chain.id(), owner).await;
                           }
                           PickerMode::Nft => {
                              sync_nft_balances(ctx.clone(), chain.id(), owner).await;
                           }
                        }

                        let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);

                        SHARED_GUI.write(|gui| {
                           gui.token_selection.syncing_balances = false;

                           if privacy_mode {
                              return;
                           }

                           match mode {
                              // Reopening re-reads the balances, which is what refreshes this list.
                              PickerMode::Fungible => {
                                 gui.token_selection.open(privacy_mode, chain.id(), owner);
                              }
                              // The whole NFT list is rebuilt, ownership included: the loader runs
                              // again on the next frame because the rows are gone.
                              PickerMode::Nft => {
                                 gui.token_selection.clear_processed_nfts();
                              }
                           }

                           gui.request_repaint();
                        });
                     });
                  }

                  ui.add_space(10.0);
               }

               let hint_text = match self.mode {
                  PickerMode::Fungible => "Search tokens or enter an address",
                  // Privacy mode lists shielded NFTs, and no pasted address can add one.
                  PickerMode::Nft if ctx.privacy_mode => "Search shielded NFTs",
                  PickerMode::Nft => "Search NFTs or paste a collection address",
               };

               let hint = RichText::new(hint_text)
                  .size(theme.typography.normal)
                  .color(theme.colors.text_muted);

               ui.add(
                  SecureTextEdit::singleline(&mut self.search_query)
                     .visuals(text_edit_visuals)
                     .hint_text(hint)
                     .desired_width(ui_width * 0.7)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.typography.normal)),
               );
               ui.add_space(10.0);
            });

            if self.mode == PickerMode::Nft {
               // The address-paste flow sits where the token list has its "Add Token" button: above
               // the list, and only once the query parses as an address. Privacy mode lists shielded
               // tokens, and no pasted address can put one there.
               if !ctx.privacy_mode {
                  ui.vertical_centered(|ui| {
                     self.get_collection_on_valid_address(theme, chain_id, owner, ui);
                  });
               }

               // Loading, nothing tracked, and no search match are single-line states; a populated
               // list owns its own scroll area, like the token list below.
               let matches = self
                  .processed_nfts
                  .iter()
                  .filter(|row| self.valid_nft_search(row, &self.search_query))
                  .count();

               if self.nfts_loading || matches == 0 {
                  ui.vertical_centered(|ui| {
                     self.show_nft_body(theme, ui);
                  });
               } else {
                  self.show_nft_list(
                     theme,
                     icons.clone(),
                     chain_id,
                     owner,
                     &mut close_window,
                     ui,
                  );
               }

               return;
            }

            ui.vertical_centered(|ui| {
               self.get_token_on_valid_address(ctx, theme, chain_id, owner, &mut close_window, ui);
            });

            let filtered_list: Vec<_> = self
               .processed_currencies
               .iter()
               .filter(|(currency, _, _)| self.valid_search(currency, &self.search_query))
               .collect();

            let num_rows = filtered_list.len();
            let row_height = 80.0;
            let tint = theme.image_tint_recommended;
            let mut frame = theme.frame2.outer_margin(Margin::same(5));
            let frame_visuals = theme.visuals.frame2_visuals;

            ScrollArea::vertical().auto_shrink(Vec2b::new(false, false)).show_rows(
               ui,
               row_height,
               num_rows,
               |ui, row_range| {
                  ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

                  for row_index in row_range {
                     if let Some((currency, balance, value)) = filtered_list.get(row_index) {
                        let name = truncate_symbol_or_name(currency.name(), 25);
                        let symbol = truncate_symbol_or_name(currency.symbol(), 10);
                        let text = format!("{}\n{}", name, symbol);
                        let icon = icons.currency_icon_x32(currency, tint);
                        let rich_text = RichText::new(text).size(theme.typography.normal);
                        let label = Label::new(rich_text, Some(icon))
                           .interactive(false)
                           .wrap()
                           .image_on_left();

                        let mut more_clicked = false;
                        let token_address = currency.erc20_opt().map(|token| token.address);

                        let res = frame_fn(&mut frame, frame_visuals, ui, |ui| {
                           ui.horizontal(|ui| {
                              ui.set_width(ui.available_width());

                              ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                                 ui.set_width(ui.available_width() * 0.4);
                                 ui.set_height(50.0);
                                 ui.add(label);
                              });

                              ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                 ui.set_width(ui.available_width() * 0.6);

                                 if let Some(token_address) = token_address {
                                    let size = vec2(28.0, 20.0);
                                    let more = dots_button(theme, size, ui);
                                    if more.clicked() {
                                       more_clicked = true;
                                    }

                                    let id = format!("{}_more_options", token_address);
                                    Menu::new(id).show_below(&more, |ui| {
                                       if ui.add(MenuItem::new("Copy Address")).clicked() {
                                          ui.ctx().copy_text(token_address.to_string());
                                       }

                                       if !currency.is_base() {
                                          if ui.add(MenuItem::new("Delete Token")).clicked() {
                                             more_clicked = true;
                                             if let Some(token) = currency.erc20_opt() {
                                                delete_token(
                                                   chain_id,
                                                   owner,
                                                   token.clone(),
                                                   ctx.privacy_mode,
                                                );
                                             }
                                          }
                                       }

                                       if ui.add(MenuItem::new("See on Block Explorer")).clicked() {
                                          let chain = ChainId::from(chain_id);
                                          let explorer = chain.block_explorer();
                                          let link =
                                             format!("{}/token/{}", explorer, token_address);
                                          let url = OpenUrl::new_tab(link);
                                          ui.ctx().open_url(url);
                                       }
                                    });

                                    ui.add_space(8.0);
                                 }

                                 if !balance.is_zero() {
                                    let value_text = format!("${:.12}", value.abbreviated());

                                    ui.vertical(|ui| {
                                       ui.label(
                                          RichText::new(value_text).size(theme.typography.normal),
                                       );

                                       ui.label(
                                          RichText::new(format!("{:.12}", balance.abbreviated()))
                                             .size(theme.typography.normal),
                                       );
                                    });
                                 }
                              });
                           });
                        });

                        if !more_clicked && res.interact(Sense::click()).clicked() {
                           self.selected_currency = Some((*currency).clone());
                           self.token_fetched = false;
                           close_window = true;
                        }
                     }
                  }
               },
            );
         });

      if close_window || !open {
         self.close();
         self.clear_processed_currencies();
         self.clear_processed_nfts();
      }
   }

   /// The Tokens / NFTs switch.
   ///
   /// Two `Button::selectable`s sharing `theme.button_visuals()`, matching the two-mode switch in
   /// `recipient_selection.rs` — this picker is a selection modal of the same shape, so it takes
   /// that pattern rather than a `TabBar`.
   fn show_mode_switch(&mut self, theme: &Theme, ui: &mut Ui) {
      let button_visuals = theme.button_visuals();
      let switch_width = mode_switch_width(theme, ui);

      ui.horizontal(|ui| {
         // A `ui.horizontal` here is a full-width region whose children start at its left edge, so the
         // pair only ends up centred if the row is padded by half of what is left over.
         ui.add_space(((ui.available_width() - switch_width) * 0.5).max(0.0));

         let tokens_text = RichText::new("Tokens").size(theme.typography.large);
         let nfts_text = RichText::new("NFTs").size(theme.typography.large);

         let tokens_button = Button::selectable(self.mode == PickerMode::Fungible, tokens_text)
            .visuals(button_visuals);

         if ui.add(tokens_button).clicked() {
            self.mode = PickerMode::Fungible;
         }

         ui.add_space(theme.spacing.sm);

         let nfts_button =
            Button::selectable(self.mode == PickerMode::Nft, nfts_text).visuals(button_visuals);

         if ui.add(nfts_button).clicked() {
            self.mode = PickerMode::Nft;
         }
      });

      ui.add_space(theme.spacing.sm);
   }

   /// NFT mode's two single-line states: still loading, or nothing tracked.
   fn show_nft_body(&self, theme: &Theme, ui: &mut Ui) {
      ui.add_space(theme.spacing.xl);

      if self.nfts_loading {
         ui.add(Spinner::new().size(25.0).color(theme.colors.text));
         return;
      }

      let text = match self.processed_nfts.is_empty() {
         true => "No NFTs to show yet",
         // The list is populated, so an empty one here is the search's doing.
         false => "No matches",
      };

      let note = RichText::new(text).size(theme.typography.normal).color(theme.colors.text_muted);

      ui.label(note);
   }

   /// The NFT rows.
   ///
   /// The same shape as the token rows below — a `frame2` card per entry, art and label on the left,
   /// the `dots_button` menu and the amount on the right — and virtualized the same way, because one
   /// tracked collection can hold hundreds of ids.
   ///
   /// The thumbnail is the 64px rendering: the 250px copy exists to inspect a single token, not for
   /// a list.
   fn show_nft_list(
      &mut self,
      theme: &Theme,
      icons: Arc<Icons>,
      chain_id: u64,
      owner: Address,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      // Borrow the rows up front (as the token list does with its filtered list) so the closure can
      // still write `selected_nft`: it captures that field, not the whole of `self`.
      let rows: Vec<&NftRow> = self
         .processed_nfts
         .iter()
         .filter(|row| self.valid_nft_search(row, &self.search_query))
         .collect();
      let num_rows = rows.len();
      let row_height = 80.0;
      let tint = theme.image_tint_recommended;
      let chain = ChainId::from(chain_id);
      let mut frame = theme.frame2.outer_margin(Margin::same(5));
      let frame_visuals = theme.visuals.frame2_visuals;

      ScrollArea::vertical().auto_shrink(Vec2b::new(false, false)).show_rows(
         ui,
         row_height,
         num_rows,
         |ui, row_range| {
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

            for row_index in row_range {
               let Some(row) = rows.get(row_index) else {
                  continue;
               };

               let token = &row.token;
               let icon = icons.nft_icon_x64(
                  token.chain_id,
                  token.collection,
                  token.token_id,
                  tint,
               );
               let text = format!("{}\n{}", row.label(), row.subtitle());
               let rich_text = RichText::new(text).size(theme.typography.normal);
               let label =
                  Label::new(rich_text, Some(icon)).interactive(false).wrap().image_on_left();

               let mut more_clicked = false;
               let collection = token.collection;
               let token_id = token.token_id;

               let res = frame_fn(&mut frame, frame_visuals, ui, |ui| {
                  ui.horizontal(|ui| {
                     ui.set_width(ui.available_width());

                     ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        ui.set_width(ui.available_width() * 0.4);
                        ui.set_height(50.0);
                        ui.add(label);
                     });

                     ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.set_width(ui.available_width() * 0.6);

                        let more = dots_button(theme, vec2(28.0, 20.0), ui);
                        if more.clicked() {
                           more_clicked = true;
                        }

                        ui.add_space(theme.spacing.sm);

                        // Left of the menu button. The catalog goes on listing a token the wallet no
                        // longer holds, so this is what tells the two apart.
                        row.ownership_badge(ui);

                        let menu_id = format!("{collection}_{token_id}_nft_more_options");
                        Menu::new(menu_id).show_below(&more, |ui| {
                           if ui.add(MenuItem::new("Copy Collection")).clicked() {
                              ui.ctx().copy_text(collection.to_string());
                           }

                           if ui.add(MenuItem::new("Copy Token ID")).clicked() {
                              ui.ctx().copy_text(token_id.to_string());
                           }

                           if ui.add(MenuItem::new("See on Block Explorer")).clicked() {
                              let url = chain.nft_url(collection, token_id);
                              ui.ctx().open_url(OpenUrl::new_tab(url));
                           }

                           // The catalog is what the picker lists; the portfolio is the user's own
                           // list. This is where that decision gets made.
                           let portfolio_item = match row.in_portfolio {
                              true => "Remove from Portfolio",
                              false => "Add to Portfolio",
                           };

                           if ui.add(MenuItem::new(portfolio_item)).clicked() {
                              more_clicked = true;
                              set_nft_in_portfolio(
                                 chain_id,
                                 owner,
                                 row.token.clone(),
                                 !row.in_portfolio,
                              );
                           }

                           if ui.add(MenuItem::new("Delete NFT")).clicked() {
                              more_clicked = true;
                              delete_nft(
                                 chain_id,
                                 owner,
                                 row.token.clone(),
                                 row.name.clone(),
                              );
                           }
                        });
                     });
                  });
               });

               if !more_clicked && res.interact(Sense::click()).clicked() {
                  self.selected_nft = Some(row.token.clone());
                  *close_window = true;
               }
            }
         },
      );
   }

   fn get_token_on_valid_address(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      chain: u64,
      owner: Address,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      if let Ok(address) = Address::from_str(&self.search_query) {
         let token = ctx.currency_db.get_erc20_token(chain, address);
         if token.is_some() {
            return;
         }

         ui.add_space(20.0);
         let size = vec2(ui.available_width() * 0.7, 40.0);
         let button_visuals = theme.button_visuals();

         let text = RichText::new("Add Token").size(theme.typography.large);
         let button = Button::new(text).min_size(size).visuals(button_visuals);

         if ui.add(button).clicked() {
            self.token_fetched = true;

            RT.spawn(async move {
               let ctx = SHARED_GUI.write(|gui| {
                  gui.loading_window.open("Retrieving token...");
                  gui.request_repaint();
                  gui.ctx.clone()
               });

               let token = match get_erc20_token(ctx, chain, owner, address).await {
                  Ok(token) => {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.reset();
                     });
                     token
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        let msg = format!("Failed to fetch token: {}", e);
                        gui.open_msg_window(msg);
                        gui.loading_window.reset();
                     });
                     return;
                  }
               };
               let currency = Currency::from(token);
               SHARED_GUI.write(|gui| {
                  gui.token_selection.selected_currency = Some(currency);
               });
            });

            // close the token selection window
            *close_window = true;
         }
      }
   }

   /// Does this NFT row match the query?
   ///
   /// Token id, collection name and collection symbol, plus an exact collection address — the NFT
   /// counterpart of [`TokenSelectionWindow::valid_search`], which matches an ERC-20 address the same
   /// NFT mode's address-paste flow: a pasted collection address becomes the wallet's tokens.
   ///
   /// Mirrors [`TokenSelectionWindow::get_token_on_valid_address`] — the button only appears once the
   /// query parses, and the chain work happens on the click, so typing an address is not a request per
   /// character. No store lookup here: adding twice is harmless (both stores dedupe by token
   /// identity), and it picks up ids acquired since the last time.
   fn get_collection_on_valid_address(
      &mut self,
      theme: &Theme,
      chain_id: u64,
      owner: Address,
      ui: &mut Ui,
   ) {
      let Ok(address) = Address::from_str(self.search_query.trim()) else {
         return;
      };

      ui.add_space(20.0);
      let size = vec2(ui.available_width() * 0.7, 40.0);
      let button_visuals = theme.button_visuals();

      let text = RichText::new("Add Collection").size(theme.typography.large);
      let button = Button::new(text).min_size(size).visuals(button_visuals);

      let clicked = ui.add(button).clicked();

      // Adding a collection reads the metadata address its contract returns — a host
      // the collection chooses, the same caveat as the asset-image opt-in.
      ui.add_space(6.0);
      ui.add(
         Label::new(
            RichText::new(
               "Reading a collection fetches the metadata address its contract returns, which is \
                the collection's own host. Zeus only uses https and refuses local or private \
                addresses.",
            )
            .size(theme.typography.small)
            .color(theme.colors.text_muted),
            None,
         )
         .wrap()
         .fill_width(true)
         .interactive(false),
      );

      if !clicked {
         return;
      }

      RT.spawn(async move {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Reading collection...");
            gui.request_repaint();
            gui.ctx.clone()
         });

         let outcome = add_nft_collection(ctx, chain_id, owner, address).await;

         SHARED_GUI.write(|gui| {
            gui.loading_window.reset();

            match outcome {
               // A collection Zeus cannot enumerate is not a success: the user has to read why.
               Ok(add) if add.is_error() => gui.open_msg_window(add.message()),
               Ok(add) => Toast::new("Collection")
                  .description(add.message())
                  .tone(BadgeTone::Ok)
                  .show(&gui.egui_ctx),
               Err(e) => gui.open_msg_window(format!("Failed to add collection: {e}")),
            }

            // The list is rebuilt from the stores on the next frame.
            gui.token_selection.clear_processed_nfts();
            gui.request_repaint();
         });
      });
   }

   /// Does this NFT row match the query?
   ///
   /// Token id, collection name and collection symbol, plus an exact collection address — the NFT
   /// counterpart of [`TokenSelectionWindow::valid_search`]. A pasted address that is *not* tracked
   /// matches nothing here; the add flow above owns that case.
   fn valid_nft_search(&self, row: &NftRow, query: &str) -> bool {
      let query = query.trim().to_lowercase();

      if query.is_empty() {
         return true;
      }

      if row.token.token_id.to_string().contains(&query) {
         return true;
      }

      if row.name.to_lowercase().contains(&query) {
         return true;
      }

      if row.symbol.to_lowercase().contains(&query) {
         return true;
      }

      if let Ok(address) = Address::from_str(&query) {
         return row.token.collection == address;
      }

      false
   }

   fn valid_search(&self, currency: &Currency, query: &str) -> bool {
      let query = query.to_lowercase();

      if query.is_empty() {
         return true;
      }

      if currency.name().to_lowercase().contains(&query) {
         return true;
      }

      if currency.symbol().to_lowercase().contains(&query) {
         return true;
      }

      if let Ok(address) = Address::from_str(&query) {
         if currency.is_erc20() {
            if let Some(token) = currency.erc20_opt() {
               return token.address == address;
            }
         }
      }
      false
   }
}

fn delete_token(chain_id: u64, owner: Address, token: ERC20Token, privacy_mode: bool) {
   RT.spawn(async move {
      SHARED_GUI.write(|gui| {
         gui.confirm_window.open(format!("Delete {}?", token.name));
         gui.request_repaint();
      });

      let confirmed = loop {
         tokio::time::sleep(Duration::from_millis(50)).await;
         let confirmed = SHARED_GUI.read(|gui| gui.confirm_window.get_confirm());
         if let Some(confirmed) = confirmed {
            SHARED_GUI.write(|gui| {
               gui.confirm_window.reset();
            });
            break confirmed;
         }
      };

      if !confirmed {
         return;
      }

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         ctx.write(|ctx| {
            ctx.currency_db.remove_token(chain_id, token.address);
         });

         ctx.write_wallet_state(|ws| {
            let mut portfolio = ws.portfolio_db.get(chain_id, owner);
            portfolio.remove_token(&token);
            ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
         });

         ctx.save_currency_db();

         if let Err(e) = ctx.save_wallet_state() {
            tracing::error!(
               "Error saving wallet state after token delete: {:?}",
               e
            );
         }

         if let Err(e) = crate::assets::icons::delete_token_icon(chain_id, token.address) {
            tracing::error!("Error deleting token icon: {:?}", e);
         }

         SHARED_GUI.write(|gui| {
            gui.icons.tokens.remove_icon(token.address, chain_id);
            gui.token_selection.process_currencies(privacy_mode, chain_id, owner);
            gui.request_repaint();
         });
      });
   });
}

async fn get_erc20_token(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
   token_address: Address,
) -> Result<ERC20Token, anyhow::Error> {
   // Fire-and-forget do not await. The placeholder stays until this finishes.
   spawn_fetch_token_icon(chain, token_address);

   let z_client = ctx.get_zeus_client();
   let rpc = z_client.get_best_rpc(chain).ok_or(anyhow!("No available RPC found"))?;
   let client = z_client.connect_with_timeout(&rpc, 10).await?;

   let token = ERC20Token::new(client, token_address, chain).await?;

   let manager = ctx.balance_manager();
   manager
      .update_tokens_balance(
         ctx.clone(),
         chain,
         owner,
         vec![token.clone()],
         false,
      )
      .await?;

   let currency = Currency::from(token.clone());

   // Update the db
   ctx.write(|ctx| {
      ctx.currency_db.insert_currency(chain, currency.clone());
   });

   // If there is a balance add the token to the portfolio
   let balance = manager.get_token_balance(chain, owner, token.address);
   if !balance.is_zero() {
      let mut portfolio = ctx.get_portfolio(chain, owner);
      portfolio.add_token(token.clone());
      ctx.write_wallet_state(|ws| ws.portfolio_db.insert_portfolio(chain, owner, portfolio));
   }

   // Sync the pools for the token
   let ctx_clone = ctx.clone();
   let token_clone = token.clone();
   RT.spawn(async move {
      ctx_clone.write(|ctx| {
         ctx.data_syncing = true;
      });

      let pool_manager = ctx_clone.pool_manager();

      if let Err(e) = pool_manager
         .discover_pools_for_tokens(
            ctx_clone.clone(),
            chain,
            vec![token_clone.clone()],
         )
         .await
      {
         tracing::error!("Error discovering pools {}", e);
      }

      if let Err(e) = pool_manager
         .update_for_currencies(ctx_clone.clone(), chain, vec![currency])
         .await
      {
         tracing::error!("Error updating pool state {}", e);
      }

      RT.spawn_blocking(move || {
         ctx_clone.update_public_data(chain, owner);
         ctx_clone.write(|ctx| ctx.data_syncing = false);
         ctx_clone.save_currency_db();
      });
   });

   Ok(token)
}

fn process_currencies(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
) -> Vec<(Currency, NumericValue, NumericValue)> {
   let currencies = ctx.get_currencies(chain_id);

   let mut currency_list: Vec<(Currency, NumericValue, NumericValue)> = currencies
      .iter()
      .map(|currency| {
         let balance = ctx.get_currency_balance(chain_id, owner, currency);
         let value = ctx.get_currency_value_for_amount(balance.f64(), currency);
         (currency.clone(), balance, value)
      })
      .collect();

   currency_list
      .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));

   currency_list
}

async fn sync_balances(ctx: ZeusCtx, chain: u64, owner: Address) {
   let manager = ctx.balance_manager();
   let currencies = ctx.get_currencies(chain);
   let tokens = currencies.iter().map(|c| c.to_erc20().into_owned()).collect::<Vec<_>>();

   match manager.update_tokens_balance(ctx.clone(), chain, owner, tokens, false).await {
      Ok(_) => {
         tracing::info!("Synced balances for chain {}", chain);
      }
      Err(e) => {
         tracing::error!(
            "Error syncing balances for chain {}: {:?}",
            chain,
            e
         );
      }
   }

   let (eth_removed, token_removed) = manager.remove_zero_balances();
   tracing::info!(
      "Removed {} eth and {} tokens zero balances",
      eth_removed,
      token_removed
   );
}

/// The NFT list for one wallet: what the user tracks (`NftDB`) unioned with what the wallet holds
/// (its portfolio), each with its collection label and its ownership resolved.
///
/// Neither source is authoritative alone — a tracked token may have been transferred away, and a held
/// token is not in the catalog until someone adds it. Ownership is therefore asked of the chain rather
/// than inferred from either list: the whole point of the catalog is to keep listing a token after it
/// has left the wallet, and the row has to be able to say so.
///
/// One Multicall3 round per standard answers it: `ownerOf` per ERC-721 id, `balanceOf` per ERC-1155 id.
async fn process_nfts(ctx: ZeusCtx, chain_id: u64, owner: Address) -> Vec<NftRow> {
   let held = ctx.get_portfolio(chain_id, owner).nfts().clone();

   // The portfolio's identities, so a row can say whether it is in there. Keyed by collection and id
   // rather than by whole token: the two lists can disagree about metadata and still be one token.
   let portfolio: HashSet<NftRef> =
      held.iter().map(|token| (token.collection, token.token_id)).collect();

   let merged = nft_candidates(&ctx, chain_id, held);

   // Ownership lives in the balance manager now — the same store the ERC-20 rows read — so refresh it
   // for what this list shows and then read it. A failure is not fatal: the rows then claim nothing.
   let manager = ctx.balance_manager();
   if let Err(e) = manager
      .update_nft_balances(
         ctx.clone(),
         chain_id,
         owner,
         merged.clone(),
         false,
      )
      .await
   {
      tracing::error!("Error updating NFT balances: {e:?}");
   }

   // Resolve the labels here rather than in the row: the row runs every frame for every visible
   // entry, and a collection lookup clones its `name`/`symbol` strings each time.
   let collections = cached_collections(ctx.read(|ctx| ctx.nft_db.get_collections(chain_id)));

   // The manager's answers, for the entries it has one for. An id it never answered is left out, and
   // its row then claims nothing rather than claiming zero.
   let amounts: HashMap<NftRef, u64> = merged
      .iter()
      .filter_map(|token| {
         manager
            .get_nft_balance(chain_id, owner, token.collection, token.token_id)
            .map(|amount| ((token.collection, token.token_id), amount))
      })
      .collect();

   attach_balances(merged, &amounts)
      .into_iter()
      .map(|(token, balance)| {
         let (name, symbol) = collection_label(&token, &collections);
         let in_portfolio = portfolio.contains(&(token.collection, token.token_id));

         // `None` when there is no answer to read — never asked, or the chain could not be reached — so
         // the row shows no ownership claim rather than a wrong one. `Some(0)` is a real answer.
         let owned = amounts.get(&(token.collection, token.token_id)).map(|amount| *amount > 0);

         NftRow {
            token,
            balance,
            owned,
            name,
            symbol,
            in_portfolio,
         }
      })
      .collect()
}

/// Build the picker's rows for the NFTs the wallet holds **privately**.
///
/// Privacy mode must list what is shielded rather than what is owned on-chain — those are the only tokens
/// that can be unshielded or privately transferred — and this is pure on purpose: there is no network
/// involved, because the portfolio's private balance scan already resolved them.
///
/// An ERC-721 has no balance to read — it is held or it is not — so every row is one token. Every row
/// is held by construction, too: these came out of the portfolio's own private scan.
fn private_nft_rows(
   nfts: &[NftToken],
   collections: &HashMap<Address, NftCollection>,
   amounts: &HashMap<NftRef, u64>,
) -> Vec<NftRow> {
   nfts
      .iter()
      .map(|token| {
         let (name, symbol) = collection_label(token, collections);

         NftRow {
            token: token.clone(),
            // What the notes say: a quantity for an ERC-1155, one for an ERC-721. A token the scan recorded
            // no amount for falls back to one, not to zero — the list *is* the shielded set, so nothing in
            // it is absent.
            balance: amounts.get(&(token.collection, token.token_id)).copied().unwrap_or(1),
            owned: Some(true),
            name,
            symbol,
            // These came from the portfolio's own private scan, so they are in it by construction.
            in_portfolio: true,
         }
      })
      .collect()
}

/// Force the ownership check for every NFT the picker can list — the NFT counterpart of
/// [`sync_balances`].
///
/// The ERC-20 sync re-reads balances and then drops what is gone; an NFT's answer is kept even when it
/// is zero, because `0` is what makes a row say "not owned" instead of claiming nothing (see
/// `BalanceManager::remove_zero_balances`). So this re-asks the chain and leaves the store alone.
///
/// It reads the sources rather than the window's rows, so it checks the same tokens whether or not the
/// list has finished loading.
async fn sync_nft_balances(ctx: ZeusCtx, chain_id: u64, owner: Address) {
   let held = ctx.get_portfolio(chain_id, owner).nfts().clone();
   let nfts = nft_candidates(&ctx, chain_id, held);

   // Ownership is not expected to move here — that is the count a `awaiting_change` retry would wait
   // on — so this is a plain read, exactly as the list loader does it.
   match ctx
      .balance_manager()
      .update_nft_balances(ctx.clone(), chain_id, owner, nfts, false)
      .await
   {
      Ok(()) => tracing::info!("Synced NFT ownership for chain {chain_id}"),
      Err(e) => tracing::error!("Error syncing NFT ownership for chain {chain_id}: {e:?}"),
   }
}

/// Every NFT the picker can list for a wallet: what the user tracks (`NftDB`) unioned with what the
/// wallet holds (its portfolio), deduped by identity.
///
/// `held` is passed in because the caller either already has the portfolio (the list loader needs it
/// for `in_portfolio`) or has just read it (the sync); this never reads it a second time.
fn nft_candidates(ctx: &ZeusCtx, chain_id: u64, held: Vec<NftToken>) -> Vec<NftToken> {
   let tracked = ctx.read(|ctx| ctx.nft_db.get_nfts(chain_id));

   merge_nft_sources(tracked, held)
}

/// Owned rows first, everything else in the order the merge produced (identity, so the tie order is
/// deterministic and does not reshuffle between loads).
///
/// The catalog keeps listing a token after it has left the wallet — that is what it is for — so on a
/// fresh install the list is mostly other people's tokens. What a picker is *for* is the token the
/// wallet holds, so those come first. Everything else keeps its identity order: a row with no answer
/// (`None`, the chain could not be asked) makes no claim either way, so it is not sorted as if it were
/// known to be absent.
fn sort_owned_first(rows: &mut [NftRow]) {
   rows.sort_by_key(|row| row.owned != Some(true));
}

/// Collection metadata cached for this chain, keyed by collection address, so a row builder can resolve
/// a name and a symbol without touching the network.
pub(crate) fn cached_collections(
   collections: Vec<NftCollection>,
) -> HashMap<Address, NftCollection> {
   collections
      .into_iter()
      .map(|collection| (collection.address, collection))
      .collect()
}

/// Union of the tracked and held lists, deduped by token identity.
///
/// The tracked entry wins a tie: it is the one that may carry a cached `metadata_uri`, and dropping
/// it would cost an on-chain `tokenURI` read later. Sorted by identity so the list does not reshuffle
/// between loads.
fn merge_nft_sources(tracked: Vec<NftToken>, held: Vec<NftToken>) -> Vec<NftToken> {
   let mut merged: Vec<NftToken> = Vec::with_capacity(tracked.len() + held.len());

   for token in tracked.into_iter().chain(held) {
      match merged.iter_mut().find(|existing| **existing == token) {
         Some(existing) => {
            if existing.standard != token.standard {
               // Identity deliberately excludes the standard, so a disagreement means one of the two
               // detections was wrong. Keep the tracked one (deterministic) but say so: both the
               // badge and the transfer encoding depend on this field.
               tracing::warn!(
                  "NFT standard mismatch for {} #{}: tracked {:?}, held {:?}",
                  existing.collection,
                  existing.token_id,
                  existing.standard,
                  token.standard
               );
            }

            if existing.metadata_uri.is_none() {
               existing.metadata_uri = token.metadata_uri;
            }
         }
         None => merged.push(token),
      }
   }

   merged.sort();
   merged
}

/// Pair each token with its balance: 1 for ERC-721 (holding one is a 1), the reported amount for
/// ERC-1155, and 0 when the chain did not answer for that id.
fn attach_balances(
   tokens: Vec<NftToken>,
   amounts: &HashMap<(Address, U256), u64>,
) -> Vec<(NftToken, u64)> {
   tokens
      .into_iter()
      .map(|token| {
         let balance = match token.is_erc1155() {
            true => amounts.get(&(token.collection, token.token_id)).copied().unwrap_or(0),
            false => 1,
         };

         (token, balance)
      })
      .collect()
}

/// The name a row shows for a collection: the cached one, or the address.
///
/// Collection metadata is cached when a token is added, so a missing name means the collection was
/// never fetched — a token that arrived from the portfolio instead of the catalog. Falling back to
/// the address keeps a row identifiable instead of blank, and an address is what the user would paste
/// into an explorer anyway. A blank string counts as missing, since a contract can return `""`.
pub(crate) fn nft_collection_name(collection: Option<&NftCollection>, address: Address) -> String {
   collection
      .and_then(|collection| collection.name.clone())
      .filter(|name| !name.trim().is_empty())
      .unwrap_or_else(|| truncate_address(address.to_string()))
}

/// The `(name, symbol)` a row shows for a token's collection.
fn collection_label(
   token: &NftToken,
   collections: &HashMap<Address, NftCollection>,
) -> (String, String) {
   let collection = collections.get(&token.collection);

   let name = nft_collection_name(collection, token.collection);

   let symbol = collection
      .and_then(|collection| collection.symbol.clone())
      .filter(|symbol| !symbol.trim().is_empty())
      .unwrap_or_default();

   (name, symbol)
}

/// What a pasted collection address turned into.
enum CollectionAdd {
   /// Its owned ids are now tracked.
   Tracked { name: String, count: usize },
   /// Enumerable, but this wallet holds none of its tokens.
   NoTokens { name: String },
   /// No on-chain way to learn which ids the wallet holds: an ERC-1155, or an ERC-721 without the
   /// Enumerable extension. Zeus has no indexer, so this is where it stops — and it says so rather
   /// than showing an empty list.
   NotEnumerable { name: String, standard: NftStandard },
}

impl CollectionAdd {
   /// The notice the user is shown.
   fn message(&self) -> String {
      match self {
         Self::Tracked { name, count } => {
            let plural = if *count == 1 { "" } else { "s" };
            format!("Added {count} token{plural} from {name}")
         }
         Self::NoTokens { name } => format!("This wallet holds no tokens of {name}"),
         Self::NotEnumerable { name, standard } => {
            let kind = match standard {
               NftStandard::Erc1155 => "ERC-1155 collections",
               NftStandard::Erc721 => "ERC-721 collections without the Enumerable extension",
            };

            format!("{name} cannot be listed: {kind} do not expose an owner's token ids")
         }
      }
   }

   /// Whether the user has to read this (a window) rather than just being told it worked (a toast).
   fn is_error(&self) -> bool {
      matches!(self, Self::NotEnumerable { .. })
   }
}

/// Resolve a pasted collection address into the wallet's owned tokens, and track them.
///
/// The NFT counterpart of `get_erc20_token`: a collection in, its owned ids out — cached collection
/// metadata plus one `NftToken` per id (carrying the `tokenURI` the art pipeline needs), written to
/// the catalog and to this wallet's portfolio. Both writes are needed: the catalog is what the picker
/// lists, the portfolio is what the wallet holds.
///
/// Enumerable ERC-721 only. For an ERC-1155 or a plain ERC-721 there is no on-chain way to learn which
/// ids a wallet holds, and Zeus deliberately has no indexer — guessing a range would invent tokens.
async fn add_nft_collection(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
   address: Address,
) -> Result<CollectionAdd, anyhow::Error> {
   let client = ctx.get_client(chain_id).await?;

   let support = erc165::probe(client.clone(), address).await?;

   if !support.is_nft() {
      bail!("{address} is not an NFT contract");
   }

   let collection = NftCollection::fetch(client.clone(), chain_id, address).await?;
   let name = collection
      .name
      .clone()
      .filter(|name| !name.trim().is_empty())
      .unwrap_or_else(|| truncate_address(address.to_string()));

   // `NftCollection::fetch` probes ERC-165 as well; the enumerable bit is not part of the collection,
   // so this costs one extra sweep — paid once, on an explicit click.
   if !support.is_erc721_enumerable() {
      let standard = match support.is_erc1155() {
         true => NftStandard::Erc1155,
         false => NftStandard::Erc721,
      };

      return Ok(CollectionAdd::NotEnumerable { name, standard });
   }

   let token_ids = match collections_of(client.clone(), chain_id, owner, &[address])
      .await?
      .into_iter()
      .next()
   {
      Some(holding) => holding.token_ids,
      // Enumerable, and the wallet holds none: a real answer, not a failure.
      None => return Ok(CollectionAdd::NoTokens { name }),
   };

   // Two aggregates for every id's `tokenURI`, instead of one call per token.
   let refs: Vec<NftRef> = token_ids.iter().map(|id| (address, *id)).collect();
   let lookups = get_erc721_owners_and_uris(client, refs, None).await?;

   let tokens: Vec<NftToken> = token_ids
      .into_iter()
      .zip(lookups)
      .map(|(token_id, lookup)| NftToken {
         chain_id,
         collection: address,
         token_id,
         standard: NftStandard::Erc721,
         metadata_uri: lookup.token_uri,
      })
      .collect();

   let count = tokens.len();
   let for_portfolio = tokens.clone();

   ctx.write(|ctx| {
      ctx.nft_db.insert_collection(chain_id, collection);

      for token in tokens {
         ctx.nft_db.insert_nft(token);
      }
   });

   ctx.write_wallet_state(|ws| {
      let mut portfolio = ws.portfolio_db.get(chain_id, owner);

      for token in for_portfolio {
         portfolio.add_nft(token);
      }

      ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
   });

   // Logs internally.
   ctx.save_nft_db();

   if let Err(e) = ctx.save_wallet_state() {
      tracing::error!(
         "Error saving wallet state after adding a collection: {:?}",
         e
      );
   }

   Ok(CollectionAdd::Tracked { name, count })
}

/// Add an NFT to the wallet's portfolio, or take it back out.
///
/// Two stores hold NFTs and they answer different questions: the catalog (`nft_db`) is everything ever
/// discovered and is what the picker lists, while the portfolio is the wallet's own list, shown in the
/// portfolio UI and refreshed by the private scan. Discovery fills the catalog by itself; this is the
/// user deciding what to keep, so the two writes stay separate.
fn set_nft_in_portfolio(chain_id: u64, owner: Address, token: NftToken, add: bool) {
   RT.spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

      ctx.write_wallet_state(|ws| {
         let mut portfolio = ws.portfolio_db.get(chain_id, owner);

         match add {
            true => portfolio.add_nft(token),
            false => portfolio.remove_nft(&token),
         }

         ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
      });

      if let Err(e) = ctx.save_wallet_state() {
         tracing::error!(
            "Error saving wallet state after an NFT portfolio update: {:?}",
            e
         );
      }

      // Drop the picker's rows so its own loader rebuilds them with the new state on the next frame,
      // the same way `delete_nft` does.
      SHARED_GUI.write(|gui| {
         gui.token_selection.clear_processed_nfts();
         gui.request_repaint();
      });
   });
}

/// Delete an NFT: untrack it, drop it from the wallet's portfolio, and forget its cached art.
///
/// Mirrors [`delete_token`], including clearing both stores — the row would come back from whichever
/// one still lists the token. The list is not rebuilt here: clearing it makes the picker's own loader
/// refetch on the next frame.
fn delete_nft(chain_id: u64, owner: Address, token: NftToken, name: String) {
   RT.spawn(async move {
      SHARED_GUI.write(|gui| {
         gui.confirm_window.open(format!("Delete {name} #{}?", token.token_id));
         gui.request_repaint();
      });

      let confirmed = loop {
         tokio::time::sleep(Duration::from_millis(50)).await;
         let confirmed = SHARED_GUI.read(|gui| gui.confirm_window.get_confirm());
         if let Some(confirmed) = confirmed {
            SHARED_GUI.write(|gui| {
               gui.confirm_window.reset();
            });
            break confirmed;
         }
      };

      if !confirmed {
         return;
      }

      let collection = token.collection;
      let token_id = token.token_id;

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         ctx.write(|ctx| {
            ctx.nft_db.remove_nft(chain_id, collection, token_id);
         });

         ctx.write_wallet_state(|ws| {
            let mut portfolio = ws.portfolio_db.get(chain_id, owner);
            portfolio.remove_nft(&token);
            ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
         });

         // Logs internally.
         ctx.save_nft_db();

         if let Err(e) = ctx.save_wallet_state() {
            tracing::error!(
               "Error saving wallet state after NFT delete: {:?}",
               e
            );
         }

         if let Err(e) = crate::assets::icons::delete_nft_icon(chain_id, collection, token_id) {
            tracing::error!("Error deleting NFT icon: {:?}", e);
         }

         SHARED_GUI.write(|gui| {
            gui.icons.nfts.remove_icon(&(collection, chain_id, token_id));
            gui.token_selection.clear_processed_nfts();
            gui.request_repaint();
         });
      });
   });
}

#[cfg(test)]
mod tests {
   use super::*;
   use eframe::egui::{CentralPanel, Context, Pos2, RawInput, Rect};
   use egui_elements::ThemeKind;
   use zeus_eth::nft::NftStandard;

   /// The ownership badge: an unanswered question draws nothing, a held ERC-1155 says how many, and a
   /// token the chain says is gone reads "not owned" rather than blank — the catalog goes on listing it
   /// on purpose, so the row has to say which is which.
   #[test]
   fn ownership_badge_text_and_tone() {
      assert!(ownership_label(None, 0).is_none());

      assert_eq!(
         ownership_label(Some(true), 1),
         Some(("Owned".to_string(), BadgeTone::Ok))
      );
      assert_eq!(
         ownership_label(Some(true), 3),
         Some(("Owned ×3".to_string(), BadgeTone::Ok))
      );
      assert_eq!(
         ownership_label(Some(false), 0),
         Some(("Not owned".to_string(), BadgeTone::Neutral))
      );

      // A not-owned row must never advertise a quantity, whatever the balance field happens to hold.
      assert_eq!(
         ownership_label(Some(false), 7),
         Some(("Not owned".to_string(), BadgeTone::Neutral))
      );
   }

   /// Privacy mode's rows come from the portfolio's private holdings — no network involved — with the
   /// collection's cached name and symbol where there is one, and the address where there is not, so a
   /// shielded token is never a blank row.
   #[test]
   fn private_nft_rows_are_built_from_the_private_holdings() {
      let collection = NftCollection {
         chain_id: 1,
         address: Address::from([0xbc; 20]),
         standard: NftStandard::Erc721,
         name: Some("BoredApeYachtClub".to_string()),
         symbol: Some("BAYC".to_string()),
      };

      let collections: HashMap<Address, NftCollection> =
         [(collection.address, collection)].into_iter().collect();

      let known = NftToken {
         chain_id: 1,
         collection: Address::from([0xbc; 20]),
         token_id: U256::from(1),
         standard: NftStandard::Erc721,
         metadata_uri: None,
      };
      let uncached = NftToken {
         chain_id: 1,
         collection: Address::from([0xdd; 20]),
         token_id: U256::from(2),
         standard: NftStandard::Erc721,
         metadata_uri: None,
      };

      let rows = private_nft_rows(&[known, uncached], &collections, &HashMap::new());

      assert_eq!(rows.len(), 2);
      assert_eq!(rows[0].label(), "BoredApeYachtClub #1");
      assert_eq!(rows[0].subtitle(), "BAYC · ERC-721");
      assert_eq!(
         rows[0].balance, 1,
         "an ERC-721 is held or it is not"
      );

      assert_eq!(
         rows[1].label(),
         format!(
            "{} #2",
            truncate_address(Address::from([0xdd; 20]).to_string())
         )
      );
      assert_eq!(rows[1].subtitle(), "ERC-721", "no symbol to show");
   }

   /// An ERC-1155 is a quantity, and the only place a *private* quantity exists is the note the scan read:
   /// the row has to carry it, so an unshield knows how much it may spend.
   #[test]
   fn a_private_erc1155_row_carries_its_note_count() {
      let collection = Address::from([0xbc; 20]);
      let batch = NftToken {
         chain_id: 1,
         collection,
         token_id: U256::from(3),
         standard: NftStandard::Erc1155,
         metadata_uri: None,
      };

      let amounts: HashMap<NftRef, u64> =
         [((collection, U256::from(3)), 3u64)].into_iter().collect();

      let rows = private_nft_rows(&[batch], &HashMap::new(), &amounts);

      assert_eq!(rows.len(), 1);
      assert_eq!(
         rows[0].balance, 3,
         "the note's count, not a floor of one"
      );
      assert!(
         rows[0].owned == Some(true),
         "shielded is held, by definition"
      );
   }

   /// The picker still opens on the ERC-20 list, and the mode only ever changes when something asks
   /// for it — that is the "no breaking change" part of adding NFT mode.
   ///
   /// `open` is not exercised here: it spawns the balance fetch through `RT` + `SHARED_GUI`, which a
   /// unit test cannot drive. Its mode reset is the same single assignment `reset` uses.
   #[test]
   fn the_picker_opens_on_tokens_and_reset_restores_it() {
      let mut picker = TokenSelectionWindow::new();
      assert_eq!(picker.get_mode(), PickerMode::Fungible);

      picker.set_mode(PickerMode::Nft);
      assert_eq!(picker.get_mode(), PickerMode::Nft);

      picker.reset();
      assert_eq!(
         picker.get_mode(),
         PickerMode::Fungible,
         "a closed and reopened picker must not come back in NFT mode"
      );
   }

   fn nft(token_id: u64, standard: NftStandard) -> NftToken {
      NftToken {
         chain_id: 1,
         collection: Address::from([0xbc; 20]),
         token_id: U256::from(token_id),
         standard,
         metadata_uri: None,
      }
   }

   fn row(token_id: u64, name: &str, symbol: &str) -> NftRow {
      NftRow {
         token: nft(token_id, NftStandard::Erc721),
         balance: 1,
         // A freshly discovered token: in the catalog, not yet in the portfolio, and not asked of the
         // chain here — the search filter under test does not read ownership.
         owned: None,
         name: name.to_string(),
         symbol: symbol.to_string(),
         in_portfolio: false,
      }
   }

   /// A reload for a new asset context drops the NFT list with the fungible one, and cancels a fetch
   /// still in flight. `load_nfts` only fetches while `nfts_loaded` is false, so a chain, wallet or
   /// privacy-mode switch that reloaded just the fungibles would leave the NFT tab on the old
   /// context's rows — and a row picked from there seeds a send with a foreign-chain token.
   ///
   /// Driven through `begin_asset_reload`, not `process_currencies`: the latter spawns through `RT` +
   /// `SHARED_GUI`, which a unit test cannot run (the same reason `open` is not exercised here).
   #[test]
   fn an_asset_reload_drops_the_nft_list_and_cancels_its_fetch() {
      let mut picker = TokenSelectionWindow::new();
      picker.processed_nfts.push(row(1, "BoredApeYachtClub", "BAYC"));
      picker.nfts_loaded = true;
      picker.nfts_loading = true;
      let generation = picker.nfts_generation;

      picker.begin_asset_reload();

      assert!(
         picker.processed_nfts.is_empty(),
         "rows fetched for the previous (chain, owner) must not survive the reload"
      );
      assert!(
         !picker.nfts_loaded,
         "the next frame has to refetch for the new context"
      );
      assert!(!picker.nfts_loading);
      assert_ne!(
         picker.nfts_generation, generation,
         "a fetch already in flight belongs to the context just dropped"
      );
      assert!(
         picker.loading,
         "the fungible reload is marked in progress"
      );
   }

   /// Search matches what the plan listed — token id, collection name, collection symbol — plus the
   /// exact collection address, the same way the token search matches an ERC-20 address.
   #[test]
   fn nft_search_matches_id_name_symbol_and_collection_address() {
      let picker = TokenSelectionWindow::new();
      let bayc = row(1234, "BoredApeYachtClub", "BAYC");

      assert!(
         picker.valid_nft_search(&bayc, ""),
         "an empty query shows everything"
      );
      assert!(
         picker.valid_nft_search(&bayc, "1234"),
         "the token id"
      );
      assert!(
         picker.valid_nft_search(&bayc, "23"),
         "part of a token id"
      );
      assert!(
         picker.valid_nft_search(&bayc, "Bored"),
         "name, case-insensitively"
      );
      assert!(picker.valid_nft_search(&bayc, "bayc"), "symbol");
      assert!(
         picker.valid_nft_search(&bayc, "  bayc  "),
         "a padded query"
      );
      assert!(
         picker.valid_nft_search(&bayc, &bayc.token.collection.to_string()),
         "the collection address"
      );

      assert!(
         !picker.valid_nft_search(&bayc, "punk"),
         "no match at all"
      );
      assert!(
         !picker.valid_nft_search(&bayc, &Address::from([0xee; 20]).to_string()),
         "another collection's address"
      );
   }

   /// The three answers an address paste can give have to read differently: "cannot be listed" is not
   /// "you own none", and neither is the silent empty list this replaced.
   #[test]
   fn the_collection_add_outcomes_are_distinguishable() {
      let one = CollectionAdd::Tracked {
         name: "BAYC".to_string(),
         count: 1,
      };
      assert_eq!(one.message(), "Added 1 token from BAYC");
      assert!(
         !one.is_error(),
         "a successful add is a toast, not a window"
      );

      let many = CollectionAdd::Tracked {
         name: "BAYC".to_string(),
         count: 4,
      };
      assert_eq!(many.message(), "Added 4 tokens from BAYC");

      let none = CollectionAdd::NoTokens {
         name: "BAYC".to_string(),
      };
      assert!(
         none.message().contains("holds no tokens"),
         "{}",
         none.message()
      );
      assert!(
         !none.is_error(),
         "owning none is an answer, not a failure"
      );

      for standard in [NftStandard::Erc1155, NftStandard::Erc721] {
         let unlistable = CollectionAdd::NotEnumerable {
            name: "BAYC".to_string(),
            standard,
         };
         assert!(
            unlistable.is_error(),
            "{standard:?} needs a window"
         );
         assert!(unlistable.message().contains("BAYC"));
         assert!(unlistable.message().contains("cannot be listed"));
      }
   }

   /// Both stores are real sources: a held token that was never added, and a tracked token, both
   /// belong in the list — with the token they share collapsed into one entry.
   #[test]
   fn the_tracked_and_held_lists_are_unioned_and_deduped() {
      let tracked = vec![nft(1, NftStandard::Erc721)];
      let held = vec![nft(1, NftStandard::Erc721), nft(2, NftStandard::Erc721)];

      let merged = merge_nft_sources(tracked, held);

      assert_eq!(merged.len(), 2, "the shared token is one entry");
      assert!(merged.contains(&nft(1, NftStandard::Erc721)));
      assert!(merged.contains(&nft(2, NftStandard::Erc721)));
   }

   /// Order comes from the token identity, not from the order the two stores happened to hand things
   /// over, so the list does not reshuffle between loads.
   #[test]
   fn the_merged_list_is_ordered_by_identity() {
      let tracked = vec![nft(9, NftStandard::Erc721)];
      let held = vec![nft(2, NftStandard::Erc721), nft(5, NftStandard::Erc721)];

      let ids: Vec<U256> =
         merge_nft_sources(tracked, held).iter().map(|token| token.token_id).collect();

      assert_eq!(
         ids,
         vec![U256::from(2), U256::from(5), U256::from(9)]
      );
   }

   /// A cached `metadata_uri` must survive the union from either side: losing it costs an on-chain
   /// `tokenURI` read.
   #[test]
   fn a_cached_metadata_uri_survives_the_union() {
      let mut tracked = nft(1, NftStandard::Erc721);
      tracked.metadata_uri = Some("ipfs://QmTracked/1".to_string());

      let merged = merge_nft_sources(vec![tracked], vec![nft(1, NftStandard::Erc721)]);

      assert_eq!(
         merged[0].metadata_uri.as_deref(),
         Some("ipfs://QmTracked/1"),
         "the tracked entry's URI survives the portfolio's copy"
      );

      // ...and the other direction: an entry that never fetched one takes it from the other side.
      let mut held = nft(1, NftStandard::Erc721);
      held.metadata_uri = Some("ipfs://QmHeld/1".to_string());

      let merged = merge_nft_sources(vec![nft(1, NftStandard::Erc721)], vec![held]);

      assert_eq!(
         merged[0].metadata_uri.as_deref(),
         Some("ipfs://QmHeld/1")
      );
   }

   /// Holding an ERC-721 is a 1 — no call spent on it, not even a map lookup — while an ERC-1155
   /// takes the amount the batched `balanceOf` reported.
   #[test]
   fn erc721_is_one_and_erc1155_takes_its_amount() {
      let tokens = vec![nft(1, NftStandard::Erc721), nft(2, NftStandard::Erc1155)];

      let mut amounts = HashMap::new();
      amounts.insert((Address::from([0xbc; 20]), U256::from(2)), 7u64);

      let with_balances = attach_balances(tokens, &amounts);

      assert_eq!(
         with_balances[0].1, 1,
         "an owned 721 is a single token"
      );
      assert_eq!(
         with_balances[1].1, 7,
         "the 1155 amount comes from the chain"
      );
   }

   /// An ERC-1155 the chain did not answer for is listed at 0 rather than dropped: a failed
   /// `balanceOf` is not the same event as the token being removed from the catalog.
   #[test]
   fn an_erc1155_the_chain_did_not_answer_for_is_zero_not_dropped() {
      let tokens = vec![nft(1, NftStandard::Erc1155)];

      let with_balances = attach_balances(tokens, &HashMap::new());

      assert_eq!(
         with_balances.len(),
         1,
         "the entry is still listed"
      );
      assert_eq!(with_balances[0].1, 0);
   }

   fn collection(name: Option<&str>, symbol: Option<&str>) -> NftCollection {
      NftCollection {
         chain_id: 1,
         address: Address::from([0xbc; 20]),
         standard: NftStandard::Erc721,
         name: name.map(str::to_string),
         symbol: symbol.map(str::to_string),
      }
   }

   /// A collection with no cached metadata still yields an identifiable row: the address is what the
   /// user would paste into an explorer anyway.
   #[test]
   fn a_collection_without_cached_metadata_falls_back_to_its_address() {
      let (fallback, symbol) = collection_label(&nft(1, NftStandard::Erc721), &HashMap::new());

      assert!(fallback.starts_with("0x"), "{fallback}");
      assert!(symbol.is_empty(), "there is no symbol to show");

      // ...and cached metadata wins over the fallback.
      let mut collections = HashMap::new();
      collections.insert(
         Address::from([0xbc; 20]),
         collection(Some("BoredApeYachtClub"), Some("BAYC")),
      );

      let (name, symbol) = collection_label(&nft(1, NftStandard::Erc721), &collections);

      assert_eq!(name, "BoredApeYachtClub");
      assert_eq!(symbol, "BAYC");
   }

   /// A contract that answers `""` is the same as one that never implemented `name()`: a blank row
   /// would be unusable, so both fall back.
   #[test]
   fn a_blank_collection_name_is_treated_as_missing() {
      let mut collections = HashMap::new();
      collections.insert(
         Address::from([0xbc; 20]),
         collection(Some("   "), Some("")),
      );

      let (name, symbol) = collection_label(&nft(1, NftStandard::Erc721), &collections);

      assert!(name.starts_with("0x"), "{name}");
      assert!(symbol.is_empty());
   }

   /// The row reads as `<collection> #<id>` over `<symbol> · <standard>`, and drops the separator when
   /// the collection has no symbol to show.
   #[test]
   fn the_row_reads_as_name_id_then_symbol_and_standard() {
      let row = NftRow {
         token: nft(7, NftStandard::Erc1155),
         balance: 3,
         owned: Some(true),
         name: "BoredApeYachtClub".to_string(),
         symbol: "BAYC".to_string(),
         in_portfolio: false,
      };

      assert_eq!(row.label(), "BoredApeYachtClub #7");
      assert_eq!(row.subtitle(), "BAYC · ERC-1155");

      let without_symbol = NftRow {
         symbol: String::new(),
         ..row.clone()
      };

      assert_eq!(without_symbol.subtitle(), "ERC-1155");
   }

   /// A row builder for the ordering test: the same token, with ownership answered or not.
   fn owned(token_id: u64, owned: Option<bool>) -> NftRow {
      NftRow {
         owned,
         ..row(token_id, "Zeus Test", "ZEUS")
      }
   }

   /// Owned rows come first; everything else keeps the identity order the merge produced. The catalog
   /// goes on listing tokens the wallet has lost, so without this the picker leads with tokens the user
   /// cannot do anything with.
   #[test]
   fn the_owned_nfts_are_listed_first() {
      let mut rows = vec![
         owned(1, Some(false)),
         owned(2, Some(true)),
         owned(3, None),
         owned(4, Some(true)),
      ];

      sort_owned_first(&mut rows);

      let order: Vec<u64> = rows.iter().map(|row| row.token.token_id.to::<u64>()).collect();

      // The two held ones, then the unanswered and the known-absent ones in the order they arrived: a
      // `None` makes no claim, so it is not sorted as though it were known to be absent.
      assert_eq!(order, vec![2, 4, 1, 3]);
   }

   /// The mode switch is centred by padding its row with half of the leftover width, so that width has
   /// to be the width the two buttons actually render — `egui_elements::Button` is
   /// `label + 2 * button_padding.x`, and every theme's typography changes the number. Padding by a
   /// guess leaves the pair visibly off centre (or overflowing), so this pins the measurement against
   /// the real widgets in every theme.
   #[test]
   fn the_mode_switch_width_matches_what_the_buttons_render() {
      for kind in [
         ThemeKind::TokyoNight,
         ThemeKind::TokyoNightLight,
         ThemeKind::McLaren650Gts,
         ThemeKind::Reverie,
         ThemeKind::ShadeSanctuary,
         ThemeKind::Wasp,
         ThemeKind::WaspLight,
      ] {
         let ctx = Context::default();
         let mut theme = Theme::new(kind.clone());
         theme.install(&ctx);

         let mut measured = 0.0f32;
         let mut rendered = 0.0f32;

         let mut out = ctx.run_ui(
            RawInput {
               screen_rect: Some(Rect::from_min_size(
                  Pos2::ZERO,
                  vec2(600.0, 200.0),
               )),
               ..Default::default()
            },
            |ctx| {
               CentralPanel::default().show(ctx, |ui| {
                  measured = mode_switch_width(&theme, ui);

                  let visuals = theme.button_visuals();
                  let mut left = 0.0;
                  let mut right = 0.0;

                  ui.horizontal(|ui| {
                     let tokens = ui.add(
                        Button::selectable(
                           true,
                           RichText::new("Tokens").size(theme.typography.large),
                        )
                        .visuals(visuals),
                     );
                     left = tokens.rect.min.x;

                     ui.add_space(theme.spacing.sm);

                     let nfts = ui.add(
                        Button::selectable(
                           false,
                           RichText::new("NFTs").size(theme.typography.large),
                        )
                        .visuals(visuals),
                     );
                     right = nfts.rect.max.x;
                  });

                  rendered = right - left;
               });
            },
         );

         // Dropping the output with unapplied texture deltas panics the test.
         out.textures_delta.clear();

         assert!(
            (measured - rendered).abs() < 0.5,
            "{kind:?}: padded by {measured}, the buttons render {rendered}"
         );
      }
   }
}
