//! This is the UI that shows the portfolio of the current wallet
//!
//! Showed when Home is selected

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::{
   SHARED_GUI,
   ui::{TokenSelectionWindow, common::show_with_fade},
};
use crate::utils::RT;
use eframe::egui::{
   Align, CornerRadius, CursorIcon, Frame, Layout, Margin, RichText, ScrollArea, Spinner, Ui, Vec2,
   vec2,
};
use std::sync::Arc;

use egui_elements::{Button, Label, Theme, visuals::ButtonVisuals};
use egui_lucide::Lucide;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
};

pub struct PortfolioUi {
   open: bool,
   _loading: bool,
   pub show_spinner: bool,
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: false,
         _loading: false,
         show_spinner: false,
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

               ui.spacing_mut().item_spacing = Vec2::new(16.0, 15.0);

               let frame = theme.frame1;

               frame.show(ui, |ui| {
                  ui.horizontal(|ui| {
                     // Wallet Name - Total Value (centered)
                     ui.vertical_centered(|ui| {
                        ui.label(
                           RichText::new(wallet_info.name_with_source())
                              .size(theme.typography.very_large),
                        );
                        ui.label(
                           RichText::new(format!("${}", portfolio_value.abbreviated()))
                              .heading()
                              .size(theme.typography.heading + 4.0),
                        );
                     });

                     // Refresh - Add Token (right)
                     ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                        let button_visuals = theme.button_visuals();
                        let text = RichText::new("Add Token").size(theme.typography.normal);
                        let add_token = Button::new(text).visuals(button_visuals);

                        if ui.add(add_token).clicked() {
                           token_selection.open(privacy_mode, chain_id, owner);
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
                           }
                        } else {
                           ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                        }
                     });
                  });
               });

               // Token List
               ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                  ui.set_width(ui.available_width());

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

                  let label_visuals = theme.label_visuals();
                  let button_visuals = theme.button_visuals();
                  let tint = theme.image_tint_recommended;
                  let row_frame = theme.frame2.outer_margin(Margin::ZERO);

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

                  // --- Body: one frame2 card per asset ---
                  ui.vertical_centered(|ui| {
                     ui.spacing_mut().item_spacing.y = 10.0;

                     // Native currency first (public mode only)
                     if !privacy_mode {
                        let native_currency = Currency::native(chain_id);
                        let price = ctx.get_currency_price(&native_currency);
                        let balance = ctx.get_currency_balance(chain_id, owner, &native_currency);
                        let value =
                           ctx.get_currency_value_for_owner(chain_id, owner, &native_currency);

                        ui.allocate_ui(vec2(row_width, row_height + 16.0), |ui| {
                           row_frame.show(ui, |ui| {
                              ui.set_width(row_width);
                              ui.spacing_mut().item_spacing.x = col_spacing;

                              ui.horizontal(|ui| {
                                 // Asset
                                 Self::row_cell(ui, column_widths[0], row_height, |ui| {
                                    let icon = icons.currency_icon_x32(&native_currency, tint);
                                    let text = RichText::new(native_currency.symbol())
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text);
                                    let label = Label::new(text, Some(icon))
                                       .image_on_left()
                                       .wrap()
                                       .visuals(label_visuals)
                                       .interactive(false);
                                    ui.scope(|ui| {
                                       ui.set_max_width(column_widths[0] - 40.0);
                                       ui.add(label).on_hover_text(native_currency.name());
                                    });
                                 });

                                 // Price
                                 Self::row_cell(ui, column_widths[1], row_height, |ui| {
                                    ui.label(
                                       RichText::new(format!("${}", price.formatted()))
                                          .size(theme.typography.normal)
                                          .color(theme.colors.text),
                                    );
                                 });

                                 // Balance
                                 Self::row_cell(ui, column_widths[2], row_height, |ui| {
                                    ui.label(
                                       RichText::new(balance.abbreviated())
                                          .size(theme.typography.normal)
                                          .color(theme.colors.text),
                                    );
                                 });

                                 // Value
                                 Self::row_cell(ui, column_widths[3], row_height, |ui| {
                                    ui.label(
                                       RichText::new(format!("${}", value.abbreviated()))
                                          .size(theme.typography.normal)
                                          .color(theme.colors.text),
                                    );
                                 });

                                 // No remove for native
                                 Self::row_cell(ui, column_widths[4], row_height, |_ui| {});
                              });
                           });
                        });
                     }

                     let token_list = if privacy_mode {
                        portfolio.private_tokens()
                     } else {
                        portfolio.public_tokens()
                     };

                     for (token, balance, value, price) in token_list {
                        ui.allocate_ui(vec2(row_width, row_height + 16.0), |ui| {
                           row_frame.show(ui, |ui| {
                              ui.set_width(row_width);
                              ui.spacing_mut().item_spacing.x = col_spacing;

                              ui.horizontal(|ui| {
                                 // Asset
                                 Self::row_cell(ui, column_widths[0], row_height, |ui| {
                                    let icon =
                                       icons.token_icon_x32(token.address, token.chain_id, tint);
                                    let text = RichText::new(&*token.symbol)
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text);
                                    let label = Label::new(text, Some(icon))
                                       .image_on_left()
                                       .wrap()
                                       .visuals(label_visuals)
                                       .interactive(false);
                                    ui.scope(|ui| {
                                       ui.set_max_width(column_widths[0] - 40.0);
                                       ui.add(label).on_hover_text(&*token.name);
                                    });
                                 });

                                 // Price
                                 Self::row_cell(ui, column_widths[1], row_height, |ui| {
                                    ui.label(
                                       RichText::new(format!("${}", price.formatted()))
                                          .size(theme.typography.normal)
                                          .color(theme.colors.text),
                                    );
                                 });

                                 // Balance
                                 Self::row_cell(ui, column_widths[2], row_height, |ui| {
                                    ui.label(
                                       RichText::new(balance.abbreviated())
                                          .size(theme.typography.normal)
                                          .color(theme.colors.text),
                                    );
                                 });

                                 // Value
                                 Self::row_cell(ui, column_widths[3], row_height, |ui| {
                                    ui.label(
                                       RichText::new(format!("${}", value.formatted()))
                                          .size(theme.typography.normal)
                                          .color(theme.colors.text),
                                    );
                                 });

                                 // Remove
                                 Self::row_cell(ui, column_widths[4], row_height, |ui| {
                                    let button =
                                       Button::new(RichText::new("X").size(theme.typography.small))
                                          .visuals(button_visuals)
                                          .small();

                                    if ui.add(button).clicked() {
                                       self.remove_token(ctx, owner, token);
                                    }
                                 });
                              });
                           });
                        });
                     }
                  });

                  let currency = token_selection.get_selected_currency().cloned();

                  if let Some(currency) = currency {
                     let token_fetched = token_selection.token_fetched;
                     token_selection.reset();
                     self.add_currency(ctx, owner, token_fetched, currency);
                  }
               });
            });
         });
      });
   }

   fn refresh(&mut self, owner: Address) {
      self.show_spinner = true;
      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let chain = ctx.chain().id();
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

         // Update the pool state that includes these tokens
         let pool_manager = ctx.pool_manager();

         match pool_manager.discover_pools_for_tokens(ctx.clone(), chain, tokens.clone()).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error syncing pools: {:?}", e),
         }

         let mut pools = Vec::new();

         for token in tokens {
            if token.is_base() {
               continue;
            }

            let c = token.into();
            pools.extend(pool_manager.get_pools_that_have_currency(&c));
         }

         match pool_manager.update_state_for_pools(ctx.clone(), chain, pools).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating pool state: {:?}", e),
         }

         ctx.update_public_data(chain, owner);
         ctx.update_private_data(chain, owner).await;

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
         tracing::info!(
            "Token {} was fetched from the blockchain, no need to sync pools or balance",
            token.symbol
         );
         return;
      }

      self.show_spinner = true;

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let manager = ctx.pool_manager();
         match manager
            .discover_pools_for_tokens(ctx.clone(), chain_id, vec![token.clone()])
            .await
         {
            Ok(_) => {
               tracing::info!("Synced Pools for {}", token.symbol);
            }
            Err(e) => tracing::error!(
               "Error syncing pools for {}: {:?}",
               token.symbol,
               e
            ),
         }

         // Avoid potentialy syncing hundreds of pools
         if !currency.is_base() {
            match manager.update_for_currencies(ctx.clone(), chain_id, vec![currency]).await {
               Ok(_) => {
                  tracing::info!("Updated pool state for {}", token.symbol);
               }
               Err(e) => {
                  tracing::error!(
                     "Error updating pool state for {}: {:?}",
                     token.symbol,
                     e
                  );
               }
            }
         }

         let balance_manager = ctx.balance_manager();
         match balance_manager
            .update_tokens_balance(ctx.clone(), chain_id, owner, vec![token], false)
            .await
         {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating tokens balance: {:?}", e),
         }

         ctx.update_public_data(chain_id, owner);
         ctx.update_private_data(chain_id, owner).await;

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
