use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::ui::*;
use egui::{
   Align, Align2, Button, Color32, FontId, Frame, Layout, Margin, RichText, TextEdit, Ui, Window,
   vec2,
};
use egui_theme::{Theme, utils::widget_visuals};
use std::sync::Arc;
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, erc20::ERC20Token, native::NativeCurrency},
};

/// Currency direction
#[derive(Copy, Clone, PartialEq)]
pub enum InOrOut {
   In,
   Out,
}

impl InOrOut {
   pub fn to_string(&self) -> String {
      (match self {
         Self::In => "Sell",
         Self::Out => "Buy",
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
      let currency = NativeCurrency::from(1);
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
      let native = NativeCurrency::from(id);
      self.currency_in = Currency::from(native);
   }

   /// Give a default output currency based on the selected chain id
   pub fn default_currency_out(&mut self, id: u64) {
      self.currency_out = Currency::from(ERC20Token::wrapped_native_token(id));
   }

   fn swap_currencies(&mut self) {
      std::mem::swap(&mut self.currency_in, &mut self.currency_out);
      std::mem::swap(&mut self.amount_in, &mut self.amount_out);
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

      let chain_id = ctx.chain().id();
      let owner = ctx.current_wallet().address;
      let currencies = ctx.get_currencies(chain_id);

      let mut open = self.open;
      let window_frame = Frame::new().inner_margin(Margin::symmetric(15, 20));

      Window::new("Swap")
         .open(&mut open)
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.vertical(|ui| {
               ui.set_width(450.0);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);

               // --- Sell Section ---
               Self::swap_section(
                  ui,
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  token_selection,
                  InOrOut::In,
                  &mut self.amount_in,
                  &self.currency_in,
                  chain_id,
                  owner,
               );

               // --- Swap Currencies ---
               ui.add_space(5.0);
               ui.vertical_centered(|ui| {
                  let swap_button =
                     Button::new(RichText::new("🡫").size(theme.text_sizes.large).strong())
                        .min_size(vec2(40.0, 40.0));

                  if ui.add(swap_button).clicked() {
                     self.swap_currencies();
                  }
               });
               ui.add_space(5.0);

               // --- Buy Section ---
               Self::swap_section(
                  ui,
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  token_selection,
                  InOrOut::Out,
                  &mut self.amount_out,
                  &self.currency_out,
                  chain_id,
                  owner,
               );

               let swap_button = Button::new(
                  RichText::new("Swap")
                     .size(theme.text_sizes.large)
                     .color(theme.colors.text_color),
               )
               .min_size(vec2(ui.available_width() * 0.8, 45.0));

               let valid = self.valid_amounts();

               ui.vertical_centered(|ui| {
                  if ui.add_enabled(valid, swap_button).clicked() {
                     println!(
                        "Initiate Swap: {} {} for {}",
                        self.amount_in,
                        self.currency_in.symbol(),
                        self.currency_out.symbol()
                     );
                  }
               });
            });

            token_selection.show(
               ctx.clone(),
               theme,
               icons,
               chain_id,
               owner,
               &currencies,
               ui,
            );

            let selected_currency = token_selection.get_currency();
            let direction = token_selection.get_currency_direction();

            if let Some(currency) = selected_currency {
               self.replace_currency(&direction, currency.clone());

               token_selection.reset();
            }
         });

      self.open = open;
   }

   /// Helper function to draw one section (Sell or Buy) of the swap UI
   fn swap_section(
      ui: &mut Ui,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      direction: InOrOut,
      amount_str: &mut String,
      currency: &Currency,
      chain_id: u64,
      owner: Address,
   ) {
      let frame = theme.frame1;
      let _frame_bg_color = frame.fill;

      frame.show(ui, |ui| {
         ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 8.0);
            ui.horizontal(|ui| {
               ui.label(
                  RichText::new(direction.to_string())
                     .size(theme.text_sizes.large)
                     .color(theme.colors.text_secondary),
               );
            });

            ui.horizontal(|ui| {
               let amount_input = TextEdit::singleline(amount_str)
                  .font(FontId::proportional(theme.text_sizes.heading))
                  .hint_text(RichText::new("0").color(theme.colors.text_secondary))
                  .background_color(theme.colors.text_edit_bg2)
                  .margin(Margin::same(10))
                  .desired_width(ui.available_width() * 0.6)
                  .min_size(vec2(0.0, 50.0));

               ui.add(amount_input);

               // Currency Selector Button
               ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                  let icon = icons.currency_icon(currency);
                  let button_text = RichText::new(currency.symbol()).size(theme.text_sizes.normal);

                  let button =
                     Button::image_and_text(icon, button_text).min_size(vec2(100.0, 40.0));

                  if ui.add(button).clicked() {
                     token_selection.currency_direction = direction.clone();
                     token_selection.open = true;
                  }
               });
            });

            // Balance and Max Button
            ui.horizontal(|ui| {
               let balance = ctx.get_currency_balance(chain_id, owner, currency);
               let balance_text = format!("Balance: {}", balance.formatted());
               ui.label(
                  RichText::new(balance_text)
                     .size(theme.text_sizes.normal)
                     .color(theme.colors.text_secondary),
               );

               // Max button
               let max_text = RichText::new("Max").size(theme.text_sizes.small);
               let max_button = Button::new(max_text).min_size(vec2(40.0, 20.0));

               if direction == InOrOut::In {
                  if ui.add(max_button).clicked() {
                     *amount_str = balance.formatted().clone();
                  }
               }
            });
         });
      });
   }

   fn valid_amounts(&self) -> bool {
      let amount_in = self.amount_in.parse().unwrap_or(0.0);
      let amount_out = self.amount_out.parse().unwrap_or(0.0);
      amount_in > 0.0 && amount_out > 0.0
   }
}
