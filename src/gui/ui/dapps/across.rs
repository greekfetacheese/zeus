use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, estimate_tx_cost, eth},
};
use crate::gui::{
   SHARED_GUI,
   ui::{
      ChainSelect, ContactsUi, RecipientSelectionWindow, Step,
      dapps::amount_field_with_currency_selector,
   },
};
use anyhow::anyhow;
use egui::{
   Align, Align2, Button, Color32, FontId, Grid, Layout, Margin, RichText, Spinner, TextEdit, Ui,
   Window, vec2,
};
use egui_theme::Theme;
use std::time::Duration;
use std::{collections::HashMap, str::FromStr, sync::Arc, time::Instant};
use zeus_eth::currency::ERC20Token;
use zeus_eth::{
   abi::protocols::across::{DepositV3Args, encode_deposit_v3},
   abi::protocols::across::{decode_filled_relay_log, filled_relay_signature},
   alloy_primitives::{Address, Bytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockNumberOrTag, Filter},
   currency::{Currency, NativeCurrency},
   types::{BSC, ChainId},
   utils::{NumericValue, address_book},
};

use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Cache the results for this many seconds
const CACHE_EXPIRE: u64 = 250;

const TIME_BETWEEN_EACH_REQUEST: u64 = 2;

type ChainPath = (u64, u64);

#[derive(Debug, Default, Clone)]
pub struct ApiResCache {
   pub res: ClientResponse,
   pub last_updated: Option<Instant>,
}

/// A UI for bridging assets between chains using the Across protocol (https://across.to)
///
/// For simplicity currently only bridges Native Currencies (ETH)
pub struct AcrossBridge {
   pub open: bool,
   pub currency: Currency,
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
         currency: NativeCurrency::from(1).into(),
         amount: String::new(),
         from_chain: ChainSelect::new("across_bridge_from_chain", 1),
         to_chain: ChainSelect::new("across_bridge_to_chain", 10),
         balance_syncing: false,
         requesting: false,
         last_request_time: None,
         api_res_cache: HashMap::new(),
         size: (450.0, 650.0),
      }
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.currency = currency.into();
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
      let depositor = ctx.current_wallet_address();
      self.currency = NativeCurrency::from(from_chain).into();

      self.get_suggested_fees(ctx.clone(), depositor, &recipient);

      let main_frame = theme.frame1;

      Window::new("across_bridge_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(main_frame)
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let ui_width = ui.available_width();

               ui.label(RichText::new("Bridge").size(theme.text_sizes.heading));

               let inner_frame = theme.frame2;

               let label = String::from("Send");
               let owner = ctx.current_wallet_address();
               let balance_fn = || ctx.get_currency_balance(from_chain, owner, &self.currency);

               let cost = self.cost(ctx.clone());
               let balance = balance_fn();
               let max_amount = || {
                  if balance.wei() > cost.0.wei() {
                     NumericValue::format_wei(
                        balance.wei() - cost.0.wei(),
                        self.currency.decimals(),
                     )
                  } else {
                     NumericValue::default()
                  }
               };
               let amount = self.amount.parse().unwrap_or(0.0);
               let value = || ctx.get_currency_value_for_amount(amount, &self.currency);

               inner_frame.show(ui, |ui| {
                  ui.set_width(ui_width);

                  amount_field_with_currency_selector(
                     ctx.clone(),
                     theme,
                     icons.clone(),
                     Some(label),
                     &self.currency,
                     &mut self.amount,
                     None,
                     None,
                     balance_fn,
                     max_amount,
                     value,
                     ui,
                  );
               });

               ui.add_space(10.0);

               // Recipient
               inner_frame.show(ui, |ui| {
                  ui.horizontal(|ui| {
                     ui.label(RichText::new("Recipient").size(theme.text_sizes.large));
                     ui.add_space(10.0);

                     if !recipient.is_empty() {
                        if let Some(name) = &recipient_name {
                           ui.label(
                              RichText::new(name)
                                 .size(theme.text_sizes.normal)
                                 .color(theme.colors.text_secondary),
                           );
                        } else {
                           ui.label(
                              RichText::new("Unknown Address")
                                 .size(theme.text_sizes.normal)
                                 .color(theme.colors.error_color),
                           );
                        }
                     }
                  });

                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     let hint = RichText::new("Search contacts or enter an address")
                        .size(theme.text_sizes.normal)
                        .color(theme.colors.text_secondary);

                     let res = ui.add(
                        TextEdit::singleline(&mut recipient_selection.recipient)
                           .hint_text(hint)
                           .min_size(vec2(ui_width, 25.0))
                           .margin(Margin::same(10))
                           .background_color(theme.colors.text_edit_bg)
                           .font(FontId::proportional(theme.text_sizes.normal)),
                     );

                     if res.clicked() {
                        recipient_selection.open();
                     }
                  });
               });

               ui.add_space(10.0);

               // From Chain
               inner_frame.show(ui, |ui| {
                  ui.set_width(ui_width);
                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     Grid::new("across_bridge_from_chain").spacing(vec2(5.0, 0.0)).show(ui, |ui| {
                        ui.label(RichText::new("From").size(theme.text_sizes.large));
                        self.from_chain.show(BSC, theme, icons.clone(), ui);
                        ui.end_row();
                     });
                  });

                  ui.add_space(10.0);

                  // To Chain
                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     Grid::new("across_bridge_to_chain").spacing(vec2(5.0, 0.0)).show(ui, |ui| {
                        ui.label(RichText::new("To").size(theme.text_sizes.large));
                        self.to_chain.show(BSC, theme, icons.clone(), ui);
                        ui.end_row();
                     });
                  });
               });

               ui.add_space(10.0);

               inner_frame.show(ui, |ui| {
                  // Network Fee
                  ui.label(
                     RichText::new(format!(
                        "Network Fee≈ ${}",
                        self.cost(ctx.clone()).1.formatted()
                     ))
                     .size(theme.text_sizes.normal),
                  );

                  // Bridge Fee
                  ui.label(
                     RichText::new(format!(
                        "Bridge Fee≈ ${}",
                        self.bridge_fee(ctx.clone()).formatted()
                     ))
                     .size(theme.text_sizes.normal),
                  );

                  if self.requesting {
                     ui.add(Spinner::new().size(20.0).color(Color32::WHITE));
                  }

                  // Estimated time to fill
                  let fill_time = self
                     .api_res_cache
                     .get(&(
                        self.from_chain.chain.id(),
                        self.to_chain.chain.id(),
                     ))
                     .map(|c| c.res.suggested_fees.estimated_fill_time_sec);
                  if let Some(fill_time) = fill_time {
                     ui.label(
                        RichText::new(format!(
                           "Estimated time to fill: {} seconds",
                           fill_time
                        ))
                        .size(theme.text_sizes.normal),
                     );
                  }
               });

               self.bridge_button(ctx.clone(), theme, depositor, recipient, ui);
            });
         });
   }

   fn bridge_button(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      depositor: Address,
      recipient: String,
      ui: &mut Ui,
   ) {
      let valid_recipient = self.valid_recipient(&recipient);
      let valid_amount = self.valid_amount();
      let has_balance = self.sufficient_balance(ctx.clone(), depositor);
      let has_entered_amount = !self.amount.is_empty();
      let valid_inputs = valid_amount && valid_recipient && has_balance;

      let mut button_text = "Bridge".to_string();

      if !valid_recipient {
         button_text = "Invalid Recipient".to_string();
      }

      if !valid_amount {
         button_text = "Invalid Amount".to_string();
      }

      if !has_entered_amount {
         button_text = "Enter Amount".to_string();
      }

      if !has_balance {
         button_text = format!("Insufficient {} Balance", self.currency.symbol());
      }

      let text = RichText::new(button_text).size(theme.text_sizes.large);
      let button = Button::new(text).min_size(vec2(ui.available_width() * 0.8, 45.0));

      if ui.add_enabled(valid_inputs, button).clicked() {
         match self.send_transaction(ctx, recipient) {
            Ok(_) => {}
            Err(e) => {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Error while sending transaction", e.to_string());
                  });
               });
            }
         }
      }
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, depositor: Address) -> bool {
      let balance = ctx.get_eth_balance(self.from_chain.chain.id(), depositor);
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      balance.wei() >= amount.wei()
   }

   /// Estimated cost of the transaction
   ///
   /// Returns (cost_wei, cost_usd)
   fn cost(&self, ctx: ZeusCtx) -> (NumericValue, NumericValue) {
      let chain = self.from_chain.chain;
      let gas_used: u64 = 70_000;
      let fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();

      estimate_tx_cost(ctx, chain.id(), gas_used, fee.wei())
   }

   /// Input amount - Minimum amount
   fn bridge_fee(&self, ctx: ZeusCtx) -> NumericValue {
      let input_amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
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
      let scale = U256::from(10).pow(U256::from(self.currency.decimals()));
      let input_amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals()).wei();
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

         NumericValue::format_wei(amount_after_fee, self.currency.decimals())
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

      NumericValue::value(amount, price.f64())
   }

   fn valid_recipient(&self, recipient: &str) -> bool {
      let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
      recipient != Address::ZERO
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn valid_inputs(&self, ctx: ZeusCtx, depositor: Address, recipient: &str) -> bool {
      self.valid_recipient(recipient)
         && self.valid_amount()
         && self.sufficient_balance(ctx, depositor)
   }

   fn should_get_suggested_fees(
      &mut self,
      ctx: ZeusCtx,
      depositor: Address,
      recipient: &str,
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

   fn _sync_balance(&mut self, ctx: ZeusCtx, depositor: Address) {
      let chain = self.from_chain.chain.id();
      let ctx_clone = ctx.clone();
      self.balance_syncing = true;
      RT.spawn(async move {
         let manager = ctx_clone.balance_manager();
         match manager.update_eth_balance(ctx_clone.clone(), chain, depositor).await {
            Ok(_) => {}
            Err(e) => {
               tracing::error!("Failed to update ETH balance: {}", e);
            }
         }

         ctx_clone.save_balance_manager();

         SHARED_GUI.write(|gui| {
            gui.across_bridge.balance_syncing = false;
         });
      });
   }

   fn get_suggested_fees(&mut self, ctx: ZeusCtx, depositor: Address, recipient: &String) {
      if !self.should_get_suggested_fees(ctx, depositor, recipient) {
         return;
      }

      let from_chain = self.from_chain.chain;
      let to_chain = self.to_chain.chain;
      let input_token = ERC20Token::wrapped_native_token(from_chain.id());
      let output_token = ERC20Token::wrapped_native_token(to_chain.id());
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      request_suggested_fees(
         from_chain.id(),
         to_chain.id(),
         input_token.address,
         output_token.address,
         amount.wei(),
      );
      tracing::info!("Requested suggested fees");
   }

   fn send_transaction(&mut self, ctx: ZeusCtx, recipient: String) -> Result<(), anyhow::Error> {
      let cache = self
         .api_res_cache
         .get(&(
            self.from_chain.chain.id(),
            self.to_chain.chain.id(),
         ))
         .cloned();

      if cache.is_none() {
         return Err(anyhow!(
            "Failed to get suggested fees, try again later or increase the amount"
         ));
      }

      let from_chain = self.from_chain.chain;
      let to_chain = self.to_chain.chain;

      // Despite we are bridging from native to native, we still need to use the wrapped token in the call
      let input_token = ERC20Token::wrapped_native_token(from_chain.id());
      let output_token = ERC20Token::wrapped_native_token(to_chain.id());
      let input_amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      let output_amount = self.minimum_amount();

      let signer = ctx.get_current_wallet().key;
      let depositor = signer.address();
      let recipient = Address::from_str(&recipient)?;

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
         input_amount: input_amount.wei(),
         output_amount: output_amount.wei(),
         destination_chain_id: to_chain.id(),
         exclusive_relayer: relayer,
         quote_timestamp: timestamp,
         fill_deadline: deadline,
         exclusivity_deadline,
         message: Bytes::default(),
      };

      let call_data = encode_deposit_v3(deposit_args.clone());
      let transact_to = address_book::across_spoke_pool_v2(self.from_chain.chain.id()).unwrap();

      RT.spawn(async move {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
         });

         match across_bridge(
            ctx.clone(),
            from_chain,
            to_chain,
            deadline,
            depositor,
            recipient,
            transact_to,
            call_data,
            input_amount.wei(),
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
                  gui.msg_window.open("Transaction Error", e.to_string());
               });
            }
         }
      });
      Ok(())
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

async fn across_bridge(
   ctx: ZeusCtx,
   chain: ChainId,
   dest_chain: ChainId,
   deadline: u32,
   from: Address,
   recipient: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<(), anyhow::Error> {
   // Across protocol is very fast on filling the orders
   // So we get the latest block from the destination chain now so we dont miss it and the progress window stucks
   let mut from_block = None;
   if ctx.client_available(dest_chain.id()) {
      let dest_client = ctx.get_client(dest_chain.id()).await?;
      let block = dest_client.get_block_number().await;
      if block.is_ok() {
         from_block = Some(block.unwrap());
      }
   }

   let mev_protect = false;
   let (_, tx_rich) = eth::send_transaction(
      ctx.clone(),
      "".to_string(),
      None,
      None,
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
   )
   .await?;

   // Update the sender's balance
   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      let manager = ctx_clone.balance_manager();
      match manager.update_eth_balance(ctx_clone.clone(), chain.id(), from).await {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Failed to update ETH balance: {}", e);
         }
      }

      ctx_clone.calculate_portfolio_value(chain.id(), from);
      ctx_clone.save_balance_manager();
      ctx_clone.save_portfolio_db();
   });

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   let step2 = Step {
      id: "step2",
      in_progress: true,
      finished: false,
      msg: "Waiting for the order to be filled".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window.open_with(vec![step1, step2], "Success!".to_string());
      gui.request_repaint();
   });

   wait_for_fill(
      ctx.clone(),
      dest_chain,
      recipient,
      from_block,
      deadline,
   )
   .await?;

   SHARED_GUI.write(|gui| {
      gui.progress_window.finish_last_step();
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   // update the recipients balance if needed
   let exists = ctx.wallet_exists(interact_to);
   RT.spawn(async move {
      let manager = ctx.balance_manager();

      if exists {
         match manager.update_eth_balance(ctx.clone(), chain.id(), interact_to).await {
            Ok(_) => {}
            Err(e) => {
               tracing::error!("Failed to update ETH balance: {}", e);
            }
         }

         ctx.calculate_portfolio_value(chain.id(), interact_to);
      }

      ctx.save_balance_manager();
      ctx.save_portfolio_db();
   });

   Ok(())
}

async fn wait_for_fill(
   ctx: ZeusCtx,
   dest_chain: ChainId,
   recipient: Address,
   from_block: Option<u64>,
   deadline: u32,
) -> Result<(), anyhow::Error> {
   if from_block.is_none() {
      return Ok(());
   }

   let from_block = from_block.unwrap();
   let mut block_time_ms = dest_chain.block_time();
   if dest_chain.is_arbitrum() {
      // give more time so we dont spam the rpc
      block_time_ms *= 3;
   }

   let now = std::time::Instant::now();
   let mut funds_received = false;

   let target = address_book::across_spoke_pool_v2(dest_chain.id())?;
   let filter = Filter::new()
      .from_block(BlockNumberOrTag::Number(from_block))
      .address(vec![target])
      .event(filled_relay_signature());

   let dest_client = ctx.get_client(dest_chain.id()).await?;

   // Wait for the order to be filled at the destination chain
   while now.elapsed().as_secs() < deadline as u64 {
      let logs = dest_client.get_logs(&filter).await?;
      for log in logs {
         if let Ok(decoded) = decode_filled_relay_log(log.data()) {
            tracing::debug!("Filled Relay Log Decoded: {:#?}", decoded);
            if decoded.recipient == recipient {
               tracing::info!("Funds received");
               funds_received = true;
               break;
            }
         }
      }

      if funds_received {
         break;
      }

      tokio::time::sleep(Duration::from_millis(block_time_ms)).await;
   }

   // I dont expect this to happen
   if funds_received {
      Ok(())
   } else {
      let err = format!(
         "Deadline exceeded\n
         No funds received on the {} chain\n
         Your deposit should be refunded shortly",
         dest_chain.name(),
      );
      Err(anyhow!(err))
   }
}

#[derive(Debug, Default, Clone)]
pub struct ClientResponse {
   /// The Origin Chain used for the request
   pub origin_chain: u64,
   /// The Destination Chain used for the request
   pub destination_chain: u64,
   /// The input token used for the request
   pub input_token: Address,
   /// The output token used for the request
   pub output_token: Address,
   /// The amount used for the request
   pub amount: U256,
   /// The suggested fees for the request
   pub suggested_fees: SuggestedFeesResponse,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FeeDetail {
   pub pct: String,   // Percentage as a string (e.g., "78930919924823")
   pub total: String, // Total fee in wei as a string (e.g., "78930919924823")
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Limits {
   #[serde(rename = "minDeposit")]
   pub min_deposit: String,
   #[serde(rename = "maxDeposit")]
   pub max_deposit: String,
   #[serde(rename = "maxDepositInstant")]
   pub max_deposit_instant: String,
   #[serde(rename = "maxDepositShortDelay")]
   pub max_deposit_short_delay: String,
   #[serde(rename = "recommendedDepositInstant")]
   pub recommended_deposit_instant: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SuggestedFeesResponse {
   #[serde(rename = "estimatedFillTimeSec")]
   pub estimated_fill_time_sec: u32,
   #[serde(rename = "capitalFeePct")]
   pub capital_fee_pct: String,
   #[serde(rename = "capitalFeeTotal")]
   pub capital_fee_total: String,
   #[serde(rename = "relayGasFeePct")]
   pub relay_gas_fee_pct: String,
   #[serde(rename = "relayGasFeeTotal")]
   pub relay_gas_fee_total: String,
   #[serde(rename = "relayFeePct")]
   pub relay_fee_pct: String,
   #[serde(rename = "relayFeeTotal")]
   pub relay_fee_total: String,
   #[serde(rename = "lpFeePct")]
   pub lp_fee_pct: String,
   pub timestamp: String,
   #[serde(rename = "isAmountTooLow")]
   pub is_amount_too_low: bool,
   #[serde(rename = "quoteBlock")]
   pub quote_block: String,
   #[serde(rename = "exclusiveRelayer")]
   pub exclusive_relayer: Address,
   #[serde(rename = "exclusivityDeadline")]
   pub exclusivity_deadline: u32,
   #[serde(rename = "spokePoolAddress")]
   pub spoke_pool_address: Address,
   #[serde(rename = "destinationSpokePoolAddress")]
   pub destination_spoke_pool_address: Address,
   #[serde(rename = "totalRelayFee")]
   pub total_relay_fee: FeeDetail,
   #[serde(rename = "relayerCapitalFee")]
   pub relayer_capital_fee: FeeDetail,
   #[serde(rename = "relayerGasFee")]
   pub relayer_gas_fee: FeeDetail,
   #[serde(rename = "lpFee")]
   pub lp_fee: FeeDetail,
   pub limits: Limits,
   #[serde(rename = "fillDeadline")]
   pub fill_deadline: String,
}

pub async fn get_suggested_fees(
   input_token: Address,
   output_token: Address,
   origin_chain_id: u64,
   destination_chain_id: u64,
   amount: U256,
) -> Result<ClientResponse, anyhow::Error> {
   let client = Client::new();
   let url = "https://app.across.to/api/suggested-fees";

   let params = [
      ("inputToken", input_token.to_string()),
      ("outputToken", output_token.to_string()),
      ("originChainId", origin_chain_id.to_string()),
      (
         "destinationChainId",
         destination_chain_id.to_string(),
      ),
      ("amount", amount.to_string()),
   ];

   let raw_response = client.get(url).query(&params).send().await?.text().await?;

   let response = serde_json::from_str::<SuggestedFeesResponse>(&raw_response)?;

   let res = ClientResponse {
      origin_chain: origin_chain_id,
      destination_chain: destination_chain_id,
      input_token,
      output_token,
      amount,
      suggested_fees: response,
   };

   Ok(res)
}

fn _supports_chain(chain_id: u64) -> Result<bool, anyhow::Error> {
   let chain = ChainId::new(chain_id)?;
   match chain {
      ChainId::Ethereum(_) => Ok(true),
      ChainId::Optimism(_) => Ok(true),
      ChainId::Base(_) => Ok(true),
      ChainId::Arbitrum(_) => Ok(true),
      ChainId::BinanceSmartChain(_) => Ok(false),
   }
}
