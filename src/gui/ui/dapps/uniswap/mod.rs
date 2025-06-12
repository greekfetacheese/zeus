pub mod pool;
pub mod position;
pub mod swap;

use pool::PoolsUi;
use position::OpenPositionUi;
use swap::SwapUi;

use egui::{Align, Button, FontId, Layout, Margin, RichText, Slider, TextEdit, Ui, vec2};

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::ui::TokenSelectionWindow;
use egui_theme::Theme;
use std::sync::Arc;

#[derive(Clone, Default, Copy, Debug, PartialEq)]
pub enum ProtocolVersion {
   V2,
   #[default]
   V3,
}

impl ProtocolVersion {
   pub fn is_v2(&self) -> bool {
      matches!(self, Self::V2)
   }

   pub fn is_v3(&self) -> bool {
      matches!(self, Self::V3)
   }

   pub fn to_str(&self) -> &'static str {
      match self {
         ProtocolVersion::V2 => "V2",
         ProtocolVersion::V3 => "V3",
      }
   }

   pub fn all() -> Vec<Self> {
      vec![ProtocolVersion::V2, ProtocolVersion::V3]
   }
}

#[derive(Clone)]
pub struct UniswapSettingsUi {
   open: bool,
   pub max_hops: usize,
   pub max_paths: usize,
   pub mev_protect: bool,
   pub slippage: String,
   /// Applies only to [SwapUi]
   pub simulate_mode: bool,
}

impl UniswapSettingsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         max_hops: 2,
         max_paths: 2,
         mev_protect: true,
         slippage: "0.5".to_string(),
         simulate_mode: false,
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn show(&mut self, swap_ui_open: bool, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.spacing_mut().item_spacing = vec2(5.0, 15.0);

      // For some fucking reason cargo fmt doesnt work here
      ui.vertical_centered(|ui| {

                  let text = RichText::new("MEV Protect").size(theme.text_sizes.normal);
                  ui.label(text).on_hover_text("Protect against front-running attacks");
                  ui.checkbox(&mut self.mev_protect, "");

                  let text = RichText::new("Slippage").size(theme.text_sizes.normal);
                  ui.label(text).on_hover_text("Your transaction will revert if the price changes unfavorably by more than this percentage");

               TextEdit::singleline(&mut self.slippage)
                  .hint_text("0.5")
                  .font(FontId::proportional(theme.text_sizes.small))
                  .desired_width(25.0)
                  .background_color(theme.colors.text_edit_bg)
                  .margin(Margin::same(10))
                  .show(ui);

               ui.label(RichText::new("Max Hops").size(theme.text_sizes.normal));
               ui.add(Slider::new(&mut self.max_hops, 1..=10));

               ui.label(RichText::new("Max Paths").size(theme.text_sizes.normal));
               ui.add(Slider::new(&mut self.max_paths, 1..=10));

               if swap_ui_open {
                  let text = RichText::new("Simulate Mode").size(theme.text_sizes.normal);
                  ui.label(text);
                  ui.checkbox(&mut self.simulate_mode, "");
               }
         });
   }
}

pub struct UniswapUi {
   open: bool,
   pub size: (f32, f32),
   pub settings: UniswapSettingsUi,
   pub swap_ui: SwapUi,
   pub pools_ui: PoolsUi,
   pub open_position_ui: OpenPositionUi,
}

impl UniswapUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (500.0, 900.0),
         settings: UniswapSettingsUi::new(),
         swap_ui: SwapUi::new(),
         pools_ui: PoolsUi::new(),
         open_position_ui: OpenPositionUi::new(),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
      self.settings.open();
   }

   pub fn close(&mut self) {
      self.open = false;
      self.settings.close();
   }

   pub fn is_open(&self) -> bool {
      self.open
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
         });

         ui.add_space(40.0);

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
