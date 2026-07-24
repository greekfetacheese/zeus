//! UI to show the decoded events for a transaction

use egui::{Align, Align2, Frame, Layout, Order, RichText, ScrollArea, Ui, Vec2, Window, vec2};
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Label, MultiLabel};

use crate::assets::icons::Icons;
use crate::core::{TransactionAnalysis, ZeusContext, tx::events::*};
use zeus_eth::{
   alloy_primitives::U256,
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
};

use std::sync::Arc;

use super::address;

pub struct DecodedEvents {
   open: bool,
   overlay: OverlayManager,
}

impl DecodedEvents {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.overlay.window_opened();
      self.open = true;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      chain: ChainId,
      theme: &Theme,
      icons: Arc<Icons>,
      analysis: &TransactionAnalysis,
      frame_size: Vec2,
      frame: Frame,
      window_size: (f32, f32),
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let title = RichText::new("Decoded Events").size(theme.text_sizes.heading);
      let mut open = self.open;

      let window_frame = theme.frame1;

      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, -100.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               let width = window_size.0 + 50.0;
               ui.set_width(width);
               ui.set_height(window_size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let all_events = analysis.total_events();
               let known_events = analysis.known_events;

               let text = format!(
                  "Decoded {} out of {} total events",
                  known_events, all_events
               );
               ui.label(RichText::new(text).size(theme.text_sizes.very_large));

               ScrollArea::vertical().max_height(window_size.1).show(ui, |ui| {
                  ui.set_width(width);

                  for event in &analysis.decoded_events {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           ui.label(RichText::new(event.name()).size(theme.text_sizes.heading));

                           show_event(ctx, chain, theme, icons.clone(), event, ui);
                        });
                     });
                  }
               });
            });
         });

      if !open {
         self.close();
      }
   }
}

pub fn eoa_delegate_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   params: &EOADelegateParams,
   ui: &mut Ui,
) {
   address(ctx, chain, "Wallet", params.eoa, theme, ui);

   address(
      ctx,
      chain,
      "Delegate to",
      params.address,
      theme,
      ui,
   );

   // Nonce
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Nonce").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(format!("{}", params.nonce)).size(theme.text_sizes.normal));
      });
   });
}

pub fn permit_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &PermitParams,
   ui: &mut Ui,
) {
   let is_unlimited = params.amount.wei() == U256::MAX;
   let amount = if is_unlimited {
      "Unlimited".to_string()
   } else {
      params.amount.abbreviated()
   };

   let show_usd_value = !is_unlimited && params.amount_usd.is_some();
   let expiration = params.expiration.to_relative();

   let tint = theme.image_tint_recommended;
   let icon = icons.currency_icon_x32(&params.token, tint);
   let text = if show_usd_value {
      let amount_usd = params.amount_usd.as_ref().unwrap();
      RichText::new(format!(
         "{} {} ~ ${}",
         amount,
         params.token.symbol(),
         amount_usd.abbreviated()
      ))
      .size(theme.text_sizes.large)
   } else {
      RichText::new(format!("{} {}", amount, params.token.symbol())).size(theme.text_sizes.large)
   };

   let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
   ui.add(label);

   // Owner
   address(ctx, chain, "Owner", params.owner, theme, ui);

   // Spender
   address(ctx, chain, "Spender", params.spender, theme, ui);

   // Expiration
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Expiration").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(expiration).size(theme.text_sizes.large));
      });
   });
}

pub fn token_approval_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &TokenApproveParams,
   ui: &mut Ui,
) {
   let token_details = params.token.iter().zip(params.amount.iter()).zip(params.amount_usd.iter());

   for ((token, amount), amount_usd) in token_details {
      let is_unlimited = amount.wei() == U256::MAX;
      let amount = if is_unlimited {
         "Unlimited".to_string()
      } else {
         amount.abbreviated()
      };

      let show_usd_value = !is_unlimited && amount_usd.is_some();
      let tint = theme.image_tint_recommended;

      let icon = icons.currency_icon_x32(&Currency::from(token.clone()), tint);
      let text = if show_usd_value {
         let amount_usd = amount_usd.as_ref().unwrap();
         RichText::new(format!(
            "{} {} ~ ${}",
            amount,
            token.symbol,
            amount_usd.abbreviated()
         ))
         .size(theme.text_sizes.large)
      } else {
         RichText::new(format!("{} {}", amount, token.symbol)).size(theme.text_sizes.large)
      };

      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.add(label);
   }

   // Owner
   address(ctx, chain, "Owner", params.owner, theme, ui);

   // Spender
   address(ctx, chain, "Spender", params.spender, theme, ui);
}

fn transfer_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &TransferParams,
   ui: &mut Ui,
) {
   let size = vec2(ui.available_width(), 30.0);
   let tint = theme.image_tint_recommended;

   // Currency to Send
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let currency = &params.currency;
            let amount = &params.amount;
            let icon = icons.currency_icon_x32(currency, tint);
            let text = RichText::new(format!(
               "{} {} ",
               amount.abbreviated(),
               currency.symbol()
            ))
            .size(theme.text_sizes.large);
            let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = params.amount_usd.clone().unwrap_or_default();
            ui.label(
               RichText::new(format!("~ ${}", amount.abbreviated())).size(theme.text_sizes.large),
            );
         });
      });
   });

   // Sender
   address(ctx, chain, "Sender", params.sender, theme, ui);

   // Recipient
   ui.allocate_ui(size, |ui| {
      address(
         ctx,
         chain,
         "Recipient",
         params.recipient,
         theme,
         ui,
      );
   });

   // Actual amount sent
   if params.real_amount_sent.is_some() {
      let real_amount_sent = params.real_amount_sent.as_ref().unwrap();
      let real_amount_sent_usd = params.real_amount_sent_usd.clone().unwrap_or_default();

      ui.horizontal(|ui| {
         let text = "Actual amount sent";
         ui.label(RichText::new(text).size(theme.text_sizes.large));

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(
               RichText::new(format!(
                  "~ ${}",
                  real_amount_sent_usd.abbreviated()
               ))
               .size(theme.text_sizes.large),
            );

            let currency = &params.currency;
            let text = RichText::new(format!(
               "{} {} ",
               real_amount_sent.abbreviated(),
               currency.symbol()
            ))
            .size(theme.text_sizes.large);
            let label = Label::new(text, None).interactive(false);
            ui.add(label);
         });
      });
   }
}

fn shield_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &ShieldParams,
   ui: &mut Ui,
) {
   let size = vec2(ui.available_width(), 30.0);
   let tint = theme.image_tint_recommended;

   // Amount Shielded
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         if let (Some(token), Some(amount), Some(amount_usd)) = (
            params.erc20.as_ref(),
            params.amount.as_ref(),
            params.amount_usd.as_ref(),
         ) {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let text = RichText::new(format!("Shield",)).size(theme.text_sizes.large);
               let label = Label::new(text, None).interactive(false);
               ui.add(label);
            });

            // Token & Amount (usd)
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let icon = icons.token_icon_x24(token.address, token.chain_id, tint);
               let text = RichText::new(format!(
                  "{} {}",
                  amount.abbreviated(),
                  token.symbol,
               ))
               .size(theme.text_sizes.large);

               let label1 = Label::new(text, Some(icon)).interactive(false);

               let text = RichText::new(format!("~ ${}", amount_usd.abbreviated()))
                  .size(theme.text_sizes.large);
               let label2 = Label::new(text, None).interactive(false);

               let multi_label = MultiLabel::new(vec![label1, label2]);
               ui.add(multi_label);
            });
         }
      });
   });

   // Recipient
   if let Some(recipient) = &params.recipient {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let text = RichText::new(format!("Recipient",)).size(theme.text_sizes.large);
            let label = Label::new(text, None).interactive(false);
            ui.add(label);
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let wallet_opt = ctx.get_wallet_info_by_zk_address(recipient);
            let contact_opt = ctx.get_contact_by_zk_address(recipient);

            let (text, evm_address_opt) = if let Some(wallet) = wallet_opt {
               let rich = RichText::new(wallet.name())
                  .size(theme.text_sizes.large)
                  .color(theme.colors.info);
               (rich, Some(wallet.address.to_string()))
            } else if let Some(contact) = contact_opt {
               let rich = RichText::new(contact.name)
                  .size(theme.text_sizes.large)
                  .color(theme.colors.info);
               (rich, Some(contact.evm_address))
            } else {
               let truncated = format!("{}...{}", &recipient[..6], &recipient[121..]);
               let rich =
                  RichText::new(truncated).size(theme.text_sizes.large).color(theme.colors.info);
               (rich, None)
            };

            if let Some(evm_address) = evm_address_opt {
               let explorer = chain.block_explorer();
               let link = format!("{}/address/{}", explorer, evm_address);
               ui.hyperlink_to(text, link);
            } else {
               ui.label(text);
            }
         });
      });
   }

   // Protocol fee
   if let (Some(token), Some(fee), Some(fee_usd)) = (
      params.erc20.as_ref(),
      params.fee.as_ref(),
      params.fee_usd.as_ref(),
   ) {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Protocol fee").size(theme.text_sizes.large));
         });
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let icon = icons.token_icon_x24(token.address, token.chain_id, tint);

            let token_text = format!("{} {}", fee.abbreviated(), token.symbol);
            let token_rich_text = RichText::new(token_text).size(theme.text_sizes.large);

            let fee_usd_text = format!("~ ${}", fee_usd.abbreviated());
            let fee_usd_rich_text = RichText::new(fee_usd_text).size(theme.text_sizes.large);

            let label1 = Label::new(token_rich_text, Some(icon)).interactive(false);
            let label2 = Label::new(fee_usd_rich_text, None).interactive(false);
            let multi_label = MultiLabel::new(vec![label1, label2]);
            ui.add(multi_label);
         });
      });
   }
}

fn unshield_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &UnshieldParams,
   ui: &mut Ui,
) {
   let size = vec2(ui.available_width(), 30.0);
   let tint = theme.image_tint_recommended;

   // Recipient
   address(
      ctx,
      chain,
      "Recipient",
      params.recipient,
      theme,
      ui,
   );

   // Amount unshielded
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         // Receive
         if let (Some(token), Some(amount), Some(amount_usd)) = (
            params.erc20.as_ref(),
            params.amount.as_ref(),
            params.amount_usd.as_ref(),
         ) {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let text = RichText::new(format!("Receive",)).size(theme.text_sizes.large);
               let label = Label::new(text, None).interactive(false);
               ui.add(label);
            });

            // Token & Amount (usd)
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let icon = icons.token_icon_x24(token.address, token.chain_id, tint);
               let text = RichText::new(format!(
                  "{} {}",
                  amount.abbreviated(),
                  token.symbol,
               ))
               .size(theme.text_sizes.large);

               let label1 = Label::new(text, Some(icon)).interactive(false);

               let text = RichText::new(format!("~ ${}", amount_usd.abbreviated()))
                  .size(theme.text_sizes.large);
               let label2 = Label::new(text, None).interactive(false);

               let multi_label = MultiLabel::new(vec![label1, label2]);
               ui.add(multi_label);
            });
         }
      });
   });

   // Protocol unshield fee (on-token fee from Unshield event)
   if let (Some(token), Some(fee), Some(fee_usd)) = (
      params.erc20.as_ref(),
      params.fee.as_ref(),
      params.fee_usd.as_ref(),
   ) {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Protocol fee").size(theme.text_sizes.large));
         });
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let icon = icons.token_icon_x24(token.address, token.chain_id, tint);

            let token_text = format!("{} {}", fee.abbreviated(), token.symbol);
            let token_rich_text = RichText::new(token_text).size(theme.text_sizes.large);

            let fee_usd_text = format!("~ ${}", fee_usd.abbreviated());
            let fee_usd_rich_text = RichText::new(fee_usd_text).size(theme.text_sizes.large);

            let label1 = Label::new(token_rich_text, Some(icon)).interactive(false);
            let label2 = Label::new(fee_usd_rich_text, None).interactive(false);
            let multi_label = MultiLabel::new(vec![label1, label2]);
            ui.add(multi_label);
         });
      });
   }

   // Broadcaster / privacy-paymaster fee.
   // Paid from private balance
   // TODO: Maybe add a ? that pops up explaining that the fee is paid from the private balance
   if !params.is_self_broadcast {
      if let (Some(bf_fee), Some(bf_fee_usd), Some(token)) = (
         params.broadcaster_fee.as_ref(),
         params.broadcaster_fee_usd.as_ref(),
         params.erc20.as_ref(),
      ) {
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Broadcaster fee").size(theme.text_sizes.large));
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let icon = icons.token_icon_x24(token.address, chain.id(), tint);

               let fee_text = format!("{} {}", bf_fee.abbreviated(), token.symbol);
               let fee_rich_text = RichText::new(fee_text).size(theme.text_sizes.large);

               let fee_usd_text = format!("~ ${}", bf_fee_usd.abbreviated());
               let fee_usd_rich_text = RichText::new(fee_usd_text).size(theme.text_sizes.large);

               let label1 = Label::new(fee_rich_text, Some(icon)).interactive(false);
               let label2 = Label::new(fee_usd_rich_text, None).interactive(false);
               let multi_label = MultiLabel::new(vec![label1, label2]);
               ui.add(multi_label);
            });
         });
      } else {
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new("Broadcaster fee")
                     .size(theme.text_sizes.large)
                     .color(theme.colors.warning),
               );
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.label(
                  RichText::new("N/A").size(theme.text_sizes.large).color(theme.colors.error),
               );
            });
         });
      }
   }
}

fn bridge_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &BridgeParams,
   ui: &mut Ui,
) {
   let origin_chain = params.origin_chain;
   let destination_chain = params.destination_chain;
   let tint = theme.image_tint_recommended;

   // EOA's always receive native ETH
   let currency_in = if params.input_currency.is_native_wrapped() {
      NativeCurrency::from(origin_chain).into()
   } else {
      params.input_currency.clone()
   };

   let currency_out = if params.output_currency.is_native_wrapped() {
      NativeCurrency::from(destination_chain).into()
   } else {
      params.output_currency.clone()
   };

   // Input currency column
   ui.horizontal(|ui| {
      let amount = &params.amount;
      let icon = icons.currency_icon_x32(&currency_in, tint);
      let text = RichText::new(format!(
         "- {} {} ",
         amount.abbreviated(),
         currency_in.symbol()
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.error);
      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.amount_usd.clone().unwrap_or_default();
         ui.label(
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large),
         );
      });
   });

   // Received Currency
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let amount = &params.received;
         let icon = icons.currency_icon_x32(&currency_out, tint);
         let text = RichText::new(format!(
            "+ {} {}",
            amount.abbreviated(),
            currency_out.symbol()
         ))
         .size(theme.text_sizes.large)
         .color(theme.colors.success);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.received_usd.clone().unwrap_or_default();
         let text =
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Depositor
   address(
      ctx,
      chain,
      "Depositor",
      params.depositor,
      theme,
      ui,
   );

   // Recipient
   address(
      ctx,
      chain,
      "Recipient",
      params.recipient,
      theme,
      ui,
   );

   // Origin Chain Column
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Origin Chain").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let chain: ChainId = params.origin_chain.into();
         let icon = icons.chain_icon(chain.id(), tint);
         let text = RichText::new(chain.name()).size(theme.text_sizes.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });
   });

   // Destination Chain Column
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Destination Chain").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let chain: ChainId = params.destination_chain.into();
         let icon = icons.chain_icon(chain.id(), tint);
         let text = RichText::new(chain.name()).size(theme.text_sizes.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });
   });
}

fn swap_event_ui(theme: &Theme, icons: Arc<Icons>, params: &SwapParams, ui: &mut Ui) {
   let tint = theme.image_tint_recommended;

   // Input currency column
   ui.horizontal(|ui| {
      let currency = &params.input_currency;
      let amount = &params.amount_in;
      let icon = icons.currency_icon_x32(currency, tint);
      let text = RichText::new(format!(
         "- {} {} ",
         amount.abbreviated(),
         currency.symbol()
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.error);
      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.amount_in_usd.clone().unwrap_or_default();
         ui.label(
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large),
         );
      });
   });

   // Received Currency
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let currency = &params.output_currency;
         let amount = &params.received;
         let icon = icons.currency_icon_x32(currency, tint);
         let text = RichText::new(format!(
            "+ {} {}",
            amount.abbreviated(),
            currency.symbol()
         ))
         .size(theme.text_sizes.large)
         .color(theme.colors.success);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.received_usd.clone().unwrap_or_default();
         let text =
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Minimum Received
   let amount = params.min_received.clone();
   let amount_usd = params.min_received_usd.clone();
   if amount.is_some() && amount_usd.is_some() {
      ui.horizontal(|ui| {
         ui.label(RichText::new("Minimum Received").size(theme.text_sizes.large));
         ui.add_space(15.0);

         let amount = amount.unwrap();
         let amount_usd = amount_usd.unwrap();
         let currency = &params.output_currency;
         let amount_symbol = format!("{} {}", amount.abbreviated(), currency.symbol());
         let amount_usd = format!("~ ${}", amount_usd.abbreviated());
         let text =
            RichText::new(format!("{} {}", amount_symbol, amount_usd)).size(theme.text_sizes.large);
         let label = Label::new(text, None).interactive(false);
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.add(label);
         });
      });
   }
}

fn wrap_eth_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &WrapETHParams,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let weth = Currency::from(ERC20Token::wrapped_native_token(chain.id()));
   let weth_icon = icons.currency_icon_x32(&weth, tint);

   // Amount received + USD Value
   ui.horizontal(|ui| {
      let text = RichText::new(format!(
         "+ {} {}",
         params.weth_received.abbreviated(),
         weth.symbol()
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.success);
      let label = Label::new(text, Some(weth_icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // USD Value
      let weth_received_usd = params.weth_received_usd.clone().unwrap_or_default();
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(format!("~ ${}", weth_received_usd.abbreviated()))
            .size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Recipient
   address(
      ctx,
      chain,
      "Recipient",
      params.recipient,
      theme,
      ui,
   );
}

fn unwrap_weth_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &UnwrapWETHParams,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let eth = NativeCurrency::from(chain.id());
   let eth_icon = icons.native_currency_icon(chain.id(), tint);

   // Amount received + USD Value
   ui.horizontal(|ui| {
      let text = RichText::new(format!(
         "+ {} {}",
         params.weth_unwrapped.abbreviated(),
         eth.symbol
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.success);
      let label = Label::new(text, Some(eth_icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // USD Value
      let weth_unwrapped_usd = params.weth_unwrapped_usd.clone().unwrap_or_default();
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(format!("~ ${}", weth_unwrapped_usd.abbreviated()))
            .size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Source
   address(ctx, chain, "Source", params.src, theme, ui);
}

fn uniswap_position_op_event_ui(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &UniswapPositionParams,
   ui: &mut Ui,
) {
   let currency0 = &params.currency0;
   let currency1 = &params.currency1;
   let amount0 = &params.amount0;
   let amount1 = &params.amount1;
   let amount0_usd = params.amount0_usd.clone().unwrap_or_default();
   let amount1_usd = params.amount1_usd.clone().unwrap_or_default();
   let min_amount0 = params.min_amount0.clone();
   let min_amount1 = params.min_amount1.clone();
   let min_amount0_usd = params.min_amount0_usd.clone().unwrap_or_default();
   let min_amount1_usd = params.min_amount1_usd.clone().unwrap_or_default();

   let tint = theme.image_tint_recommended;

   // Currency A and Amount & value
   ui.horizontal(|ui| {
      let icon = icons.currency_icon_x32(currency0, tint);

      let text = format!("{} {}", amount0.abbreviated(), currency0.symbol());
      let text = RichText::new(text).size(theme.text_sizes.large);

      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = amount0_usd.abbreviated();
         ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
      });
   });

   // Currency B and Amount & value
   ui.horizontal(|ui| {
      let icon = icons.currency_icon_x32(currency1, tint);
      let text = format!("{} {}", amount1.abbreviated(), currency1.symbol());

      let text = RichText::new(text).size(theme.text_sizes.large);
      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = amount1_usd.abbreviated();
         ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
      });
   });

   let text = if params.op_is_add_liquidity() {
      "Minimum Liquidity to be added"
   } else {
      "Minimum Liquidity to be removed"
   };

   if min_amount0.is_some() && min_amount1.is_some() {
      ui.label(RichText::new(text).size(theme.text_sizes.large));
   }

   // Minimum Amount A and Amount & value
   if min_amount0.is_some() {
      let min_amount0 = min_amount0.unwrap();
      ui.horizontal(|ui| {
         let icon = icons.currency_icon_x32(currency0, tint);
         let text = format!(
            "{} {}",
            min_amount0.abbreviated(),
            currency0.symbol()
         );
         let text = RichText::new(text).size(theme.text_sizes.large);

         let label = Label::new(text, Some(icon)).image_on_left();
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = min_amount0_usd.abbreviated();
            ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
         });
      });
   }

   // Minimum Amount B and Amount & value
   if min_amount1.is_some() {
      let min_amount1 = min_amount1.unwrap();
      ui.horizontal(|ui| {
         let icon = icons.currency_icon_x32(currency1, tint);
         let text = format!(
            "{} {}",
            min_amount1.abbreviated(),
            currency1.symbol()
         );

         let text = RichText::new(text).size(theme.text_sizes.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = min_amount1_usd.abbreviated();
            ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
         });
      });
   }

   // Recipient
   if params.recipient.is_some() {
      address(
         ctx,
         chain,
         "Recipient",
         params.recipient.unwrap(),
         theme,
         ui,
      );
   }
}

pub fn show_event(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   event: &DecodedEvent,
   ui: &mut Ui,
) {
   if event.is_native_transfer() || event.is_erc20_transfer() {
      let params = event.transfer_params();
      transfer_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_token_approval() {
      let params = event.token_approval_params();
      token_approval_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_permit() {
      let params = event.permit_params();
      permit_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_wrap_eth() {
      let params = event.wrap_eth_params();
      wrap_eth_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_unwrap_weth() {
      let params = event.unwrap_weth_params();
      unwrap_weth_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_uniswap_position_op() {
      let params = event.uniswap_position_params();
      uniswap_position_op_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_bridge() {
      let params = event.bridge_params();
      bridge_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_swap() {
      let params = event.swap_params();
      swap_event_ui(theme, icons.clone(), params, ui);
   }

   if event.is_eoa_delegate() {
      let params = event.eoa_delegate_params();
      eoa_delegate_event_ui(ctx, chain, theme, params, ui);
   }

   if event.is_shield() {
      let params = event.shield_params();
      shield_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }

   if event.is_unshield() {
      let params = event.unshield_params();
      unshield_event_ui(ctx, chain, theme, icons.clone(), params, ui);
   }
}
