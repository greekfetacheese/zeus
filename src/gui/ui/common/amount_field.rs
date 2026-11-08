//! An amount field with an optional currency selector and customizable balance & max amount logic.

use crate::assets::icons::Icons;
use crate::gui::ui::token_selection::{InOrOut, TokenSelectionWindow};
use egui::{Align, Color32, FontId, Layout, Margin, Response, RichText, Ui, vec2};
use std::sync::Arc;
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::Currency,
   utils::NumericValue,
};

use egui_elements::layout::{CrossAlign, Justify};
use egui_elements::{Button, Flex, Item, Label, SecureTextEdit, Slider, Spinner, Theme};
use egui_lucide::Lucide;

/// Arguments for [`AmountField::show`].
///
/// Build with [`AmountFieldParams::new`] then chain the values that differ per call site.
pub struct AmountFieldParams<'a> {
   pub theme: &'a Theme,
   pub icons: Arc<Icons>,
   pub currency: &'a Currency,
   pub owner: Address,
   pub chain_id: u64,
   pub privacy_mode: bool,
   pub balance: NumericValue,
   /// Used when the slider is at 100%. Zero is fine when [`Self::show_slider`] is false.
   pub max_amount: NumericValue,
   pub value: NumericValue,
   pub label: Option<&'a str>,
   pub token_selection: Option<&'a mut TokenSelectionWindow>,
   pub direction: Option<InOrOut>,
   pub loading: bool,
   pub show_slider: bool,
}

impl<'a> AmountFieldParams<'a> {
   pub fn new(
      theme: &'a Theme,
      icons: Arc<Icons>,
      currency: &'a Currency,
      owner: Address,
      chain_id: u64,
   ) -> Self {
      Self {
         theme,
         icons,
         currency,
         owner,
         chain_id,
         privacy_mode: false,
         balance: NumericValue::default(),
         max_amount: NumericValue::default(),
         value: NumericValue::default(),
         label: None,
         token_selection: None,
         direction: None,
         loading: false,
         show_slider: false,
      }
   }

   pub fn privacy_mode(mut self, privacy_mode: bool) -> Self {
      self.privacy_mode = privacy_mode;
      self
   }

   pub fn balance(mut self, balance: NumericValue) -> Self {
      self.balance = balance;
      self
   }

   pub fn max_amount(mut self, max_amount: NumericValue) -> Self {
      self.max_amount = max_amount;
      self
   }

   pub fn value(mut self, value: NumericValue) -> Self {
      self.value = value;
      self
   }

   pub fn label(mut self, label: &'a str) -> Self {
      self.label = Some(label);
      self
   }

   pub fn token_selection(
      mut self,
      token_selection: &'a mut TokenSelectionWindow,
      direction: Option<InOrOut>,
   ) -> Self {
      self.token_selection = Some(token_selection);
      self.direction = direction;
      self
   }

   pub fn loading(mut self, loading: bool) -> Self {
      self.loading = loading;
      self
   }

   pub fn show_slider(mut self, show_slider: bool) -> Self {
      self.show_slider = show_slider;
      self
   }
}

/// An amount field with an optional currency selector and customizable balance & max amount logic.
pub struct AmountField {
   /// The selected amount % from the slider
   pub amount_percent: f64,
   /// The amount in String
   pub amount: String,
   /// The amount in Wei
   pub amount_wei: U256,
}

impl AmountField {
   pub fn new() -> Self {
      Self {
         amount_percent: 0.0,
         amount: String::new(),
         amount_wei: U256::ZERO,
      }
   }

   pub fn reset(&mut self) {
      *self = Self::new();
   }

   /// Draw the amount field.
   ///
   /// Returns whether the amount changed this frame.
   pub fn show(&mut self, params: AmountFieldParams<'_>, ui: &mut Ui) -> bool {
      let AmountFieldParams {
         theme,
         icons,
         currency,
         owner,
         chain_id,
         privacy_mode,
         balance,
         max_amount,
         value,
         label,
         token_selection,
         direction,
         loading,
         show_slider,
      } = params;

      let mut amount_changed = false;
      let tint = theme.image_tint_recommended;

      ui.vertical(|ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

         ui.horizontal(|ui| {
            if let Some(label) = label {
               ui.label(RichText::new(label).size(theme.typography.large).color(theme.colors.text));
            }

            if loading {
               ui.add(Spinner::new().size(13.0).color(theme.colors.text));
            }

            if show_slider {
               let (slider, _) = percent_slider(ui, theme, &mut self.amount_percent);

               if slider.changed() {
                  if self.amount_percent == 100.0 {
                     self.amount = max_amount.f64().to_string();
                     self.amount_wei = max_amount.wei();
                  } else {
                     let new_amount =
                        balance.calc_percent(self.amount_percent, currency.decimals());
                     self.amount = new_amount.f64().to_string();
                     self.amount_wei = new_amount.wei();
                  }
                  amount_changed = true;
               }
            }
         });

         ui.horizontal(|ui| {
            ui.vertical(|ui| {
               let visuals = theme.text_edit_visuals();
               let hint =
                  RichText::new("0").color(theme.colors.text_muted).size(theme.typography.heading);

               let amount_input = SecureTextEdit::singleline(&mut self.amount)
                  .visuals(visuals)
                  .font(FontId::proportional(theme.typography.heading))
                  .hint_text(hint)
                  .margin(Margin::same(10))
                  .desired_width(ui.available_width() * 0.6)
                  .min_size(vec2(0.0, 50.0));

               let res = ui.add(amount_input);
               if res.changed() {
                  amount_changed = true;
                  let new_amount_wei =
                     NumericValue::parse_to_wei(&self.amount, currency.decimals());
                  self.amount_wei = new_amount_wei.wei();
               }

               ui.label(
                  RichText::new(format!("${:.10}", value.abbreviated())).size(theme.typography.normal),
               );
            });

            ui.add_space(10.0);

            ui.vertical(|ui| {
               let visuals = theme.button_visuals();
               let icon = icons.currency_icon_x32(currency, tint);
               let button_text = RichText::new(currency.symbol()).size(theme.typography.normal);
               let width = ui.available_width() * 0.5;
               let button = Button::image_and_text(icon, button_text)
                  .visuals(visuals)
                  .min_size(vec2(width, 40.0));

               if ui.add(button).clicked()
                  && let Some(token_selection) = token_selection
               {
                  token_selection.open(privacy_mode, chain_id, owner);
                  if let Some(direction) = direction {
                     token_selection.currency_direction = direction;
                  }
               }

               let icon = Lucide::WalletMinimal.size(17.0).color(theme.colors.text).image();

               let text = RichText::new(format!("{:.10}", balance.abbreviated()))
                  .size(theme.typography.normal)
                  .color(theme.colors.text);
               let label = Label::new(text, Some(icon)).interactive(false);

               ui.add(label);
            });
         });
      });
      amount_changed
   }
}

/// The width the row reserves for the percent label: the widest label it can show ("100%").
///
/// The row is right-aligned, so the label's width decides where the slider starts: a label that
/// grows from "9%" to "10%" to "100%" would drag the slider left mid-drag. egui has no `ch` unit,
/// so the width comes from the theme's font.
fn percent_width(ui: &Ui, theme: &Theme) -> f32 {
   let font = FontId::proportional(theme.typography.normal);

   ui.fonts_mut(|fonts| {
      fonts
         .layout_no_wrap("100%".to_owned(), font, Color32::PLACEHOLDER)
         .size()
         .x
   })
}

/// Draw the amount percent, right-aligned in the cell [`percent_slider`] reserves for it.
fn percent_label(ui: &mut Ui, theme: &Theme, amount_percent: f64) -> Response {
   let text = RichText::new(format!("{amount_percent:.0}%")).size(theme.typography.normal);

   ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(text))
      .inner
}

/// Draw the percent slider and its label as one row, right-aligned and cross-aligned.
///
/// Returns the slider's and the label's responses.
fn percent_slider(ui: &mut Ui, theme: &Theme, amount_percent: &mut f64) -> (Response, Response) {
   let label_width = percent_width(ui, theme);
   let slider_width = ui.spacing().slider_width;

   Flex::row(ui.auto_id_with("amount_percent"))
      .gap(5.0)
      .justify(Justify::End)
      .align_items(CrossAlign::Center)
      .show(ui, |flex| {
         let slider = flex.item(Item::fixed(slider_width), |ui| {
            ui.add(
               Slider::new(amount_percent, 0.0..=100.0)
                  .step_by(1.0)
                  .show_value(false)
                  .width(slider_width),
            )
         });

         let label = flex.item(Item::fixed(label_width), |ui| {
            percent_label(ui, theme, *amount_percent)
         });

         (slider.inner, label.inner)
      })
      .inner
}

#[cfg(test)]
mod tests {
   use super::*;

   /// The percent label grows from "9%" to "10%" to "100%". A fixed-width label cell keeps the
   /// slider's x independent of the digit count, and the flex row centres the label on the
   /// slider's cross axis: the built-in horizontal layout only centres against `interact_size.y`
   /// (20 points), so it leaves the short label high above the taller slider.
   ///
   /// A flex row measures its items on the first frame and places them on the next pass, so the
   /// test reads the geometry after a few frames.
   #[test]
   fn the_percent_label_does_not_move_the_slider() {
      let ctx = egui::Context::default();
      let mut rows = [(egui::Rect::NOTHING, egui::Rect::NOTHING); 2];

      for _ in 0..3 {
         let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 240.0))),
            ..Default::default()
         };

         let mut output = ctx.run_ui(input, |ui| {
            let theme = Theme::current(ui.ctx());
            let mut one_digit = 9.0;
            let mut three_digits = 100.0;

            let (slider, label) = percent_slider(ui, &theme, &mut one_digit);
            rows[0] = (slider.rect, label.rect);

            let (slider, label) = percent_slider(ui, &theme, &mut three_digits);
            rows[1] = (slider.rect, label.rect);
         });

         output.textures_delta.clear();
      }

      let (one_digit_slider, one_digit_label) = rows[0];
      let (three_digit_slider, three_digit_label) = rows[1];

      assert_eq!(
         one_digit_slider.left(),
         three_digit_slider.left(),
         "the slider must not move when the percent label gains a digit"
      );
      for (label, slider) in [
         (one_digit_label, one_digit_slider),
         (three_digit_label, three_digit_slider),
      ] {
         let off = (label.center().y - slider.center().y).abs();
         assert!(
            off < 0.5,
            "the label must be centred on the slider's cross axis, off by {off}"
         );
      }
   }
}
