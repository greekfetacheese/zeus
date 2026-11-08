//! This is the UI that shows the portfolio of the current wallet
//!
//! Showed when Home is selected

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
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
   Align, CornerRadius, CursorIcon, Frame, Image, Layout, Margin, Order, RichText, Spinner,
   TextWrapMode, Ui, vec2,
};
use std::collections::HashMap;
use std::sync::Arc;

use egui_elements::{
   Button, Label, Modal, Tabs, TabsStyle, Theme,
   layout::{Column, Table, TableScroll},
   visuals::ButtonVisuals,
};
use egui_lucide::Lucide;
use zeus_eth::{
   alloy_primitives::{Address, U256},
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

/// One row of the portfolio's token list: the strings a row shows, plus what its icon and Remove need.
///
/// The table is data-driven, so the list is resolved once per frame instead of inside a cell closure
/// that runs twice on the frame it measures.
struct TokenRow {
   symbol: String,
   name: String,
   price: String,
   balance: String,
   value: String,
   icon: Image<'static>,
   /// The token `Remove` drops; `None` for the chain's native currency, which cannot be removed.
   token: Option<ERC20Token>,
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

         Frame::new().outer_margin(Margin::same(5)).show(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
               ui.set_width(ui.available_width() * 0.7);

               ui.spacing_mut().item_spacing = vec2(theme.spacing.lg, theme.spacing.md);

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
                        let text = RichText::new(label).size(theme.typography.normal);
                        let add_token = Button::new(text).visuals(button_visuals);

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
                           ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                        }
                     });
                  });
               });

               if privacy_mode
                  && ctx.railgun_is_supported(ctx.chain)
                  && !ctx.is_railgun_enabled(chain_id)
               {
                  ui.label(
                     RichText::new("Railgun is disabled")
                        .size(theme.typography.large)
                        .color(theme.colors.warning),
                  );
                  ui.label(
                     RichText::new("Enable it in Settings/Railgun to see private balances.")
                        .size(theme.typography.normal),
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
                  let nfts = if privacy_mode {
                     portfolio.private_nfts()
                  } else {
                     portfolio.nfts()
                  };

                  self.show_nft_list(
                     ctx,
                     theme,
                     &icons,
                     nfts,
                     chain_id,
                     owner,
                     privacy_mode,
                     portfolio.private_nft_amounts(),
                     tint,
                     ui,
                  );
               } else {
                  // Token List
                  let token_list = if privacy_mode {
                     portfolio.private_tokens()
                  } else {
                     portfolio.public_tokens()
                  };

                  // The native currency has no portfolio entry, so it is prepended rather than listed.
                  let mut rows = Vec::with_capacity(token_list.len() + 1);
                  if !privacy_mode {
                     let native = Currency::native(chain_id);
                     rows.push(TokenRow {
                        symbol: native.symbol().to_string(),
                        name: native.name().to_string(),
                        price: format!(
                           "${:.10}",
                           ctx.get_currency_price(&native).abbreviated()
                        ),
                        balance: format!(
                           "{:.10}",
                           ctx.get_currency_balance(chain_id, owner, &native).abbreviated()
                        ),
                        value: format!(
                           "${:.10}",
                           ctx.get_currency_value_for_owner(chain_id, owner, &native).abbreviated()
                        ),
                        icon: icons.currency_icon_x32(&native, tint),
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
                        icon: icons.token_icon_x32(token.address, token.chain_id, tint),
                        token: Some(token.clone()),
                     });
                  }

                  let mut remove_request: Option<ERC20Token> = None;

                  ui.spacing_mut().item_spacing.y = theme.spacing.sm;
                  ui.scope(|ui| {
                     ui.spacing_mut().button_padding =
                        vec2(theme.spacing.xs, theme.spacing.xs / 2.0);

                     Table::new("portfolio_tokens", &rows)
                        .columns([
                           Column::Fr(2.2), // Asset
                           Column::Fr(1.8), // Price
                           Column::Fr(1.8), // Balance
                           Column::Fr(1.8), // Value
                           Column::Auto,    // Remove
                        ])
                        .gap(20.0)
                        // 12 pt, not xs: a themed `Button` paints ~6 pt above and below its band.
                        .row_gap(theme.spacing.md)
                        .scroll(TableScroll::new().auto_shrink([false; 2]).content_margin(15))
                        .row_divider(true)
                        .header_rule(true)
                        .key((chain_id, privacy_mode, rows.len()))
                        .show(ui, |t| {
                           t.header(|h| {
                              for header in ["Asset", "Price", "Balance", "Value", ""] {
                                 h.cell(|ui| {
                                    if !header.is_empty() {
                                       ui.label(
                                          RichText::new(header)
                                             .strong()
                                             .size(theme.typography.large)
                                             .color(theme.colors.text),
                                       );
                                    }
                                 });
                              }
                           });

                           t.rows(|r, row| {
                              r.cell(|ui| {
                                 let text = RichText::new(&row.symbol)
                                    .size(theme.typography.normal)
                                    .color(theme.colors.text);
                                 let label = Label::new(text, Some(row.icon.clone()))
                                    .image_on_left()
                                    .wrap()
                                    .visuals(theme.label_visuals())
                                    .interactive(false);
                                 ui.add(label).on_hover_text(&row.name);
                              });

                              r.cell(|ui| {
                                 ui.label(
                                    RichText::new(&row.price)
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text),
                                 );
                              });

                              r.cell(|ui| {
                                 ui.label(
                                    RichText::new(&row.balance)
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text),
                                 );
                              });

                              r.cell(|ui| {
                                 ui.label(
                                    RichText::new(&row.value)
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text),
                                 );
                              });

                              r.cell(|ui| {
                                 let Some(token) = &row.token else {
                                    return;
                                 };

                                 let text = RichText::new("X").size(theme.typography.normal);
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

   /// The NFT list: the wallet's own NFTs, or the shielded ones in privacy mode.
   ///
   /// The same table as the token list, but with no column header: there is one column that means
   /// anything here, and «Price / Balance / Value» would have nothing underneath them.
   ///
   /// `privacy_mode` picks which side of the holdings the rows read: the badges must agree with the list
   /// they are beside, and the two sides answer differently. `private_amounts` is that side's counts —
   /// the notes' quantities, which the balance manager cannot know.
   fn show_nft_list(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Icons,
      nfts: &[NftToken],
      chain_id: u64,
      owner: Address,
      privacy_mode: bool,
      private_amounts: &HashMap<(Address, U256), u64>,
      tint: bool,
      ui: &mut Ui,
   ) {
      let collections = cached_collections(ctx.nft_db.get_collections(chain_id));

      if nfts.is_empty() {
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

      let label_visuals = theme.label_visuals();
      let button_visuals = theme.button_visuals();

      ui.spacing_mut().item_spacing.y = theme.spacing.sm;
      ui.scope(|ui| {
         ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);

         Table::new("portfolio_nfts", nfts)
            // The artwork's thumbnail size is the row height this list was laid out for.
            .columns([Column::Fixed(64.0), Column::Fr(1.0), Column::Auto])
            .gap(20.0)
            .row_gap(theme.spacing.md)
            .scroll(TableScroll::new().auto_shrink([false; 2]).content_margin(15))
            .row_divider(true)
            .key((chain_id, privacy_mode, nfts.len()))
            .show(ui, |t| {
               t.rows(|r, token| {
                  let collection = collections.get(&token.collection);
                  // The id is part of the title, not an afterthought: a collection is many tokens,
                  // and which one this row is is the first thing to read.
                  let title = format!(
                     "{} #{}",
                     nft_collection_name(collection, token.collection),
                     token.token_id
                  );
                  let subtitle = Self::nft_subtitle(collection, token.standard);
                  // Public ownership is the balance manager's, the same store the token rows read;
                  // the private side is «in `private_nfts` ⇒ held», with the count its note carried.
                  let holding = match privacy_mode {
                     true => Some(
                        private_amounts
                           .get(&(token.collection, token.token_id))
                           .copied()
                           .unwrap_or(1),
                     ),
                     false => {
                        ctx.get_nft_balance(chain_id, owner, token.collection, token.token_id)
                     }
                  };

                  r.cell(|ui| {
                     ui.add(icons.nft_icon_x64(
                        token.chain_id,
                        token.collection,
                        token.token_id,
                        tint,
                     ));
                  });

                  r.cell(|ui| {
                     // Two labels rather than one with a newline: `TextWrapMode::Truncate` keeps a
                     // label to a single line, so the second line of a two-line label would never
                     // be drawn.
                     ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing.xs;

                        let title_text = RichText::new(&title)
                           .size(theme.typography.normal)
                           .color(theme.colors.text);
                        let title_label = Label::new(title_text, None)
                           .wrap_mode(TextWrapMode::Truncate)
                           .visuals(label_visuals)
                           .interactive(false);
                        ui.add(title_label).on_hover_text(&title);

                        let subtitle_text = RichText::new(&subtitle)
                           .size(theme.typography.small)
                           .color(theme.colors.text_muted);
                        let subtitle_label = Label::new(subtitle_text, None)
                           .wrap_mode(TextWrapMode::Truncate)
                           .visuals(label_visuals)
                           .interactive(false);
                        ui.add(subtitle_label);
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

                     let remove = Button::new(RichText::new("X").size(theme.typography.normal))
                        .visuals(button_visuals);
                     if ui.add(remove).clicked() {
                        remove_request = Some(token.clone());
                     }

                     let view = Button::new(RichText::new("View").size(theme.typography.normal))
                        .visuals(button_visuals);
                     if ui.add(view).clicked() {
                        preview_request = Some(token.clone());
                     }
                  });
               });
            });
      });

      // Applied after the list: a removal rewrites the portfolio, and doing that from inside the row
      // loop would change the list it is iterating.
      if let Some(nft) = preview_request {
         // The collection comes from the map the list already built: the modal draws every frame, so the
         // lookup cannot live in its body.
         self.preview = Some(NftPreview {
            collection: collections.get(&nft.collection).cloned(),
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
