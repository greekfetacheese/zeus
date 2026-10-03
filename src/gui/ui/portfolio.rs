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
use crate::utils::{RT, nft_icon::start_nft_art_downloads};
use eframe::egui::{
   Align, CornerRadius, CursorIcon, Frame, Image, Layout, Margin, Order, RichText, ScrollArea,
   Spinner, TextWrapMode, Ui, vec2,
};
use std::collections::HashMap;
use std::sync::Arc;

use egui_elements::{Button, Label, Modal, Theme, visuals::ButtonVisuals};
use egui_lucide::Lucide;
use elegance::TabBar;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
   nft::{NftCollection, NftStandard, NftToken, verify_ownership_batch},
   utils::batch::NftRef,
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

/// What an NFT row's buttons asked for.
#[derive(Default)]
struct NftRowAction {
   /// Open the artwork at inspection size.
   view: bool,
   /// Drop the NFT from the portfolio.
   remove: bool,
}

/// What the portfolio knows about NFT ownership, and whose NFTs it asked about.
///
/// Ownership is a fact about one (chain, wallet) pair, so the answer has to be held with the pair it
/// belongs to: a wallet or chain switch must re-ask rather than put the previous wallet's badges on
/// these rows. `Loading` is a real state for the same reason in reverse — the frame must not start a
/// second load while the first is in flight.
enum NftHoldings {
   /// Nothing asked yet.
   Unknown,
   /// A load is in flight for this (chain, wallet).
   Loading(u64, Address),
   /// The answer for this (chain, wallet). The map is `(collection, id) -> amount`, where `0` means
   /// "not held"; `None` means the chain could not be asked, so no row claims anything — the pair
   /// still counts as covered, or the frame would re-ask it forever.
   Ready(u64, Address, Option<HashMap<NftRef, u64>>),
}

impl NftHoldings {
   /// Has this (chain, wallet) been asked? An in-flight load counts, so it is not started twice.
   fn covers(&self, chain_id: u64, owner: Address) -> bool {
      match self {
         NftHoldings::Unknown => false,
         NftHoldings::Loading(chain, wallet) | NftHoldings::Ready(chain, wallet, _) => {
            *chain == chain_id && *wallet == owner
         }
      }
   }

   /// How many of `token` this (chain, wallet) holds, when that is what the answer is about. `None`
   /// when it is not — an unanswered question shows no ownership claim rather than a wrong one.
   fn amount(&self, chain_id: u64, owner: Address, token: &NftToken) -> Option<u64> {
      match self {
         NftHoldings::Ready(chain, wallet, holdings) if *chain == chain_id && *wallet == owner => {
            holdings.as_ref().map(|holdings| {
               holdings.get(&(token.collection, token.token_id)).copied().unwrap_or(0)
            })
         }
         _ => None,
      }
   }
}

pub struct PortfolioUi {
   open: bool,
   _loading: bool,
   pub show_spinner: bool,
   mode: PortfolioMode,
   /// The NFT whose artwork the user opened at inspection size, if any.
   preview: Option<NftToken>,
   /// What the chain says about the NFTs in the portfolio list, for the (chain, wallet) it was asked
   /// about. Absent or not about this pair: a row shows no ownership claim rather than a wrong one.
   holdings: NftHoldings,
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: false,
         _loading: false,
         show_spinner: false,
         mode: PortfolioMode::Tokens,
         preview: None,
         holdings: NftHoldings::Unknown,
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
   }

   /// Fixed-size cell with vertically centered content so every column
   /// shares one baseline across framed rows.
   fn row_cell(ui: &mut Ui, width: f32, height: f32, add_contents: impl FnOnce(&mut Ui)) {
      ui.allocate_ui_with_layout(
         vec2(width, height),
         Layout::left_to_right(Align::Center),
         |ui| {
            ui.set_min_size(vec2(width, height));
            ui.set_max_size(vec2(width, height));
            add_contents(ui);
         },
      );
   }

   /// One framed portfolio row. Returns true if Remove was clicked.
   fn asset_row(
      ui: &mut Ui,
      theme: &Theme,
      column_widths: [f32; 5],
      row_width: f32,
      row_height: f32,
      icon: Image<'static>,
      symbol: &str,
      name: &str,
      price: &str,
      balance: &str,
      value: &str,
      show_remove: bool,
   ) -> bool {
      let label_visuals = theme.label_visuals();
      let row_frame = theme.frame1.outer_margin(Margin::ZERO);
      let mut remove_clicked = false;

      ui.allocate_ui(vec2(row_width, row_height + 16.0), |ui| {
         row_frame.show(ui, |ui| {
            ui.set_width(row_width);
            ui.spacing_mut().item_spacing.x = 20.0;

            ui.horizontal(|ui| {
               Self::row_cell(ui, column_widths[0], row_height, |ui| {
                  let text =
                     RichText::new(symbol).size(theme.typography.normal).color(theme.colors.text);
                  let label = Label::new(text, Some(icon))
                     .image_on_left()
                     .wrap()
                     .visuals(label_visuals)
                     .interactive(false);
                  ui.scope(|ui| {
                     ui.set_max_width(column_widths[0] - 40.0);
                     ui.add(label).on_hover_text(name);
                  });
               });

               Self::row_cell(ui, column_widths[1], row_height, |ui| {
                  ui.label(
                     RichText::new(price).size(theme.typography.normal).color(theme.colors.text),
                  );
               });

               Self::row_cell(ui, column_widths[2], row_height, |ui| {
                  ui.label(
                     RichText::new(balance).size(theme.typography.normal).color(theme.colors.text),
                  );
               });

               Self::row_cell(ui, column_widths[3], row_height, |ui| {
                  ui.label(
                     RichText::new(value).size(theme.typography.normal).color(theme.colors.text),
                  );
               });

               Self::row_cell(ui, column_widths[4], row_height, |ui| {
                  if show_remove {
                     let button = Button::new(RichText::new("X").size(theme.typography.normal));
                     remove_clicked = ui.add(button).clicked();
                  }
               });
            });
         });
      });

      remove_clicked
   }

   /// One framed row of the NFT list: the artwork, what it is, and what can be done with it.
   ///
   /// No price, balance or value cells — an NFT has none of those, so those columns exist only for
   /// tokens. The artwork cell is the thumbnail the icon store keeps for lists; the picture at
   /// inspection size lives in the preview modal.
   fn nft_row(
      ui: &mut Ui,
      theme: &Theme,
      column_widths: [f32; 3],
      col_spacing: f32,
      row_width: f32,
      row_height: f32,
      icon: Image<'static>,
      title: &str,
      subtitle: &str,
      owned: Option<bool>,
      amount: u64,
   ) -> NftRowAction {
      let label_visuals = theme.label_visuals();
      let row_frame = theme.frame1.outer_margin(Margin::ZERO);
      let mut action = NftRowAction::default();

      ui.allocate_ui(vec2(row_width, row_height + 16.0), |ui| {
         row_frame.show(ui, |ui| {
            ui.set_width(row_width);
            ui.spacing_mut().item_spacing.x = col_spacing;

            ui.horizontal(|ui| {
               Self::row_cell(ui, column_widths[0], row_height, |ui| {
                  ui.add(icon);
               });

               Self::row_cell(ui, column_widths[1], row_height, |ui| {
                  // Two labels rather than one with a newline: `TextWrapMode::Truncate` keeps a label to
                  // a single line, so the second line of a two-line label would never be drawn.
                  ui.vertical(|ui| {
                     ui.spacing_mut().item_spacing.y = theme.spacing.xs;

                     let title_text =
                        RichText::new(title).size(theme.typography.normal).color(theme.colors.text);
                     let title_label = Label::new(title_text, None)
                        .wrap_mode(TextWrapMode::Truncate)
                        .visuals(label_visuals)
                        .interactive(false);
                     ui.add(title_label).on_hover_text(title);

                     let subtitle_text = RichText::new(subtitle)
                        .size(theme.typography.small)
                        .color(theme.colors.text_muted);
                     let subtitle_label = Label::new(subtitle_text, None)
                        .wrap_mode(TextWrapMode::Truncate)
                        .visuals(label_visuals)
                        .interactive(false);
                     ui.add(subtitle_label);
                  });
               });

               Self::row_cell(ui, column_widths[2], row_height, |ui| {
                  ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                     let visual = theme.button_visuals();

                     let view = Button::new(RichText::new("View").size(theme.typography.normal))
                        .visuals(visual);
                     action.view = ui.add(view).clicked();

                     let remove = Button::new(RichText::new("X").size(theme.typography.normal))
                        .visuals(visual);
                     action.remove = ui.add(remove).clicked();

                     // Left of the two buttons: whether this wallet really holds the token. The
                     // portfolio is a claim about what it holds, and the chain is what settles it.
                     ownership_badge(ui, owned, amount);
                  });
               });
            });
         });
      });

      action
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

                     ui.add(TabBar::new(&mut tab, ["Tokens", "NFTs"]));

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
                           let mut visuals = ButtonVisuals::default();
                           visuals.bg_hover = button_visuals.bg_hover;
                           visuals.corner_radius = CornerRadius::same(25);
                           let button = Button::image(icon).small().visuals(visuals);
                           let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

                           if res.clicked() {
                              self.refresh(owner);

                              // Drop the ownership answers so the NFT list re-reads them: this is how a
                              // token that has just left — or one that has come back — updates.
                              self.holdings = NftHoldings::Unknown;
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

               // Ownership is a fact about one (chain, wallet) pair, so ask once for this pair. Entering
               // the mode, a refresh, a wallet or chain switch and adding a token all land here.
               if self.mode == PortfolioMode::Nfts && !self.holdings.covers(chain_id, owner) {
                  self.holdings = NftHoldings::Loading(chain_id, owner);
                  Self::load_nft_list(chain_id, owner);
               }

               if self.mode == PortfolioMode::Nfts {
                  let nfts = if privacy_mode {
                     portfolio.private_nfts()
                  } else {
                     portfolio.nfts()
                  };

                  self.show_nft_list(
                     ctx, theme, &icons, nfts, chain_id, owner, tint, ui,
                  );
               } else {
                  // Token List
                  let row_height = 40.0;
                  let col_spacing = 20.0;
                  let column_widths = [
                     ui.available_width() * 0.22, // Asset
                     ui.available_width() * 0.18, // Price
                     ui.available_width() * 0.18, // Balance
                     ui.available_width() * 0.18, // Value
                     ui.available_width() * 0.10, // Remove
                  ];
                  let row_width: f32 = column_widths.iter().sum::<f32>()
                     + col_spacing * (column_widths.len() as f32 - 1.0);
                  let row_height_sans_spacing = row_height + 16.0;

                  // --- Header (same widths as body cells; not inside a frame) ---
                  ui.horizontal(|ui| {
                     ui.add_space((ui.available_width() - row_width).max(0.0) / 2.0);
                     ui.spacing_mut().item_spacing.x = col_spacing;
                     for (i, header) in
                        ["Asset", "Price", "Balance", "Value", ""].into_iter().enumerate()
                     {
                        Self::row_cell(ui, column_widths[i], 28.0, |ui| {
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

                  ui.add_space(8.0);

                  let token_list = if privacy_mode {
                     portfolio.private_tokens()
                  } else {
                     portfolio.public_tokens()
                  };
                  let show_native = !privacy_mode;
                  let num_rows = token_list.len() + usize::from(show_native);

                  ui.spacing_mut().item_spacing.y = theme.spacing.sm;
                  ScrollArea::vertical().auto_shrink([false; 2]).content_margin(5).show_rows(
                     ui,
                     row_height_sans_spacing,
                     num_rows,
                     |ui, row_range| {
                        ui.vertical_centered(|ui| {
                           ui.spacing_mut().item_spacing.y = theme.spacing.md;
                           ui.spacing_mut().button_padding =
                              vec2(theme.spacing.sm, theme.spacing.xs);

                           for row_index in row_range {
                              if show_native && row_index == 0 {
                                 let native_currency = Currency::native(chain_id);
                                 let price = ctx.get_currency_price(&native_currency);
                                 let balance =
                                    ctx.get_currency_balance(chain_id, owner, &native_currency);
                                 let value = ctx.get_currency_value_for_owner(
                                    chain_id,
                                    owner,
                                    &native_currency,
                                 );
                                 let price_text = format!("${:.10}", price.abbreviated());
                                 let balance_text = format!("{:.10}", balance.abbreviated());
                                 let value_text = format!("${:.10}", value.abbreviated());
                                 let _ = Self::asset_row(
                                    ui,
                                    theme,
                                    column_widths,
                                    row_width,
                                    row_height,
                                    icons.currency_icon_x32(&native_currency, tint),
                                    native_currency.symbol(),
                                    native_currency.name(),
                                    &price_text,
                                    &balance_text,
                                    &value_text,
                                    false,
                                 );
                                 continue;
                              }

                              let token_idx = row_index - usize::from(show_native);
                              let Some((token, balance, value, price)) = token_list.get(token_idx)
                              else {
                                 continue;
                              };

                              let price_text = format!("${:.10}", price.abbreviated());
                              let balance_text = format!("{:.10}", balance.abbreviated());
                              let value_text = format!("${:.10}", value.abbreviated());
                              if Self::asset_row(
                                 ui,
                                 theme,
                                 column_widths,
                                 row_width,
                                 row_height,
                                 icons.token_icon_x32(token.address, token.chain_id, tint),
                                 &token.symbol,
                                 &token.name,
                                 &price_text,
                                 &balance_text,
                                 &value_text,
                                 true,
                              ) {
                                 self.remove_token(ctx, owner, token);
                              }
                           }
                        });
                     },
                  );
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

                  // The new token's ownership is not in the answers yet, so drop them: the next frame's
                  // load re-reads them.
                  self.holdings = NftHoldings::Unknown;
               }

               // The artwork at inspection size, if a row asked for it.
               self.show_nft_preview(ctx, theme, &icons, ui);
            });
         });
      });
   }

   /// The NFT list: the wallet's own NFTs, or the shielded ones in privacy mode.
   ///
   /// Virtualized like the token list, but with no column header: there is one column that means
   /// anything here, and «Price / Balance / Value» would have nothing underneath them.
   fn show_nft_list(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Icons,
      nfts: &[NftToken],
      chain_id: u64,
      owner: Address,
      tint: bool,
      ui: &mut Ui,
   ) {
      let collections = cached_collections(ctx.nft_db.get_collections(chain_id));

      let col_spacing = 20.0;
      let row_height = 64.0;
      let row_height_sans_spacing = row_height + 16.0;
      let row_width = ui.available_width();
      let column_widths = [
         row_height,       // the artwork, thumbnail-sized
         row_width * 0.62, // what it is
         row_width * 0.18, // inspect / remove
      ];

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

      ui.spacing_mut().item_spacing.y = theme.spacing.sm;
      ScrollArea::vertical().auto_shrink([false; 2]).content_margin(5).show_rows(
         ui,
         row_height_sans_spacing,
         nfts.len(),
         |ui, row_range| {
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);

               for row_index in row_range {
                  let Some(token) = nfts.get(row_index) else {
                     continue;
                  };

                  let collection = collections.get(&token.collection);
                  // The id is part of the title, not an afterthought: a collection is many tokens, and
                  // which one this row is is the first thing to read.
                  let title = format!(
                     "{} #{}",
                     nft_collection_name(collection, token.collection),
                     token.token_id
                  );
                  let subtitle = Self::nft_subtitle(collection, token.standard);
                  let holding = self.holdings.amount(chain_id, owner, token);

                  let icon = icons.nft_icon_x64(
                     token.chain_id,
                     token.collection,
                     token.token_id,
                     tint,
                  );

                  let action = Self::nft_row(
                     ui,
                     theme,
                     column_widths,
                     col_spacing,
                     row_width,
                     row_height,
                     icon,
                     &title,
                     &subtitle,
                     holding.map(|amount| amount > 0),
                     holding.unwrap_or(0),
                  );

                  if action.view {
                     preview_request = Some(token.clone());
                  }

                  if action.remove {
                     remove_request = Some(token.clone());
                  }
               }
            });
         },
      );

      // Applied after the list: a removal rewrites the portfolio, and doing that from inside the row
      // loop would change the list it is iterating.
      if let Some(nft) = preview_request {
         self.preview = Some(nft);
      }

      if let Some(nft) = remove_request {
         Self::remove_nft(ctx, owner, &nft);
      }
   }

   /// The artwork at inspection size, opened from a row.
   fn show_nft_preview(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: &Icons,
      ui: &mut Ui,
   ) {
      let Some(token) = self.preview.clone() else {
         return;
      };

      // The collection is looked up rather than kept alongside the preview: the catalog is the one
      // place that knows the name, and the modal is opened by a click, not by a frame.
      let collection = ctx
         .nft_db
         .get_collections(token.chain_id)
         .into_iter()
         .find(|collection| collection.address == token.collection);

      let heading = RichText::new(format!(
         "{} #{}",
         nft_collection_name(collection.as_ref(), token.collection),
         token.token_id
      ))
      .size(theme.typography.heading);
      let subtitle = Self::nft_subtitle(collection.as_ref(), token.standard);
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

   /// Load the NFT list: which of these tokens the wallet still holds, and the artwork to show for them.
   ///
   /// A worker, never the frame path — it reads and writes `SHARED_GUI`, which the frame holds
   /// write-locked. The list itself needs no fetching: it is the portfolio, and that is local. What
   /// does need the chain is ownership, because the catalog goes on listing a token after it has left
   /// the wallet, and the row has to be able to say so.
   fn load_nft_list(chain_id: u64, owner: Address) {
      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let portfolio = ctx.get_portfolio(chain_id, owner);
         let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);

         let tokens = match privacy_mode {
            true => portfolio.private_nfts(),
            false => portfolio.nfts(),
         };

         // Shielded tokens are held in Railgun custody, so a balance read against this address would
         // call them unowned. The private scan already established them: record them as held.
         let holdings = match privacy_mode {
            true => {
               Some(tokens.iter().map(|token| ((token.collection, token.token_id), 1)).collect())
            }
            false => match ctx.get_client(chain_id).await {
               Ok(client) => verify_ownership_batch(client, owner, tokens).await,
               Err(e) => {
                  tracing::error!("Failed to get client for chain {chain_id}: {e:?}");
                  None
               }
            },
         };

         start_nft_art_downloads(chain_id, tokens.iter());

         SHARED_GUI.write(|gui| {
            gui.portofolio.holdings = NftHoldings::Ready(chain_id, owner, holdings);
            gui.request_repaint();
         });
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
      RT.spawn_blocking(|| {
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
      RT.spawn(async move {
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

      RT.spawn(async move {
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

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         ctx.update_public_data(chain, owner);
         ctx.update_private_data(chain, owner).await;

         SHARED_GUI.write(|gui| {
            gui.portofolio.show_spinner = false;
         });
      });
   }
}
