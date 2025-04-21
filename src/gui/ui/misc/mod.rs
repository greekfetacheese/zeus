use eframe::egui::{
   Align, Align2, Button, Color32, Frame, Grid, Id, Layout, Order, RichText, ScrollArea, Sense,
   Spinner, TextEdit, Ui, Vec2, Window, vec2,
};
use egui::{FontId, Margin};
use std::sync::Arc;
use zeus_eth::currency::NativeCurrency;
use zeus_eth::utils::NumericValue;

use crate::assets::icons::Icons;
use crate::core::utils::format_expiry;
use crate::core::utils::sign::SignMsgType;
use crate::core::utils::tx::TxSummary;
use crate::core::utils::{
   action::OnChainAction, estimate_tx_cost, truncate_address, truncate_hash,
   update::update_portfolio_state,
};
use crate::core::{
   WalletInfo, ZeusCtx,
   utils::{RT, eth},
};
use crate::gui::SHARED_GUI;
use crate::gui::ui::TokenSelectionWindow;

use egui_theme::{Theme, utils::*};
use egui_widgets::{ComboBox, Label};
use zeus_eth::{
   alloy_primitives::{Address, TxHash},
   currency::Currency,
   types::ChainId,
};

use super::GREEN_CHECK;

pub mod tx_history;

pub struct PriorityFeeTextBox {
   chain: ChainId,
   fee: String,
}

impl PriorityFeeTextBox {
   pub fn new() -> Self {
      Self {
         chain: ChainId::default(),
         fee: "1".to_string(),
      }
   }

   pub fn set_priority_fee(&mut self, chain: ChainId, fee: String) {
      // No priority fee for Binance Smart Chain
      // Set empty to avoid frame shutter due to invalid fee
      if chain.is_bsc() {
         self.fee = String::new();
         self.chain = chain;
      } else {
         self.fee = fee;
         self.chain = chain;
      }
   }

   pub fn get_chain(&self) -> ChainId {
      self.chain
   }

   pub fn get_fee(&self) -> String {
      self.fee.clone()
   }

   pub fn show(
      &mut self,
      min_size: Vec2,
      margin: Margin,
      bg_color: Color32,
      font_size: f32,
      ui: &mut Ui,
   ) {
      ui.add(
         TextEdit::singleline(&mut self.fee)
            .min_size(min_size)
            .margin(margin)
            .background_color(bg_color)
            .font(egui::FontId::proportional(font_size)),
      );
   }
}

/// A ComboBox to select a chain
pub struct ChainSelect {
   pub id: &'static str,
   pub grid_id: &'static str,
   pub chain: ChainId,
   pub size: Vec2,
   pub show_icon: bool,
}

impl ChainSelect {
   pub fn new(id: &'static str, default_chain: u64) -> Self {
      Self {
         id,
         grid_id: "chain_select_grid",
         chain: ChainId::new(default_chain).unwrap(),
         size: (200.0, 25.0).into(),
         show_icon: true,
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn show_icon(mut self, show: bool) -> Self {
      self.show_icon = show;
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the chain was changed
   pub fn show(
      &mut self,
      ignore_chain: u64,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) -> bool {
      let selected_chain = self.chain;
      let mut clicked = false;
      let supported_chains = ChainId::supported_chains();
      let icon = icons.chain_icon(&selected_chain.id());
      let selected_chain = Label::new(
         RichText::new(selected_chain.name()).size(theme.text_sizes.normal),
         Some(icon),
      )
      .text_first(false)
      .sense(Sense::click());

      // Add the ComboBox with the specified size
      ComboBox::new(self.id, selected_chain)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            for chain in supported_chains {
               if chain.id() == ignore_chain {
                  continue;
               }

               let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
               let icon = icons.chain_icon(&chain.id());
               let chain_label = Label::new(text.clone(), Some(icon))
                  .text_first(false)
                  .sense(Sense::click());

               if ui.add(chain_label).clicked() {
                  self.chain = chain.clone();
                  clicked = true;
               }
            }
         });
      clicked
   }
}

/// A ComboBox to select a wallet
pub struct WalletSelect {
   pub id: &'static str,
   /// Selected Wallet
   pub wallet: WalletInfo,
   pub size: Vec2,
   pub button_padding: Vec2,
}

impl WalletSelect {
   pub fn new(id: &'static str) -> Self {
      Self {
         id,
         wallet: WalletInfo::default(),
         size: (200.0, 25.0).into(),
         button_padding: vec2(10.0, 4.0),
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn button_padding(mut self, button_padding: impl Into<Vec2>) -> Self {
      self.button_padding = button_padding.into();
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the wallet was changed
   pub fn show(
      &mut self,
      theme: &Theme,
      wallets: &Vec<WalletInfo>,
      _icons: Arc<Icons>,
      ui: &mut Ui,
   ) -> bool {
      let mut clicked = false;
      let text = RichText::new(&self.wallet.name).size(theme.text_sizes.normal);

      ComboBox::new(
         self.id,
         Label::new(text, None).sense(Sense::click()),
      )
      .width(self.size.x)
      .show_ui(ui, |ui| {
         ui.spacing_mut().item_spacing.y = 5.0;
         ui.spacing_mut().button_padding = self.button_padding;

         for wallet in wallets {
            let text = RichText::new(wallet.name.clone()).size(theme.text_sizes.normal);
            let wallet_label = Label::new(text, None).sense(Sense::click());

            if ui.add(wallet_label).clicked() {
               self.wallet = wallet.clone();
               clicked = true;
            }
         }
      });

      clicked
   }
}

/// Testing Window
pub struct TestingWindow {
   pub open: bool,
   pub size: (f32, f32),
   pub chain: ChainId,
   pub id: Id,
}

impl TestingWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (500.0, 400.0),
         chain: ChainId::new(1).unwrap(),
         id: Id::new("test_window"),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn reset(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new(RichText::new("Testing Window").size(theme.text_sizes.normal))
         .title_bar(true)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               // ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);

               let mut chain_select = ChainSelect::new("testing13232", 1);
               chain_select.show(0, theme, icons.clone(), ui);

               if ui.button("Close").clicked() {
                  self.open = false;
               }
            });
         });
   }
}

pub struct Step {
   pub id: &'static str,
   pub in_progress: bool,
   pub finished: bool,
   pub msg: String,
}

pub struct ProgressWindow {
   open: bool,
   steps: Vec<Step>,
   final_msg: String,
   tx_summary: Option<TxSummary>,
   size: (f32, f32),
}

impl ProgressWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         steps: Vec::new(),
         final_msg: String::new(),
         tx_summary: None,
         size: (350.0, 150.0),
      }
   }

   pub fn open_test(&mut self) {
      let steps = vec![
         Step {
            id: "step1",
            in_progress: true,
            finished: false,
            msg: "Step 1".to_string(),
         },
         Step {
            id: "step2",
            in_progress: false,
            finished: false,
            msg: "Step 2".to_string(),
         },
         Step {
            id: "step3",
            in_progress: false,
            finished: false,
            msg: "Step 3".to_string(),
         },
      ];
      self.open_with(steps, "Done".to_string());
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn set_tx_summary(&mut self, summary: TxSummary) {
      self.tx_summary = Some(summary);
   }

   pub fn add_step(&mut self, id: &'static str, in_progress: bool, finished: bool, msg: String) {
      self.steps.push(Step {
         id,
         in_progress,
         finished,
         msg,
      });
   }

   pub fn open_with(&mut self, steps: Vec<Step>, final_msg: String) {
      self.reset();
      self.open = true;
      self.steps = steps;
      self.final_msg = final_msg;
   }

   pub fn current_step(&mut self) -> Option<&mut Step> {
      self.steps.iter_mut().find(|s| s.in_progress)
   }

   pub fn next_step(&mut self, step_id: &str) -> Option<&mut Step> {
      self.steps.iter_mut().find(|s| s.id == step_id)
   }

   /// Proceed to the next step, finishing the current one
   pub fn proceed_with(&mut self, step_id: &str) {
      if let Some(step) = self.current_step() {
         step.in_progress = false;
         step.finished = true;
      }

      if let Some(step) = self.next_step(step_id) {
         step.in_progress = true;
      } else {
         tracing::error!("Step with id {} not found", step_id);
      }
   }

   pub fn finish_last_step(&mut self) {
      if let Some(step) = self.current_step() {
         step.in_progress = false;
         step.finished = true;
      }
   }

   pub fn finished(&self) -> bool {
      self.steps.iter().all(|s| s.finished)
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.tx_summary = None;
      self.final_msg.clear();
      self.steps.clear();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Progress Window")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            Frame::new().inner_margin(Margin::same(20)).show(ui, |ui| {
               let normal = theme.text_sizes.normal;
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.vertical(|ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  for step in &self.steps {
                     ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        ui.add_space(120.0);
                        ui.label(RichText::new(step.msg.clone()).size(normal));
                        if step.in_progress {
                           ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                        }

                        if step.finished {
                           ui.label(RichText::new(GREEN_CHECK).size(normal));
                        }
                     });
                  }

                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     ui.add_space(120.0);
                     if self.finished() {
                        ui.label(RichText::new(self.final_msg.clone()).size(normal));
                     } else {
                        // occupy the space
                        ui.label(RichText::new("").size(normal));
                     }
                  });
                  ui.add_space(20.0);

                  ui.horizontal(|ui| {
                     ui.add_space(110.0);
                     let close = Button::new(RichText::new("Close").size(normal));
                     if ui.add(close).clicked() {
                        self.open = false;
                     }

                     let summary_btn = Button::new(RichText::new("Summary").size(normal));
                     if self.finished() {
                        if ui.add(summary_btn).clicked() {
                           let summary = self.tx_summary.take();
                           self.reset();

                           RT.spawn_blocking(move || {
                              SHARED_GUI.write(|gui| {
                                 gui.tx_confirm_window
                                    .open_with_summary(summary.unwrap_or_default());
                              });
                           });
                        }
                     }
                  });
               });
            });
         });
   }
}

pub struct SignMsgWindow {
   open: bool,
   dapp: String,
   chain: ChainId,
   msg: Option<SignMsgType>,
   signed: Option<bool>,
   size: (f32, f32),
}

impl SignMsgWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         dapp: String::new(),
         chain: ChainId::default(),
         msg: None,
         signed: None,
         size: (400.0, 550.0),
      }
   }

   pub fn open(&mut self, dapp: String, chain: u64, msg: SignMsgType) {
      self.dapp = dapp;
      self.chain = chain.into();
      self.open = true;
      self.msg = Some(msg);
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.dapp.clear();
      self.msg = None;
      self.signed = None;
   }

   pub fn is_signed(&self) -> Option<bool> {
      self.signed
   }

   fn permit2_approval(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let msg = self.msg.as_ref().unwrap();
      if !msg.is_permit2() {
         return;
      }

      let details = msg.permit2_details();

      ui.label(RichText::new("Permit2 Token Approval").size(theme.text_sizes.normal));

      // Chain
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Chain").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let text = RichText::new(self.chain.name()).size(theme.text_sizes.normal);
            let icon = icons.chain_icon(&self.chain.id());
            let label = Label::new(text, Some(icon)).text_first(false);
            ui.add(label);
         });
      });

      // Token
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Approve Token").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = details.amount_str();
            let text = format!("{} {}", amount, details.token.symbol);
            let icon = icons.token_icon_x24(details.token.address, details.token.chain_id);
            let label = Label::new(
               RichText::new(text).size(theme.text_sizes.normal),
               Some(icon),
            )
            .wrap();
            ui.add(label);
         });
      });

      // Approval expire
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Approval expire").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let expire = format_expiry(details.expiration);
            let text = RichText::new(expire).size(theme.text_sizes.normal);
            ui.label(text);
         });
      });

      // Permit2 Contract
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Permit2 Contract").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let contract = details.permit2_contract;
            let explorer = self.chain.block_explorer();
            let link = format!("{}/address/{}", explorer, contract);
            ui.hyperlink_to(
               RichText::new(truncate_address(contract.to_string()))
                  .size(theme.text_sizes.normal)
                  .strong(),
               link,
            );
         });
      });

      // Spender
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Spender").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let spender = details.spender;
            let explorer = self.chain.block_explorer();
            let link = format!("{}/address/{}", explorer, spender);
            ui.hyperlink_to(
               RichText::new(truncate_address(spender.to_string()))
                  .size(theme.text_sizes.normal)
                  .strong(),
               link,
            );
         });
      });

      // Protocol/Dapp
      // TODO:
   }

   fn unknown_msg(&mut self, theme: &Theme, ui: &mut Ui) {
      let msg = self.msg.as_ref().unwrap();
      if msg.is_other() {
         ui.label(RichText::new("Unknown Message").size(theme.text_sizes.large));
      }
   }

   pub fn show(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Sign Message")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()).inner_margin(Margin::same(20)))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               if self.msg.is_none() {
                  ui.label("No message to sign");
                  return;
               }

               ui.label(RichText::new(&self.dapp).size(theme.text_sizes.large));

               self.permit2_approval(theme, icons, ui);
               self.unknown_msg(theme, ui);

               ui.add_space(20.0);

               // Show the JSON value
               let msg = self.msg.as_ref().unwrap();
               let mut value = msg.msg_value().clone().to_string();

               let text_edit = TextEdit::multiline(&mut value)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .background_color(theme.colors.text_edit_bg);

               ui.label(RichText::new("Sign Data").size(theme.text_sizes.normal));
               ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                  ui.add(text_edit);
               });

               ui.add_space(20.0);
               ui.horizontal(|ui| {
                  let width = ui.available_width() * 0.9;

                  let ok_btn = Button::new(RichText::new("Sign").size(theme.text_sizes.normal))
                     .min_size(vec2(width * 0.75, 50.0));
                  if ui.add(ok_btn).clicked() {
                     self.open = false;
                     self.signed = Some(true)
                  }

                  let cancel_btn =
                     Button::new(RichText::new("Cancel").size(theme.text_sizes.normal))
                        .min_size(vec2(width * 0.25, 50.0));
                  if ui.add(cancel_btn).clicked() {
                     self.open = false;
                     self.signed = Some(false)
                  }
               });
            });
         });
   }
}

/// A Window to confirm a transaction from a dapp
pub struct TxConfirmWindow {
   open: bool,
   /// Use this Window as a Tx summary or to confirm a transaction
   ///
   /// True for confirming a transaction and False for showing a summary
   confrim_window: bool,
   simulating: bool,
   confirm: Option<bool>,
   dapp: String,
   chain: ChainId,
   native_currency: NativeCurrency,
   /// Not tx cost but how much eth is being sent
   eth_spent: NumericValue,
   eth_spent_value: NumericValue,
   /// Tx cost
   tx_cost: NumericValue,
   tx_cost_usd: NumericValue,
   gas_used: u64,
   sender: Address,
   interact_to: Address,
   tx_hash: TxHash,
   /// Is this a contract interaction?
   ///
   /// If not interact to is an EOA
   contract_interact: bool,
   /// The action that is being performed
   action: OnChainAction,
   priority_fee: String,
   size: (f32, f32),
}

impl TxConfirmWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         confrim_window: true,
         simulating: false,
         confirm: None,
         dapp: String::new(),
         chain: ChainId::default(),
         native_currency: NativeCurrency::default(),
         eth_spent: NumericValue::default(),
         eth_spent_value: NumericValue::default(),
         tx_cost: NumericValue::default(),
         tx_cost_usd: NumericValue::default(),
         gas_used: 60_000,
         sender: Address::ZERO,
         interact_to: Address::ZERO,
         tx_hash: TxHash::ZERO,
         contract_interact: false,
         action: OnChainAction::dummy_swap(),
         priority_fee: "1".to_string(),
         size: (400.0, 550.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn open_with_summary(&mut self, summary: TxSummary) {
      self.open = true;
      let chain = summary.chain;
      let native = NativeCurrency::from_chain_id(chain).unwrap();

      self.confrim_window = false;
      self.chain = chain.into();
      self.native_currency = native;
      self.eth_spent = summary.eth_spent.clone();
      self.eth_spent_value = summary.eth_spent_usd.clone();
      self.tx_cost = summary.tx_cost.clone();
      self.tx_cost_usd = summary.tx_cost_usd.clone();
      self.gas_used = summary.gas_used;
      self.sender = summary.from;
      self.interact_to = summary.to;
      self.tx_hash = summary.hash;
      self.contract_interact = summary.contract_interact;
      self.action = summary.action;
   }

   pub fn open_with(
      &mut self,
      dapp: String,
      chain: ChainId,
      confrim_window: bool,
      eth_spent: NumericValue,
      eth_spent_value: NumericValue,
      tx_cost: NumericValue,
      tx_cost_usd: NumericValue,
      gas_used: u64,
      sender: Address,
      interact_to: Address,
      contract_interact: bool,
      action: OnChainAction,
      priority_fee: String,
   ) {
      let native = NativeCurrency::from_chain_id(chain.id()).unwrap();
      self.set_priority_fee(chain, priority_fee);
      self.dapp = dapp;
      self.chain = chain;
      self.confrim_window = confrim_window;
      self.native_currency = native;
      self.eth_spent = eth_spent;
      self.eth_spent_value = eth_spent_value;
      self.tx_cost = tx_cost;
      self.tx_cost_usd = tx_cost_usd;
      self.gas_used = gas_used;
      self.sender = sender;
      self.interact_to = interact_to;
      self.contract_interact = contract_interact;
      self.action = action;
      self.open = true;
   }

   pub fn get_confirm(&self) -> Option<bool> {
      self.confirm
   }

   pub fn set_priority_fee(&mut self, chain: ChainId, fee: String) {
      // No priority fee for Binance Smart Chain
      // Set empty to avoid frame shutter due to invalid fee
      if chain.is_bsc() {
         self.priority_fee = String::new();
      } else {
         self.priority_fee = fee;
      }
   }

   pub fn get_priority_fee(&self) -> NumericValue {
      NumericValue::parse_to_gwei(&self.priority_fee)
   }

   pub fn simulating(&mut self) {
      self.simulating = true;
   }

   pub fn done_simulating(&mut self) {
      self.simulating = false;
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.simulating = false;
      self.confirm = None;
   }

   /// (Cost in ETH, Cost in USD)
   fn cost(&mut self, ctx: ZeusCtx) {
      let chain = self.chain;
      let fee = NumericValue::parse_to_gwei(&self.priority_fee);
      let fee = if fee.is_zero() {
         NumericValue::parse_to_gwei("1")
      } else {
         fee
      };

      let (cost_in_wei, cost_in_usd) = estimate_tx_cost(ctx, chain.id(), self.gas_used, fee.wei2());
      self.tx_cost = cost_in_wei;
      self.tx_cost_usd = cost_in_usd;
   }

   fn pay_column(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if self.action.is_other() {
         return;
      }

      let text = match self.action {
         OnChainAction::Transfer(_) => "Send",
         OnChainAction::TokenApprove(_) => "Approve",
         _ => "Pay",
      };

      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new(text).size(theme.text_sizes.normal));
         });

         // Currency to Pay/Send/Approve
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let currency = self.action.input_currency();
            let amount = if self.action.is_token_approval() {
               self.action.token_approval_amount_str()
            } else {
               self.action.amount().formatted().clone()
            };

            let icon = icons.currency_icon_x24(&currency);
            let text = RichText::new(format!("{} {}", amount, currency.symbol()))
               .size(theme.text_sizes.normal);
            let label = Label::new(text, Some(icon)).text_first(false);
            ui.add(label);
         });
      });

      if self.action.is_bridge() {
         // Origin Chain Column
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Origin Chain").size(theme.text_sizes.normal));
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let chain: ChainId = self.action.bridge_params().origin_chain.into();
               let icon = icons.chain_icon(&chain.id());
               let label = Label::new(
                  RichText::new(chain.name()).size(theme.text_sizes.normal),
                  Some(icon),
               )
               .text_first(false);
               ui.add(label);
            });
         });
      }
   }

   fn receive_column(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if self.action.is_other() || self.action.is_transfer() || self.action.is_token_approval() {
         return;
      }

      // Currency to Receive
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Receive").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let currency = self.action.output_currency();
            let amount = self.action.received();
            let icon = icons.currency_icon_x24(&currency);
            let text = RichText::new(format!(
               "{} {}",
               amount.formatted(),
               currency.symbol()
            ))
            .size(theme.text_sizes.normal);
            let label = Label::new(text, Some(icon)).text_first(false);
            ui.add(label);
         });
      });

      if self.action.is_bridge() {
         // Destination Chain Column
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Destination Chain").size(theme.text_sizes.normal));
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let chain: ChainId = self.action.bridge_params().destination_chain.into();
               let icon = icons.chain_icon(&chain.id());
               let label = Label::new(
                  RichText::new(chain.name()).size(theme.text_sizes.normal),
                  Some(icon),
               )
               .text_first(false);
               ui.add(label);
            });
         });
      }
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, sender: Address) -> bool {
      let balance = ctx
         .get_eth_balance(self.chain.id(), sender)
         .unwrap_or_default();
      let total_cost = self.eth_spent.wei2() + self.tx_cost.wei2();
      balance.wei2() >= total_cost
   }

   pub fn simulation_result(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = if self.confrim_window {
            "Simulation Results"
         } else {
            "Transaction Summary"
         };
         ui.label(RichText::new(text).size(theme.text_sizes.large));
      });

      // * If the Action is unknown/Transfer/Token Approval we just show the eth spent (Tx cost)
      // * For Swaps and Cross Swaps we show the currency spent and currency received

      if self.action.is_other() || self.action.is_transfer() || self.action.is_token_approval() {
         // Eth spent Column
         ui.horizontal(|ui| {
            let icon = icons.eth_x24();
            let text = RichText::new(&format!(
               "- {} {} ",
               self.eth_spent.formatted(),
               self.native_currency.symbol
            ))
            .size(theme.text_sizes.normal)
            .color(Color32::RED);
            let label = Label::new(text, Some(icon)).text_first(false);
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.add(label);
            });

            // Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.label(
                  RichText::new(&format!(
                     "~ ${}",
                     self.eth_spent_value.formatted()
                  ))
                  .size(theme.text_sizes.small),
               );
            });
         });
      }

      if self.action.is_bridge() || self.action.is_swap() {
         // Currency to Pay/Send
         ui.horizontal(|ui| {
            let currency = self.action.input_currency();
            let amount = self.action.amount();
            let icon = icons.currency_icon_x24(&currency);
            let text = RichText::new(&format!(
               "- {} {} ",
               amount.formatted(),
               currency.symbol()
            ))
            .size(theme.text_sizes.normal)
            .color(Color32::RED);
            let label = Label::new(text, Some(icon)).text_first(false);
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.add(label);
            });

            // Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = self.action.amount_usd().unwrap_or_default();
               ui.label(
                  RichText::new(&format!("~ ${}", value.formatted())).size(theme.text_sizes.small),
               );
            });
         });

         // Received Currency and Value Column
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let currency = self.action.output_currency();
               let amount = self.action.received();
               let icon = icons.currency_icon_x24(&currency);
               let text = RichText::new(format!(
                  "+ {} {}",
                  amount.formatted(),
                  currency.symbol()
               ))
               .size(theme.text_sizes.normal)
               .color(Color32::GREEN);
               let label = Label::new(text, Some(icon)).text_first(false);
               ui.add(label);
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = self.action.received_usd().unwrap_or_default();
               let text =
                  RichText::new(format!("~ ${}", value.formatted())).size(theme.text_sizes.small);
               ui.label(text);
            });
         });
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Tx Confirm")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
               ui.vertical(|ui| {
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);
                  let normal = theme.text_sizes.normal;
                  let large = theme.text_sizes.large;

                  if self.simulating {
                     ui.horizontal(|ui| {
                        ui.add_space(150.0);
                        ui.label(RichText::new("Simulating").size(large));
                        ui.add_space(5.0);
                        ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                     });
                  } else {
                     // Dapp Url
                     ui.vertical_centered(|ui| {
                        if self.confrim_window {
                           ui.label(RichText::new(&self.dapp).size(large));
                        }
                     });

                     self.simulation_result(theme, icons.clone(), ui);

                     // Action Name Column
                     ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        let action_name = self.action.name();
                        ui.label(RichText::new(action_name).size(large).strong());
                     });

                     // Chain Column
                     ui.horizontal(|ui| {
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           ui.label(RichText::new("Chain").size(normal));
                        });

                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                           let chain = self.chain.name();
                           let icon = icons.chain_icon(&self.chain.id());
                           let label = Label::new(RichText::new(chain).size(normal), Some(icon))
                              .text_first(false);
                           ui.add(label);
                        });
                     });

                     self.pay_column(theme, icons.clone(), ui);
                     self.receive_column(theme, icons.clone(), ui);

                     // Contract interaction / Recipient Column
                     ui.horizontal(|ui| {
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           let text = if self.action.is_token_approval() {
                              "Spender"
                           } else {
                              match self.contract_interact {
                                 true => "Contract interaction",
                                 false => "Recipient",
                              }
                           };
                           ui.label(RichText::new(text).size(normal));
                        });

                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                           let (address, address_full) = if self.action.is_token_approval() {
                              let p = self.action.token_approval_params();
                              (
                                 truncate_address(p.spender.to_string()),
                                 p.spender.to_string(),
                              )
                           } else {
                              (
                                 truncate_address(self.interact_to.to_string()),
                                 self.interact_to.to_string(),
                              )
                           };

                           let explorer = self.chain.block_explorer();
                           let link = format!("{}/address/{}", explorer, address_full);
                           ui.hyperlink_to(RichText::new(address).size(normal).strong(), link);
                        });
                     });

                     if !self.confrim_window {
                        ui.horizontal(|ui| {
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              let text = "Transaction hash";
                              ui.label(RichText::new(text).size(normal));
                           });

                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let hash_str = truncate_hash(self.tx_hash.to_string());
                              let explorer = self.chain.block_explorer();
                              let link = format!("{}/tx/{}", explorer, self.tx_hash.to_string());
                              ui.hyperlink_to(
                                 RichText::new(hash_str).size(normal).strong(),
                                 link,
                              );
                           });
                        });
                     }

                     self.cost(ctx.clone());

                     // Transaction cost
                     ui.horizontal(|ui| {
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           let cost = format!(
                              "Cost {} {}",
                              self.tx_cost.formatted(),
                              self.native_currency.symbol
                           );
                           ui.label(RichText::new(cost).size(normal));
                        });

                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                           ui.label(
                              RichText::new(format!("${}", self.tx_cost_usd.formatted()))
                                 .size(theme.text_sizes.small),
                           );
                        });
                     });

                     // Priority Fee
                     let sufficient_balance = self.sufficient_balance(ctx.clone(), self.sender);
                     if self.confrim_window {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           ui.label(RichText::new("Priority Fee").size(theme.text_sizes.normal));
                           ui.add_space(2.0);
                           ui.label(RichText::new("Gwei").size(theme.text_sizes.small));
                        });

                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           if self.chain.is_bsc() {
                              ui.disable();
                           }
                           ui.set_width(ui.available_width() * 0.2);
                           ui.set_height(10.0);
                           ui.add(
                              TextEdit::singleline(&mut self.priority_fee)
                                 .margin(Margin::same(10))
                                 .background_color(theme.colors.text_edit_bg2)
                                 .font(egui::FontId::proportional(
                                    theme.text_sizes.normal,
                                 )),
                           );
                        });
                        if !sufficient_balance {
                           ui.label(
                              RichText::new("Insufficient balance to send transaction")
                                 .size(theme.text_sizes.normal)
                                 .color(Color32::RED),
                           );
                        }
                     }
                     ui.add_space(15.0);

                     ui.horizontal(|ui| {
                        let width = ui.available_width() * 0.9;

                        if self.confrim_window {
                           ui.scope(|ui| {
                              if !sufficient_balance {
                                 ui.disable();
                              }
                              if ui
                                 .add(
                                    Button::new(RichText::new("Confirm").size(normal))
                                       .min_size(vec2(width * 0.75, 50.0)),
                                 )
                                 .clicked()
                              {
                                 self.open = false;
                                 self.confirm = Some(true);
                              }
                           });

                           ui.add_space(10.0);

                           if ui
                              .add(
                                 Button::new(RichText::new("Reject").size(theme.text_sizes.normal))
                                    .min_size(vec2(width * 0.25, 50.0)),
                              )
                              .clicked()
                           {
                              self.open = false;
                              self.confirm = Some(false);
                           }
                        } else {
                           if ui
                              .add(Button::new("Close").min_size(vec2(width * 0.75, 50.0)))
                              .clicked()
                           {
                              self.open = false;
                           }
                        }
                     });
                  } // end of else
               });
            });
         });
   }
}

/// A Window to prompt the user to confirm an action
pub struct ConfirmWindow {
   pub open: bool,
   pub confirm: Option<bool>,
   pub msg: String,
   pub msg2: Option<String>,
   pub size: (f32, f32),
}

impl ConfirmWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         confirm: None,
         msg: String::new(),
         msg2: None,
         size: (200.0, 100.0),
      }
   }

   pub fn open(&mut self, msg: impl Into<String>) {
      self.open = true;
      self.msg = msg.into();
   }

   pub fn set_msg2(&mut self, msg: impl Into<String>) {
      self.msg2 = Some(msg.into());
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.msg.clear();
      self.msg2.take();
      self.confirm = None;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("confirm_window")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               ui.label(RichText::new(&self.msg).size(theme.text_sizes.normal));

               if let Some(msg) = &self.msg2 {
                  ui.label(RichText::new(msg).size(theme.text_sizes.normal));
               }

               if ui
                  .add(Button::new(
                     RichText::new("Confirm").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.open = false;
                  self.confirm = Some(true);
               }

               if ui
                  .add(Button::new(
                     RichText::new("Reject").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.open = false;
                  self.confirm = Some(false);
               }
            });
         });
   }
}

/// Window to indicate a loading state
pub struct LoadingWindow {
   pub open: bool,
   pub msg: String,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl LoadingWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         msg: String::new(),
         size: (200.0, 100.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn open(&mut self, msg: impl Into<String>) {
      self.open = true;
      self.msg = msg.into();
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.msg = String::new();
   }

   pub fn show(&mut self, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Loading")
         .title_bar(false)
         .order(Order::Foreground)
         .resizable(false)
         .anchor(self.anchor.0, self.anchor.1)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.add(Spinner::new().size(50.0).color(Color32::WHITE));
               ui.label(RichText::new(&self.msg).size(17.0));
            });
         });
   }
}

/// Simple window diplaying a message, for example an error
#[derive(Default)]
pub struct MsgWindow {
   pub open: bool,
   pub title: String,
   pub message: String,
}

impl MsgWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         title: String::new(),
         message: String::new(),
      }
   }

   /// Open the window with this title and message
   pub fn open(&mut self, title: impl Into<String>, msg: impl Into<String>) {
      self.open = true;
      self.title = title.into();
      self.message = msg.into();
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.title.clear();
      self.message.clear();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new(self.title.clone()).size(theme.text_sizes.large);
      let msg = RichText::new(&self.message).size(theme.text_sizes.normal);
      let ok = Button::new(RichText::new("Ok").size(theme.text_sizes.normal));

      Window::new(title)
         .resizable(false)
         .order(Order::Foreground)
         .movable(true)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_min_size(vec2(300.0, 100.0));
               ui.scope(|ui| {
                  ui.spacing_mut().item_spacing.y = 20.0;
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  ui.label(msg);

                  if ui.add(ok).clicked() {
                     self.open = false;
                  }
               });
            });
         });
   }
}

pub struct PortfolioUi {
   pub open: bool,
   pub show_spinner: bool,
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: true,
         show_spinner: false,
      }
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let chain_id = ctx.chain().id();
      let current_wallet = ctx.current_wallet();
      let owner = current_wallet.address;
      let portfolio = ctx.get_portfolio(chain_id, owner);
      let currencies = portfolio.currencies();

      ui.vertical_centered_justified(|ui| {
         ui.set_width(ui.available_width() * 0.8);

         ui.spacing_mut().item_spacing = Vec2::new(16.0, 20.0);

         ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let visuals = theme.get_button_visuals(theme.colors.bg_color);
               widget_visuals(ui, visuals);

               let add_token =
                  Button::new(RichText::new("Add Token").size(theme.text_sizes.normal));
               if ui.add(add_token).clicked() {
                  token_selection.open = true;
               }

               let refresh = Button::new(RichText::new("Refresh").size(theme.text_sizes.normal));
               if ui.add(refresh).clicked() {
                  self.refresh(owner, ctx.clone());
               }

               if self.show_spinner {
                  ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
               }
            });
         });

         // Total Value
         ui.vertical(|ui| {
            Frame::group(ui.style())
               .inner_margin(16.0)
               .fill(ui.style().visuals.extreme_bg_color)
               .show(ui, |ui| {
                  ui.vertical_centered(|ui| {
                     let wallet_name = current_wallet.name.clone();
                     ui.label(RichText::new(wallet_name).size(theme.text_sizes.very_large));
                     ui.add_space(8.0);
                     ui.label(
                        RichText::new(format!("${}", portfolio.value.formatted()))
                           .heading()
                           .size(theme.text_sizes.heading + 4.0),
                     );
                  });
               });
         });

         // Token List
         ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
               ui.set_width(ui.available_width());

               let column_widths = [
                  ui.available_width() * 0.2, // Asset
                  ui.available_width() * 0.2, // Price
                  ui.available_width() * 0.2, // Balance
                  ui.available_width() * 0.2, // Value
                  ui.available_width() * 0.1, // Remove button
               ];

               // Center the grid within the available space
               ui.horizontal(|ui| {
                  ui.add_space((ui.available_width() - column_widths.iter().sum::<f32>()) / 2.0);

                  Grid::new("currency_grid")
                     .num_columns(5)
                     .spacing([20.0, 30.0])
                     .show(ui, |ui| {
                        // Header
                        ui.label(RichText::new("Asset").size(theme.text_sizes.large));

                        ui.label(RichText::new("Price").size(theme.text_sizes.large));

                        ui.label(RichText::new("Balance").size(theme.text_sizes.large));

                        ui.label(RichText::new("Value").size(theme.text_sizes.large));

                        ui.end_row();

                        // Token Rows
                        let native_wrapped = currencies.iter().find(|c| c.is_native_wrapped());
                        let native_currency = currencies.iter().find(|c| c.is_native());
                        let tokens: Vec<_> = currencies.iter().filter(|c| c.is_erc20()).collect();

                        if let Some(native) = native_currency {
                           self.token(theme, icons.clone(), native, ui, column_widths[0]);
                           self.price_balance_value(
                              ctx.clone(),
                              theme,
                              chain_id,
                              owner,
                              native,
                              ui,
                              column_widths[0],
                           );
                           self.remove_currency(ctx.clone(), owner, native, ui, column_widths[4]);
                           ui.end_row();
                        }

                        if let Some(wrapped) = native_wrapped {
                           self.token(
                              theme,
                              icons.clone(),
                              wrapped,
                              ui,
                              column_widths[0],
                           );
                           self.price_balance_value(
                              ctx.clone(),
                              theme,
                              chain_id,
                              owner,
                              wrapped,
                              ui,
                              column_widths[0],
                           );
                           self.remove_currency(ctx.clone(), owner, wrapped, ui, column_widths[4]);
                           ui.end_row();
                        }

                        for token in tokens {
                           if token.is_native_wrapped() {
                              continue;
                           }
                           self.token(theme, icons.clone(), token, ui, column_widths[0]);
                           self.price_balance_value(
                              ctx.clone(),
                              theme,
                              chain_id,
                              owner,
                              token,
                              ui,
                              column_widths[0],
                           );
                           self.remove_currency(ctx.clone(), owner, token, ui, column_widths[4]);
                           ui.end_row();
                        }
                     });
               });

               // Token selection
               let all_currencies = ctx.get_currencies(chain_id);
               token_selection.show(
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  chain_id,
                  owner,
                  &all_currencies,
                  ui,
               );
               let currency = token_selection.get_currency().cloned();

               if let Some(currency) = currency {
                  let token_fetched = token_selection.token_fetched;
                  token_selection.reset();
                  self.add_currency(ctx.clone(), owner, token_fetched, currency);
               }
            });
      });
   }

   fn token(&self, theme: &Theme, icons: Arc<Icons>, currency: &Currency, ui: &mut Ui, width: f32) {
      let icon = icons.currency_icon(currency);
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.add(icon);
         ui.label(RichText::new(currency.symbol()).size(theme.text_sizes.normal))
            .on_hover_text(currency.name());
      });
   }

   fn price_balance_value(
      &self,
      ctx: ZeusCtx,
      theme: &Theme,
      chain: u64,
      owner: Address,
      currency: &Currency,
      ui: &mut Ui,
      width: f32,
   ) {
      let price = ctx.get_currency_price(currency);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", price.formatted())).size(theme.text_sizes.normal));
      });

      let balance = ctx.get_currency_balance(chain, owner, currency);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(balance.formatted()).size(theme.text_sizes.normal));
      });

      let value = ctx.get_currency_value(chain, owner, currency);
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", value.formatted())).size(theme.text_sizes.normal));
      });
   }

   fn refresh(&mut self, owner: Address, ctx: ZeusCtx) {
      self.show_spinner = true;
      RT.spawn(async move {
         let chain = ctx.chain().id();

         match update_portfolio_state(ctx, chain, owner).await {
            Ok(_) => {
               tracing::info!("Updated portfolio state");
            }
            Err(e) => {
               tracing::error!("Error updating portfolio state: {:?}", e);
            }
         }

         SHARED_GUI.write(|gui| {
            gui.portofolio.show_spinner = false;
         });
      });
   }

   // Add a currency to the portfolio and update the portfolio value
   fn add_currency(
      &mut self,
      ctx: ZeusCtx,
      owner: Address,
      token_fetched: bool,
      currency: Currency,
   ) {
      let chain_id = ctx.chain().id();

      ctx.write(|ctx| {
         ctx.portfolio_db
            .add_currency(chain_id, owner, currency.clone());
      });

      let ctx_clone = ctx.clone();
      RT.spawn_blocking(move || {
         let _ = ctx_clone.save_portfolio_db();
      });

      if currency.is_native() {
         return;
      }

      let token = currency.erc20().cloned().unwrap();

      // if token was fetched from the blockchain, we don't need to sync the pools or the balance
      if token_fetched {
         tracing::info!(
            "Token {} was fetched from the blockchain, no need to sync pools or balance",
            token.symbol
         );
         return;
      }

      let v3_pools = ctx.get_v3_pools(&token);
      let token2 = token.clone();
      let ctx2 = ctx.clone();
      self.show_spinner = true;
      RT.spawn(async move {
         match eth::sync_pools_for_token(
            ctx2.clone(),
            token2.clone(),
            true,
            v3_pools.is_empty(),
         )
         .await
         {
            Ok(_) => {
               tracing::info!("Synced Pools for {}", token2.symbol);
            }
            Err(e) => tracing::error!(
               "Error syncing pools for {}: {:?}",
               token2.symbol,
               e
            ),
         }

         let pool_manager = ctx2.pool_manager();
         let client = ctx2.get_client_with_id(chain_id).unwrap();
         match pool_manager.update(client, chain_id).await {
            Ok(_) => {
               tracing::info!("Updated pool state for {}", token2.symbol);
               SHARED_GUI.write(|gui| {
                  gui.portofolio.show_spinner = false;
               });
            }
            Err(e) => {
               tracing::error!(
                  "Error updating pool state for {}: {:?}",
                  token2.symbol,
                  e
               );
               SHARED_GUI.write(|gui| {
                  gui.portofolio.show_spinner = false;
               });
            }
         }

         let balance = match eth::get_token_balance(ctx2.clone(), owner, token.clone()).await {
            Ok(b) => b,
            Err(e) => {
               tracing::error!("Error getting token balance: {:?}", e);
               NumericValue::default()
            }
         };
         ctx2.write(|ctx| {
            ctx.balance_db
               .insert_token_balance(chain_id, owner, balance.wei().unwrap(), &token);
         });
         RT.spawn_blocking(move || {
            ctx2.update_portfolio_value(chain_id, owner);
            ctx2.save_all();
         });
      });
   }

   fn remove_currency(
      &self,
      ctx: ZeusCtx,
      owner: Address,
      currency: &Currency,
      ui: &mut Ui,
      width: f32,
   ) {
      ui.horizontal(|ui| {
         ui.set_width(width);
         if ui.button("X").clicked() {
            let chain = ctx.chain().id();
            ctx.write(|ctx| {
               ctx.portfolio_db.remove_currency(chain, owner, currency);
            });
            RT.spawn_blocking(move || {
               ctx.update_portfolio_value(chain, owner);
               ctx.save_all();
            });
         }
      });
   }
}
