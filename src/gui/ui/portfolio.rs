//! This is the UI that shows the portfolio of the current wallet
//!
//! Showed when Home is selected

use crate::assets::icons::Icons;
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::{
   SHARED_GUI,
   ui::{
      common::{name_text, show_with_fade},
      token_selection::{
         PickerMode, TokenSelectionWindow, cached_collections, nft_collection_name, ownership_badge,
      },
   },
};
use crate::utils::nft_icon::start_nft_art_downloads;
use eframe::egui::{
   Align, CornerRadius, CursorIcon, Layout, Margin, Order, RichText, Role, TextWrapMode, Ui,
   WidgetInfo, vec2,
};
use std::collections::HashMap;
use std::sync::Arc;

use egui_elements::{
   Button, Frame, Label, Modal, Spinner, Tabs, TabsStyle, Theme,
   layout::{Column, Table, TableScroll},
   visuals::ButtonVisuals,
};
use egui_lucide::Lucide;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
   nft::{NftCollection, NftStandard, NftToken},
   utils::NumericValue,
};

const NFT_PREVIEW_WIDTH: f32 = 450.0;

/// Which asset class the portfolio is showing.
///
/// An NFT has no price, no balance and no value, so the two lists share nothing but their shape.
#[derive(Copy, Clone, PartialEq, Eq)]
enum PortfolioMode {
   Tokens,
   Nfts,
}

/// What one build of the panel's rows was made for.
///
/// Compared on every frame, so it is `Copy` and holds nothing borrowed — the portfolio itself is
/// a deep clone of its token, NFT and value lists, and reading it is what must not happen on the
/// frame path. `generation` is bumped by `WalletPortfolio` whenever the portfolio changes, so an
/// update invalidates the rows without the panel asking for one.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PortfolioKey {
   owner: Address,
   chain_id: u64,
   privacy_mode: bool,
   generation: u64,
}

/// The artwork modal's contents: the token, and the collection it belongs to.
///
/// The collection is resolved when the preview *opens* rather than in the modal's body: the modal is
/// drawn on every frame it is up, and looking the collection up there read the whole catalog each time
/// to find one name.
#[derive(Clone)]
struct NftPreview {
   token: NftToken,
   collection: Option<NftCollection>,
}

/// One row of the portfolio's token list: the strings a row shows, plus what its Remove needs.
///
/// The strings are resolved by the worker, not by cell closures that run twice on the frame they
/// measure; the icon is resolved while the cell is drawn, because a texture downloaded later must reach
/// the row without waiting for a portfolio update.
struct TokenRow {
   symbol: String,
   name: String,
   price: String,
   balance: String,
   value: String,
   /// The token `Remove` drops; `None` for the chain's native currency, which cannot be removed.
   token: Option<ERC20Token>,
}

/// One row of the NFT list, with its labels already resolved.
struct NftRow {
   token: NftToken,
   /// "<collection> #<token id>" — the format the row and the artwork preview use.
   title: String,
   /// The second line: the collection symbol and the standard, e.g. BAYC · ERC-721.
   subtitle: String,
   /// How much of it the wallet holds **privately**, per the portfolio's own scan, or `None` on
   /// the public side — whose answer is the balance manager's and is read when the row is drawn,
   /// because it moves without a portfolio update.
   private_holding: Option<u64>,
}

/// The rows the panel paints, built off the GUI thread.
#[derive(Default)]
struct PortfolioRows {
   /// The key these rows were built for; `None` before the first build.
   key: Option<PortfolioKey>,
   /// Whether the rows below are the ones built for `key`, rather than the previous build's still
   /// on screen while a new one runs.
   landed: bool,
   /// Total value of the side on screen — public, or private in privacy mode.
   total_value: NumericValue,
   tokens: Vec<TokenRow>,
   nfts: Vec<NftRow>,
   /// Collection metadata for the chain: the artwork preview is opened long after its row was
   /// built, so the resolved map rides with the rows.
   collections: HashMap<Address, NftCollection>,
}

impl PortfolioRows {
   /// The total the header draws, once the rows under it are the ones for the current key.
   ///
   /// `None` while a build is in flight: the number on screen is the previous build's, and a total
   /// that is about to be replaced is not one to read out.
   fn shown_total(&self) -> Option<String> {
      self.landed.then(|| total_text(&self.total_value))
   }
}

pub struct PortfolioUi {
   open: bool,
   _loading: bool,
   pub show_spinner: bool,
   mode: PortfolioMode,
   /// What the artwork modal is showing, if it is open.
   preview: Option<NftPreview>,
   /// Which (chain, wallet) the NFT list has been kicked off for.
   ///
   /// A marker that the ask went out, not an answer — the answers live in the balance manager, beside the
   /// token balances. It is here so that opening the list starts one refresh rather than one per frame.
   nft_list_asked_for: Option<(u64, Address)>,
   /// Which `(chain, wallet)` the list currently on screen belongs to.
   ///
   /// The NFT preview belongs to it too: switching either must not leave the previous token's artwork
   /// over the new list, and a reopen must not re-pop it.
   shown_for: Option<(u64, Address)>,
   /// The rows on screen, rebuilt when the key under them changes.
   rows: PortfolioRows,
   /// The total last read out on this visit, so the same number is not said twice.
   spoken_total: Option<String>,
   /// The portfolio generation the user's own Refresh was pressed against, until the rows it
   /// produced have landed and been read out.
   refresh_from: Option<u64>,
   /// The words the page hands back to the frame, which owns the announcement channel.
   notice: Option<String>,
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: false,
         _loading: false,
         show_spinner: false,
         mode: PortfolioMode::Tokens,
         preview: None,
         nft_list_asked_for: None,
         shown_for: None,
         rows: PortfolioRows::default(),
         spoken_total: None,
         refresh_from: None,
         notice: None,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
      // A reopen starts a new list; the previous session's preview must not re-appear over it.
      self.preview = None;
      // Leaving re-arms the total: coming back is a new arrival, and the number may have moved
      // while the page was away. The refresh marker goes with it — a press whose answer nobody
      // was on the page to hear is not one to speak on the way back.
      self.spoken_total = None;
      self.refresh_from = None;
   }

   /// The words this page should say, drained by the frame right after it is drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      show_with_fade(ui, "portfolio_ui_fade", self.open, |ui| {
         let chain_id = ctx.chain.id();
         let wallet_info = ctx.current_wallet_info();
         let privacy_mode = ctx.privacy_mode;
         let owner = wallet_info.address;

         // The preview belongs to the (chain, wallet) it was opened under: switching either must not
         // leave the previous token's artwork over the new list.
         if self.shown_for != Some((chain_id, owner)) {
            self.shown_for = Some((chain_id, owner));
            self.preview = None;
            // Rows built for another wallet are a claim about another wallet, not merely old data.
            self.rows = PortfolioRows::default();
            // A refresh pressed for another wallet is not an answer about this one, and this wallet
            // was not asked about at all.
            self.refresh_from = None;
         }

         let key = PortfolioKey {
            owner,
            chain_id,
            privacy_mode,
            generation: ctx.read_wallet_state(|ws| ws.portfolio_db.generation(chain_id, owner)),
         };

         // Claimed before the spawn, so a build that never lands cannot start one per frame.
         if self.rows.key != Some(key) {
            self.rows.key = Some(key);
            // What is on screen is the previous build's until the new rows land.
            self.rows.landed = false;
            Self::build_rows(key);
         }

         // The rows are resolved off the GUI thread, so the page's first frames have no total and
         // one arrives a frame or two after the page opens. Only an arrival and the user's own
         // Refresh are read out: a rebuild and a wallet switch move the number without anything
         // being asked of it, and a total read out on every frame it changes is noise.
         let shown_total = self.rows.shown_total();
         let refreshed = self.refresh_from.is_some_and(|generation| generation != key.generation);

         if let Some(text) = announcement(
            self.spoken_total.as_deref(),
            shown_total.as_deref(),
            refreshed,
            ctx.chain.name(),
         ) {
            self.notice = Some(text);
         }

         if let Some(total) = &shown_total {
            self.spoken_total = Some(total.clone());

            // The rows for the newer generation have landed: the refresh's answer is on screen now,
            // whether or not it moved the number.
            if refreshed {
               self.refresh_from = None;
            }
         }

         Frame::new().outer_margin(Margin::same(5)).show(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
               ui.set_width(ui.available_width() * 0.7);

               ui.spacing_mut().item_spacing = vec2(theme.spacing.lg, theme.spacing.md);

               let normal = theme.typography.normal;
               let large = theme.typography.large;

               let warning = theme.colors.warning;

               let frame = theme.frame1;

               frame.show(ui, |ui| {
                  // Mode switch, on top of the wallet identity: what is being listed, before whose
                  // wallet it is.
                  ui.horizontal(|ui| {
                     let mode = self.mode;
                     let mut tab = usize::from(mode == PortfolioMode::Nfts);

                     Tabs::new("portfolio_tabs", &mut tab, ["Tokens", "NFTs"])
                        .style(TabsStyle::Underline)
                        .fill_width(false)
                        .show(ui);

                     let picked = match tab {
                        0 => PortfolioMode::Tokens,
                        _ => PortfolioMode::Nfts,
                     };

                     if picked != mode {
                        self.mode = picked;
                     }
                  });

                  ui.horizontal(|ui| {
                     // Wallet Name - Total Value (centered)
                     ui.vertical_centered(|ui| {
                        ui.label(
                           RichText::new(wallet_info.name_with_source())
                              .size(theme.typography.very_large),
                        );
                        let total_value = total_text(&self.rows.total_value);

                        let total = ui.label(
                           RichText::new(total_value.as_str())
                              .heading()
                              .size(theme.typography.heading + 4.0),
                        );

                        // The number on its own says neither what it is nor which chain it is on.
                        name_text(
                           ui,
                           &total,
                           total_sentence(ctx.chain.name(), &total_value),
                        );
                     });

                     // Refresh - Add Token (right)
                     ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().button_padding = theme.button_padding;

                        let button_visuals = theme.button_visuals();
                        let (label, nft_mode) = match self.mode {
                           PortfolioMode::Tokens => ("Add Token", false),
                           PortfolioMode::Nfts => ("Add NFT", true),
                        };
                        let text = RichText::new(label).size(normal);
                        let add_token = Button::new(text);

                        if ui.add(add_token).clicked() {
                           token_selection.open(privacy_mode, chain_id, owner);

                           // The picker is the one place that lists discovered-but-untracked tokens,
                           // so picking one there is how it joins the portfolio. It opens on the
                           // ERC-20 list, which is what the token mode wants anyway.
                           if nft_mode {
                              token_selection.set_mode(PickerMode::Nft);
                           }
                        }

                        let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();

                        if !self.show_spinner {
                           let visuals = ButtonVisuals {
                              bg_hover: button_visuals.bg_hover,
                              corner_radius: CornerRadius::same(25),
                              ..Default::default()
                           };
                           let button = Button::image(icon).small().visuals(visuals);
                           let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

                           // The button is a picture of a refresh arrow and says nothing else.
                           res.widget_info(|| {
                              WidgetInfo::labeled(
                                 Role::Button,
                                 ui.is_enabled(),
                                 "Refresh portfolio",
                              )
                           });

                           if res.clicked() {
                              self.refresh(owner, key.generation);

                              // Let the NFT list be asked for again: this is how a token that has just
                              // left — or one that has come back — updates its badge.
                              self.nft_list_asked_for = None;
                           }
                        } else {
                           ui.add(Spinner::new().size(17.0));
                        }
                     });
                  });
               });

               if privacy_mode
                  && ctx.railgun_is_supported(ctx.chain)
                  && !ctx.is_railgun_enabled(chain_id)
               {
                  ui.label(RichText::new("Railgun is disabled").size(large).color(warning));
                  ui.label(
                     RichText::new("Enable it in Settings/Railgun to see private balances.")
                        .size(normal),
                  );
               }

               let tint = theme.image_tint_recommended;

               // One refresh per (chain, wallet): the answers themselves are the balance manager's, and
               // this marker only keeps the ask from going out every frame. Entering the mode, a refresh,
               // a wallet or chain switch and adding a token all land here.
               if self.mode == PortfolioMode::Nfts
                  && self.nft_list_asked_for != Some((chain_id, owner))
               {
                  self.nft_list_asked_for = Some((chain_id, owner));
                  Self::load_nft_list(chain_id, owner);
               }

               if self.mode == PortfolioMode::Nfts {
                  self.show_nft_list(
                     ctx,
                     theme,
                     &icons,
                     chain_id,
                     owner,
                     privacy_mode,
                     tint,
                     ui,
                  );
               } else {
                  // Token List
                  let rows = &self.rows.tokens;

                  let mut remove_request: Option<ERC20Token> = None;

                  ui.spacing_mut().item_spacing.y = theme.spacing.sm;
                  ui.scope(|ui| {
                     ui.spacing_mut().button_padding =
                        vec2(theme.spacing.xs, theme.spacing.xs / 2.0);

                     let visuals = theme.frame1_visuals();
                     let inner = theme.inner_margin;
                     let corner = theme.corner_radius;
                     let row_frame = Frame::new()
                        .interactive(true)
                        .inner_margin(inner)
                        .corner_radius(corner)
                        .visuals(visuals);

                     Table::new("portfolio_tokens", rows)
                        .columns([
                           Column::Fr(2.2), // Asset
                           Column::Fr(1.8), // Price
                           Column::Fr(1.8), // Balance
                           Column::Fr(1.8), // Value
                           Column::Auto,    // Remove
                        ])
                        .gap(20.0)
                        .row_gap(theme.spacing.md)
                        .scroll(TableScroll::new().auto_shrink([false; 2]).content_margin(15))
                        .row_frame(row_frame)
                        .header_rule(true)
                        .key((chain_id, privacy_mode, rows.len()))
                        .show(ui, |t| {
                           t.header(|h| {
                              for header in ["Asset", "Price", "Balance", "Value", ""] {
                                 h.cell(|ui| {
                                    if !header.is_empty() {
                                       ui.label(RichText::new(header).strong().size(large));
                                    }
                                 });
                              }
                           });

                           t.rows(|r, row| {
                              r.cell(|ui| {
                                 let icon = match &row.token {
                                    Some(token) => {
                                       icons.token_icon_x32(token.address, token.chain_id, tint)
                                    }
                                    None => {
                                       icons.currency_icon_x32(&Currency::native(chain_id), tint)
                                    }
                                 };

                                 let text = RichText::new(&row.symbol).size(normal);

                                 let label = Label::new(text, Some(icon))
                                    .image_on_left()
                                    .wrap()
                                    .interactive(false);
                                 let asset = ui.add(label).on_hover_text(&row.name);

                                 // The full name is hover-only, so a cell read by the drawn symbol
                                 // alone loses the one word that says which token it is.
                                 let (name, symbol) = asset_name(&row.name, &row.symbol);
                                 name_text(ui, &asset, name);

                                 if let Some(symbol) = symbol {
                                    ui.ctx().accesskit_node_builder(asset.id, |node| {
                                       node.set_description(symbol.as_str());
                                    });
                                 }
                              });

                              r.cell(|ui| {
                                 let text = RichText::new(&row.price).size(normal);
                                 let label = Label::text(text);
                                 let price = ui.add(label);
                                 name_text(ui, &price, format!("Price {}", row.price));
                              });

                              r.cell(|ui| {
                                 let text = RichText::new(&row.balance).size(normal);
                                 let label = Label::text(text);
                                 let balance = ui.add(label);
                                 name_text(ui, &balance, format!("Balance {}", row.balance));
                              });

                              r.cell(|ui| {
                                 let text = RichText::new(&row.value).size(normal);
                                 let label = Label::text(text);
                                 let value = ui.add(label);
                                 name_text(ui, &value, format!("Value {}", row.value));
                              });

                              r.cell(|ui| {
                                 let Some(token) = &row.token else {
                                    return;
                                 };

                                 let text = RichText::new("X").size(normal);
                                 let remove = ui.add(Button::new(text));
                                 let name = format!("Remove {}", row.symbol);

                                 // The glyph is the same on every row; the sentence says which
                                 // token this one drops.
                                 remove.widget_info(|| {
                                    WidgetInfo::labeled(Role::Button, ui.is_enabled(), name.clone())
                                 });

                                 if remove.clicked() {
                                    remove_request = Some(token.clone());
                                 }
                              });
                           });
                        });
                  });

                  if let Some(token) = remove_request {
                     self.remove_token(ctx, owner, &token);
                  }
               }

               let currency = token_selection.get_selected_currency();

               if let Some(currency) = currency {
                  let currency = currency.clone();
                  let token_fetched = token_selection.token_fetched;
                  token_selection.reset();
                  self.add_currency(ctx, owner, token_fetched, currency);
               }

               // An NFT picked in the picker joins the portfolio the same way a token does — but only
               // while the picker is on its NFT list, so a token picked from the ERC-20 list is never
               // mistaken for one.
               let picked_nft = match token_selection.get_mode() {
                  PickerMode::Nft => token_selection.get_selected_nft().cloned(),
                  PickerMode::Fungible => None,
               };

               if let Some(nft) = picked_nft {
                  token_selection.reset();
                  Self::add_nft(ctx, owner, nft);

                  // The new token has not been asked about yet, so let the next frame's load ask.
                  self.nft_list_asked_for = None;
               }

               // The artwork at inspection size, if a row asked for it.
               self.show_nft_preview(theme, &icons, ui);
            });
         });
      });
   }

   /// Build the rows for `key` off the GUI thread.
   ///
   /// Spawned, never inline: it clones the portfolio out of the wallet state and formats every row.
   /// The writeback lands only while the key still matches, so a build for a previous wallet cannot
   /// paint over a newer one.
   fn build_rows(key: PortfolioKey) {
      crate::runtime::rt().spawn_blocking(move || {
         // The handle is only reachable once the frame has dropped `SHARED_GUI`.
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let rows = resolve_rows(&ctx, key);

         SHARED_GUI.write(|gui| {
            if gui.portofolio.rows.key != Some(key) {
               return;
            }

            gui.portofolio.rows = rows;
            gui.request_repaint();
         });
      });
   }

   /// The NFT list: the wallet's own NFTs, or the shielded ones in privacy mode.
   ///
   /// The same table as the token list, but with no column header: there is one column that means
   /// anything here, and «Price / Balance / Value» would have nothing underneath them.
   ///
   /// `privacy_mode` picks which side the rows' badges speak for, and is part of the table's key because
   /// the two sides answer differently. The labels rode with the rows; the public holding does not,
   /// because it moves without a portfolio update.
   fn show_nft_list(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Icons,
      chain_id: u64,
      owner: Address,
      privacy_mode: bool,
      tint: bool,
      ui: &mut Ui,
   ) {
      let rows = &self.rows.nfts;

      if rows.is_empty() {
         ui.add_space(theme.spacing.md);
         ui.label(
            RichText::new("Nothing here yet. Add NFT picks from the ones Zeus has found.")
               .size(theme.typography.normal)
               .color(theme.colors.text_muted),
         );
         return;
      }

      let mut preview_request: Option<NftToken> = None;
      let mut remove_request: Option<NftToken> = None;

      let normal = theme.typography.normal;

      ui.spacing_mut().item_spacing.y = theme.spacing.sm;
      ui.scope(|ui| {
         ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);

         let visuals = theme.frame1_visuals();
         let inner = theme.inner_margin;
         let corner = theme.corner_radius;
         let row_frame = Frame::new()
            .interactive(true)
            .inner_margin(inner)
            .corner_radius(corner)
            .visuals(visuals);

         Table::new("portfolio_nfts", rows)
            // The artwork's thumbnail size is the row height this list was laid out for.
            .columns([Column::Fixed(64.0), Column::Fr(1.0), Column::Auto])
            .gap(20.0)
            .row_gap(theme.spacing.md)
            .scroll(TableScroll::new().auto_shrink([false; 2]).content_margin(15))
            .row_frame(row_frame)
            .key((chain_id, privacy_mode, rows.len()))
            .show(ui, |t| {
               t.rows(|r, row| {
                  // The private count is the scan's, so it rides with the row; the public count is the
                  // balance manager's and is read now, because it moves without a portfolio update.
                  let holding = row.private_holding.or_else(|| {
                     ctx.get_nft_balance(
                        chain_id,
                        owner,
                        row.token.collection,
                        row.token.token_id,
                     )
                  });

                  r.cell(|ui| {
                     let artwork = ui.add(icons.nft_icon_x64(
                        row.token.chain_id,
                        row.token.collection,
                        row.token.token_id,
                        tint,
                     ));

                     // The thumbnail is this token's, and a picture on its own says nothing about
                     // which token that is.
                     artwork.widget_info(|| {
                        WidgetInfo::labeled(Role::Image, ui.is_enabled(), row.title.clone())
                     });
                  });

                  r.cell(|ui| {
                     // Two labels rather than one with a newline: `TextWrapMode::Truncate` keeps a
                     // label to a single line, so the second line of a two-line label would never
                     // be drawn.
                     ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing.xs;

                        let title_text = RichText::new(&row.title)
                           .size(theme.typography.normal)
                           .color(theme.colors.text);
                        let title_label = Label::new(title_text, None)
                           .wrap_mode(TextWrapMode::Truncate)
                           .interactive(false);
                        let title = ui.add(title_label).on_hover_text(&row.title);

                        // The column truncates the drawn title; the node carries the whole of it,
                        // so nothing a reader hears depends on how wide the column happens to be.
                        name_text(ui, &title, row.title.clone());

                        let subtitle_text = RichText::new(&row.subtitle)
                           .size(theme.typography.small)
                           .color(theme.colors.text_muted);
                        let subtitle_label = Label::new(subtitle_text, None)
                           .wrap_mode(TextWrapMode::Truncate)
                           .interactive(false);
                        let subtitle = ui.add(subtitle_label);
                        name_text(ui, &subtitle, row.subtitle.clone());
                     });
                  });

                  r.cell(|ui| {
                     // Left of the two buttons: whether this wallet really holds the token. The
                     // portfolio is a claim about what it holds, and the chain is what settles it.
                     ownership_badge(
                        ui,
                        holding.map(|amount| amount > 0),
                        holding.unwrap_or(0),
                     );

                     let remove = ui.add(Button::new(RichText::new("X").size(normal)));
                     let name = format!("Remove {}", row.title);

                     // The drawn X is the same glyph on every row; the sentence says whose.
                     remove.widget_info(|| {
                        WidgetInfo::labeled(Role::Button, ui.is_enabled(), name.clone())
                     });

                     if remove.clicked() {
                        remove_request = Some(row.token.clone());
                     }

                     let view = ui.add(Button::new(RichText::new("View").size(normal)));
                     let name = format!("View {}", row.title);

                     view.widget_info(|| {
                        WidgetInfo::labeled(Role::Button, ui.is_enabled(), name.clone())
                     });

                     if view.clicked() {
                        preview_request = Some(row.token.clone());
                     }
                  });
               });
            });
      });

      // Applied after the list: a removal rewrites the portfolio, and doing that from inside the row
      // loop would change the list it is iterating.
      if let Some(nft) = preview_request {
         // The collection comes from the map the rows were built with: the modal draws every frame, so the
         // lookup cannot live in its body.
         self.preview = Some(NftPreview {
            collection: self.rows.collections.get(&nft.collection).cloned(),
            token: nft,
         });
      }

      if let Some(nft) = remove_request {
         Self::remove_nft(ctx, owner, &nft);
      }
   }

   /// The artwork at inspection size, opened from a row.
   fn show_nft_preview(&mut self, theme: &Theme, icons: &Icons, ui: &mut Ui) {
      let Some(NftPreview { token, collection }) = self.preview.clone() else {
         return;
      };
      let collection = collection.as_ref();

      let title = format!(
         "{} #{}",
         nft_collection_name(collection, token.collection),
         token.token_id
      );
      let heading = RichText::new(title.as_str()).size(theme.typography.heading);
      let subtitle = Self::nft_subtitle(collection, token.standard);
      let frame = theme.window_frame.fill(theme.frame1.fill);

      let mut open = true;

      Modal::new("nft_preview", &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(heading)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(NFT_PREVIEW_WIDTH);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;

               let artwork = ui.add(icons.nft_icon_x250(
                  token.chain_id,
                  token.collection,
                  token.token_id,
                  theme.image_tint_recommended,
               ));

               // The artwork is this token's, and the picture alone says nothing about which.
               artwork
                  .widget_info(|| WidgetInfo::labeled(Role::Image, ui.is_enabled(), title.clone()));

               ui.label(
                  RichText::new(subtitle)
                     .size(theme.typography.normal)
                     .color(theme.colors.text_muted),
               );
               ui.label(
                  RichText::new(token.collection.to_string())
                     .size(theme.typography.small)
                     .color(theme.colors.text_muted)
                     .monospace(),
               );
            });
         });

      // Closing it is the user's decision to make, not ours: X, Escape and the backdrop all land here.
      if !open {
         self.preview = None;
      }
   }

   /// Kick off the NFT side of a portfolio load: ask the balance manager to refresh this wallet's public
   /// ownership, and fetch the artwork to draw.
   ///
   /// A worker, never the frame path — it reads and writes `SHARED_GUI`, which the frame holds
   /// write-locked. It keeps no answers: those are the manager's, like the token balances. The private
   /// side needs no refresh either, because it is the shielded set that the railgun scan maintains.
   fn load_nft_list(chain_id: u64, owner: Address) {
      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let portfolio = ctx.get_portfolio(chain_id, owner);

         if !portfolio.nfts().is_empty() {
            let manager = ctx.balance_manager();
            if let Err(e) = manager
               .update_nft_balances(
                  ctx.clone(),
                  chain_id,
                  owner,
                  portfolio.nfts().clone(),
                  false,
               )
               .await
            {
               tracing::error!("Error updating NFT balances: {e:?}");
            }
         }

         // Both sides' artwork: which list is on screen is the user's toggle, not ours to guess.
         start_nft_art_downloads(
            chain_id,
            portfolio.nfts().iter().chain(portfolio.private_nfts().iter()),
         );

         SHARED_GUI.write(|gui| gui.request_repaint());
      });
   }

   /// Add an NFT to the wallet's portfolio.
   ///
   /// The catalog already holds it — discovery and the picker both write `nft_db` — so this only writes
   /// the user's own list.
   fn add_nft(ctx: &mut ZeusContext, owner: Address, nft: NftToken) {
      let chain_id = ctx.chain.id();

      let mut portfolio = ctx.read_wallet_state(|ws| ws.portfolio_db.get(chain_id, owner));
      portfolio.add_nft(nft);
      ctx.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
      });

      Self::save_wallet_state();
   }

   /// Drop an NFT from the wallet's portfolio.
   ///
   /// The catalog keeps it, so the picker still lists it, marked as not owned, and it can be added back.
   fn remove_nft(ctx: &mut ZeusContext, owner: Address, nft: &NftToken) {
      let chain_id = ctx.chain.id();

      let mut portfolio = ctx.read_wallet_state(|ws| ws.portfolio_db.get(chain_id, owner));
      portfolio.remove_nft(nft);
      ctx.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
      });

      Self::save_wallet_state();
   }

   /// The second line of an NFT row or preview: the collection symbol and the standard.
   fn nft_subtitle(collection: Option<&NftCollection>, standard: NftStandard) -> String {
      let standard = match standard {
         NftStandard::Erc721 => "ERC-721",
         NftStandard::Erc1155 => "ERC-1155",
      };

      match collection.and_then(|collection| collection.symbol.as_deref()) {
         Some(symbol) if !symbol.is_empty() => format!("{symbol} · {standard}"),
         _ => standard.to_string(),
      }
   }

   /// Persist the wallet state after the portfolio changed.
   ///
   /// Off the frame path: `save_wallet_state` lives on the `ZeusCtx` handle, which is only reachable
   /// through `SHARED_GUI` — and the frame holds that write-locked.
   fn save_wallet_state() {
      crate::runtime::rt().spawn_blocking(|| {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         if let Err(e) = ctx.save_wallet_state() {
            tracing::error!(
               "Error saving wallet state after the portfolio changed: {:?}",
               e
            );
         }
      });
   }

   fn refresh(&mut self, owner: Address, generation: u64) {
      self.show_spinner = true;
      // The portfolio generation this press started from: the rows built from a newer one are its
      // answer, and only those may be read out as the result of it.
      self.refresh_from = Some(generation);
      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let chain = ctx.chain().id();
         let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);
         let portfolio = ctx.get_portfolio(chain, owner);
         let tokens = portfolio.tokens().clone();

         // Update the eth and token balances
         let balance_manager = ctx.balance_manager();

         match balance_manager.update_eth_balance(ctx.clone(), chain, vec![owner], false).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating eth balance: {:?}", e),
         }

         match balance_manager
            .update_tokens_balance(ctx.clone(), chain, owner, tokens.clone(), false)
            .await
         {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating tokens balance: {:?}", e),
         }

         // Update the token prices
         let price_manager = ctx.price_manager();
         let pool_manager = ctx.pool_manager();

         if let Err(e) =
            price_manager.calculate_prices(ctx.clone(), chain, pool_manager, tokens).await
         {
            tracing::error!("Error updating pool state: {:?}", e);
         }

         ctx.update_public_data(chain, owner);
         if privacy_mode {
            ctx.update_private_data(chain, owner).await;
         }

         SHARED_GUI.write(|gui| {
            gui.portofolio.show_spinner = false;
         });
      });
   }

   // Add a currency to the portfolio and update the portfolio value
   fn add_currency(
      &mut self,
      ctx: &mut ZeusContext,
      owner: Address,
      token_fetched: bool,
      currency: Currency,
   ) {
      if currency.is_native() {
         return;
      }

      let chain_id = ctx.chain.id();

      let mut portfolio = ctx.read_wallet_state(|ws| ws.portfolio_db.get(chain_id, owner));
      portfolio.add_token(currency.to_erc20().into_owned());
      ctx.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
      });

      let token = currency.to_erc20().into_owned();

      // if token was fetched from the blockchain, we don't need to sync the pools or the balance
      if token_fetched {
         #[cfg(feature = "dev")]
         tracing::info!(
            "Token {} was fetched from the blockchain, no need to sync pools or balance",
            token.symbol
         );
         return;
      }

      self.show_spinner = true;
      let privacy_mode = ctx.privacy_mode;

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let pool_manager = ctx.pool_manager();
         let price_manager = ctx.price_manager();
         let tokens = vec![token];

         if let Err(e) = price_manager
            .calculate_prices(
               ctx.clone(),
               chain_id,
               pool_manager,
               tokens.clone(),
            )
            .await
         {
            tracing::error!("Error updating pool state: {:?}", e);
         }

         let balance_manager = ctx.balance_manager();
         match balance_manager
            .update_tokens_balance(ctx.clone(), chain_id, owner, tokens, false)
            .await
         {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating tokens balance: {:?}", e),
         }

         ctx.update_public_data(chain_id, owner);
         if privacy_mode {
            ctx.update_private_data(chain_id, owner).await;
         }

         SHARED_GUI.write(|gui| {
            gui.portofolio.show_spinner = false;
         });
      });
   }

   fn remove_token(&mut self, ctx: &mut ZeusContext, owner: Address, token: &ERC20Token) {
      self.show_spinner = true;
      let chain = ctx.chain.id();

      let mut portfolio = ctx.read_wallet_state(|ws| ws.portfolio_db.get(chain, owner));
      portfolio.remove_token(token);
      ctx.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain, owner, portfolio);
      });

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         ctx.update_public_data(chain, owner);
         ctx.update_private_data(chain, owner).await;

         SHARED_GUI.write(|gui| {
            gui.portofolio.show_spinner = false;
         });
      });
   }
}

/// The total as the header draws it: a dollar sign and the abbreviated value, cut to ten
/// characters.
///
/// The header and every sentence about the total read it from here, so what is spoken and what is
/// drawn are the same number in the same form.
fn total_text(total_value: &NumericValue) -> String {
   format!("${:.10}", total_value.abbreviated())
}

/// The sentence a reader gets for the total: the number, and the chain it is on.
fn total_sentence(chain: &str, total: &str) -> String {
   format!("Portfolio value on {chain}, {total}")
}

/// What a token's own cell says, and the symbol it may add as a description.
///
/// The full name is hover-only, so a cell read by the drawn symbol alone loses the one word that
/// says which token it is. A name that only repeats the symbol is said once, and a token Zeus has
/// no name for is named by the symbol it draws.
fn asset_name(name: &str, symbol: &str) -> (String, Option<String>) {
   let name = name.trim();

   if name.is_empty() || name == symbol {
      (symbol.to_string(), None)
   } else {
      (name.to_string(), Some(symbol.to_string()))
   }
}

/// What a reader hears for the portfolio's total, if anything.
///
/// `previous` is the total last read out on this visit and `current` the one on screen now, which
/// is `None` until the rows behind it have landed: they are built off the GUI thread, so the page's
/// first frames have no total to read. The number moves on its own whenever a rebuild lands and on
/// every wallet switch, and neither of those is the user asking about it, so exactly two things are
/// said: the total the first time it is known on a visit, and a number the user's own Refresh
/// changed. Saying nothing is the answer for every other frame.
pub(crate) fn announcement(
   previous: Option<&str>,
   current: Option<&str>,
   refreshed: bool,
   chain: &str,
) -> Option<String> {
   let current = current?;

   match previous {
      None => Some(total_sentence(chain, current)),
      Some(previous) if refreshed && previous != current => {
         Some(format!("Portfolio refreshed. Value {current}"))
      }
      Some(_) => None,
   }
}

/// Resolve the portfolio into the rows the panel paints, off the GUI thread.
///
/// It does what the frame path used to do, in the same order: read the portfolio, take the value of the
/// side on screen, format its rows, then resolve the NFT labels from the cached collections.
fn resolve_rows(ctx: &ZeusCtx, key: PortfolioKey) -> PortfolioRows {
   let portfolio = ctx.get_portfolio(key.chain_id, key.owner);

   let total_value = match key.privacy_mode {
      false => portfolio.public_value(),
      true => portfolio.private_value(),
   };

   let token_list = if key.privacy_mode {
      portfolio.private_tokens()
   } else {
      portfolio.public_tokens()
   };

   // The native currency has no portfolio entry, so it is prepended rather than listed.
   let mut tokens = Vec::with_capacity(token_list.len() + 1);

   if !key.privacy_mode {
      let native = Currency::native(key.chain_id);

      tokens.push(TokenRow {
         symbol: native.symbol().to_string(),
         name: native.name().to_string(),
         price: format!(
            "${:.10}",
            ctx.get_currency_price(&native).abbreviated()
         ),
         balance: format!(
            "{:.10}",
            ctx.get_currency_balance(key.chain_id, key.owner, &native).abbreviated()
         ),
         value: format!(
            "${:.10}",
            ctx.read(|ctx| ctx.get_currency_value_for_owner(key.chain_id, key.owner, &native))
               .abbreviated()
         ),
         token: None,
      });
   }

   for (token, balance, value, price) in token_list {
      tokens.push(TokenRow {
         symbol: token.symbol.to_string(),
         name: token.name.to_string(),
         price: format!("${:.10}", price.abbreviated()),
         balance: format!("{:.10}", balance.abbreviated()),
         value: format!("${:.10}", value.abbreviated()),
         token: Some(token.clone()),
      });
   }

   let collections = cached_collections(ctx.read(|ctx| ctx.nft_db.get_collections(key.chain_id)));

   let nft_list = if key.privacy_mode {
      portfolio.private_nfts()
   } else {
      portfolio.nfts()
   };

   let nfts = nft_list
      .iter()
      .map(|token| {
         let collection = collections.get(&token.collection);

         // The private count is the note's; the public side's answer is the balance manager's, read
         // when the row is drawn because it moves without a portfolio update.
         let private_holding = match key.privacy_mode {
            true => Some(
               portfolio
                  .private_nft_amounts()
                  .get(&(token.collection, token.token_id))
                  .copied()
                  .unwrap_or(1),
            ),
            false => None,
         };

         NftRow {
            token: token.clone(),
            title: format!(
               "{} #{}",
               nft_collection_name(collection, token.collection),
               token.token_id
            ),
            subtitle: PortfolioUi::nft_subtitle(collection, token.standard),
            private_holding,
         }
      })
      .collect();

   PortfolioRows {
      key: Some(key),
      landed: true,
      total_value,
      tokens,
      nfts,
      collections,
   }
}
