//! This is the UI that shows the portfolio of the current wallet
//!
//! Showed when Home is selected

use crate::assets::icons::Icons;
use crate::core::{NftDB, WalletPortfolio, ZeusContext};
use crate::gui::{
   SHARED_GUI,
   ui::{
      common::show_with_fade,
      token_selection::{PickerMode, TokenSelectionWindow, nft_collection_name, ownership_badge},
   },
};
use crate::utils::nft_icon::start_nft_art_downloads;
use eframe::egui::{
   Align, CornerRadius, CursorIcon, Layout, Margin, Order, RichText, TextWrapMode, Ui, vec2,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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

/// Which (chain, wallet) — and which side of the holdings — a view was built for.
///
/// Rows are only ever drawn for the identity they were built from: another wallet's rows, or the
/// other privacy side's, are a claim about a portfolio nobody asked about. A view for a different
/// identity leaves the screen to the spinner until its own build lands.
#[derive(Copy, Clone, PartialEq, Eq)]
struct ViewIdentity {
   chain_id: u64,
   owner: Address,
   privacy_mode: bool,
}

/// The identity, plus a digest of every input its rows are built from.
///
/// The fingerprint is what keeps a cached view honest. It is taken on the frame path from reads that
/// clone nothing — token identities with the balance / value / price beside them, NFT identities with
/// this wallet's holdings and the cached collection names, and the two totals — so a balance, a price,
/// a new token or a freshly fetched collection name all land as a new key, and a frame where nothing
/// moved costs one hash instead of a rebuild.
#[derive(Copy, Clone, PartialEq, Eq)]
struct ViewKey {
   identity: ViewIdentity,
   data: u64,
}

/// Everything the portfolio draws, built off the GUI thread.
///
/// The frame path only reads this: no portfolio clone, no cell formatting and no catalog walk happen
/// while a frame is being laid out.
struct PortfolioView {
   key: ViewKey,
   /// The wallet's total, already formatted for the side this view was built from.
   total: String,
   tokens: Vec<TokenRow>,
   nfts: Vec<NftRow>,
}

/// One row of the portfolio's token list: the strings a row shows, plus what its Remove needs.
///
/// The table is data-driven, so the list is a value rather than a closure. The icon is deliberately
/// not one of these fields: artwork arrives later than the row that wants it, so it stays a frame-path
/// lookup rather than being frozen into the view.
struct TokenRow {
   symbol: String,
   name: String,
   price: String,
   balance: String,
   value: String,
   /// The token `Remove` drops; `None` for the chain's native currency, which cannot be removed.
   token: Option<ERC20Token>,
}

/// One row of the NFT list: the strings the row shows, and what its buttons act on.
struct NftRow {
   token: NftToken,
   /// `«collection» #«id»` — the id is part of the title, not an afterthought: a collection is many
   /// tokens, and which one this row is is the first thing to read.
   title: String,
   /// The collection symbol and the standard: the row's second line.
   subtitle: String,
   /// Whether this wallet really holds the token, and how many. `None` when nothing answered.
   holding: Option<u64>,
}

/// What an NFT row asked for, applied once the list has been drawn.
///
/// A removal rewrites the portfolio and a preview opens a modal over the list: doing either from
/// inside the row loop would change the list it is iterating.
#[derive(Default)]
struct NftRequests {
   preview: Option<NftPreview>,
   remove: Option<NftToken>,
}

/// The artwork modal's contents, taken from the row that asked for it.
///
/// Resolved when the preview *opens* rather than in the modal's body: the modal is drawn on every
/// frame it is up, and re-deriving its labels there would undo the point of building them once.
#[derive(Clone)]
struct NftPreview {
   token: NftToken,
   title: String,
   subtitle: String,
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
   /// The rows on screen, built off the GUI thread and replaced when its inputs move.
   view: Option<PortfolioView>,
   /// The key whose build is in flight.
   ///
   /// One build at a time: a change that lands during one is picked up by the frame after its result
   /// is applied, which keeps a burst of updates from queueing a build each.
   building: Option<ViewKey>,
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
         view: None,
         building: None,
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
         }

         let identity = ViewIdentity {
            chain_id,
            owner,
            privacy_mode,
         };

         // The rows are built on a worker; the frame only decides whether the view it holds is still
         // the one this identity needs. Entering the page, a refresh, a wallet or chain switch, a
         // privacy toggle and every balance or price update all land here.
         let key = ViewKey {
            identity,
            data: Self::data_fingerprint(ctx, identity),
         };

         if self.building.is_none() && self.view.as_ref().map(|view| view.key) != Some(key) {
            self.building = Some(key);
            Self::rebuild(key);
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

                  // A copy rather than a borrow of the view: the buttons beside it write UI state,
                  // so the view must not be borrowed across them.
                  let total = self
                     .view
                     .as_ref()
                     .filter(|view| view.key.identity == identity)
                     .map(|view| view.total.clone());

                  ui.horizontal(|ui| {
                     // Wallet Name - Total Value (centered)
                     ui.vertical_centered(|ui| {
                        ui.label(
                           RichText::new(wallet_info.name_with_source())
                              .size(theme.typography.very_large),
                        );

                        match total.as_deref() {
                           Some(total) => {
                              ui.label(
                                 RichText::new(total)
                                    .heading()
                                    .size(theme.typography.heading + 4.0),
                              );
                           }
                           // Nothing has been built for this (chain, wallet) yet.
                           None => {
                              ui.add(Spinner::new().size(theme.typography.heading + 4.0));
                           }
                        }
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

                           if res.clicked() {
                              self.refresh(owner);

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

               // A view built for another identity is never drawn: the rows of the previous wallet are
               // not a claim about this one, and neither are the shielded ones a public claim.
               let view = self.view.as_ref().filter(|view| view.key.identity == identity);

               let mut remove_token: Option<ERC20Token> = None;
               let mut nft_requests = NftRequests::default();

               match (self.mode, view) {
                  // The first build for this (chain, wallet): there is nothing to draw yet.
                  (_, None) => {
                     ui.add_space(theme.spacing.md);
                     ui.vertical_centered(|ui| {
                        ui.add(Spinner::new().size(24.0));
                     });
                  }
                  (PortfolioMode::Nfts, Some(view)) => {
                     nft_requests = show_nft_list(theme, &icons, identity, &view.nfts, tint, ui);
                  }
                  (PortfolioMode::Tokens, Some(view)) => {
                     // Token List
                     let rows = &view.tokens;

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
                                    let text = RichText::new(&row.symbol).size(normal);

                                    // The native currency has no portfolio entry of its own, so its
                                    // icon is the chain's rather than the token store's.
                                    let icon = match &row.token {
                                       Some(token) => {
                                          icons.token_icon_x32(token.address, token.chain_id, tint)
                                       }
                                       None => icons.native_currency_icon(chain_id, tint),
                                    };

                                    let label = Label::new(text, Some(icon))
                                       .image_on_left()
                                       .wrap()
                                       .interactive(false);
                                    ui.add(label).on_hover_text(&row.name);
                                 });

                                 r.cell(|ui| {
                                    let text = RichText::new(&row.price).size(normal);
                                    let label = Label::text(text);
                                    ui.add(label);
                                 });

                                 r.cell(|ui| {
                                    let text = RichText::new(&row.balance).size(normal);
                                    let label = Label::text(text);
                                    ui.add(label);
                                 });

                                 r.cell(|ui| {
                                    let text = RichText::new(&row.value).size(normal);
                                    let label = Label::text(text);
                                    ui.add(label);
                                 });

                                 r.cell(|ui| {
                                    let Some(token) = &row.token else {
                                       return;
                                    };

                                    let text = RichText::new("X").size(normal);
                                    if ui.add(Button::new(text)).clicked() {
                                       remove_token = Some(token.clone());
                                    }
                                 });
                              });
                           });
                     });
                  }
               }

               // Applied after the list: a removal rewrites the portfolio, and doing that from inside
               // the row loop would change the list it is iterating.
               if let Some(token) = remove_token {
                  self.remove_token(ctx, owner, &token);
               }

               if let Some(preview) = nft_requests.preview {
                  self.preview = Some(preview);
               }

               if let Some(nft) = nft_requests.remove {
                  Self::remove_nft(ctx, owner, &nft);
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

   /// A cheap digest of every input the view is built from.
   ///
   /// Read under the wallet-state lock and cloning nothing: the token identities with the balance,
   /// value and price beside them, the NFT identities with this wallet's holdings and the cached
   /// collection names, and the two totals. Comparing this against the key of the view on screen is
   /// what decides whether the frame owes a rebuild.
   fn data_fingerprint(ctx: &ZeusContext, identity: ViewIdentity) -> u64 {
      let ViewIdentity {
         chain_id,
         owner,
         privacy_mode,
      } = identity;

      let mut hasher = DefaultHasher::new();

      // The native currency is not a portfolio entry: its three values are read from the balance and
      // the price manager, so they are part of the key in their own right.
      let native = Currency::native(chain_id);
      if !privacy_mode {
         hash_numeric(&ctx.get_currency_price(&native), &mut hasher);
         hash_numeric(
            &ctx.get_currency_balance(chain_id, owner, &native),
            &mut hasher,
         );
         hash_numeric(
            &ctx.get_currency_value_for_owner(chain_id, owner, &native),
            &mut hasher,
         );
      }

      let nft_db = &ctx.nft_db;

      ctx.wallet_state.read(|ws| {
         let Some(portfolio) = ws.portfolio_db.portfolios.get(&(chain_id, owner)) else {
            // No portfolio entry yet: the view is the native row and nothing else.
            0u8.hash(&mut hasher);
            return;
         };

         hash_numeric(&portfolio.public_value(), &mut hasher);
         hash_numeric(&portfolio.private_value(), &mut hasher);

         let tokens = match privacy_mode {
            true => portfolio.private_tokens(),
            false => portfolio.public_tokens(),
         };

         tokens.len().hash(&mut hasher);
         for (token, balance, value, price) in tokens {
            // The labels are part of the row: a token that gains a real name later must redraw.
            token.address.hash(&mut hasher);
            token.symbol.hash(&mut hasher);
            token.name.hash(&mut hasher);
            hash_numeric(balance, &mut hasher);
            hash_numeric(value, &mut hasher);
            hash_numeric(price, &mut hasher);
         }

         let nfts = match privacy_mode {
            true => portfolio.private_nfts(),
            false => portfolio.nfts(),
         };

         nfts.len().hash(&mut hasher);
         for token in nfts {
            token.collection.hash(&mut hasher);
            token.token_id.hash(&mut hasher);
            token.standard.hash(&mut hasher);

            // The name is part of the title, and discovery caches it while the view is on screen.
            match cached_collection(nft_db, chain_id, token.collection) {
               Some(collection) => {
                  collection.name.as_deref().hash(&mut hasher);
                  collection.symbol.as_deref().hash(&mut hasher);
               }
               None => 0u8.hash(&mut hasher),
            }

            // The badge is the balance manager's answer on the public side; the private side is
            // «in `private_nfts` ⇒ held», with the count its note carried.
            let holding = match privacy_mode {
               true => portfolio
                  .private_nft_amounts()
                  .get(&(token.collection, token.token_id))
                  .copied(),
               false => ws.balance_manager.get_nft_balance(
                  chain_id,
                  owner,
                  token.collection,
                  token.token_id,
               ),
            };

            holding.hash(&mut hasher);
         }
      });

      hasher.finish()
   }

   /// Build the view for `key` off the GUI thread and install it when it lands.
   ///
   /// A worker, never the frame path: the frame holds the context write-locked while it paints, and
   /// every cell of this view is a string built from the wallet state.
   fn rebuild(key: ViewKey) {
      crate::runtime::rt().spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let view = ctx.read(|ctx| PortfolioView::build(ctx, key));

         SHARED_GUI.write(|gui| {
            gui.portofolio.apply_view(view);
            gui.request_repaint();
         });
      });
   }

   /// Install a freshly built view, under the `SHARED_GUI` write lock.
   fn apply_view(&mut self, view: PortfolioView) {
      self.building = None;
      self.view = Some(view);
   }

   /// The artwork at inspection size, opened from a row.
   fn show_nft_preview(&mut self, theme: &Theme, icons: &Icons, ui: &mut Ui) {
      let Some(NftPreview {
         token,
         title,
         subtitle,
      }) = self.preview.clone()
      else {
         return;
      };

      let heading = RichText::new(title).size(theme.typography.heading);
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

               ui.add(icons.nft_icon_x250(
                  token.chain_id,
                  token.collection,
                  token.token_id,
                  theme.image_tint_recommended,
               ));

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

   fn refresh(&mut self, owner: Address) {
      self.show_spinner = true;
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

impl PortfolioView {
   /// Build both lists and the wallet total for `key` from the wallet state.
   ///
   /// Runs on a worker and only ever reads the context: it never takes a second lock on a store it
   /// already holds (`ws` is the wallet state it was given), and it never asks the frame path for
   /// anything.
   fn build(ctx: &ZeusContext, key: ViewKey) -> Self {
      let ViewIdentity {
         chain_id,
         owner,
         privacy_mode,
      } = key.identity;

      let native = Currency::native(chain_id);

      // Read before the wallet state, and never from inside it: the price manager is a lock of its
      // own, and the row it feeds is the only reader here.
      let native_price = (!privacy_mode).then(|| ctx.get_currency_price(&native));

      let nft_db = &ctx.nft_db;

      let (total, tokens, nfts) = ctx.wallet_state.read(|ws| {
         // A wallet with no entry yet has an empty portfolio, not a missing one: the native row is
         // drawn either way. The same fallback `PortfolioDB::get` makes.
         let empty = WalletPortfolio::new(owner, chain_id);
         let portfolio = ws.portfolio_db.portfolios.get(&(chain_id, owner)).unwrap_or(&empty);

         let total = match privacy_mode {
            true => portfolio.private_value(),
            false => portfolio.public_value(),
         };

         let token_list = match privacy_mode {
            true => portfolio.private_tokens(),
            false => portfolio.public_tokens(),
         };

         // The native currency has no portfolio entry, so it is prepended rather than listed.
         let mut rows = Vec::with_capacity(token_list.len() + 1);

         if let Some(price) = native_price {
            let balance = ws.balance_manager.get_eth_balance(chain_id, owner);

            rows.push(TokenRow {
               symbol: native.symbol().to_string(),
               name: native.name().to_string(),
               price: format!("${:.10}", price.abbreviated()),
               balance: format!("{:.10}", balance.abbreviated()),
               value: format!(
                  "${:.10}",
                  NumericValue::value(balance.f64(), price.f64()).abbreviated()
               ),
               token: None,
            });
         }

         for (token, balance, value, price) in token_list {
            rows.push(TokenRow {
               symbol: token.symbol.to_string(),
               name: token.name.to_string(),
               price: format!("${:.10}", price.abbreviated()),
               balance: format!("{:.10}", balance.abbreviated()),
               value: format!("${:.10}", value.abbreviated()),
               token: Some(token.clone()),
            });
         }

         let (list, amounts) = match privacy_mode {
            true => (
               portfolio.private_nfts(),
               Some(portfolio.private_nft_amounts()),
            ),
            false => (portfolio.nfts(), None),
         };

         let nft_rows = list
            .iter()
            .map(|token| {
               let collection = cached_collection(nft_db, chain_id, token.collection);

               // Public ownership is the balance manager's, the same store the token rows read; the
               // private side is «in `private_nfts` ⇒ held», with the count its note carried.
               let holding = match privacy_mode {
                  true => Some(
                     amounts
                        .and_then(|amounts| {
                           amounts.get(&(token.collection, token.token_id)).copied()
                        })
                        .unwrap_or(1),
                  ),
                  false => ws.balance_manager.get_nft_balance(
                     chain_id,
                     owner,
                     token.collection,
                     token.token_id,
                  ),
               };

               NftRow {
                  title: format!(
                     "{} #{}",
                     nft_collection_name(collection, token.collection),
                     token.token_id
                  ),
                  subtitle: Self::nft_subtitle(collection, token.standard),
                  token: token.clone(),
                  holding,
               }
            })
            .collect();

         (
            format!("${:.10}", total.abbreviated()),
            rows,
            nft_rows,
         )
      });

      Self {
         key,
         total,
         tokens,
         nfts,
      }
   }

   /// The second line of an NFT row: the collection symbol and the standard.
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
}

/// The NFT list: the wallet's own NFTs, or the shielded ones in privacy mode.
///
/// The same table as the token list, but with no column header: there is one column that means
/// anything here, and «Price / Balance / Value» would have nothing underneath them.
///
/// Drawn from the rows the worker built — the badge's answer and both labels are already in them, so
/// no store is read per row on the frame path.
fn show_nft_list(
   theme: &Theme,
   icons: &Icons,
   ident: ViewIdentity,
   rows: &[NftRow],
   tint: bool,
   ui: &mut Ui,
) -> NftRequests {
   let mut requests = NftRequests::default();

   if rows.is_empty() {
      ui.add_space(theme.spacing.md);
      ui.label(
         RichText::new("Nothing here yet. Add NFT picks from the ones Zeus has found.")
            .size(theme.typography.normal)
            .color(theme.colors.text_muted),
      );
      return requests;
   }

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
         .key((ident.chain_id, ident.privacy_mode, rows.len()))
         .show(ui, |t| {
            t.rows(|r, row| {
               r.cell(|ui| {
                  ui.add(icons.nft_icon_x64(
                     row.token.chain_id,
                     row.token.collection,
                     row.token.token_id,
                     tint,
                  ));
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
                     ui.add(title_label).on_hover_text(&row.title);

                     let subtitle_text = RichText::new(&row.subtitle)
                        .size(theme.typography.small)
                        .color(theme.colors.text_muted);
                     let subtitle_label = Label::new(subtitle_text, None)
                        .wrap_mode(TextWrapMode::Truncate)
                        .interactive(false);
                     ui.add(subtitle_label);
                  });
               });

               r.cell(|ui| {
                  // Left of the two buttons: whether this wallet really holds the token. The
                  // portfolio is a claim about what it holds, and the chain is what settles it.
                  ownership_badge(
                     ui,
                     row.holding.map(|amount| amount > 0),
                     row.holding.unwrap_or(0),
                  );

                  let remove = Button::new(RichText::new("X").size(normal));

                  if ui.add(remove).clicked() {
                     requests.remove = Some(row.token.clone());
                  }

                  let view = Button::new(RichText::new("View").size(normal));

                  if ui.add(view).clicked() {
                     requests.preview = Some(NftPreview {
                        token: row.token.clone(),
                        title: row.title.clone(),
                        subtitle: row.subtitle.clone(),
                     });
                  }
               });
            });
         });
   });

   requests
}

/// The cached metadata of one collection, or `None` when it was never fetched.
///
/// Borrowed rather than cloned: both the row builder and the view's fingerprint ask for it, and the
/// catalog can hold thousands of collections the portfolio does not list.
fn cached_collection(nft_db: &NftDB, chain_id: u64, address: Address) -> Option<&NftCollection> {
   nft_db.collections.get(&chain_id)?.get(&address)
}

/// Hash the parts of a [`NumericValue`] a row shows.
///
/// The displayed string is the input, not the float behind it: `abbreviated()` carries the suffix
/// ("12.5M") and falls back to `formatted`, so both are hashed.
fn hash_numeric(value: &NumericValue, hasher: &mut DefaultHasher) {
   value.f64.to_bits().hash(hasher);
   value.abbreviated().hash(hasher);
}
