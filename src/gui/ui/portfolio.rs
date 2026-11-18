//! This is the UI that shows the portfolio of the current wallet
//!
//! Showed when Home is selected

use crate::assets::icons::Icons;
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::{
   SHARED_GUI,
   ui::{
      common::show_with_fade,
      token_selection::{
         PickerMode, TokenSelectionWindow, cached_collections, nft_collection_name, ownership_badge,
      },
   },
};
use crate::utils::nft_icon::start_nft_art_downloads;
use eframe::egui::{
   Align, CornerRadius, CursorIcon, Layout, Margin, Order, RichText, TextWrapMode, Ui, vec2,
};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use egui_elements::{
   Button, Frame, Label, Modal, Spinner, Tabs, TabsStyle, Theme,
   layout::{Column, Table, TableScroll},
   visuals::ButtonVisuals,
};
use egui_lucide::Lucide;
use zeus_core::WalletPortfolio;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
   nft::{NftCollection, NftStandard, NftToken},
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
/// Built on a worker and cached (see `RowsCache`): the strings come from `NumericValue`'s formatters,
/// which are the expensive half of a row, and the table is data-driven, so the list is resolved once
/// per key change instead of inside a cell closure that runs twice on the frame it measures. The icon
/// is resolved on the frame from the row's `currency` — a texture lookup, and textures may only be
/// loaded from the GUI thread.
struct TokenRow {
   currency: Currency,
   symbol: String,
   name: String,
   price: String,
   balance: String,
   value: String,
   /// The token `Remove` drops; `None` for the chain's native currency, which cannot be removed.
   token: Option<ERC20Token>,
}

/// One row of the portfolio's NFT list, cached beside the token rows for the same reason.
///
/// `holding` is the ownership answer the badge shows: the balance manager's for the public side, the
/// note count for the private one, and `None` when nobody has asked — a row claims nothing rather
/// than claiming "not owned".
struct NftRow {
   token: NftToken,
   title: String,
   subtitle: String,
   holding: Option<u64>,
}

/// What the two row caches are keyed by.
///
/// `chain`, `owner` and `privacy` say which list this is; `fingerprint` folds in everything the rows
/// are *computed from* — the portfolio's own cached lists and the balance and price answers — so a
/// background sync that moved a balance invalidates the cache without anyone pointing at it.
#[derive(Clone, PartialEq, Eq, Hash)]
struct RowsKey {
   chain: u64,
   owner: Address,
   privacy: bool,
   fingerprint: u64,
}

/// The row lists for the current `RowsKey`, built on a worker.
///
/// Either list may be empty; `loading` says a build is in flight, and an empty list only means
/// "nothing here" once it is false.
#[derive(Default)]
struct RowsCache {
   key: Option<RowsKey>,
   loading: bool,
   tokens: Vec<TokenRow>,
   nfts: Vec<NftRow>,
}

pub struct PortfolioUi {
   open: bool,
   _loading: bool,
   pub show_spinner: bool,
   mode: PortfolioMode,
   /// What the artwork modal is showing, if it is open.
   preview: Option<NftPreview>,
   /// The cached row lists, built off the GUI thread.
   rows: RowsCache,
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
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: false,
         _loading: false,
         show_spinner: false,
         mode: PortfolioMode::Tokens,
         preview: None,
         rows: RowsCache::default(),
         nft_list_asked_for: None,
         shown_for: None,
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
      // A rebuild claimed while closed would land on no one: drop the cache with the view.
      self.rows = RowsCache::default();
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
         let portfolio = ctx.read_wallet_state(|ws| ws.portfolio_db.get(chain_id, owner));

         // The preview belongs to the (chain, wallet) it was opened under: switching either must not
         // leave the previous token's artwork over the new list.
         if self.shown_for != Some((chain_id, owner)) {
            self.shown_for = Some((chain_id, owner));
            self.preview = None;
         }

         let portfolio_value = match privacy_mode {
            false => portfolio.public_value(),
            true => portfolio.private_value(),
         };

         // Rebuild the rows off the GUI thread when anything they are computed from moved. Taking the
         // key is a few map lookups and a hash; the rows behind it are the expensive part, which is
         // the point of the cache.
         self.refresh_rows(ctx, chain_id, owner, privacy_mode, &portfolio);

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
                        ui.label(
                           RichText::new(format!("${:.10}", portfolio_value.abbreviated()))
                              .heading()
                              .size(theme.typography.heading + 4.0),
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

               if self.mode == PortfolioMode::Nfts {
                  self.show_nft_list(ctx, theme, &icons, chain_id, owner, tint, ui);
               } else {
                  self.show_token_list(ctx, theme, &icons, owner, tint, ui);
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

   /// Take the rows cache's key, and start a worker rebuild on a miss.
   ///
   /// The key is read straight from the sources the rows are computed from — the portfolio's cached
   /// lists, the balance manager's answers and the collection catalog — but nothing is formatted here:
   /// taking the key every frame must cost a few map lookups and a hash, or the cache has already
   /// spent what it saves. The rows themselves are built by [`build_rows`], on a worker.
   fn refresh_rows(
      &mut self,
      ctx: &mut ZeusContext,
      chain_id: u64,
      owner: Address,
      privacy_mode: bool,
      portfolio: &WalletPortfolio,
   ) {
      // One switch bit per source list, so a token fingerprint and an NFT fingerprint over equal
      // parts can never fold into the same key and skip a rebuild the other list still needs.
      let mut fingerprint = 1u64;
      fold_hash(&mut fingerprint, (chain_id, owner, privacy_mode));

      let key = RowsKey {
         chain: chain_id,
         owner,
         privacy: privacy_mode,
         fingerprint: match self.mode {
            PortfolioMode::Tokens => fingerprint.wrapping_add(token_fingerprint(
               ctx,
               chain_id,
               owner,
               privacy_mode,
               portfolio,
            )),
            PortfolioMode::Nfts => fingerprint.wrapping_add(nft_fingerprint(
               ctx,
               chain_id,
               owner,
               privacy_mode,
               portfolio,
            )),
         },
      };

      if self.rows.key.as_ref() == Some(&key) {
         return;
      }

      self.rows.loading = true;
      self.rows.key = Some(key.clone());

      crate::runtime::rt().spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let built = build_rows(&ctx, &key);

         SHARED_GUI.write(|gui| {
            let portfolio = &mut gui.portofolio;

            // A newer key has already claimed the cache: these rows are stale, and clearing
            // `loading` here would end the newer build's empty state before its rows exist.
            if portfolio.rows.key.as_ref() != Some(&key) {
               return;
            }

            portfolio.rows.tokens = built.tokens;
            portfolio.rows.nfts = built.nfts;
            portfolio.rows.loading = false;
            gui.request_repaint();
         });
      });
   }

   /// The token list, painted from the cached rows.
   ///
   /// The rows are the worker's; this resolves each row's icon — a texture lookup that must stay on
   /// the GUI thread — and reads the clicks.
   fn show_token_list(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Icons,
      owner: Address,
      tint: bool,
      ui: &mut Ui,
   ) {
      let normal = theme.typography.normal;
      let large = theme.typography.large;

      if self.rows.loading && self.rows.tokens.is_empty() {
         ui.add_space(theme.spacing.md);
         ui.add(Spinner::new().size(25.0));
         return;
      }

      let rows = &self.rows.tokens;

      if rows.is_empty() {
         ui.add_space(theme.spacing.md);
         ui.label(
            RichText::new("Nothing here yet. Add a token to start the portfolio.")
               .size(normal)
               .color(theme.colors.text_muted),
         );
         return;
      }

      let mut remove_request: Option<ERC20Token> = None;

      ui.spacing_mut().item_spacing.y = theme.spacing.sm;
      ui.scope(|ui| {
         ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs / 2.0);

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
            .key((&self.rows.key, rows.len()))
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
                  let icon = icons.currency_icon_x32(&row.currency, tint);

                  r.cell(|ui| {
                     let text = RichText::new(&row.symbol).size(normal);
                     let label =
                        Label::new(text, Some(icon)).image_on_left().wrap().interactive(false);
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

   /// The NFT list: the wallet's own NFTs, or the shielded ones in privacy mode.
   ///
   /// The same table as the token list, but with no column header: there is one column that means
   /// anything here, and «Price / Balance / Value» would have nothing underneath them.
   fn show_nft_list(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Icons,
      chain_id: u64,
      owner: Address,
      tint: bool,
      ui: &mut Ui,
   ) {
      let normal = theme.typography.normal;

      if self.rows.loading && self.rows.nfts.is_empty() {
         ui.add_space(theme.spacing.md);
         ui.add(Spinner::new().size(25.0));
         return;
      }

      let rows = &self.rows.nfts;

      if rows.is_empty() {
         ui.add_space(theme.spacing.md);
         ui.label(
            RichText::new("Nothing here yet. Add NFT picks from the ones Zeus has found.")
               .size(normal)
               .color(theme.colors.text_muted),
         );
         return;
      }

      let mut preview_request: Option<NftToken> = None;
      let mut remove_request: Option<NftToken> = None;

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
            .key((&self.rows.key, rows.len()))
            .show(ui, |t| {
               t.rows(|r, row| {
                  let icon = icons.nft_icon_x64(
                     row.token.chain_id,
                     row.token.collection,
                     row.token.token_id,
                     tint,
                  );

                  r.cell(|ui| {
                     ui.add(icon);
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
                        remove_request = Some(row.token.clone());
                     }

                     let view = Button::new(RichText::new("View").size(normal));

                     if ui.add(view).clicked() {
                        preview_request = Some(row.token.clone());
                     }
                  });
               });
            });
      });

      // Applied after the list: a removal rewrites the portfolio, and doing that from inside the row
      // loop would change the list it is iterating.
      if let Some(nft) = preview_request {
         // The collection is resolved the moment the preview opens, not in the modal's body — the modal
         // draws every frame, and the lookup reads the whole catalog.
         self.preview = Some(NftPreview {
            collection: ctx.nft_db.get_collection(chain_id, nft.collection),
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

      let heading = RichText::new(format!(
         "{} #{}",
         nft_collection_name(collection, token.collection),
         token.token_id
      ))
      .size(theme.typography.heading);
      let subtitle = nft_subtitle_text(collection, token.standard);
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

/// One order-independent hash step: the fingerprint folds the *set* of entries, so a map whose
/// iteration order changes between frames does not invalidate the cache by itself.
fn fold_hash(fingerprint: &mut u64, parts: impl Hash) {
   let mut hasher = DefaultHasher::new();
   parts.hash(&mut hasher);
   *fingerprint = fingerprint.wrapping_add(hasher.finish());
}

/// Everything the token rows are computed from, hashed without formatting anything.
///
/// The ERC-20 side reads the portfolio's own cached lists — `update_public_data` /
/// `update_private_data` rewrite them (balance, price and value together) on every refresh,
/// background state sync, send and swap — so hashing them is what makes a moved balance invalidate
/// the rows. The native currency has no portfolio entry, so its balance and price are read from the
/// balance manager directly, exactly as the row itself reads them.
fn token_fingerprint(
   ctx: &ZeusContext,
   chain_id: u64,
   owner: Address,
   privacy_mode: bool,
   portfolio: &WalletPortfolio,
) -> u64 {
   let mut fingerprint = 0u64;

   let token_list = if privacy_mode {
      portfolio.private_tokens()
   } else {
      portfolio.public_tokens()
   };

   for (token, balance, value, price) in token_list {
      fold_hash(
         &mut fingerprint,
         (
            token.chain_id,
            token.address,
            balance.f64().to_bits(),
            value.f64().to_bits(),
            price.f64().to_bits(),
         ),
      );
   }

   if !privacy_mode {
      let native = Currency::native(chain_id);
      let price = ctx.get_currency_price(&native);
      let balance = ctx.get_currency_balance(chain_id, owner, &native);
      fold_hash(
         &mut fingerprint,
         (balance.f64().to_bits(), price.f64().to_bits()),
      );
   }

   fingerprint
}

/// Everything the NFT rows are computed from, hashed without building a single row string.
///
/// Holdings are hashed per token rather than folding the manager's map whole, so a balance that moved
/// for a token the portfolio does not list does not rebuild the list; the collection catalog is hashed
/// with them, because a name or symbol cached after the list was built has to reach the rows.
fn nft_fingerprint(
   ctx: &ZeusContext,
   chain_id: u64,
   owner: Address,
   privacy_mode: bool,
   portfolio: &WalletPortfolio,
) -> u64 {
   let mut fingerprint = 0u64;

   let nfts = if privacy_mode {
      portfolio.private_nfts()
   } else {
      portfolio.nfts()
   };

   for token in nfts {
      // Public ownership is the balance manager's, the same store the token rows read;
      // the private side is «in `private_nfts` ⇒ held», with the count its note carried.
      let holding = match privacy_mode {
         true => portfolio
            .private_nft_amounts()
            .get(&(token.collection, token.token_id))
            .copied(),
         false => ctx.get_nft_balance(chain_id, owner, token.collection, token.token_id),
      };

      fold_hash(
         &mut fingerprint,
         (
            token.collection,
            token.token_id,
            token.standard,
            holding,
         ),
      );
   }

   for collection in ctx.nft_db.get_collections(chain_id) {
      fold_hash(
         &mut fingerprint,
         (
            collection.address,
            collection.name,
            collection.symbol,
         ),
      );
   }

   fingerprint
}

/// Build both row lists for `key`, on a worker.
///
/// Both are built together — one worker, one writeback — because they share the one [`RowsKey`]: the
/// tables key on it whole, so a build that filled only the visible list would leave the other stale
/// against a key that claims it.
fn build_rows(ctx: &ZeusCtx, key: &RowsKey) -> RowsCache {
   let portfolio = ctx.get_portfolio(key.chain, key.owner);
   let collections = cached_collections(ctx.read(|ctx| ctx.nft_db.get_collections(key.chain)));

   let mut tokens = Vec::new();

   if !key.privacy {
      // The native currency has no portfolio entry, so it is prepended rather than listed.
      let native = Currency::native(key.chain);
      let price = ctx.get_currency_price(&native);
      let balance = ctx.get_currency_balance(key.chain, key.owner, &native);
      tokens.push(TokenRow {
         currency: native.clone(),
         symbol: native.symbol().to_string(),
         name: native.name().to_string(),
         price: format!("${:.10}", price.abbreviated()),
         balance: format!("{:.10}", balance.abbreviated()),
         value: format!(
            "${:.10}",
            zeus_eth::utils::NumericValue::value(balance.f64(), price.f64()).abbreviated()
         ),
         token: None,
      });
   }

   let token_list = if key.privacy {
      portfolio.private_tokens()
   } else {
      portfolio.public_tokens()
   };

   for (token, balance, value, price) in token_list {
      tokens.push(TokenRow {
         currency: Currency::from(token.clone()),
         symbol: token.symbol.to_string(),
         name: token.name.to_string(),
         price: format!("${:.10}", price.abbreviated()),
         balance: format!("{:.10}", balance.abbreviated()),
         value: format!("${:.10}", value.abbreviated()),
         token: Some(token.clone()),
      });
   }

   let mut nfts = Vec::new();

   let nft_list = if key.privacy {
      portfolio.private_nfts()
   } else {
      portfolio.nfts()
   };

   for token in nft_list {
      let collection = collections.get(&token.collection);
      // The id is part of the title, not an afterthought: a collection is many tokens,
      // and which one this row is is the first thing to read.
      let title = format!(
         "{} #{}",
         nft_collection_name(collection, token.collection),
         token.token_id
      );
      let subtitle = nft_subtitle_text(collection, token.standard);

      // Public ownership is the balance manager's, the same store the token rows read;
      // the private side is «in `private_nfts` ⇒ held», with the count its note carried.
      let holding = match key.privacy {
         true => Some(
            portfolio
               .private_nft_amounts()
               .get(&(token.collection, token.token_id))
               .copied()
               .unwrap_or(1),
         ),
         false => ctx.read(|ctx| {
            ctx.get_nft_balance(
               key.chain,
               key.owner,
               token.collection,
               token.token_id,
            )
         }),
      };

      nfts.push(NftRow {
         token: token.clone(),
         title,
         subtitle,
         holding,
      });
   }

   RowsCache {
      key: Some(key.clone()),
      loading: false,
      tokens,
      nfts,
   }
}

/// The second line of an NFT row or preview: the collection symbol and the standard.
///
/// Free rather than a method on [`PortfolioUi`]: the row builder runs on a worker, where the
/// struct's private methods are out of reach.
fn nft_subtitle_text(collection: Option<&NftCollection>, standard: NftStandard) -> String {
   let standard = match standard {
      NftStandard::Erc721 => "ERC-721",
      NftStandard::Erc1155 => "ERC-1155",
   };

   match collection.and_then(|collection| collection.symbol.as_deref()) {
      Some(symbol) if !symbol.is_empty() => format!("{symbol} · {standard}"),
      _ => standard.to_string(),
   }
}
