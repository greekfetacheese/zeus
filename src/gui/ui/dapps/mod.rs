pub mod across;
pub mod uniswap;

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::ui::dapps::uniswap::swap::InOrOut;
use crate::gui::ui::token_selection::TokenSelectionWindow;
use egui::{Align, Button, Color32, FontId, Layout, Margin, RichText, Spinner, TextEdit, Ui, vec2};
use egui_theme::Theme;
use egui_widgets::Label;
use std::sync::Arc;
use zeus_eth::{currency::Currency, utils::NumericValue};

/// Helper function to draw an amount field with optional currency selector and customizable balance/max logic.
///
/// Arguments:
/// - `ctx`: The Zeus context.
/// - `theme`: The current theme.
/// - `icons`: Shared icons.
/// - `label`: Optional text to display as a label above the amount field (e.g., "Sell" or "Buy").
/// - `currency`: The selected currency.
/// - `amount`: Mutable reference to the amount string.
/// - `token_selection`: Optional mutable reference to the token selection window for currency selection.
/// - `direction`: Optional direction to set on the token selection if provided.
/// - `get_balance`: Closure to compute the balance of the owner.
/// - `get_max_amount`: Closure to compute the max amount as NumericValue (Set to zero if no max button should be shown).
/// - `get_value`: Closure to compute the value of the currency amount.
/// - `loading`: Indicates a loading state for the UI.
/// - `ui`: Mutable reference to the UI.
///
/// Returns: If the amount field was changed.
pub fn amount_field_with_currency_selector(
   _ctx: ZeusCtx,
   theme: &Theme,
   icons: Arc<Icons>,
   label: Option<String>,
   currency: &Currency,
   amount: &mut String,
   token_selection: Option<&mut TokenSelectionWindow>,
   direction: Option<InOrOut>,
   get_balance: impl FnOnce() -> NumericValue,
   get_max_amount: impl FnOnce() -> NumericValue,
   get_value: impl FnOnce() -> NumericValue,
   loading: bool,
   ui: &mut Ui,
) -> bool {
   let mut amount_changed = false;
   let tint = theme.image_tint_recommended;

   ui.vertical(|ui| {
      ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

      ui.horizontal(|ui| {
         // Optional label
         if let Some(label) = label {
            ui.label(RichText::new(label).size(theme.text_sizes.large).color(theme.colors.text));
         }

         if loading {
            ui.add(Spinner::new().size(13.0).color(Color32::WHITE));
         }

         // Balance and Max Button
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            // Max button
            let max_amount = get_max_amount();
            if !max_amount.is_zero() {
               ui.spacing_mut().button_padding = vec2(2.0, 2.0);

               let max_text = RichText::new("Max").size(theme.text_sizes.small);
               let max_button = Button::new(max_text);

               if ui.add(max_button).clicked() {
                  *amount = max_amount.flatten();
                  amount_changed = true;
               }
            }

            ui.add_space(5.0);

            // Balance
            let wallet_icon = match theme.dark_mode {
               true => icons.wallet_light(tint),
               _ => icons.wallet_dark(),
            };

            let balance = get_balance();
            let text = RichText::new(balance.format_abbreviated())
               .size(theme.text_sizes.normal)
               .color(theme.colors.text);
            let label = Label::new(text, Some(wallet_icon));

            ui.add(label);
         });
      });

      // Amount input
      ui.horizontal(|ui| {
         let amount_input = TextEdit::singleline(amount)
            .font(FontId::proportional(theme.text_sizes.heading))
            .hint_text(RichText::new("0").color(theme.colors.text_muted))
            .margin(Margin::same(10))
            .desired_width(ui.available_width() * 0.6)
            .min_size(vec2(0.0, 50.0));

         let res = ui.add(amount_input);
         if res.changed() {
            amount_changed = true;
         }

         ui.add_space(10.0);

         // Currency Selector Button
         let icon = icons.currency_icon(currency, tint);
         let button_text = RichText::new(currency.symbol()).size(theme.text_sizes.normal);
         let width = ui.available_width() * 0.5;
         let button = Button::image_and_text(icon, button_text).min_size(vec2(width, 40.0));

         if ui.add(button).clicked() {
            if let Some(token_selection) = token_selection {
               token_selection.open = true;
               if let Some(direction) = direction {
                  token_selection.currency_direction = direction;
               }
            }
         }
      });

      // USD Value
      ui.horizontal(|ui| {
         let value = get_value();
         ui.label(
            RichText::new(format!("${}", value.format_abbreviated())).size(theme.text_sizes.normal),
         );
      });
   });
   amount_changed
}
