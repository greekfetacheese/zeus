pub mod across;
pub mod uniswap;

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::ui::dapps::uniswap::swap::InOrOut;
use crate::gui::ui::token_selection::TokenSelectionWindow;
use egui::{
   Align, Button, Color32, FontId, Layout, Margin, RichText, Slider, Spinner, TextEdit, Ui, vec2,
};
use zeus_widgets::Label;
use std::sync::Arc;
use zeus_eth::{alloy_primitives::Address, currency::Currency, utils::NumericValue};
use zeus_theme::Theme;

pub struct AmountFieldWithCurrencySelect {
   pub amount_percent: f64,
   pub amount: String,
}

impl AmountFieldWithCurrencySelect {
   pub fn new() -> Self {
      Self {
         amount_percent: 0.0,
         amount: String::new(),
      }
   }

   pub fn reset(&mut self) {
      *self = Self::new();
   }

   /// Helper function to draw an amount field with optional currency selector and customizable balance/max logic.
   ///
   /// Arguments:
   /// - `ctx`: The Zeus context.
   /// - `theme`: The current theme.
   /// - `icons`: Shared icons.
   /// - `label`: Optional text to display as a label above the amount field (e.g., "Sell" or "Buy").
   /// - `owner`: The address of the owner whose balance is to be displayed.
   /// - `currency`: The selected currency.
   /// - `token_selection`: Optional mutable reference to the token selection window for currency selection.
   /// - `direction`: Optional direction to set on the token selection if provided.
   /// - `get_balance`: Closure to compute the balance of the owner.
   /// - `get_max_amount`: Closure to compute the max amount as NumericValue (Set to zero if no max button should be shown).
   /// - `get_value`: Closure to compute the value of the currency amount.
   /// - `loading`: Indicates a loading state for the UI.
   /// - `show_slider`: Show a slider to adjust the amount.
   /// - `ui`: Mutable reference to the UI.
   ///
   /// Returns: If the amount field was changed.
   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      label: Option<String>,
      owner: Address,
      currency: &Currency,
      token_selection: Option<&mut TokenSelectionWindow>,
      direction: Option<InOrOut>,
      get_balance: impl FnOnce() -> NumericValue,
      get_max_amount: impl FnOnce() -> NumericValue,
      get_value: impl FnOnce() -> NumericValue,
      loading: bool,
      show_slider: bool,
      ui: &mut Ui,
   ) -> bool {
      let mut amount_changed = false;
      let tint = theme.image_tint_recommended;

      let balance = get_balance();

      ui.vertical(|ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

         ui.horizontal(|ui| {
            // Optional label
            if let Some(label) = label {
               ui.label(RichText::new(label).size(theme.text_sizes.large).color(theme.colors.text));
            }

            if loading {
               ui.add(Spinner::new().size(13.0).color(Color32::WHITE));
            }

            // Amount % slider
            if show_slider {
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let amount_percent = self.amount_percent;

                  let text =
                     RichText::new(format!("{}%", amount_percent)).size(theme.text_sizes.normal);

                  ui.label(text);

                  ui.add_space(5.0);

                  let res = ui.horizontal(|ui| {
                     ui.add(Slider::new(&mut self.amount_percent, 0.0..=100.0).show_value(false))
                  });

                  if res.inner.changed() {
                     if self.amount_percent == 100.0 {
                        self.amount = get_max_amount().f64().to_string();
                        amount_changed = true;
                     } else {
                        let new_amount =
                           balance.calc_percent(self.amount_percent, currency.decimals());
                        self.amount = new_amount.f64().to_string();
                        amount_changed = true;
                     }
                  }
               });
            }
         });

         // Amount input
         ui.horizontal(|ui| {
            ui.vertical(|ui| {
               let amount_input = TextEdit::singleline(&mut self.amount)
                  .font(FontId::proportional(theme.text_sizes.heading))
                  .hint_text(RichText::new("0").color(theme.colors.text_muted))
                  .margin(Margin::same(10))
                  .desired_width(ui.available_width() * 0.6)
                  .min_size(vec2(0.0, 50.0));

               let res = ui.add(amount_input);
               if res.changed() {
                  amount_changed = true;
               }

               // USD Value
               let value = get_value();
               ui.label(
                  RichText::new(format!("${}", value.abbreviated())).size(theme.text_sizes.normal),
               );
            });

            ui.add_space(10.0);

            ui.vertical(|ui| {
               // Currency Selector Button

               let icon = icons.currency_icon(currency, tint);
               let button_text = RichText::new(currency.symbol()).size(theme.text_sizes.normal);
               let width = ui.available_width() * 0.5;
               let button = Button::image_and_text(icon, button_text).min_size(vec2(width, 40.0));

               if ui.add(button).clicked() {
                  if let Some(token_selection) = token_selection {
                     token_selection.open(ctx.clone(), currency.chain_id(), owner);
                     if let Some(direction) = direction {
                        token_selection.currency_direction = direction;
                     }
                  }
               }

               // Balance
               let wallet_icon = match theme.dark_mode {
                  true => icons.wallet_light(tint),
                  _ => icons.wallet_dark(),
               };

               let text = RichText::new(format!("{:.12}", balance.abbreviated()))
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.text);
               let label = Label::new(text, Some(wallet_icon));

               ui.add(label);
            });
         });
      });
      amount_changed
   }
}
