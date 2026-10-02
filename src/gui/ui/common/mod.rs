//! Common UI components

pub mod amount_field;
pub mod chain_select;
pub mod chain_switch;
pub mod fade;
pub mod wallet_identity;
pub mod wallet_list;
pub mod wallet_select;
pub mod window_frame;
pub mod windows;

pub use amount_field::{AmountField, AmountFieldParams};
pub use chain_select::ChainSelect;
pub use chain_switch::switch_chain;
pub use fade::{panel_fade, show_with_fade};
pub use wallet_identity::wallet_identity;
pub use wallet_list::WalletListByValue;
pub use wallet_select::WalletSelect;
pub use window_frame::{WindowCtx, window_frame};
pub use windows::{ConfirmWindow, LoadingWindow, MsgWindow, UpdateWindow};

use crate::core::ZeusContext;
use crate::gui::{SHARED_GUI, ui::dapps::railgun::RailgunMode};
use crate::utils::RT;
use egui::{Align, Layout, Response, RichText, Ui, pos2, Vec2, vec2};
use egui_elements::{Button, Theme};
use egui_lucide::Lucide;
use elegance::{Accent, Switch};
use std::time::{Duration, Instant};

/// Delay before Sign/Confirm is clickable after a prompt is brought to the front.
pub const ACTION_UNLOCK_DELAY: Duration = Duration::from_secs(2);

/// `(enabled, label)` for Sign/Confirm. `None` means no delay (in-app Zeus prompts).
pub fn delayed_action_label(opened_at: Option<Instant>, ready_label: &str) -> (bool, String) {
   let Some(opened_at) = opened_at else {
      return (true, ready_label.to_string());
   };
   let remaining = ACTION_UNLOCK_DELAY.saturating_sub(opened_at.elapsed());
   if remaining.is_zero() {
      (true, ready_label.to_string())
   } else {
      let secs = remaining.as_secs_f32().ceil() as u64;
      (false, format!("Available in {secs}s"))
   }
}

pub fn privacy_mode_switch(ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
   let text = match ctx.privacy_mode {
      true => "Privacy mode",
      false => "Public mode",
   };

   let icon = match ctx.privacy_mode {
      true => Lucide::EyeOff.size(20.0).color(theme.colors.text).image(),
      false => Lucide::Eye.size(20.0).color(theme.colors.text).image(),
   };

   let rich_text = RichText::new(text).size(theme.typography.normal);

   let switch = Switch::new(&mut ctx.privacy_mode, rich_text).accent(Accent::Green);

   let size = vec2(150.0, 20.0);
   let mut clicked = false;

   ui.allocate_ui(size, |ui| {
      ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
         clicked = ui.add(switch).clicked();
         ui.add_space(3.0);
         ui.add(icon);
      });
   });

   if clicked {
      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let chain = ctx.chain();
         let owner = ctx.current_wallet_info().address;
         let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);

         let new_mode = match privacy_mode {
            false => RailgunMode::Shield,
            true => RailgunMode::Unshield,
         };

         SHARED_GUI.write(|gui| {
            gui.shield_ui.set_mode(new_mode);
            gui.shield_ui.default_currency(chain.id());
            gui.send_crypto.default_currency(privacy_mode, chain.id());
            gui.token_selection.process_currencies(privacy_mode, chain.id(), owner);
            gui.wallet_ui.calc_wallet_value();
            gui.recipient_selection.calc_wallet_value();
         });
      });
   }
}

pub fn dots_button(theme: &Theme, size: Vec2, ui: &mut Ui) -> Response {
   let visuals = theme.button_visuals();
   let btn = Button::new("").small().min_size(size).visuals(visuals);

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
