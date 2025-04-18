use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::ui::*;
use egui::{Align, Align2, Sense, RichText, Button, Color32, FontId, Frame, Grid, Layout, TextEdit, Ui, Window, vec2};
use egui_theme::Theme;
use std::sync::Arc;
use zeus_eth::currency::{Currency, erc20::ERC20Token, native::NativeCurrency};

/// Currency direction
#[derive(Clone)]
pub enum InOrOut {
   In,
   Out,
}

impl InOrOut {
   pub fn to_string(&self) -> String {
      (match self {
         Self::In => "In",
         Self::Out => "Out",
      })
      .to_string()
   }
}

pub struct SwapUi {
   pub open: bool,

   pub currency_in: Currency,

   pub currency_out: Currency,

   pub amount_in: String,

   pub amount_out: String,
}

impl SwapUi {
   pub fn new() -> Self {
      let currency = NativeCurrency::from_chain_id(1).unwrap();
      let currency_in = Currency::from(currency);
      let currency_out = Currency::from(ERC20Token::wrapped_native_token(1));
      Self {
         open: false,
         currency_in,
         currency_out,
         amount_in: "".to_string(),
         amount_out: "".to_string(),
      }
   }

   fn amount_in(&mut self) -> &mut String {
      &mut self.amount_in
   }

   fn amount_out(&mut self) -> &mut String {
      &mut self.amount_out
   }

   /// Get the currency_in or currency_out based on the direction
   fn get_currency(&self, in_or_out: &InOrOut) -> &Currency {
      match in_or_out {
         InOrOut::In => &self.currency_in,
         InOrOut::Out => &self.currency_out,
      }
   }

   /// Replace the currency_in or currency_out based on the direction
   pub fn replace_currency(&mut self, in_or_out: &InOrOut, currency: Currency) {
      match in_or_out {
         InOrOut::In => {
            self.currency_in = currency;
         }
         InOrOut::Out => {
            self.currency_out = currency;
         }
      }
   }

   /// Give a default input currency based on the selected chain id
   pub fn default_currency_in(&mut self, id: u64) {
      let native = NativeCurrency::from_chain_id(id).unwrap_or_default();
      self.currency_in = Currency::from(native);
   }

   /// Give a default output currency based on the selected chain id
   pub fn default_currency_out(&mut self, id: u64) {
      self.currency_out = Currency::from(ERC20Token::wrapped_native_token(id));
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      icons: Arc<Icons>,
      theme: &Theme,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }
      ui.label("Swap UI");

      let chain_id = ctx.chain().id();
      let owner = ctx.current_wallet().address;
      let currencies = ctx.get_currencies(chain_id);

      let sell_text = RichText::new("Sell").size(23.0);
      let buy_text = RichText::new("Buy").size(23.0);

      let frame = theme.frame2.fill(Color32::TRANSPARENT);

      let mut open = self.open;
      Window::new("Swap_ui")
         .open(&mut open)
         .title_bar(false)
         .resizable(false)
         .movable(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered_justified(|ui| {
               ui.set_width(500.0);
               ui.set_height(550.0);

               // Tx Settings
               ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                  ui.label("Tx Settings goes here");
               });

               Grid::new("swap_ui")
                  .min_col_width(50.0)
                  .spacing((0.0, 10.0))
                  .show(ui, |ui| {
                     // Sell currency field
                     frame.clone().show(ui, |ui| {
                        ui.set_max_width(400.0);
                        ui.set_max_height(100.0);

                        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                           ui.label(sell_text);
                        });

                        self.amount_field(ui, InOrOut::In);

                        ui.scope(|ui| {
                           ui.set_max_width(30.0);
                           ui.set_max_height(20.0);

                           self.token_button(ui, icons.clone(), InOrOut::In, token_selection);
                           ui.add_space(10.0);

                           let balance = ctx.get_currency_balance(chain_id, owner, &self.currency_in);
                           ui.label(balance.formatted());
                           ui.add_space(5.0);

                           let max = RichText::new("Max").size(17.0).color(Color32::RED);
                           // TODO: on hover change the cursor to a pointer
                           if ui.label(max).clicked() {
                              *self.amount_in() = balance.wei().unwrap_or_default().to_string();
                           }
                        });
                     });

                     ui.end_row();

                     frame.show(ui, |ui| {
                        ui.set_max_width(400.0);
                        ui.set_max_height(100.0);

                        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                           ui.label(buy_text);
                        });

                        self.amount_field(ui, InOrOut::Out);

                        ui.scope(|ui| {
                           ui.set_max_width(30.0);
                           ui.set_max_height(20.0);

                           self.token_button(ui, icons.clone(), InOrOut::Out, token_selection);
                           ui.add_space(10.0);
                           let balance = ctx.get_currency_balance(chain_id, owner, &self.currency_out);
                           ui.label(balance.formatted());

                           ui.add_space(5.0);

                           let max = RichText::new("Max").size(17.0).color(Color32::RED);
                           if ui.label(max).clicked() {
                              *self.amount_out() = balance.wei().unwrap_or_default().to_string();
                           }
                        });
                     });

                     token_selection.show(ctx, theme, icons, chain_id, owner, &currencies, ui);

                     let selected_currency = token_selection.get_currency();
                     let direction = token_selection.get_currency_direction();

                     if let Some(currency) = selected_currency {
                        self.replace_currency(&direction, currency.clone());
                        token_selection.reset();
                     }
                  });
            });
         });
      self.open = open;
   }

   /// Creates the amount field
   fn amount_field(&mut self, ui: &mut Ui, in_or_out: InOrOut) {
      let font = FontId::proportional(23.0);
      let hint = RichText::new("0").size(23.0);

      let amount = match in_or_out {
         InOrOut::In => self.amount_in(),
         InOrOut::Out => self.amount_out(),
      };

      let field = TextEdit::singleline(amount)
         .font(font)
         .min_size(vec2(100.0, 30.0))
         .hint_text(hint);

      ui.add(field);
   }

   /// Create the token button
   ///
   /// If clicked it will show the [TokenSelectionWindow]
   fn token_button(
      &mut self,
      ui: &mut Ui,
      icons: Arc<Icons>,
      in_or_out: InOrOut,
      token_selection: &mut TokenSelectionWindow,
   ) {
      ui.push_id(in_or_out.to_string(), |ui| {
         let currency = self.get_currency(&in_or_out);
         let symbol_text = RichText::new(currency.symbol()).size(17.0);

         let icon = icons.currency_icon(currency);
         let button = Button::image_and_text(icon, symbol_text)
            .min_size(vec2(50.0, 25.0))
            .sense(Sense::click());

         if ui.add(button).clicked() {
            token_selection.currency_direction = in_or_out;
            token_selection.open = true;
         }
      });
   }
}
