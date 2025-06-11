pub mod pool;
pub mod position;
pub mod swap;

use pool::PoolsUi;
use position::OpenPositionUi;
use swap::SwapUi;

use egui::{
   Align, Align2, Button, FontId, Frame, Id, Layout, Margin, Order, RichText, Slider, TextEdit, Ui,
   Window, vec2,
};

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::ui::TokenSelectionWindow;
use egui_theme::Theme;
use std::sync::Arc;

#[derive(Clone)]
pub struct Settings {
   pub open: bool,
   pub size: (f32, f32),
   pub max_hops: usize,
   pub max_paths: usize,
   pub mev_protect: bool,
   pub slippage: String,
}

impl Settings {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (400.0, 250.0),
         max_hops: 2,
         max_paths: 2,
         mev_protect: true,
         slippage: "0.5".to_string(),
      }
   }

   fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;
      let frame = Frame::window(ui.style()).inner_margin(Margin::same(10));

      Window::new("")
      .id(Id::new("uniswap_settings"))
         .open(&mut open)
         .resizable(false)
         .order(Order::Foreground)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(10.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            let ui_width = ui.available_width();
            ui.vertical_centered(|ui| {

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.add_space(ui_width * 0.3);
                  let text = RichText::new("MEV Protect").size(theme.text_sizes.normal);
                  ui.label(text).on_hover_text("Protect against front-running attacks");
                  ui.checkbox(&mut self.mev_protect, "");
               });

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.add_space(ui_width * 0.3);
                  let text = RichText::new("Slippage").size(theme.text_sizes.normal);
                  ui.label(text).on_hover_text("Your transaction will revert if the price changes unfavorably by more than this percentage");
               TextEdit::singleline(&mut self.slippage)
                  .hint_text("0.5")
                  .font(FontId::proportional(theme.text_sizes.small))
                  .desired_width(25.0)
                  .background_color(theme.colors.text_edit_bg)
                  .margin(Margin::same(10))
                  .show(ui);
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.add_space(ui_width * 0.3);
               ui.label(RichText::new("Max Hops").size(theme.text_sizes.normal));
               ui.add(Slider::new(&mut self.max_hops, 1..=10));
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.add_space(ui_width * 0.3);
               ui.label(RichText::new("Max Paths").size(theme.text_sizes.normal));
               ui.add(Slider::new(&mut self.max_paths, 1..=10));
            });
         });
         });

      self.open = open;
   }
}

pub struct UniswapUi {
   pub open: bool,
   pub size: (f32, f32),
   pub settings: Settings,
   pub swap_ui: SwapUi,
   pub pools_ui: PoolsUi,
   pub open_position_ui: OpenPositionUi,
}

impl UniswapUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (500.0, 900.0),
         settings: Settings::new(),
         swap_ui: SwapUi::new(),
         pools_ui: PoolsUi::new(),
         open_position_ui: OpenPositionUi::new(),
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

      // ui.add_space(50.0);

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         // Header
         ui.horizontal(|ui| {
            // Swap - Pool - Position Buttons
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.spacing_mut().item_spacing.x = 10.0;

               let text = RichText::new("Swap").size(theme.text_sizes.large);
               let swap_button = Button::new(text);
               if ui.add(swap_button).clicked() {
                  self.swap_ui.open = true;
                  self.pools_ui.open = false;
                  self.open_position_ui.open = false;
               }

               let text = RichText::new("Pools").size(theme.text_sizes.large);
               let pools_button = Button::new(text);
               if ui.add(pools_button).clicked() {
                  self.pools_ui.open = true;
                  self.swap_ui.open = false;
                  self.open_position_ui.open = false;
               }

               let text = RichText::new("Positions").size(theme.text_sizes.large);
               let positions_button = Button::new(text);
               if ui.add(positions_button).clicked() {
                  self.open_position_ui.open = true;
                  self.swap_ui.open = false;
                  self.pools_ui.open = false;
               }
            });

            // Settings Button
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let button = Button::new(RichText::new("Settings").size(theme.text_sizes.normal));
               if ui.add(button).clicked() {
                  self.settings.open = true;
               }
            });
         });

         ui.add_space(40.0);

         self.settings.show(theme, ui);

         self.swap_ui.show(
            ctx.clone(),
            theme,
            icons.clone(),
            token_selection,
            &self.settings,
            ui,
         );

         self.pools_ui.show(ctx.clone(), theme, icons.clone(), ui);

         self.open_position_ui.show(
            ctx.clone(),
            theme,
            icons.clone(),
            token_selection,
            &self.settings,
            ui,
         );
      });
   }
}
