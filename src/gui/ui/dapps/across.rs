use crate::assets::icons::Icons;
use crate::core::utils::action::OnChainAction;
use crate::core::utils::eth::get_eth_balance;
use crate::core::{
   ZeusCtx,
   utils::{RT, estimate_tx_cost, eth},
};
use crate::gui::{
   SHARED_GUI,
   ui::{ChainSelect, ContactsUi, RecipientSelectionWindow},
};
use egui::{
   Align, Align2, Button, Color32, FontId, Frame, Grid, Layout, Margin, RichText, Spinner,
   TextEdit, Ui, Window, vec2,
};
use egui_theme::{Theme, utils::widget_visuals};
use egui_widgets::Label;
use std::{collections::HashMap, str::FromStr, sync::Arc, time::Instant};
use zeus_eth::currency::ERC20Token;
use zeus_eth::dapps::Dapp;
use zeus_eth::{
   abi::protocols::across::{DepositV3Args, encode_deposit_v3},
   alloy_primitives::{Address, Bytes, U256},
   currency::{Currency, NativeCurrency},
   dapps::across::*,
   types::BSC,
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
   pub currency: NativeCurrency,
   pub amount: String,
   pub from_chain: ChainSelect,
   pub to_chain: ChainSelect,
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
      Self {
         open: false,
         currency: NativeCurrency::from_chain_id(1).unwrap(),
         amount: String::new(),
         from_chain: ChainSelect::new("across_bridge_from_chain", 1),
         to_chain: ChainSelect::new("across_bridge_to_chain", 10),
         balance_syncing: false,
         requesting: false,
         last_request_time: None,
         api_res_cache: HashMap::new(),
         size: (550.0, 750.0),
      }
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.currency = currency.native().cloned().unwrap();
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

      recipient_selection.show(ctx.clone(), theme, icons.clone(), contacts_ui, ui);
      let recipient = recipient_selection.get_recipient();
      let recipient_name = recipient_selection.get_recipient_name();
      let from_chain = self.from_chain.chain.id();
      let depositor = ctx.current_wallet().address;
      self.currency = NativeCurrency::from_chain_id(from_chain).unwrap();

      self.get_suggested_fees(ctx.clone(), depositor, &recipient);

      let frame = theme.frame1;
      let bg_color = frame.fill;
      Window::new("Across Bridge")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
               ui.set_max_width(self.size.0);
               ui.set_max_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let ui_width = ui.available_width();

               // Header
               ui.vertical_centered(|ui| {
                  ui.label(RichText::new("Bridge").size(theme.text_sizes.heading));
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  if ui
                     .add(Button::new(
                        RichText::new("⟲").size(theme.text_sizes.small),
                     ))
                     .clicked()
                  {
                     self.get_balance(ctx.clone(), depositor);
                  }
               });

               // Asset and amount selection
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new("Amount").size(theme.text_sizes.large));
               });

               let balance = ctx
                  .get_eth_balance(from_chain, depositor)
                  .unwrap_or_default();

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.set_max_width(ui_width * 0.25);
                  Grid::new("asset_and_amount")
                     .spacing(vec2(5.0, 0.0))
                     .show(ui, |ui| {
                        ui.add(
                           TextEdit::singleline(&mut self.amount)
                              .hint_text("0")
                              .font(FontId::proportional(theme.text_sizes.normal))
                              .min_size(vec2(ui_width * 0.25, 25.0))
                              .background_color(theme.colors.text_edit_bg2)
                              .margin(Margin::same(10)),
                        );
                        let icon = icons.native_currency_icon(self.currency.chain_id);
                        let text =
                           RichText::new(&self.currency.symbol).size(theme.text_sizes.normal);
                        ui.add(Label::new(text, Some(icon)));
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
                        ui.label(RichText::new("Recipient").size(theme.text_sizes.large));

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
                           if !self.valid_recipient(&recipient) {
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
                        ui.label(RichText::new("From").size(theme.text_sizes.large));
                        self.from_chain.show(BSC, theme, icons.clone(), ui);
                        ui.end_row();
                     });
               });
               ui.add_space(10.0);

               // To Chain
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  Grid::new("to_chain")
                     .spacing(vec2(5.0, 0.0))
                     .show(ui, |ui| {
                        ui.label(RichText::new("To").size(theme.text_sizes.large));
                        self.to_chain.grid_id = "across_bridge_to_chain";
                        self.to_chain.show(BSC, theme, icons.clone(), ui);
                        ui.end_row();
                     });
               });
               ui.add_space(10.0);

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
                  if self.requesting {
                     ui.add_space(10.0);
                     ui.add(Spinner::new().size(20.0).color(Color32::WHITE));
                  }
               });

               // Estimated time to fill
               let fill_time = self
                  .api_res_cache
                  .get(&(
                     self.from_chain.chain.id(),
                     self.to_chain.chain.id(),
                  ))
                  .map(|c| c.res.suggested_fees.estimated_fill_time_sec);
               if let Some(fill_time) = fill_time {
                  ui.add_space(10.0);
                  ui.label(
                     RichText::new(format!(
                        "Estimated time to fill: {} seconds",
                        fill_time
                     ))
                     .size(theme.text_sizes.normal),
                  );
                  ui.add_space(20.0);
               } else {
                  ui.add_space(20.0);
               }

               // Bridge Button
               widget_visuals(ui, theme.get_button_visuals(bg_color));

               let bridge = Button::new(RichText::new("Bridge").size(theme.text_sizes.normal))
                  .min_size(vec2(ui_width * 0.90, 40.0));

               if !self.valid_inputs(ctx.clone(), depositor, &recipient) {
                  ui.disable();
               }

               ui.vertical_centered(|ui| {
                  if ui.add(bridge).clicked() {
                     self.send_transaction(ctx, recipient);
                  }
               });
            });
         });
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, depositor: Address) -> bool {
      let balance = ctx
         .get_eth_balance(self.from_chain.chain.id(), depositor)
         .unwrap_or_default();
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals);
      let amount = amount.wei2();
      balance.wei2() >= amount
   }

   /// Max amount = Balance - cost
   fn max_amount(&self, ctx: ZeusCtx) -> NumericValue {
      let chain = self.from_chain.chain;
      let owner = ctx.current_wallet().address;
      let balance = ctx.get_eth_balance(chain.id(), owner).unwrap_or_default();
      let (cost_wei, _) = self.cost(ctx.clone());

      if balance.wei2() < cost_wei.wei2() {
         return NumericValue::default();
      }

      let max = balance.wei2() - cost_wei.wei2();
      NumericValue::format_wei(max, self.currency.decimals)
   }

   fn valid_recipient(&self, recipient: &String) -> bool {
      let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
      recipient != Address::ZERO
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn valid_inputs(&self, ctx: ZeusCtx, depositor: Address, recipient: &String) -> bool {
      self.valid_recipient(recipient)
         && self.valid_amount()
         && self.sufficient_balance(ctx, depositor)
   }

   fn should_get_suggested_fees(
      &mut self,
      ctx: ZeusCtx,
      depositor: Address,
      recipient: &String,
   ) -> bool {
      // Don't request if already in progress
      if self.requesting {
         return false;
      }

      // Don't request if inputs are invalid
      if !self.valid_inputs(ctx, depositor, recipient) {
         return false;
      }

      let chain_path = (
         self.from_chain.chain.id(),
         self.to_chain.chain.id(),
      );
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
                  tracing::debug!(
                     "Cache still valid ({}s <= {}s)",
                     elapsed,
                     CACHE_EXPIRE
                  );
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

   fn get_suggested_fees(&mut self, ctx: ZeusCtx, depositor: Address, recipient: &String) {
      if !self.should_get_suggested_fees(ctx, depositor, recipient) {
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
      let cache = self.api_res_cache.get(&(
         self.from_chain.chain.id(),
         self.to_chain.chain.id(),
      ));
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
      let price = ctx.get_currency_price(&Currency::from(self.currency.clone()));

      if amount == 0.0 {
         return NumericValue::default();
      }

      return NumericValue::value(amount, price.f64());
   }

   /// Estimated cost of the transaction
   ///
   /// Returns (cost_wei, cost_usd)
   fn cost(&self, ctx: ZeusCtx) -> (NumericValue, NumericValue) {
      let chain = self.from_chain.chain;
      let gas_used: u64 = 70_000;
      let fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();

      estimate_tx_cost(ctx, chain.id(), gas_used, fee.wei2())
   }

   fn send_transaction(&mut self, ctx: ZeusCtx, recipient: String) {
      let cache = self
         .api_res_cache
         .get(&(
            self.from_chain.chain.id(),
            self.to_chain.chain.id(),
         ))
         .cloned();

      if cache.is_none() {
         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               gui.msg_window.open(
                  "Failed to get suggested fees, try again later or increase the amount",
                  String::new(),
               );
            });
         });
         return;
      }

      let from_chain = self.from_chain.chain.clone();
      let to_chain = self.to_chain.chain.clone();

      // Despite we are bridging from native to native, we still need to use the wrapped token in the call
      let input_token = ERC20Token::wrapped_native_token(from_chain.id());
      let output_token = ERC20Token::wrapped_native_token(to_chain.id());
      let input_amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals);
      let input_usd = ctx.get_currency_value2(
         input_amount.f64(),
         &Currency::from(input_token.clone()),
      );
      let output_amount = self.minimum_amount();
      let output_usd = ctx.get_currency_value2(
         output_amount.f64(),
         &Currency::from(output_token.clone()),
      );

      let current_wallet = ctx.current_wallet();
      let signer = ctx.get_wallet(current_wallet.address).key;
      let depositor = signer.borrow().address();
      let recipient = Address::from_str(&recipient).unwrap_or(Address::ZERO);

      let cache = cache.unwrap();
      let relayer = cache.res.suggested_fees.exclusive_relayer;
      let timestamp = u32::from_str(&cache.res.suggested_fees.timestamp).unwrap_or_default();
      let fill_deadline =
         u32::from_str(&cache.res.suggested_fees.fill_deadline).unwrap_or_default();
      let exclusivity_deadline = cache.res.suggested_fees.exclusivity_deadline;

      // add a 5 minute deadline, because the fill deadline from the api is very high
      let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
      let deadline: u32 = now
         .is_ok()
         .then(|| (now.unwrap().as_secs() + 300) as u32)
         .unwrap_or(fill_deadline);

      let deposit_args = DepositV3Args {
         depositor,
         recipient,
         input_token: input_token.address,
         output_token: output_token.address,
         input_amount: input_amount.wei2(),
         output_amount: output_amount.wei2(),
         destination_chain_id: to_chain.id(),
         exclusive_relayer: relayer,
         quote_timestamp: timestamp,
         fill_deadline: deadline,
         exclusivity_deadline,
         message: Bytes::default(),
      };

      let call_data = encode_deposit_v3(deposit_args.clone());
      let transact_to = spoke_pool_address(self.from_chain.chain.id()).unwrap();
      let action = OnChainAction::new_bridge(
         Dapp::Across,
         from_chain.id(),
         to_chain.id(),
         input_token.into(),
         output_token.into(),
         input_amount.clone(),
         input_usd,
         output_amount,
         output_usd,
         depositor,
         recipient,
      );

      RT.spawn(async move {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
         });

         match eth::across_bridge(
            ctx.clone(),
            from_chain,
            to_chain,
            deadline,
            action,
            depositor,
            recipient,
            transact_to,
            call_data,
            input_amount.wei2(),
         )
         .await
         {
            Ok(_) => {
               tracing::info!("Bridge Transaction Sent");
            }
            Err(e) => {
               tracing::error!("Bridge Transaction Error: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.progress_window.reset();
                  gui.loading_window.reset();
                  gui.msg_window.open("Transaction Error", &e.to_string());
               });
            }
         }
      });
   }
}

fn request_suggested_fees(
   from_chain: u64,
   to_chain: u64,
   input_token: Address,
   output_token: Address,
   amount: U256,
) {
   RT.spawn(async move {
      let res = match get_suggested_fees(
         input_token,
         output_token,
         from_chain,
         to_chain,
         amount,
      )
      .await
      {
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
