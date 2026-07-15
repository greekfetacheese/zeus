use eframe::egui::{
   Align2, CursorIcon, FontId, Frame, Margin, OpenUrl, RichText, Ui, Window, vec2,
};

use std::{
   collections::HashMap,
   sync::Arc,
   time::{Duration, Instant},
};

use crate::utils::{RT, estimate_tx_cost};
use crate::{
   core::{DecodedEvent, ShieldParams, TransactionAnalysis, ZeusCtx, send_transaction},
   utils::simulate::simulate_transaction,
};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      ContactsUi, RecipientSelectionWindow, TokenSelectionWindow, common::AmountField,
      dapps::uniswap::swap::wrap_eth,
   },
};
use crate::utils::simulate::fetch_accounts_info;
use zeus_theme::Theme;
use zeus_widgets::{Button, SecureTextEdit};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, NativeCurrency},
   revm_utils::{ForkFactory, Host, new_evm},
   types::ChainId,
   utils::NumericValue,
};

use zeus_railgun::{RailgunAddress, caip::AssetId, rand};

use anyhow::anyhow;
use tracing::{error, info};

const POOL_UPDATE_TIMEOUT: u64 = 60;

pub struct ShieldUi {
   open: bool,
   currency: Currency,
   amount_field: AmountField,
   recipient: String,
   recipient_name: Option<String>,
   search_query: String,
   size: (f32, f32),
   price_syncing: bool,
   syncing_balance: bool,
   sending_tx: bool,
   last_price_update: HashMap<Address, Instant>,
}

impl ShieldUi {
   pub fn new() -> Self {
      Self {
         open: false,
         currency: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         amount_field: AmountField::new(),
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         size: (500.0, 500.0),
         price_syncing: false,
         syncing_balance: false,
         sending_tx: false,
         last_price_update: HashMap::new(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      *self = Self::new();
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.currency = currency;
   }

   pub fn clear_recipient(&mut self) {
      self.recipient_name = None;
      self.recipient = String::new();
   }

   pub fn clear_search_query(&mut self) {
      self.search_query = String::new();
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      recipient_selection: &mut RecipientSelectionWindow,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let frame = theme.frame1;

      Window::new("shield_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 30.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
                  ui.set_width(self.size.0);
                  ui.set_max_height(self.size.1);
                  ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let text_edit_visuals = theme.text_edit_visuals();

                  ui.label(RichText::new("Shield").size(theme.text_sizes.heading));

                  let owner = ctx.current_wallet_info(false).address;

                  let chain = ctx.chain();
                  let inner_frame = theme.frame2;

                  // Currency Selection
                  let label = String::from("Amount");
                  let balance_fn = || ctx.get_currency_balance(chain.id(), owner, &self.currency);
                  let cost = self.cost(ctx.clone());
                  let balance = balance_fn();

                  let max_amount = || {
                     let currency = self.currency.clone();
                     if currency.is_erc20() {
                        return balance;
                     } else {
                        if balance.wei() < cost.wei() {
                           return NumericValue::default();
                        }
                        let max = balance.wei() - cost.wei();
                        NumericValue::format_wei(max, currency.decimals())
                     }
                  };

                  let amount = self.amount_field.amount.clone();
                  let currency = self.currency.clone();
                  let data_syncing = self.price_syncing || self.syncing_balance;
                  let should_calculate_price = self.should_calculate_price(&currency);

                  let value = || {
                     value(
                        ctx.clone(),
                        currency,
                        amount,
                        should_calculate_price,
                     )
                  };

                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     self.amount_field.show(
                        ctx.clone(),
                        theme,
                        icons.clone(),
                        Some(label),
                        owner,
                        &self.currency,
                        Some(token_selection),
                        None,
                        balance_fn,
                        max_amount,
                        value,
                        data_syncing,
                        true,
                        ui,
                     );
                  });

                  token_selection.show(
                     ctx.clone(),
                     theme,
                     icons.clone(),
                     chain.id(),
                     owner,
                     ui,
                  );

                  if let Some(currency) = token_selection.get_currency() {
                     self.currency = currency.clone();
                     token_selection.reset();
                     self.sync_balance(owner, ctx.clone());
                  }

                  recipient_selection.show(ctx.clone(), theme, icons.clone(), true, contacts_ui, ui);
                  let recipient = recipient_selection.get_recipient();
                  let recipient_name = recipient_selection.get_recipient_name();

                  // Recipient Selection
                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     ui.horizontal(|ui| {
                        ui.label(RichText::new("Recipient").size(theme.text_sizes.large));
                        ui.add_space(10.0);

                        if !recipient.is_empty() {
                           if let Some(name) = &recipient_name {
                              ui.label(
                                 RichText::new(name.clone())
                                    .size(theme.text_sizes.large)
                                    .color(theme.colors.info),
                              );
                           } else {
                              ui.label(
                                 RichText::new("Unknown Address")
                                    .size(theme.text_sizes.large)
                                    .color(theme.colors.error),
                              );
                           }

                           let block_explorer = chain.block_explorer();
                           let link = format!("{}/address/{}", block_explorer, recipient);
                           let tint = theme.image_tint_recommended;
                           let icon = match theme.dark_mode {
                              true => icons.external_link_white_x18(tint),
                              false => icons.external_link_dark_x18(tint),
                           };

                           let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

                           if res.clicked() {
                              let url = OpenUrl::new_tab(link);
                              ui.ctx().open_url(url);
                           }
                        }
                     });

                     ui.horizontal(|ui| {
                        let hint = RichText::new("Search contacts or enter an address")
                           .size(theme.text_sizes.normal)
                           .color(theme.colors.text_muted);

                        let res = ui.add(
                           SecureTextEdit::singleline(&mut recipient_selection.recipient)
                              .visuals(text_edit_visuals)
                              .hint_text(hint)
                              .min_size(vec2(ui.available_width(), 25.0))
                              .margin(Margin::same(10))
                              .font(FontId::proportional(theme.text_sizes.normal)),
                        );
                        if res.clicked() {
                           recipient_selection.open(ctx.clone());
                        }
                     });
                  });

                  self.shield_button(ctx, theme, owner, recipient, ui);
               });
            });
         });
   }

   fn shield_button(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      owner: Address,
      recipient: String,
      ui: &mut Ui,
   ) {
      let button_visuals = theme.button_visuals();
      let sending_tx = self.sending_tx;
      let valid_amount = self.valid_amount();
      let has_balance = self.sufficient_balance(ctx.clone(), owner);
      let has_entered_amount = !self.amount_field.amount.is_empty();

      let valid_inputs = has_balance && has_entered_amount && valid_amount && !sending_tx;

      let mut button_text = "Shield".to_string();

      if has_entered_amount && !valid_amount {
         button_text = "Invalid Amount".to_string();
      }

      if !has_balance {
         button_text = format!("Insufficient {} Balance", self.currency.symbol());
      }

      let text = RichText::new(button_text).size(theme.text_sizes.large);
      let send = Button::new(text)
         .min_size(vec2(ui.available_width() * 0.8, 45.0))
         .visuals(button_visuals);

      if ui.add_enabled(valid_inputs, send).clicked() {
         self.sending_tx = true;
         self.send_transaction(ctx, recipient);
      }
   }

   fn send_transaction(&mut self, ctx: ZeusCtx, recipient: String) {
      let chain = ctx.chain();
      let from = ctx.current_wallet_info(false).address;
      let currency = self.currency.clone();
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );

      RT.spawn(async move {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
         });

         match shield(
            ctx.clone(),
            chain,
            currency,
            amount,
            from,
            recipient,
         )
         .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.shield_ui.sending_tx = false;
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.shield_ui.sending_tx = false;
                  gui.notification.reset();
                  gui.loading_window.reset();
                  gui.msg_window.open("Transaction Error", e.to_string());
                  gui.request_repaint();
               });
            }
         }
      });
   }

   fn should_calculate_price(&self, currency: &Currency) -> bool {
      let now = Instant::now();
      let last_updated = self.last_price_update.get(&currency.address()).cloned();
      if last_updated.is_none() {
         return true;
      }

      let last_updated = last_updated.unwrap();
      let timeout = Duration::from_secs(POOL_UPDATE_TIMEOUT);
      let time_passed = now.duration_since(last_updated);
      time_passed > timeout
   }

   fn sync_balance(&mut self, owner: Address, ctx: ZeusCtx) {
      self.syncing_balance = true;
      let currency = self.currency.clone();
      let chain = currency.chain_id();
      RT.spawn(async move {
         let balance_manager = ctx.balance_manager();
         if currency.is_native() {
            match balance_manager.update_eth_balance(ctx.clone(), chain, vec![owner], false).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Failed to update ETH balance: {}", e);
               }
            }
         } else {
            let token = currency.to_erc20().into_owned();
            match balance_manager
               .update_tokens_balance(ctx.clone(), chain, owner, vec![token], false)
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Failed to update token balance: {}", e);
               }
            }
         }
         SHARED_GUI.write(|gui| {
            gui.shield_ui.syncing_balance = false;
         });
      });
   }

   fn cost(&self, ctx: ZeusCtx) -> NumericValue {
      let gas_used = 750_000;

      let fee = ctx.get_priority_fee(ctx.chain().id()).unwrap_or_default();
      let (cost_in_wei, _) = estimate_tx_cost(ctx.clone(), ctx.chain().id(), gas_used, fee.wei());
      cost_in_wei
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount_field.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, sender: Address) -> bool {
      let balance = ctx.get_currency_balance(ctx.chain().id(), sender, &self.currency);
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );
      balance.wei() >= amount.wei()
   }
}

fn value(
   ctx: ZeusCtx,
   currency: Currency,
   amount: String,
   should_fetch_price: bool,
) -> NumericValue {
   let price = ctx.get_currency_price(&currency);
   let amount = amount.parse().unwrap_or(0.0);
   let value = NumericValue::value(amount, price.f64());

   if should_fetch_price {
      let price_manager = ctx.price_manager();
      let pool_manager = ctx.pool_manager();
      let chain = currency.chain_id();

      RT.spawn(async move {
         SHARED_GUI.write(|gui| {
            gui.shield_ui.price_syncing = true;
            gui.shield_ui.last_price_update.insert(currency.address(), Instant::now());
         });

         match price_manager
            .calculate_prices(
               ctx,
               chain,
               pool_manager,
               vec![currency.to_erc20().into_owned()],
            )
            .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.shield_ui.price_syncing = false;
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.shield_ui.price_syncing = false;
               });
               tracing::error!("Error calculating price: {:?}", e);
            }
         }
      });
   }

   value
}

async fn shield(
   ctx: ZeusCtx,
   chain: ChainId,
   currency: Currency,
   amount: NumericValue,
   from: Address,
   recipient: String,
) -> Result<(), anyhow::Error> {
   let is_supported = ctx.railgun_is_supported(chain);

   if !is_supported {
      return Err(anyhow!(
         "Railgun is not supported for the {} network",
         chain.name()
      ));
   }

   let recipient = match RailgunAddress::from_zk_address(&recipient) {
      Ok(address) => address,
      Err(e) => {
         return Err(anyhow!("Invalid Railgun Address {}", e));
      }
   };

   let mut railgun_provider = ctx.get_railgun_provider(chain.id()).await?;

   if railgun_provider.chain_id() != chain.id() {
      return Err(anyhow!(
         "Railgun provider chain id {} does not match the current chain id {}",
         railgun_provider.chain_id(),
         chain.id()
      ));
   }

   let railgun_address = railgun_provider.railgun_address();
   let client = ctx.get_client(chain.id()).await?;

   // TODO: At some point we want the WRAP + Approve + Shield to be done with a single transaction

   // Check if railgun contract needs approval
   let token = currency.to_erc20().into_owned();
   let allowance_fut = token.allowance(client.clone(), from, railgun_address);

   // Wrap ETH into WETH if needed
   if currency.is_native() {
      wrap_eth(ctx.clone(), from, chain, amount.clone()).await?;
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
   });

   let allowance = allowance_fut.await?;

   // Approve if needed
   if allowance < amount.wei() {
      let calldata = token.encode_approve(railgun_address, amount.wei());
      let value = U256::ZERO;
      let dapp = "".to_string();
      let mev_protect = false;
      let auth_list = vec![];
      let interact_to = token.address;

      let (_, _) = send_transaction(
         ctx.clone(),
         dapp,
         None,
         chain,
         mev_protect,
         from,
         interact_to,
         calldata,
         value,
         auth_list,
      )
      .await?;
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
   });

   let asset = AssetId::Erc20(token.address);
   let amount_u128: u128 = amount.wei().try_into()?;

   let shield_tx = {
      let mut rng = rand::rng();
      railgun_provider
         .shield()
         .shield(recipient, asset, amount_u128)
         .build(&mut rng)?
   };
   let calldata = shield_tx[0].data.clone();

   let interact_to = railgun_provider.railgun_address();
   let mev_protect = false;
   let dapp = "".to_string();
   let auth_list = Vec::new();
   let value = U256::ZERO;

   let z_client = ctx.get_zeus_client();

   let eth_balance_before_fut = z_client.request(chain.id(), |client| async move {
      client
         .get_balance(from)
         .block_id(BlockId::latest())
         .await
         .map_err(|e| anyhow!("{:?}", e))
   });

   let block = z_client
      .request(chain.id(), |client| async move {
         client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let block = if let Some(block) = block {
      block
   } else {
      return Err(anyhow!(
         "No block found, this is usally a provider issue"
      ));
   };

   let block_id = BlockId::number(block.header.number);

   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(interact_to);
   accounts.push(block.header.beneficiary);

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, Some(&block), fork_db.clone());

      let time = Instant::now();
      sim_res = simulate_transaction(
         &mut evm,
         from,
         interact_to,
         calldata.clone(),
         value,
         vec![],
      )?;
      tracing::info!(
         "Simulate Transaction took {} ms",
         time.elapsed().as_millis()
      );

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let logs = sim_res.clone().into_logs();

   let mut shield_events = Vec::new();

   for log in &logs {
      if let Ok(params) = ShieldParams::from_log(ctx.clone(), chain.id(), log).await {
         shield_events.extend(params);
      }
   }

   tracing::info!("Shield Events {:?}", shield_events);

   // Should not happen
   if shield_events.len() > 1 {
      return Err(anyhow!("More than one shield event found"));
   }

   if shield_events.is_empty() {
      return Err(anyhow!("No shield event found"));
   }

   let shield_params = shield_events[0].clone();
   let contract_interact = Some(true);
   let eth_balance_before = eth_balance_before_fut.await?;

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      calldata.clone(),
      value,
      logs,
      sim_res.tx_gas_used(),
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   let main_event = DecodedEvent::Shield(shield_params.clone());
   tx_analysis.set_main_event(main_event);

   let (_, _) = send_transaction(
      ctx.clone(),
      dapp,
      Some(tx_analysis),
      chain,
      mev_protect,
      from,
      interact_to,
      calldata,
      value,
      auth_list,
   )
   .await?;

   RT.spawn(async move {
      let manager = ctx.balance_manager();
      match manager
         .update_tokens_balance(ctx.clone(), chain.id(), from, vec![token], true)
         .await
      {
         Ok(_) => {}
         Err(e) => error!("Error updating weth balance: {:?}", e),
      }

      match manager.update_eth_balance(ctx.clone(), chain.id(), vec![from], true).await {
         Ok(_) => {}
         Err(e) => error!("Error updating eth balance: {:?}", e),
      }

      ctx.update_public_data(chain.id(), from);
      ctx.save_balance_manager();

      match railgun_provider.sync().await {
         Ok(_) => {
            info!("Railgun provider Synced");
         }
         Err(e) => error!("Error syncing Railgun provider: {:?}", e),
      }

      ctx.update_private_data(chain.id(), from).await;
   });

   Ok(())
}
