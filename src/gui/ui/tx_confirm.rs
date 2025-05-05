use egui::{
   Align, Align2, Button, Color32, Frame, Layout, Margin, Order, RichText, Spinner, TextEdit, Ui,
   Window, vec2,
};
use egui_theme::Theme;
use egui_widgets::Label;

use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{
      action::OnChainAction, estimate_tx_cost, truncate_address, truncate_hash, tx::TxSummary,
   },
};
use zeus_eth::{
   alloy_primitives::{Address, TxHash},
   currency::NativeCurrency,
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;

pub struct TxConfirmWindow {
   open: bool,
   /// Use this Window as a Tx summary or to confirm a transaction
   ///
   /// `True` for confirming a transaction and `False` for showing a summary
   confrim_window: bool,
   simulating: bool,
   confirm: Option<bool>,
   dapp: String,
   chain: ChainId,
   native_currency: NativeCurrency,
   /// Not tx cost but how much eth is being sent
   ///
   /// It's Tx value + anything that can spent eth
   eth_spent: NumericValue,
   eth_spent_value: NumericValue,
   /// Did we received eth from this tx?
   eth_received: NumericValue,
   eth_received_value: NumericValue,
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
         eth_received: NumericValue::default(),
         eth_received_value: NumericValue::default(),
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
      let native = NativeCurrency::from(chain);

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
      eth_received: NumericValue,
      eth_received_value: NumericValue,
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
      self.eth_received = eth_received;
      self.eth_received_value = eth_received_value;
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

   /// Quick summary of the tx
   ///
   /// It shows the amount of ETH spent/received
   fn simulation_result(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = if self.confrim_window {
            "Simulation Results"
         } else {
            "Transaction Summary"
         };
         ui.label(RichText::new(text).size(theme.text_sizes.large));
      });

      let mut eth_amount = self.eth_spent.clone();
      let mut eth_value = self.eth_spent_value.clone();
      let mut sign = "-";
      let mut color = Color32::RED;

      if self.eth_spent.is_zero() && !self.eth_received.is_zero() {
         eth_amount = self.eth_received.clone();
         eth_value = self.eth_received_value.clone();
         sign = "+";
         color = Color32::GREEN;
      }

      // Eth Spent/Received Column
      ui.horizontal(|ui| {
         let icon = icons.eth_x24();
         let text = RichText::new(&format!(
            "{} {} {}",
            sign,
            eth_amount.formatted(),
            self.native_currency.symbol
         ))
         .size(theme.text_sizes.normal)
         .color(color);
         let label = Label::new(text, Some(icon)).text_first(false);
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(
               RichText::new(&format!("~ ${}", eth_value.formatted())).size(theme.text_sizes.small),
            );
         });
      });
   }

   fn action_is_token_approval(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.action.token_approval_params();
      let amount = params.amount.formatted();
      let currency = &params.token;
      let spender_addr = params.spender.to_string();
      let spender_short = truncate_address(spender_addr.clone());
      let explorer = self.chain.block_explorer();
      let link = format!("{}/address/{}", explorer, spender_addr);

      ui.horizontal(|ui| {
         let approve_text = RichText::new("Approve").size(theme.text_sizes.normal);
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(approve_text);
         });

         // Currency to Approve
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let icon = icons.currency_icon_x24(currency);
            let text = RichText::new(format!("{} {}", amount, currency.symbol()))
               .size(theme.text_sizes.normal);
            let label = Label::new(text, Some(icon)).text_first(false);
            ui.add(label);
         });
      });

      // Spender
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let text = RichText::new("Spender").size(theme.text_sizes.normal);
            ui.label(text);
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.hyperlink_to(
               RichText::new(spender_short)
                  .size(theme.text_sizes.normal)
                  .strong(),
               link,
            );
         });
      });
   }

   fn action_is_transfer(&self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      // Currency to Send
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let currency = self.action.input_currency();
            let amount = self.action.amount();
            let icon = icons.currency_icon_x24(&currency);
            let text = RichText::new(&format!(
               "{} {} ",
               amount.formatted(),
               currency.symbol()
            ))
            .size(theme.text_sizes.normal);
            let label = Label::new(text, Some(icon)).text_first(false);
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = self.action.amount_usd().unwrap_or_default();
            ui.label(
               RichText::new(&format!("~ ${}", amount.formatted())).size(theme.text_sizes.small),
            );
         });
      });

      // Recipient
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Recipient").size(theme.text_sizes.large));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let recipient_address = self.action.transfer_params().recipient;
            let recipient_short = truncate_address(recipient_address.to_string());
            let explorer = self.chain.block_explorer();
            let link = format!(
               "{}/address/{}",
               explorer,
               recipient_address.to_string()
            );
            let contact = ctx.get_contact_by_address(&recipient_address.to_string());
            let wallet = ctx.get_wallet_info(recipient_address);
            let recipient = if contact.is_some() {
               contact.unwrap().name
            } else if wallet.is_some() {
               wallet.unwrap().name
            } else {
               recipient_short
            };
            ui.hyperlink_to(
               RichText::new(recipient)
                  .size(theme.text_sizes.normal)
                  .strong(),
               link,
            );
         });
      });
   }

   fn action_is_swap_or_bridge(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      // Input currency column
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

      // Received Currency
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

                  if self.simulating {
                     ui.horizontal(|ui| {
                        ui.add_space(150.0);
                        ui.label(RichText::new("Simulating").size(theme.text_sizes.large));
                        ui.add_space(5.0);
                        ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                     });
                  } else {
                     // Dapp Url
                     ui.vertical_centered(|ui| {
                        if self.confrim_window {
                           ui.label(RichText::new(&self.dapp).size(theme.text_sizes.large));
                        }
                     });

                     self.simulation_result(theme, icons.clone(), ui);
                     ui.add_space(10.0);

                     // Action Name Column
                     ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        let action_name = self.action.name();
                        ui.label(
                           RichText::new(action_name)
                              .size(theme.text_sizes.large)
                              .strong(),
                        );
                     });

                     if self.action.is_swap() || self.action.is_bridge() {
                        self.action_is_swap_or_bridge(theme, icons.clone(), ui);
                        ui.add_space(10.0);
                     }

                     if self.action.is_token_approval() {
                        self.action_is_token_approval(theme, icons.clone(), ui);
                        ui.add_space(10.0);
                     }

                     if self.action.is_transfer() {
                        self.action_is_transfer(ctx.clone(), theme, icons.clone(), ui);
                        ui.add_space(10.0);
                     }

                     // Chain Column
                     if !self.action.is_bridge() {
                        ui.horizontal(|ui| {
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              ui.label(RichText::new("Chain").size(theme.text_sizes.normal));
                           });

                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let chain = self.chain.name();
                              let icon = icons.chain_icon(&self.chain.id());
                              let label = Label::new(
                                 RichText::new(chain).size(theme.text_sizes.normal),
                                 Some(icon),
                              )
                              .text_first(false);
                              ui.add(label);
                           });
                        });
                     }

                     // Contract interaction
                     if self.contract_interact {
                        ui.horizontal(|ui| {
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              let text = RichText::new("Contract interaction")
                                 .size(theme.text_sizes.normal);
                              ui.label(text);
                           });

                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let interact_addr = self.interact_to.to_string();
                              let interact_short = truncate_address(interact_addr.clone());
                              let explorer = self.chain.block_explorer();
                              let link = format!("{}/address/{}", explorer, interact_addr);

                              ui.hyperlink_to(
                                 RichText::new(interact_short)
                                    .size(theme.text_sizes.normal)
                                    .strong(),
                                 link,
                              );
                           });
                        });
                     }

                     // Show Tx Hash if needed
                     if !self.confrim_window {
                        ui.horizontal(|ui| {
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              let text = "Transaction hash";
                              ui.label(RichText::new(text).size(theme.text_sizes.normal));
                           });

                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let hash_str = truncate_hash(self.tx_hash.to_string());
                              let explorer = self.chain.block_explorer();
                              let link = format!("{}/tx/{}", explorer, self.tx_hash.to_string());
                              ui.hyperlink_to(
                                 RichText::new(hash_str)
                                    .size(theme.text_sizes.normal)
                                    .strong(),
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
                           ui.label(RichText::new(cost).size(theme.text_sizes.normal));
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
                        ui.add_space(5.0);

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

                     // Buttons
                     ui.horizontal(|ui| {
                        let width = ui.available_width() * 0.9;

                        if self.confrim_window {
                           ui.scope(|ui| {
                              if !sufficient_balance {
                                 ui.disable();
                              }
                              if ui
                                 .add(
                                    Button::new(
                                       RichText::new("Confirm").size(theme.text_sizes.normal),
                                    )
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
                  }
               });
            });
         });
   }
}
