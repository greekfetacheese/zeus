use eframe::egui::{
   Align2, CollapsingHeader, CursorIcon, FontId, Frame, Margin, OpenUrl, RichText, Ui, Window, vec2,
};

use std::{
   collections::HashMap,
   sync::Arc,
   time::{Duration, Instant},
};

use crate::utils::{RT, estimate_tx_cost, simulate::railgun_common_accounts};
use crate::{
   core::{
      DecodedEvent, ShieldParams, TransactionAnalysis, ZeusContext, ZeusCtx, data_dir,
      send_transaction,
   },
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
use crate::utils::simulate::{fetch_accounts_info, fetch_storage_for_railgun};
use zeus_theme::Theme;
use zeus_widgets::{Button, SecureTextEdit};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token, NativeCurrency},
   revm_utils::{ForkFactory, Host, new_evm},
   types::ChainId,
   utils::NumericValue,
};

use zeus_railgun::{RailgunAddress, caip::AssetId, rand::SeedableRng, rand_chacha::ChaCha12Rng};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use super::unshield::{default_bundler_url, unshield};

const POOL_UPDATE_TIMEOUT: u64 = 60;

const BUNDLER_URL_FILE: &str = "bundler_url.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlerUrl {
   pub url: String,
}

impl Default for BundlerUrl {
   fn default() -> Self {
      Self {
         url: default_bundler_url(1),
      }
   }
}

impl BundlerUrl {
   fn new(url: String) -> Self {
      Self { url }
   }

   fn save(&self) -> Result<(), anyhow::Error> {
      let dir = data_dir()?;
      let file = dir.join(BUNDLER_URL_FILE);
      let data = serde_json::to_string(&self)?;
      std::fs::write(file, data)?;

      Ok(())
   }

   fn load() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?;
      let file = dir.join(BUNDLER_URL_FILE);
      let data = std::fs::read_to_string(file)?;
      let url = serde_json::from_str(&data)?;

      Ok(url)
   }
}

/// Enum to determine which railgun mode to use.
///
/// This is to avoid duplicating ui code for shield and unshield.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RailgunMode {
   Shield,
   Unshield,
}

impl RailgunMode {
   pub fn is_shield(&self) -> bool {
      matches!(self, RailgunMode::Shield)
   }

   pub fn is_unshield(&self) -> bool {
      matches!(self, RailgunMode::Unshield)
   }
}

pub struct ShieldUi {
   open: bool,
   mode: RailgunMode,
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
   /// Emergency path: submit unshield from the user's EOA (breaks anonymity).
   self_broadcast: bool,
   /// Post unshield call to unwrap WETH to ETH
   unwrap_to_eth: bool,
   /// Bundler JSON-RPC URL for paymaster UserOps (ignored when self_broadcast).
   bundler_url: String,
}

impl ShieldUi {
   pub fn new() -> Self {
      let bundler_url = BundlerUrl::load().unwrap_or_default();
      Self {
         open: false,
         mode: RailgunMode::Shield,
         currency: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         amount_field: AmountField::new(),
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         size: (500.0, 560.0),
         price_syncing: false,
         syncing_balance: false,
         sending_tx: false,
         last_price_update: HashMap::new(),
         self_broadcast: false,
         unwrap_to_eth: false,
         bundler_url: bundler_url.url,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, mode: RailgunMode) {
      self.mode = mode;
      self.open = true;
   }

   pub fn close(&mut self) {
      *self = Self::new();
   }

   pub fn set_mode(&mut self, mode: RailgunMode) {
      self.mode = mode;
   }

   pub fn default_currency(&mut self, chain_id: u64) {
      let currency = match self.mode {
         RailgunMode::Shield => Currency::from(NativeCurrency::from(chain_id)),
         RailgunMode::Unshield => Currency::from(ERC20Token::wrapped_native_token(chain_id)),
      };
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
      ctx: &mut ZeusContext,
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

                  let title = match self.mode {
                     RailgunMode::Shield => "Shield",
                     RailgunMode::Unshield => "Unshield",
                  };

                  ui.label(RichText::new(title).size(theme.text_sizes.heading));

                  let owner = ctx.current_wallet_info().address;
                  let chain = ctx.chain;

                  // Keep default bundler URL in sync with the active chain when still on public Pimlico.
                  if self.mode.is_unshield() {
                     let default_for_chain = default_bundler_url(chain.id());
                     let looks_like_default = self.bundler_url.contains("public.pimlico.io");
                     if self.bundler_url.is_empty() || looks_like_default {
                        if !self.bundler_url.contains(&format!("/{}/rpc", chain.id())) {
                           self.bundler_url = default_for_chain;
                        }
                     }
                  }

                  let inner_frame = theme.frame2;

                  // Currency Selection
                  let label = String::from("Amount");
                  let balance = self.balance_for_mode(ctx, owner);
                  let cost = self.cost(ctx);

                  let max_amount = if balance.wei() > cost.wei() {
                     NumericValue::format_wei(
                        balance.wei() - cost.wei(),
                        self.currency.decimals(),
                     )
                  } else {
                     NumericValue::default()
                  };

                  let amount = self.amount_field.amount.clone();
                  let currency = self.currency.clone();
                  let data_syncing = self.price_syncing || self.syncing_balance;
                  let should_calculate_price = self.should_calculate_price(&currency);

                  let value = value(ctx, currency, amount, should_calculate_price);

                  // Token list: public tokens for shield, private notes for unshield.
                  let token_privacy_mode = self.mode.is_unshield();
                  // Recipient: 0zk for shield, public 0x for unshield.
                  let recipient_privacy_mode = self.mode.is_shield();

                  // TODO: In Unshield mode if there are no tokens at all it still shows the default currency
                  // TODO: Change the amount field to accept an Option<Currency> ??
                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     self.amount_field.show(
                        chain.id(),
                        token_privacy_mode,
                        theme,
                        icons.clone(),
                        Some(label),
                        owner,
                        &self.currency,
                        Some(token_selection),
                        None,
                        || balance,
                        || max_amount,
                        || value,
                        data_syncing,
                        true,
                        ui,
                     );
                  });

                  token_selection.show(ctx, theme, icons.clone(), chain.id(), owner, ui);

                  if let Some(currency) = token_selection.get_selected_currency() {
                     self.currency = currency.clone();
                     token_selection.reset();
                     self.sync_balance(owner);
                  }

                  recipient_selection.show(
                     ctx,
                     theme,
                     icons.clone(),
                     recipient_privacy_mode,
                     contacts_ui,
                     ui,
                  );

                  let recipient = recipient_selection.get_recipient();

                  // Recipient Selection
                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     ui.horizontal(|ui| {
                        ui.label(RichText::new("Recipient").size(theme.text_sizes.large));
                        ui.add_space(10.0);

                        if !recipient.is_empty(recipient_privacy_mode) {
                           if let Some(name) = &recipient.name {
                              ui.label(
                                 RichText::new(name)
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

                           if !recipient_privacy_mode && !recipient.evm_address.is_empty() {
                              let block_explorer = chain.block_explorer();
                              let link = format!(
                                 "{}/address/{}",
                                 block_explorer, recipient.evm_address
                              );
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
                        }
                     });

                     ui.horizontal(|ui| {
                        let hint = if recipient_privacy_mode {
                           RichText::new("Search contacts or enter a 0zk address")
                              .size(theme.text_sizes.normal)
                              .color(theme.colors.text_muted)
                        } else {
                           RichText::new("Search contacts or enter a 0x address")
                              .size(theme.text_sizes.normal)
                              .color(theme.colors.text_muted)
                        };

                        let address_edit = if recipient_privacy_mode {
                           &mut recipient_selection.recipient.zk_address
                        } else {
                           &mut recipient_selection.recipient.evm_address
                        };

                        let res = ui.add(
                           SecureTextEdit::singleline(address_edit)
                              .visuals(text_edit_visuals)
                              .hint_text(hint)
                              .min_size(vec2(ui.available_width(), 25.0))
                              .margin(Margin::same(10))
                              .font(FontId::proportional(theme.text_sizes.normal)),
                        );
                        if res.clicked() {
                           recipient_selection.open();
                        }
                     });
                  });

                  if self.mode.is_unshield() {
                     self.unshield_options(theme, chain.id(), ui);
                  }

                  let recipient_str = if self.mode.is_shield() {
                     recipient.zk_address
                  } else {
                     recipient.evm_address
                  };

                  self.action_button(ctx, theme, owner, recipient_str, ui);
               });
            });
         });
   }

   fn unshield_options(&mut self, theme: &Theme, chain_id: u64, ui: &mut Ui) {
      let inner_frame = theme.frame2;
      let text_edit_visuals = theme.text_edit_visuals();
      let default_url = default_bundler_url(chain_id);
      let bundler_overridden =
         !self.self_broadcast && self.bundler_url.trim() != default_url.as_str();

      inner_frame.show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

         ui.horizontal(|ui| {
            ui.checkbox(
               &mut self.self_broadcast,
               RichText::new("Self-broadcast (emergency)").size(theme.text_sizes.normal),
            );
         });

         ui.label(
            RichText::new(
               "Submits the unshield from your public wallet. Breaks anonymity — only use if private broadcast is unavailable.",
            )
            .size(theme.text_sizes.small)
            .color(theme.colors.text_muted),
         );

         if self.currency.is_native_wrapped() && !self.self_broadcast {
            ui.horizontal(|ui| {
            ui.checkbox(
               &mut self.unwrap_to_eth,
               RichText::new("Unwrap to ETH").size(theme.text_sizes.normal),
            );
         });

         ui.label(
            RichText::new(
               "Unwraps WETH to ETH. Useful if the recipient doesn't have native ETH for gas.",
            )
            .size(theme.text_sizes.small)
            .color(theme.colors.text_muted),
         );
         }

         ui.add_space(4.0);

         if bundler_overridden {
            ui.label(
               RichText::new(
                  "WARNING: Custom bundler URL is set.",
               )
               .size(theme.text_sizes.small)
               .color(theme.colors.warning),
            );
         }

         CollapsingHeader::new(
            RichText::new("Advanced broadcast options")
               .size(theme.text_sizes.normal)
               .color(theme.colors.info),
         )
         .default_open(false)
         .id_salt("railgun_unshield_advanced_broadcast")
         .show(ui, |ui| {
            ui.add_enabled_ui(!self.self_broadcast, |ui| {
               ui.horizontal(|ui| {
                  ui.label(RichText::new("Bundler URL").size(theme.text_sizes.normal));
                  ui.add_space(8.0);
                  let res = ui.add(
                     SecureTextEdit::singleline(&mut self.bundler_url)
                        .visuals(text_edit_visuals)
                        .hint_text(
                           RichText::new("https://public.pimlico.io/v2/{chainId}/rpc")
                              .size(theme.text_sizes.small)
                              .color(theme.colors.text_muted),
                        )
                        .desired_width(ui.available_width())
                        .margin(Margin::same(6))
                        .font(FontId::proportional(theme.text_sizes.small)),
                  );

                  if res.changed() {
                     let bundler_url = self.bundler_url.clone();
                     RT.spawn_blocking(move || {
                        let url = BundlerUrl::new(bundler_url);
                        url.save().unwrap();
                     });
                  }
               });

               ui.horizontal(|ui| {
                  let text = RichText::new("Reset to public Pimlico").size(theme.text_sizes.small);
                  let button = Button::new(text).visuals(theme.button_visuals());
                  if ui.add(button).clicked() {
                     self.bundler_url = default_bundler_url(chain_id);
                     let url = BundlerUrl::new(self.bundler_url.clone());
                     RT.spawn_blocking(move || {
                        url.save().unwrap();
                     });
                  }
               });

               ui.label(
                  RichText::new(
                     "Uses Railgun Privacy Paymaster + ERC-4337. Fee is paid from private WETH balance. Point this at a self-hosted Alto for less reliance on public Pimlico.",
                  )
                  .size(theme.text_sizes.small)
                  .color(theme.colors.text_muted),
               );
            });

            if self.self_broadcast {
               ui.label(
                  RichText::new("Bundler options disabled while self-broadcast is enabled.")
                     .size(theme.text_sizes.small)
                     .color(theme.colors.warning),
               );
            }
         });
      });
   }

   fn action_button(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      owner: Address,
      recipient: String,
      ui: &mut Ui,
   ) {
      let button_visuals = theme.button_visuals();
      let sending_tx = self.sending_tx;
      let valid_amount = self.valid_amount();
      let has_balance = self.sufficient_balance(ctx, owner);
      let has_entered_amount = !self.amount_field.amount.is_empty();
      let has_recipient = !recipient.trim().is_empty();
      let valid_token = if self.mode == RailgunMode::Unshield {
         self.currency.is_erc20()
      } else {
         true
      };

      let valid_inputs = has_balance
         && has_entered_amount
         && valid_amount
         && valid_token
         && has_recipient
         && !sending_tx;

      let mut button_text = match self.mode {
         RailgunMode::Shield => "Shield".to_string(),
         RailgunMode::Unshield => {
            if self.self_broadcast {
               "Unshield (self-broadcast)".to_string()
            } else {
               "Unshield (private broadcast)".to_string()
            }
         }
      };

      if has_entered_amount && !valid_amount {
         button_text = "Invalid Amount".to_string();
      }

      if !has_balance {
         button_text = format!("Insufficient {} Balance", self.currency.symbol());
      }

      if !valid_token {
         button_text = "Invalid Token".to_string();
      }

      if !has_recipient {
         button_text = "Enter Recipient".to_string();
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

   fn send_transaction(&mut self, ctx: &mut ZeusContext, recipient: String) {
      let chain = ctx.chain;
      let from = ctx.current_wallet_info().address;
      let currency = self.currency.clone();
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );

      if self.mode.is_shield() {
         RT.spawn(async move {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
               gui.ctx.clone()
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
      } else {
         let self_broadcast = self.self_broadcast;
         let unwrap_to_eth = self.unwrap_to_eth;
         let bundler_url = self.bundler_url.clone();
         // Unshield futures are not `Send` (`&dyn Bundler` / `&dyn Signer` across awaits).
         // Do NOT spin up a nested current_thread runtime: revm's ForkDB uses
         // `tokio::task::block_in_place`, which panics outside a multi-thread runtime.
         // Drive the non-Send future on the existing multi-thread `RT` via `block_on`
         // from a blocking thread (no Send bound, block_in_place still works).
         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
               gui.ctx.clone()
            });

            let result = RT.block_on(unshield(
               ctx.clone(),
               chain,
               currency,
               amount,
               from,
               recipient,
               self_broadcast,
               unwrap_to_eth,
               bundler_url,
            ));

            match result {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.shield_ui.sending_tx = false;
                     gui.loading_window.reset();
                     gui.request_repaint();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.shield_ui.sending_tx = false;
                     gui.notification.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open("Unshield Error", e.to_string());
                     gui.request_repaint();
                  });
               }
            }
         });
      }
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

   fn sync_balance(&mut self, owner: Address) {
      self.syncing_balance = true;
      let currency = self.currency.clone();
      let chain = currency.chain_id();
      let privacy = self.mode.is_unshield();

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         if privacy {
            ctx.update_private_data(chain, owner).await;
         } else {
            let balance_manager = ctx.balance_manager();

            if currency.is_native() {
               match balance_manager
                  .update_eth_balance(ctx.clone(), chain, vec![owner], false)
                  .await
               {
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
         }
         SHARED_GUI.write(|gui| {
            gui.shield_ui.syncing_balance = false;
         });
      });
   }

   fn cost(&self, ctx: &mut ZeusContext) -> NumericValue {
      // An estimation, the TxConfirmWindow will show a much closer cost
      let gas_used = if self.mode.is_shield() {
         750_000
      } else {
         500_000
      };

      let fee = ctx.priority_fee.get(ctx.chain.id()).cloned().unwrap_or_default();
      let (cost_in_wei, _) = estimate_tx_cost(ctx, ctx.chain.id(), gas_used, fee.wei());
      cost_in_wei
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount_field.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn balance_for_mode(&self, ctx: &mut ZeusContext, owner: Address) -> NumericValue {
      if self.mode.is_shield() {
         return ctx.get_currency_balance(ctx.chain.id(), owner, &self.currency);
      }

      // Private note balances from portfolio cache
      let portfolio = ctx.portfolio_db.get(ctx.chain.id(), owner);
      if let Some(token) = self.currency.erc20_opt() {
         for (t, balance, _value, _price) in portfolio.private_tokens() {
            if t.address == token.address {
               return balance.clone();
            }
         }
      }
      NumericValue::default()
   }

   fn sufficient_balance(&self, ctx: &mut ZeusContext, sender: Address) -> bool {
      let balance = self.balance_for_mode(ctx, sender);
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );
      balance.wei() >= amount.wei()
   }
}

fn value(
   ctx: &mut ZeusContext,
   currency: Currency,
   amount: String,
   should_fetch_price: bool,
) -> NumericValue {
   let price = ctx.get_currency_price(&currency);
   let amount = amount.parse().unwrap_or(0.0);
   let value = NumericValue::value(amount, price.f64());

   if should_fetch_price {
      let chain = currency.chain_id();

      RT.spawn(async move {
         let now = Instant::now();
         let ctx = SHARED_GUI.write(|gui| {
            gui.shield_ui.price_syncing = true;
            gui.shield_ui.last_price_update.insert(currency.address(), now);
            gui.ctx.clone()
         });
         let price_manager = ctx.price_manager();
         let pool_manager = ctx.pool_manager();
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

   let z_client = ctx.get_zeus_client();

   let token = currency.to_erc20().into_owned();
   let railgun_address = railgun_provider.railgun_address();
   let client = ctx.get_client(chain.id()).await?;

   // TODO: At some point we want the WRAP + Approve + Shield to be done with a single transaction

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
      let mut rng = ChaCha12Rng::from_os_rng();
      railgun_provider
         .shield()
         .shield(recipient.clone(), asset, amount_u128)
         .build(&mut rng)?
   };
   let calldata = shield_tx[0].data.clone();

   let interact_to = railgun_provider.railgun_address();
   let mev_protect = false;
   let dapp = "".to_string();
   let auth_list = Vec::new();
   let value = U256::ZERO;

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

   // Prefetch accounts and storage for the sim
   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(token.address);
   accounts.push(railgun_address);
   accounts.push(block.header.beneficiary);

   let common_accounts = railgun_common_accounts(chain.id());
   accounts.extend(common_accounts);

   let accounts_info_fut = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts);
   let storage_info_fut =
      fetch_storage_for_railgun(ctx.clone(), chain.id(), block_id, railgun_address);

   let accounts_info = accounts_info_fut.await;
   let storage_info = storage_info_fut.await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   for info in storage_info {
      match factory.insert_account_storage(info.address, info.slot, info.value) {
         Ok(_) => {}
         Err(e) => tracing::error!("Failed to insert account storage: {:?}", e),
      }
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

   // Should not happen
   if shield_events.len() > 1 {
      return Err(anyhow!("More than one shield event found"));
   }

   if shield_events.is_empty() {
      return Err(anyhow!("No shield event found"));
   }

   let mut shield_params = shield_events[0].clone();
   shield_params.recipient = Some(recipient.address.clone());

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
