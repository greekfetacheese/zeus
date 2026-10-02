//! A Window that allows the user to select a token

use eframe::egui::{
   Align, FontId, Id, Layout, Margin, OpenUrl, Order, RichText, ScrollArea, Sense, Spinner, Ui,
   emath::Vec2b, vec2,
};

use crate::assets::icons::Icons;
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::{SHARED_GUI, dots_button};
use crate::utils::{RT, token_icon::spawn_fetch_token_icon, truncate_symbol_or_name};
use elegance::{Menu, MenuItem};
use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   nft::NftToken,
   types::ChainId,
   utils::{
      NumericValue,
      batch::{NftRef, get_erc1155_balances},
   },
};

use anyhow::anyhow;
use egui_elements::{Button, Label, Modal, SecureTextEdit, Theme, utils::frame as frame_fn};

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

   /// The NFT list: `(token, balance)`.
   ///
   /// The union of what the user tracks (`NftDB`) and what the wallet holds (its portfolio). ERC-721
   /// has no amount — holding one is a 1 — so only ERC-1155 entries carry a real quantity.
   processed_nfts: Vec<(NftToken, u64)>,
   /// Is the NFT list being fetched? Kept apart from `loading`, which is the ERC-20 balance fetch and
   /// hides the mode switch while it runs.
   nfts_loading: bool,
   /// Did that fetch finish? An empty list is a legitimate result, so `processed_nfts.is_empty()`
   /// cannot stand in for "never fetched".
   nfts_loaded: bool,
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
         token_fetched: false,
         currency_direction: InOrOut::In,
         mode: PickerMode::Fungible,
         processed_currencies: Vec::new(),
         processed_nfts: Vec::new(),
         nfts_loading: false,
         nfts_loaded: false,
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
      // Both the tracked tokens and the wallet's holdings can have moved since last time.
      self.clear_processed_nfts();
      self.process_currencies(privacy_mode, chain_id, owner);
   }

   pub fn reset(&mut self) {
      self.close();
      self.title = "Select Token".to_string();
      self.search_query.clear();
      self.selected_currency = None;
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

   pub fn process_currencies(&mut self, privacy_mode: bool, chain_id: u64, owner: Address) {
      self.loading = true;

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
   }

   /// Kick off the NFT list fetch, at most once per opening.
   ///
   /// Spawned, never inline: it reads the wallet's ERC-1155 balances over the network, and this is
   /// the frame path.
   fn load_nfts(&mut self, chain_id: u64, owner: Address) {
      if self.nfts_loaded || self.nfts_loading {
         return;
      }

      self.nfts_loading = true;

      RT.spawn(async move {
         // Read on a worker: the handle is only reachable once the frame has dropped `SHARED_GUI`.
         let ctx = SHARED_GUI.write(|gui| gui.ctx.clone());
         let nfts = process_nfts(ctx, chain_id, owner).await;

         SHARED_GUI.write(|gui| {
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
         self.load_nfts(chain_id, owner);
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

               // NFT mode renders its own body below; nothing after this point in the closure is
               // about NFTs.
               if self.mode == PickerMode::Nft {
                  return;
               }

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
                     RT.spawn(async move {
                        let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                        sync_balances(ctx.clone(), chain.id(), owner).await;

                        let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);

                        SHARED_GUI.write(|gui| {
                           gui.token_selection.syncing_balances = false;
                           // Reopen the window if we are still in public mode
                           // so the balances are updated
                           if !privacy_mode {
                              gui.token_selection.open(privacy_mode, chain.id(), owner);
                           }
                        });
                     });
                  }

                  ui.add_space(10.0);
               }

               let hint = RichText::new("Search tokens or enter an address")
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
               ui.vertical_centered(|ui| {
                  self.show_nft_body(theme, ui);
               });

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

      ui.horizontal(|ui| {
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

   /// What NFT mode shows for now: the state of its list.
   ///
   /// Row rendering (thumbnail, `name #id`, the `dots_button` menu) is the next task; until then
   /// this reports what the loader actually found, so the mode is truthful instead of empty.
   fn show_nft_body(&self, theme: &Theme, ui: &mut Ui) {
      ui.add_space(theme.spacing.xl);

      if self.nfts_loading {
         ui.add(Spinner::new().size(25.0).color(theme.colors.text));
         return;
      }

      let text = match self.processed_nfts.len() {
         0 => "No NFTs to show yet".to_string(),
         1 => "1 NFT".to_string(),
         count => format!("{count} NFTs"),
      };

      let note = RichText::new(text).size(theme.typography.normal).color(theme.colors.text_muted);

      ui.label(note);
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
/// (its portfolio), each with a balance.
///
/// Neither source is authoritative alone — a tracked token may have been transferred away, and a
/// held token is not in the catalog until someone adds it. ERC-721 spends no call (holding one is a
/// 1); ERC-1155 amounts exist nowhere off-chain, so they take one batched Multicall3 round.
async fn process_nfts(ctx: ZeusCtx, chain_id: u64, owner: Address) -> Vec<(NftToken, u64)> {
   let tracked = ctx.read(|ctx| ctx.nft_db.get_nfts(chain_id));
   let held = ctx.get_portfolio(chain_id, owner).nfts().clone();
   let merged = merge_nft_sources(tracked, held);

   let refs: Vec<NftRef> = merged
      .iter()
      .filter(|token| token.is_erc1155())
      .map(|token| (token.collection, token.token_id))
      .collect();

   let amounts = fetch_erc1155_amounts(&ctx, chain_id, owner, refs).await;

   attach_balances(merged, &amounts)
}

/// Batched ERC-1155 amounts, keyed by `(collection, token id)`.
///
/// An empty map on failure: the list is still worth showing without amounts, so a transport error
/// degrades the quantities rather than making the whole mode unavailable.
async fn fetch_erc1155_amounts(
   ctx: &ZeusCtx,
   chain_id: u64,
   owner: Address,
   refs: Vec<NftRef>,
) -> HashMap<(Address, U256), u64> {
   if refs.is_empty() {
      return HashMap::new();
   }

   let client = match ctx.get_client(chain_id).await {
      Ok(client) => client,
      Err(e) => {
         tracing::error!("Failed to get client for chain {chain_id}: {e:?}");
         return HashMap::new();
      }
   };

   match get_erc1155_balances(client, owner, refs, None).await {
      Ok(rows) => rows
         .into_iter()
         .map(|(collection, token_id, amount)| ((collection, token_id), to_u64(amount)))
         .collect(),
      Err(e) => {
         tracing::error!("Failed to read ERC-1155 balances: {e:?}");
         HashMap::new()
      }
   }
}

/// A `balanceOf` returns `uint256` whatever the contract feels like, so saturate instead of
/// panicking on a hostile value.
fn to_u64(amount: U256) -> u64 {
   u64::try_from(amount).unwrap_or(u64::MAX)
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

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::nft::NftStandard;

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
}
