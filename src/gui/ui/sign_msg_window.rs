use egui::{
   Align, Align2, Button, FontId, Frame, Layout, Margin, Order, RichText, ScrollArea, TextEdit, Ui,
   Window, vec2,
};
use egui_theme::Theme;
use egui_widgets::Label;

use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{format_expiry, sign::SignMsgType, truncate_address},
};

use zeus_eth::{alloy_primitives::U256, types::ChainId};

use std::sync::Arc;

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

   pub fn is_open(&self) -> bool {
      self.open
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Sign Message")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.spacing_mut().item_spacing.y = 15.0;
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let msg = self.msg.clone();

                  if msg.is_none() {
                     ui.label("No message to sign");
                     return;
                  }

                  let msg = msg.unwrap();

                  ui.label(RichText::new(&self.dapp).size(theme.text_sizes.large));

                  let frame = theme.frame2;
                  let frame_size = vec2(ui.available_width(), 45.0);

                  ui.label(RichText::new(msg.title()).size(theme.text_sizes.large));

                  if msg.is_permit2_single() {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           permit2_single_approval(
                              ctx.clone(),
                              self.chain,
                              &msg,
                              theme,
                              icons.clone(),
                              ui,
                           );
                        });
                     });
                  }

                  ui.add_space(30.0);

                  // Show the msg
                  let mut value = msg.msg_value().clone().to_string();

                  let text_edit = TextEdit::multiline(&mut value)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .margin(Margin::same(10))
                     .desired_width(ui.available_width() * 0.9)
                     .background_color(theme.colors.text_edit_bg);

                  ui.label(RichText::new("Sign Data").size(theme.text_sizes.large));
                  ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                     ui.add(text_edit);
                  });

                  ui.add_space(20.0);
                  let ui_size = vec2(ui.available_width() * 0.9, 45.0);

                  ui.allocate_ui(ui_size, |ui| {
                     ui.spacing_mut().item_spacing.x = 20.0;
                     let button_size = vec2(ui.available_width() * 0.5, 45.0);

                     ui.horizontal(|ui| {
                        let ok_btn =
                           Button::new(RichText::new("Sign").size(theme.text_sizes.normal))
                              .min_size(button_size);
                        if ui.add(ok_btn).clicked() {
                           self.open = false;
                           self.signed = Some(true)
                        }

                        let cancel_btn =
                           Button::new(RichText::new("Cancel").size(theme.text_sizes.normal))
                              .min_size(button_size);
                        if ui.add(cancel_btn).clicked() {
                           self.open = false;
                           self.signed = Some(false)
                        }
                     });
                  });
               });
            });
         });
   }
}

fn permit2_single_approval(
   ctx: ZeusCtx,
   chain: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.permit2_details();

   let size = vec2(ui.available_width(), 30.0);

   ui.allocate_ui(size, |ui| {
      // Chain
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Chain").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
            let icon = icons.chain_icon(chain.id());
            let label = Label::new(text, Some(icon)).image_on_left();
            ui.add(label);
         });
      });

      // Token
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Approve Token").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = details.amount();
            let text = format!("{} {}", amount, details.token.symbol);
            let icon = icons.token_icon(details.token.address, details.token.chain_id);
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
            ui.label(RichText::new("Contract").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let contract_address = details.permit2_contract;
            let explorer = chain.block_explorer();
            let link = format!(
               "{}/address/{}",
               explorer,
               contract_address
            );
            ui.hyperlink_to(
               RichText::new("Uniswap Protocol: Permit2")
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.hyperlink_color),
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
            let spender_address = details.spender;
            let spender_name = ctx.get_address_name(chain.id(), spender_address);
            let spender = if let Some(spender_name_str) = spender_name {
               spender_name_str
            } else {
               truncate_address(spender_address.to_string())
            };

            let explorer = chain.block_explorer();
            let link = format!(
               "{}/address/{}",
               explorer,
               spender_address
            );
            ui.hyperlink_to(
               RichText::new(spender)
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.hyperlink_color),
               link,
            );
         });
      });
   });
}

fn _permit2_batch_approval_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.permit2_batch_details();

   ui.label(RichText::new("Permit2 Batch Token Approval").size(theme.text_sizes.normal));

   // Chain
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Chain").size(theme.text_sizes.normal));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
         let icon = icons.chain_icon(chain.id());
         let label = Label::new(text, Some(icon)).image_on_left();
         ui.add(label);
      });
   });

   ui.horizontal(|ui| {
      ui.label(RichText::new("Approve Tokens").size(theme.text_sizes.normal));
   });

   let token_details = details
      .tokens
      .iter()
      .zip(details.amounts.iter())
      .zip(details.amounts_usd.iter());

   // Tokens
   for ((token, amount), _amount_usd) in token_details {
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = if amount.wei() == U256::MAX {
            "Unlimited".to_string()
         } else {
            amount.format_abbreviated()
         };

         let text = format!("{} {}", amount, token.symbol);
         let icon = icons.token_icon(token.address, token.chain_id);
         let label = Label::new(
            RichText::new(text).size(theme.text_sizes.normal),
            Some(icon),
         )
         .wrap();
         ui.add(label);
      });
   }

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
         ui.label(RichText::new("Contract").size(theme.text_sizes.normal));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let contract_address = details.permit2_contract;
         let name = ctx.get_address_name(chain.id(), contract_address);
         let contract = if let Some(name_str) = name {
            name_str
         } else {
            truncate_address(contract_address.to_string())
         };

         let explorer = chain.block_explorer();
         let link = format!(
            "{}/address/{}",
            explorer,
            contract_address
         );

         ui.hyperlink_to(
            RichText::new(contract)
               .size(theme.text_sizes.normal)
               .color(theme.colors.hyperlink_color),
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
         let explorer = chain.block_explorer();
         let link = format!("{}/address/{}", explorer, spender);
         ui.hyperlink_to(
            RichText::new(truncate_address(spender.to_string()))
               .size(theme.text_sizes.normal)
               .color(theme.colors.hyperlink_color),
            link,
         );
      });
   });

   // Protocol/Dapp
   // TODO:
}
