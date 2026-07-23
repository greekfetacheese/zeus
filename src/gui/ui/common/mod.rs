//! Common UI components

pub mod amount_field;
pub mod chain_select;
pub mod wallet_select;
pub mod windows;

pub use amount_field::AmountField;
pub use chain_select::ChainSelect;
pub use wallet_select::WalletSelect;
pub use windows::{ConfirmWindow, LoadingWindow, MsgWindow, UpdateWindow};

use egui::{Response, Ui, pos2, vec2};
use zeus_theme::Theme;
use zeus_widgets::Button;

pub fn dots_button(theme: &Theme, ui: &mut Ui) -> Response {
   let visuals = theme.button_visuals();
   let btn = Button::new("").small().min_size(vec2(28.0, 20.0)).visuals(visuals);

   let resp = ui.add(btn);

   if ui.is_rect_visible(resp.rect) {
      let color = if resp.hovered() {
         visuals.border_hover.color
      } else {
         theme.colors.text
      };

      let center = resp.rect.center();
      let spacing = 4.0;
      let radius = 1.4;
      for dx in [-spacing, 0.0, spacing] {
         ui.painter().circle_filled(pos2(center.x + dx, center.y), radius, color);
      }
   }
   resp
}
