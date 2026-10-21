//! Common UI components

pub mod amount_field;
pub mod chain_select;
pub mod chain_switch;
pub mod fade;
pub mod recipient_field;
pub mod wallet_identity;
pub mod wallet_list;
pub mod wallet_select;
pub mod window_frame;
pub mod windows;

pub use amount_field::{AmountField, AmountFieldParams};
pub use chain_select::ChainSelect;
pub use chain_switch::switch_chain;
pub use fade::{panel_fade, show_with_fade};
pub use recipient_field::recipient_field;
pub use wallet_identity::wallet_identity;
pub use wallet_list::WalletListByValue;
pub use wallet_select::WalletSelect;
pub use window_frame::{WindowCtx, window_frame};
pub use windows::{ConfirmWindow, ConfirmWindowClaim, LoadingWindow, MsgWindow, UpdateWindow};

use crate::core::ZeusContext;
use crate::gui::{SHARED_GUI, ui::dapps::railgun::RailgunMode};
use crate::utils::TimeStamp;
use egui::{Align, Layout, Response, RichText, Ui, Vec2, pos2, vec2};
use egui_elements::{Button, Theme};
use egui_lucide::Lucide;
use elegance::{Accent, Switch};
use std::time::{Duration, Instant};
use zeus_eth::types::ChainId;

/// The chain a stored id names, when Zeus has an entry for it.
///
/// Rows rebuilt from the sealed stores — `tx_history.db`, the Railgun spent-note db, the approval
/// manager — carry whatever chain id was persisted, and nothing re-validates it on load. Everything
/// that needs a real [`ChainId`] for such a row asks here first instead of converting it: a chain
/// Zeus knows comes back, and one it has no entry for is `None` — there is no explorer, no contract
/// address book, no fork spec, and `ChainId::from` would panic. The caller names the refused id in
/// its own message, so the row can still say which chain it was on.
pub fn usable_chain(id: u64) -> Option<ChainId> {
   ChainId::new(id).ok()
}

/// The name to draw for a chain id that came out of a store.
///
/// A chain Zeus knows is named as usual; one it has no entry for is labelled with its own id, so a
/// row can still say what it was, rather than aborting the whole wallet. See [`usable_chain`] for
/// the same split where an action needs the chain itself.
pub fn chain_label(id: u64) -> String {
   match usable_chain(id) {
      Some(chain) => chain.name().to_string(),
      None => format!("Unknown chain ({id})"),
   }
}

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

/// Events that count as the user doing something (they reset the auto-lock
/// idle timer).
///
/// Window/focus, modifier, accesskit and screenshot events are deliberately
/// excluded: they fire without the user interacting and would keep the app
/// unlocked on their own.
pub fn is_user_input_event(event: &egui::Event) -> bool {
   use egui::Event;
   matches!(
      event,
      Event::PointerMoved(_)
         | Event::MouseMoved(_)
         | Event::PointerButton { .. }
         | Event::MouseWheel { .. }
         | Event::Zoom(_)
         | Event::Key { pressed: true, .. }
         | Event::Text(_)
         | Event::Paste(_)
         | Event::Cut
         | Event::Copy
         | Event::Touch { .. }
         | Event::Ime(_)
   )
}

/// Refresh the auto-lock idle timer when this frame carried real user input.
///
/// Called on both the main-window and the Settings-viewport passes, so
/// interacting with either counts as activity.
pub fn record_input_activity(egui_ctx: &egui::Context, ctx: &mut ZeusContext) {
   if egui_ctx.input(|input| input.events.iter().any(is_user_input_event)) {
      ctx.last_activity_ms = TimeStamp::now_as_millis().unwrap_or_default().timestamp();
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
      crate::runtime::rt().spawn_blocking(move || {
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

#[cfg(test)]
mod tests {
   use super::*;
   use egui::{Event, Key, Modifiers, Pos2};

   fn key(pressed: bool) -> Event {
      Event::Key {
         key: Key::A,
         physical_key: None,
         pressed,
         repeat: false,
         modifiers: Modifiers::NONE,
      }
   }

   #[test]
   fn user_input_events_are_recognized() {
      assert!(is_user_input_event(&Event::PointerMoved(
         Pos2::ZERO
      )));
      assert!(is_user_input_event(&Event::Text("a".to_string())));
      assert!(is_user_input_event(&Event::Copy));
      assert!(is_user_input_event(&key(true)));
   }

   /// Events that fire on their own must not reset the idle timer.
   #[test]
   fn passive_events_are_not_user_input() {
      assert!(!is_user_input_event(&key(false)));
      assert!(!is_user_input_event(&Event::WindowFocused(true)));
      assert!(!is_user_input_event(&Event::ModifiersChanged(
         Modifiers::NONE
      )));
   }

   /// A stored chain id is named when Zeus knows the chain and labelled when it does not — never
   /// converted with the panicking `ChainId::from`. Swept over every supported chain so a new chain
   /// picks up its own name, and over an ordinary unsupported id and one that cannot even be a chain
   /// id, which must fall back without panicking.
   #[test]
   fn chain_label_names_supported_chains_and_labels_the_rest() {
      use zeus_eth::types::SUPPORTED_CHAINS;

      for id in SUPPORTED_CHAINS {
         let chain = ChainId::new(id).unwrap();
         assert_eq!(chain_label(id), chain.name(), "chain {id}");
      }

      assert_eq!(chain_label(137), "Unknown chain (137)");
      assert_eq!(
         chain_label(u64::MAX),
         format!("Unknown chain ({})", u64::MAX)
      );
   }

   /// Every action that needs a real `ChainId` for a stored row asks this, so it has to accept
   /// exactly Zeus's chains and refuse everything else — the refused id is what the callers name in
   /// their message and what keeps `ChainId::from` off the path. Swept over the supported set so a
   /// new chain is picked up, plus an ordinary unsupported id and one that cannot be a chain at all.
   #[test]
   fn usable_chain_accepts_exactly_zeus_chains() {
      use zeus_eth::types::SUPPORTED_CHAINS;

      for id in SUPPORTED_CHAINS {
         assert_eq!(
            usable_chain(id).map(|c| c.id()),
            Some(id),
            "chain {id}"
         );
      }

      assert!(usable_chain(137).is_none());
      assert!(usable_chain(u64::MAX).is_none());
   }
}
