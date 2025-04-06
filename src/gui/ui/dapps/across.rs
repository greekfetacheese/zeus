use crate::assets::icons::Icons;
use crate::core::utils::eth::get_eth_balance;
use crate::core::{
   ZeusCtx,
   utils::{
      RT, estimate_gas_cost, eth,
      tx::{TxMethod, TxParams},
   },
};
use crate::gui::{
   SHARED_GUI,
   ui::{ChainSelect, ContactsUi, GREEN_CHECK, RecipientSelectionWindow, WalletSelect},
};
use egui::{
   Align, Align2, Button, Color32, FontId, Frame, Grid, Layout, Margin, Order, RichText, Spinner, TextEdit, Ui, Window,
   vec2,
};
use egui_theme::{Theme, utils::widget_visuals};
use std::{collections::HashMap, str::FromStr, sync::Arc, time::Instant};
use zeus_eth::currency::ERC20Token;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256, utils::parse_units},
   currency::{Currency, NativeCurrency},
   dapps::across::*,
   types::{BSC, ChainId},
   utils::NumericValue,
};

/// Cache the results for this many seconds
const CACHE_EXPIRE: u64 = 250;

const TIME_BETWEEN_EACH_REQUEST: u64 = 2;

type ChainPath = (u64, u64);

#[derive(Debug, Default, Clone)]
pub struct ApiResCache {
   pub res: ClientResponse,
   pub last_updated: Option<Instant>,
}

pub struct AcrossBridge {
   pub open: bool,
   pub progress_window: ProgressWindow,
   pub currency: NativeCurrency,
   pub amount: String,
   pub from_wallet: WalletSelect,
   pub from_chain: ChainSelect,
   pub to_chain: ChainSelect,
   pub priority_fee: String,
   pub review_tx_window: bool,
   pub data_syncing: bool,
   pub balance_syncing: bool,
   /// API request in progress
   pub requesting: bool,
   /// time passed since last request
   pub last_request_time: Option<Instant>,
   /// Cache API responses
   pub api_res_cache: HashMap<ChainPath, ApiResCache>,
   pub size: (f32, f32),
}

impl AcrossBridge {
   pub fn new() -> Self {
      let progress_window = ProgressWindow::new("across_bridge_multi_step".to_string());

      Self {
         open: false,
         progress_window,
         currency: NativeCurrency::from_chain_id(1).unwrap(),
         amount: String::new(),
         from_wallet: WalletSelect::new("across_bridge_wallet_select"),
         from_chain: ChainSelect::new("across_bridge_from_chain", 1),
         to_chain: ChainSelect::new("across_bridge_to_chain", 10),
         priority_fee: "1".to_string(),
         review_tx_window: false,
         data_syncing: false,
         balance_syncing: false,
         requesting: false,
         last_request_time: None,
         api_res_cache: HashMap::new(),
         size: (550.0, 700.0),
      }
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      recipient_selection: &mut RecipientSelectionWindow,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      self.progress_window.show(theme, ui);

      recipient_selection.show(ctx.clone(), theme, &self.from_wallet, contacts_ui, ui);
      let recipient = recipient_selection.get_recipient();
      let recipient_name = recipient_selection.get_recipient_name();
      let from_chain = self.from_chain.chain.id();
      let depositor = self.from_wallet.wallet.key.borrow().address();
      self.currency = NativeCurrency::from_chain_id(from_chain).unwrap();

      self.get_suggested_fees(ctx.clone(), depositor, recipient.clone());

      self.review_transaction(
         ctx.clone(),
         theme,
         icons.clone(),
         recipient.clone(),
         recipient_name.clone(),
         ui,
      );

      let frame = theme.frame1;
      let bg_color = frame.fill;
      Window::new("Across Bridge")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            let ui_width = ui.available_width();

            // Header
            ui.vertical_centered(|ui| {
               ui.label(RichText::new("Bridge").size(theme.text_sizes.heading));
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               if ui
                  .add(Button::new(RichText::new("⟲").size(theme.text_sizes.small)))
                  .clicked()
               {
                  self.get_balance(ctx.clone(), depositor);
               }
            });

            widget_visuals(ui, theme.get_widget_visuals(bg_color));

            // Sender
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new("Sender")
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.large),
               );
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ctx.read(|ctx| {
                  let wallets = &ctx.account.wallets;
                  self.from_wallet.show(theme, wallets, icons.clone(), ui);
               });
            });
            ui.add_space(10.0);

            // Asset and amount selection
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Amount").size(theme.text_sizes.large));
            });

            let balance = ctx
               .get_eth_balance(from_chain, depositor)
               .unwrap_or_default();

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.set_max_width(ui_width * 0.15);
               Grid::new("asset_and_amount")
                  .spacing(vec2(5.0, 0.0))
                  .show(ui, |ui| {
                     ui.add(
                        TextEdit::singleline(&mut self.amount)
                           .hint_text("0")
                           .font(FontId::proportional(theme.text_sizes.normal))
                           .min_size(vec2(ui_width * 0.15, 25.0))
                           .background_color(theme.colors.text_edit_bg2)
                           .margin(Margin::same(10)),
                     );
                     let icon = icons.native_currency_icon(self.currency.chain_id);
                     ui.label(RichText::new(&self.currency.symbol).size(theme.text_sizes.normal));
                     ui.add(icon);
                     let text = format!("Balance: {}", balance.formatted());
                     ui.label(RichText::new(text).size(theme.text_sizes.normal));

                     // Max amount button
                     ui.spacing_mut().button_padding = vec2(5.0, 5.0);
                     widget_visuals(ui, theme.get_button_visuals(bg_color));
                     let button = Button::new(RichText::new("Max").size(theme.text_sizes.small));
                     if ui.add(button).clicked() {
                        self.amount = self.max_amount(ctx.clone()).flatten().clone();
                     }

                     if self.balance_syncing {
                        ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                     }
                     ui.end_row();

                     // Amount check
                     if !self.amount.is_empty() && !self.valid_amount() {
                        ui.label(
                           RichText::new("Invalid Amount")
                              .size(theme.text_sizes.small)
                              .color(Color32::RED),
                        );
                        ui.end_row();
                     }

                     // Balance check
                     if !self.sufficient_balance(ctx.clone(), depositor) {
                        ui.label(
                           RichText::new("Insufficient balance")
                              .size(theme.text_sizes.small)
                              .color(Color32::RED),
                        );
                        ui.end_row();
                     }
                  });
            });

            let amount = self.amount.parse().unwrap_or(0.0);
            // Value
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new(format!(
                     "Value≈ ${}",
                     self.value(ctx.clone(), amount).formatted()
                  ))
                  .size(theme.text_sizes.normal),
               );
            });
            ui.add_space(10.0);

            // Recipient
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               Grid::new("recipient")
                  .spacing(vec2(5.0, 0.0))
                  .show(ui, |ui| {
                     ui.label(
                        RichText::new("Recipient")
                           .color(theme.colors.text_secondary)
                           .size(theme.text_sizes.large),
                     );

                     if !recipient.is_empty() {
                        if let Some(name) = &recipient_name {
                           ui.label(RichText::new(name).size(theme.text_sizes.normal).strong());
                        } else {
                           ui.label(
                              RichText::new("Unknown Address")
                                 .size(theme.text_sizes.normal)
                                 .color(Color32::RED),
                           );
                        }
                        if !self.valid_recipient(recipient.clone()) {
                           ui.label(
                              RichText::new("Invalid Address")
                                 .size(theme.text_sizes.normal)
                                 .color(Color32::RED),
                           );
                        }
                     }
                     ui.end_row();
                  });
            });

            // widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let res = ui.add(
                  TextEdit::singleline(&mut recipient_selection.recipient)
                     .hint_text("Search contacts or enter an address")
                     .min_size(vec2(ui_width * 0.5, 25.0))
                     .margin(Margin::same(10))
                     .background_color(theme.colors.text_edit_bg2)
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               if res.clicked() {
                  recipient_selection.open = true;
               }
            });
            ui.add_space(10.0);

            widget_visuals(ui, theme.get_widget_visuals(bg_color));

            // From Chain
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               Grid::new("from_chain")
                  .spacing(vec2(5.0, 0.0))
                  .show(ui, |ui| {
                     ui.label(
                        RichText::new("From")
                           .color(theme.colors.text_secondary)
                           .size(theme.text_sizes.large),
                     );

                     let changed = self.from_chain.show(BSC, theme, icons.clone(), ui);
                     if changed {
                        let chain = self.from_chain.chain.id();
                        self.priority_fee = ctx
                           .get_priority_fee(chain)
                           .unwrap_or_default()
                           .formatted()
                           .clone();
                     }
                     ui.end_row();
                  });
            });
            ui.add_space(10.0);

            // To Chain
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               Grid::new("to_chain")
                  .spacing(vec2(5.0, 0.0))
                  .show(ui, |ui| {
                     ui.label(
                        RichText::new("To")
                           .color(theme.colors.text_secondary)
                           .size(theme.text_sizes.large),
                     );
                     self.to_chain.grid_id = "across_bridge_to_chain";
                     self.to_chain.show(BSC, theme, icons.clone(), ui);
                     ui.end_row();
                  });
            });
            ui.add_space(10.0);

            // Priority Fee
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Priority Fee").size(theme.text_sizes.normal));
               ui.add_space(2.0);
               ui.label(RichText::new("Gwei").size(theme.text_sizes.small));
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.set_width(ui_width * 0.25);
               ui.add(
                  TextEdit::singleline(&mut self.priority_fee)
                     .min_size(vec2(ui_width * 0.25, 25.0))
                     .margin(Margin::same(10))
                     .background_color(theme.colors.text_edit_bg2)
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(5.0);
               ui.spacing_mut().button_padding = vec2(5.0, 5.0);
               widget_visuals(ui, theme.get_button_visuals(bg_color));
               if ui
                  .add(Button::new(
                     RichText::new("Reset").size(theme.text_sizes.small),
                  ))
                  .clicked()
               {
                  self.reset_priority_fee(ctx.clone());
               }
            });

            if !self.priority_fee.is_empty() {
               if self.fee_is_zero() {
                  ui.label(
                     RichText::new("Fee cannot be zero")
                        .size(theme.text_sizes.normal)
                        .color(Color32::RED),
                  );
               }
            }

            // Network Fee
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new(format!(
                     "Network Fee≈ ${}",
                     self.cost(ctx.clone()).1.formatted()
                  ))
                  .size(theme.text_sizes.normal),
               );
            });

            // Protocol Fee
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new(format!(
                     "Protocol Fee≈ ${}",
                     self.protocol_fee(ctx.clone()).formatted()
                  ))
                  .size(theme.text_sizes.normal),
               );
            });

            // Estimated time to fill
            let fill_time = self
               .api_res_cache
               .get(&(self.from_chain.chain.id(), self.to_chain.chain.id()))
               .map(|c| c.res.suggested_fees.estimated_fill_time_sec);
            if let Some(fill_time) = fill_time {
               ui.add_space(10.0);
               ui.label(
                  RichText::new(format!("Estimated time to fill: {} seconds", fill_time)).size(theme.text_sizes.normal),
               );
               ui.add_space(20.0);
            } else {
               ui.add_space(20.0);
            }

            // Bridge Button
            widget_visuals(ui, theme.get_button_visuals(bg_color));

            let bridge =
               Button::new(RichText::new("Bridge").size(theme.text_sizes.normal)).min_size(vec2(ui_width * 0.90, 40.0));

            if !self.valid_inputs(ctx, depositor, recipient.clone()) {
               ui.disable();
            }

            ui.vertical_centered(|ui| {
               if ui.add(bridge).clicked() {
                  self.review_tx_window = true;
               }
            });
         });
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, depositor: Address) -> bool {
      let balance = ctx
         .get_eth_balance(self.from_chain.chain.id(), depositor)
         .unwrap_or_default();
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals);
      let amount = amount.wei().unwrap();
      balance.wei().unwrap() >= amount
   }

   /// Max amount = Balance - cost
   fn max_amount(&self, ctx: ZeusCtx) -> NumericValue {
      let chain = self.from_chain.chain;
      let owner = self.from_wallet.wallet.key.borrow().address();
      let balance = ctx.get_eth_balance(chain.id(), owner).unwrap_or_default();
      let (cost_wei, _) = self.cost(ctx.clone());

      if balance.wei().unwrap() < cost_wei {
         return NumericValue::default();
      }

      let max = balance.wei().unwrap() - cost_wei;
      NumericValue::format_wei(max, self.currency.decimals)
   }

   fn valid_recipient(&self, recipient: String) -> bool {
      let recipient = Address::from_str(&recipient).unwrap_or(Address::ZERO);
      recipient != Address::ZERO
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn fee_is_zero(&self) -> bool {
      let fee = self.priority_fee.parse().unwrap_or(0.0);
      let chain = self.from_chain.chain;
      if chain.uses_priority_fee() {
      fee == 0.0
      } else {
         false
      }
   }

   /// Reset priority fee to the suggested fee
   fn reset_priority_fee(&mut self, ctx: ZeusCtx) {
      let chain = self.from_chain.chain.id();
      let fee = ctx.get_priority_fee(chain).unwrap_or_default();
      self.priority_fee = fee.formatted().clone();
   }

   fn valid_inputs(&self, ctx: ZeusCtx, depositor: Address, recipient: String) -> bool {
      self.valid_recipient(recipient) && self.valid_amount() && self.sufficient_balance(ctx, depositor)
   }

   fn should_get_suggested_fees(&mut self, ctx: ZeusCtx, depositor: Address, recipient: String) -> bool {
      // Don't request if already in progress
      if self.requesting {
         return false;
      }

      // Don't request if inputs are invalid
      if !self.valid_inputs(ctx, depositor, recipient) {
         return false;
      }

      let chain_path = (self.from_chain.chain.id(), self.to_chain.chain.id());
      let now = Instant::now();

      // Check cache
      match self.api_res_cache.get(&chain_path) {
         None => {
            // No cache exists, check rate limit
            if let Some(last_time) = self.last_request_time {
               let elapsed = now.duration_since(last_time).as_secs();
               if elapsed < TIME_BETWEEN_EACH_REQUEST {
                  tracing::debug!(
                     "Too soon since last request ({}s < {}s)",
                     elapsed,
                     TIME_BETWEEN_EACH_REQUEST
                  );
                  return false;
               }
            }
            tracing::debug!("No cache found, requesting");
            self.requesting = true;
            return true;
         }
         Some(cache) => {
            // Check if chain path changed
            if cache.res.origin_chain != self.from_chain.chain.id()
               || cache.res.destination_chain != self.to_chain.chain.id()
            {
               tracing::debug!("Chain path changed, requesting");
               self.requesting = true;
               return true;
            }

            // Check cache expiration
            if let Some(last_updated) = cache.last_updated {
               let elapsed = last_updated.elapsed().as_secs();
               if elapsed <= CACHE_EXPIRE {
                  tracing::debug!("Cache still valid ({}s <= {}s)", elapsed, CACHE_EXPIRE);
                  return false; // Cache is valid, no need to request
               }
               // Cache expired, check rate limit
               if let Some(last_time) = self.last_request_time {
                  let elapsed_since_last = now.duration_since(last_time).as_secs();
                  if elapsed_since_last < TIME_BETWEEN_EACH_REQUEST {
                     tracing::debug!(
                        "Cache expired but too soon since last request ({}s < {}s)",
                        elapsed_since_last,
                        TIME_BETWEEN_EACH_REQUEST
                     );
                     return false;
                  }
               }
               tracing::debug!(
                  "Cache expired ({}s > {}s), requesting",
                  elapsed,
                  CACHE_EXPIRE
               );
               self.requesting = true;
               return true;
            } else {
               tracing::debug!("Cache exists but no last_updated, requesting");
               self.requesting = true;
               return true;
            }
         }
      }
   }

   fn get_balance(&mut self, ctx: ZeusCtx, depositor: Address) {
      let chain = self.from_chain.chain.id();
      let ctx_clone = ctx.clone();
      self.balance_syncing = true;
      RT.spawn(async move {
         match get_eth_balance(ctx_clone, chain, depositor).await {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.across_bridge.balance_syncing = false;
               });
            }
            Err(e) => {
               tracing::error!("Error getting balance: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.across_bridge.balance_syncing = false;
               });
            }
         }
      });
   }

   fn get_suggested_fees(&mut self, ctx: ZeusCtx, depositor: Address, recipient: String) {
      if !self.should_get_suggested_fees(ctx, depositor, recipient.clone()) {
         return;
      }

      let from_chain = self.from_chain.chain.clone();
      let to_chain = self.to_chain.chain.clone();
      let input_token = ERC20Token::wrapped_native_token(from_chain.id());
      let output_token = ERC20Token::wrapped_native_token(to_chain.id());
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals);
      request_suggested_fees(
         from_chain.id(),
         to_chain.id(),
         input_token.address,
         output_token.address,
         amount.wei().unwrap(),
      );
      tracing::info!("Requested suggested fees");
   }

   /// Input amount - Minimum amount
   fn protocol_fee(&self, ctx: ZeusCtx) -> NumericValue {
      let input_amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals);
      if input_amount.is_zero() {
         return NumericValue::default();
      }

      let minimum_amount = self.minimum_amount();
      if minimum_amount.is_zero() {
         return NumericValue::default();
      }

      let amount = input_amount.f64() - minimum_amount.f64();
      self.value(ctx.clone(), amount)
   }

   /// Calculate the minimum amount to receive
   fn minimum_amount(&self) -> NumericValue {
      let scale = U256::from(10).pow(U256::from(self.currency.decimals));
      let input_amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals)
         .wei()
         .unwrap();
      let cache = self
         .api_res_cache
         .get(&(self.from_chain.chain.id(), self.to_chain.chain.id()));
      if cache.is_some() {
         let cache = cache.unwrap();
         let fee_pct = cache.res.suggested_fees.total_relay_fee.pct.clone();
         let fee_pct = U256::from_str(&fee_pct).unwrap_or_default();
         let fee_amount = (input_amount * fee_pct) / scale;
         let amount_after_fee = input_amount - fee_amount;

         NumericValue::format_wei(amount_after_fee, self.currency.decimals)
      } else {
         NumericValue::default()
      }
   }

   /// Currency value
   fn value(&self, ctx: ZeusCtx, amount: f64) -> NumericValue {
      let price = ctx.get_currency_price(&Currency::from_native(self.currency.clone()));

      if amount == 0.0 {
         return NumericValue::default();
      }

      return NumericValue::value(amount, price.f64());
   }

   /// Estimated cost of the transaction
   ///
   /// Returns (cost_wei, cost_usd)
   fn cost(&self, ctx: ZeusCtx) -> (U256, NumericValue) {
      let priority_fee = if self.priority_fee.is_empty() {
         parse_units("1", "gwei").unwrap().get_absolute()
      } else {
         parse_units(&self.priority_fee, "gwei")
            .unwrap_or(parse_units("1", "gwei").unwrap())
            .get_absolute()
      };

      let chain = self.from_chain.chain;
      let gas_used: u64 = 70_000;

      estimate_gas_cost(ctx, chain.id(), gas_used, priority_fee)
   }

   fn review_transaction(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      recipient: String,
      _recipient_name: Option<String>,
      ui: &mut Ui,
   ) {
      if !self.review_tx_window {
         return;
      }

      let from_chain = self.from_chain.chain.clone();
      let to_chain = self.to_chain.chain.clone();

      let cache = self
         .api_res_cache
         .get(&(self.from_chain.chain.id(), self.to_chain.chain.id()))
         .cloned();

      let miner_tip = if self.priority_fee.is_empty() {
         parse_units("1", "gwei").unwrap().get_absolute()
      } else {
         parse_units(&self.priority_fee, "gwei")
            .unwrap_or(parse_units("1", "gwei").unwrap())
            .get_absolute()
      };

      let input_amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals);
      let depositor = self.from_wallet.wallet.key.borrow().address();
      let recipient = Address::from_str(&recipient).unwrap_or(Address::ZERO);

      // Despite we are bridging from native to native, we still need to use the wrapped token in the call
      let input_token = ERC20Token::wrapped_native_token(from_chain.id());
      let output_token = ERC20Token::wrapped_native_token(to_chain.id());
      let output_amount = self.minimum_amount();

      let min_received_value = self.value(ctx.clone(), output_amount.f64());

      let mut should_send_tx = false;

      Window::new("Review Transaction")
         .title_bar(false)
         .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(400.0);
            ui.set_height(300.0);
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            let width = ui.available_width();

            ui.vertical_centered(|ui| {
               if cache.is_none() {
                  ui.label(
                     RichText::new("Failed to get suggested fees, try again later or try with a higher amount")
                        .size(theme.text_sizes.normal),
                  );
                  return;
               }

               ui.add_space(20.0);
               ui.label(RichText::new("You are about to Bridge").size(theme.text_sizes.heading));

               // Amount - Currency - Value
               ui.add_sized(vec2(width * 0.33, 20.0), |ui: &mut Ui| {
                  let res = Grid::new("amount_value")
                     .spacing(vec2(5.0, 0.0))
                     .show(ui, |ui| {
                        ui.label(
                           RichText::new(format!(
                              "{} {}",
                              input_amount.formatted(),
                              self.currency.symbol
                           ))
                           .size(theme.text_sizes.normal),
                        );
                        ui.add(icons.currency_icon(&Currency::from(self.currency.clone())));
                        ui.label(
                           RichText::new(format!(
                              "${}",
                              self.value(ctx.clone(), input_amount.f64()).formatted()
                           ))
                           .size(theme.text_sizes.normal),
                        );
                        ui.end_row();
                     });
                  res.response
               });

               ui.label(RichText::new("You will receive at least").size(theme.text_sizes.normal));

               // Minimum amount received - Value
               ui.add_sized(vec2(width * 0.33, 20.0), |ui: &mut Ui| {
                  let res = Grid::new("min_received_value")
                     .spacing(vec2(5.0, 0.0))
                     .show(ui, |ui| {
                        ui.label(
                           RichText::new(format!(
                              "{} {}",
                              output_amount.formatted(),
                              self.currency.symbol
                           ))
                           .size(theme.text_sizes.normal),
                        );

                        ui.label(
                           RichText::new(format!("${}", min_received_value.formatted())).size(theme.text_sizes.normal),
                        );
                        ui.end_row();
                     });
                  res.response
               });

               // Source - Destination
               let text = format!(
                  "{} -> {}",
                  self.from_chain.chain.name(),
                  self.to_chain.chain.name()
               );
               ui.label(RichText::new(text).size(theme.text_sizes.normal));

               let confirm = Button::new(RichText::new("Confirm").size(theme.text_sizes.normal));
               if ui.add(confirm).clicked() {
                  should_send_tx = true;
                  self.review_tx_window = false;
               }

               let cancel = Button::new(RichText::new("Cancel").size(theme.text_sizes.normal));
               if ui.add(cancel).clicked() {
                  self.review_tx_window = false;
               }
            });
         });

      if should_send_tx {
         let currency = Currency::from(self.currency.clone());
         let tx_method = TxMethod::Bridge((currency.clone(), input_amount.clone()));
         let base_fee = ctx
            .get_base_fee(self.from_chain.chain.id())
            .unwrap_or_default()
            .next;
         let cache = cache.unwrap();
         let relayer = cache.res.suggested_fees.exclusive_relayer;
         let timestamp = u32::from_str(&cache.res.suggested_fees.timestamp).unwrap_or_default();
         let fill_deadline = u32::from_str(&cache.res.suggested_fees.fill_deadline).unwrap_or_default();
         let exclusivity_deadline = cache.res.suggested_fees.exclusivity_deadline;

         // add a 5 minute deadline, because the fill deadline from the api is very high
         let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
         let deadline: u32 = now
            .is_ok()
            .then(|| (now.unwrap().as_secs() + 300) as u32)
            .unwrap_or(fill_deadline);
         tracing::info!("Deadline: {}", deadline);

         let deposit_args = DepositV3Args {
            depositor,
            recipient,
            input_token: input_token.address,
            output_token: output_token.address,
            input_amount: input_amount.wei().unwrap(),
            output_amount: output_amount.wei().unwrap(),
            destination_chain_id: to_chain.id(),
            exclusive_relayer: relayer,
            quote_timestamp: timestamp,
            fill_deadline: deadline,
            exclusivity_deadline,
            message: Bytes::default(),
         };

         let call_data = encode_deposit_v3(deposit_args.clone());

         let transact_to = spoke_pool_address(self.from_chain.chain.id()).unwrap();

         let params = TxParams::new(
            tx_method,
            self.from_wallet.wallet.key.clone(),
            transact_to,
            input_amount.wei().unwrap(),
            self.from_chain.chain,
            miner_tip,
            base_fee,
            call_data,
            70_000,
         );

         let dest_chain = self.to_chain.chain.id();
         RT.spawn(async move {
            match eth::across_bridge(
               ctx.clone(),
               currency,
               deadline,
               output_amount.clone(),
               dest_chain,
               recipient,
               params,
            )
            .await
            {
               Ok(_) => {
                  tracing::info!("Bridge Transaction Sent");
               }
               Err(e) => {
                  tracing::error!("Bridge Transaction Error: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.across_bridge.progress_window.reset_and_close();
                     gui.msg_window.open("Transaction Error", &e.to_string());
                  });
               }
            }
         });
      }
   }
}

fn request_suggested_fees(from_chain: u64, to_chain: u64, input_token: Address, output_token: Address, amount: U256) {
   RT.spawn(async move {
      let res = match get_suggested_fees(input_token, output_token, from_chain, to_chain, amount).await {
         Ok(res) => res,
         Err(e) => {
            tracing::error!("Failed to get suggested fees: {:?}", e);
            {
               SHARED_GUI.write(|gui| {
                  gui.across_bridge.requesting = false;
                  gui.across_bridge.last_request_time = Some(Instant::now());
               });
            }
            return;
         }
      };

      SHARED_GUI.write(|gui| {
         gui.across_bridge.api_res_cache.insert(
            (from_chain, to_chain),
            ApiResCache {
               res,
               last_updated: Some(Instant::now()),
            },
         );
         gui.across_bridge.requesting = false;
         gui.across_bridge.last_request_time = Some(Instant::now())
      });
   });
}

/// A progress window
pub struct ProgressWindow {
   pub id: String,
   pub open: bool,
   // Step 1
   pub simulating: bool,
   pub simulating_done: bool,
   // Step 2
   pub sending: bool,
   pub sending_done: bool,
   // Step 3
   pub order_filling: bool,
   pub order_filling_done: bool,
   pub funds_received: bool,
   pub currency_received: Currency,
   pub amount_received: NumericValue,
   pub dest_chain: ChainId,
   pub size: (f32, f32),
}

impl ProgressWindow {
   pub fn new(id: String) -> Self {
      Self {
         id,
         open: false,
         simulating: true,
         simulating_done: false,
         sending: false,
         sending_done: false,
         order_filling: false,
         order_filling_done: false,
         funds_received: false,
         currency_received: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         amount_received: NumericValue::default(),
         dest_chain: ChainId::new(1).unwrap(),
         size: (200.0, 150.0),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn reset_and_close(&mut self) {
      self.open = false;
      self.sending = false;
      self.sending_done = false;
      self.order_filling = false;
      self.order_filling_done = false;
      self.funds_received = false;
      self.simulating();
   }

   pub fn simulating(&mut self) {
      self.simulating = true;
      self.simulating_done = false;
   }

   pub fn done_simulating(&mut self) {
      self.simulating = false;
      self.simulating_done = true;
   }

   pub fn sending(&mut self) {
      self.sending = true;
      self.sending_done = false;
   }

   pub fn done_sending(&mut self) {
      self.sending = false;
      self.sending_done = true;
   }

   /// Deposit was success
   ///
   /// Now waiting for the order to be filled
   pub fn order_filling(&mut self) {
      self.order_filling = true;
      self.order_filling_done = false;
   }

   /// Order was filled
   pub fn done_order_filling(&mut self) {
      self.order_filling = false;
      self.order_filling_done = true;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new(&self.id)
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.add_space(20.0);
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let ui_width = ui.available_width();

               ui.add_sized(vec2(ui_width * 0.33, ui_width * 0.10), |ui: &mut Ui| {
                  let res = Grid::new("across_progress_window")
                     .spacing(vec2(5.0, 10.0))
                     .show(ui, |ui| {
                        // Step 1
                        ui.label(RichText::new("Simulating Transaction").size(theme.text_sizes.normal));
                        if self.simulating {
                           ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                        }
                        if self.simulating_done {
                           ui.label(RichText::new(GREEN_CHECK).size(theme.text_sizes.normal));
                        }
                        ui.end_row();

                        // Step 2
                        ui.label(RichText::new("Sending the Deposit").size(theme.text_sizes.normal));
                        if self.sending {
                           ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                        }
                        if self.sending_done {
                           ui.label(RichText::new(GREEN_CHECK).size(theme.text_sizes.normal));
                        }
                        ui.end_row();

                        // Step 3
                        ui.label(RichText::new("Waiting for the order to be filled").size(theme.text_sizes.normal));
                        if self.order_filling {
                           ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                        }
                        if self.order_filling_done {
                           ui.label(RichText::new(GREEN_CHECK).size(theme.text_sizes.normal));
                        }
                        ui.end_row();

                        if self.funds_received {
                           let text = format!(
                              "You have sent {} {} on the {} chain {}",
                              self.amount_received.formatted(),
                              self.currency_received.symbol(),
                              self.dest_chain.name(),
                              GREEN_CHECK
                           );
                           ui.label(RichText::new(text).size(theme.text_sizes.normal));
                        }
                        ui.end_row();
                     });
                  res.response
               });

               if ui
                  .add(Button::new(
                     RichText::new("Ok").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.reset_and_close();
               }
            });
         });
   }
}
