//! Common UI components

pub mod address;
pub mod amount_field;
pub mod autolock_timing;
pub mod chain_select;
pub mod chain_switch;
pub mod fade;
pub mod heading;
pub mod notices;
pub mod recipient_field;
pub mod status_pill;
pub mod wallet_identity;
pub mod wallet_list;
pub mod wallet_select;
pub mod wallet_switch;
pub mod windows;

pub use address::{
   SpokenAddress, address_for_speech, drawn_and_spoken, name_address, transfer_for_speech,
};
pub use amount_field::{AmountField, AmountFieldParams};
pub use autolock_timing::{GRACE_NOTICE, LockNotices, effective_idle_secs};
pub use chain_select::ChainSelect;
pub use chain_switch::switch_chain;
pub use fade::{panel_fade, show_with_fade};
pub use heading::heading;
pub use recipient_field::recipient_field;
pub use wallet_identity::{spoken_wallet_name, wallet_identity};
pub use wallet_list::WalletListByValue;
pub use wallet_select::WalletSelect;
pub use wallet_switch::switch_wallet;
pub use windows::{ConfirmWindow, ConfirmWindowClaim, LoadingWindow, MsgWindow, UpdateWindow};

use crate::core::ZeusContext;
use crate::gui::{SHARED_GUI, ui::dapps::railgun::RailgunMode};
use crate::utils::TimeStamp;
use egui::{Align, Id, Layout, Response, RichText, Role, Ui, Vec2, WidgetInfo, pos2, vec2};
use egui_elements::{Button, Frame, Switch, Theme, ToggleVisuals};
use egui_lucide::Lucide;
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

/// The whole seconds left before a delayed action is available, or `None` when it already is.
///
/// `None` in and `None` out both mean "no wait": an in-app Zeus prompt opens with no
/// `opened_at` at all, and a prompt whose delay has run out reports the same.
pub fn delayed_action_remaining(opened_at: Option<Instant>) -> Option<u64> {
   let opened_at = opened_at?;
   let remaining = ACTION_UNLOCK_DELAY.saturating_sub(opened_at.elapsed());

   if remaining.is_zero() {
      None
   } else {
      Some(remaining.as_secs_f32().ceil() as u64)
   }
}

/// `(enabled, label)` for Sign/Confirm. `None` means no delay (in-app Zeus prompts).
pub fn delayed_action_label(opened_at: Option<Instant>, ready_label: &str) -> (bool, String) {
   match delayed_action_remaining(opened_at) {
      Some(secs) => (false, format!("Available in {secs}s")),
      None => (true, ready_label.to_string()),
   }
}

/// Draw the sentence that says why an action is blocked, directly above its button.
///
/// A disabled button takes no focus, so a reader could not reach the reason while it lived in
/// the button's label — exactly when it mattered. The line is drawn in the button's own column,
/// so a long sentence wraps where the button ends, and nothing is drawn while nothing is
/// blocking. The Send form and the transaction confirmation share it, so both say a block the
/// same way.
pub fn blocked_reason_line(ui: &mut Ui, theme: &Theme, reason: &str) {
   let width = ui.available_width() * 0.8;
   let text = RichText::new(reason).size(theme.typography.normal).color(theme.colors.error);

   ui.allocate_ui_with_layout(
      vec2(width, 0.0),
      Layout::top_down(Align::Center),
      |ui| {
         ui.label(text);
      },
   );
}

/// The sentence to announce for a blocked action whose reason changed between two frames.
///
/// `None` when there is nothing to say: no reason before and none now, or the same reason still
/// blocking. A reason that goes away says nothing either — an enabled button is its own news, and
/// announcing the loss of a sentence nobody asked about would talk over what the user is doing.
pub fn blocked_announcement(previous: Option<&str>, current: Option<&str>) -> Option<String> {
   match (previous, current) {
      (None, None) => None,
      (Some(previous), Some(current)) if previous == current => None,
      (Some(_), None) => None,
      (None, Some(current)) | (Some(_), Some(current)) => Some(current.to_string()),
   }
}

/// Events that count as the user doing something (they reset the auto-lock
/// idle timer).
///
/// An AccessKit action request is input: only an assistive technology sends
/// one, and it is what its user produces by moving to a node or activating one.
/// Reading a page with a screen reader otherwise arrives as no activity at all,
/// and the wallet would lock mid-read. Reading without moving focus ("read
/// all") is undetectable from inside the process, which is why the idle timeout
/// itself is longer while a reader is attached.
///
/// Window/focus and modifier events are deliberately excluded: they fire
/// without the user interacting and would keep the app unlocked on their own.
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
         | Event::AccessKitActionRequest(_)
   )
}

/// Refresh the auto-lock idle timer when this frame carried real user input.
///
/// Called on the root viewport's pass, which draws every surface the user can touch.
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

   let green = ToggleVisuals {
      bg_checked: theme.colors.success,
      border_checked: theme.colors.success,
      mark: theme.colors.ink_on(theme.colors.success),
      ..ToggleVisuals::from_theme(theme.colors)
   };
   let switch = Switch::new(&mut ctx.privacy_mode, rich_text).visuals(green);

   let size = vec2(150.0, 20.0);
   let mut clicked = false;

   let mut switch_response = None;

   ui.allocate_ui(size, |ui| {
      ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
         switch_response = Some(ui.add(switch));
         ui.add_space(3.0);
         ui.add(icon);
      });
   });

   // The switch has already applied a click to `ctx.privacy_mode` when it is added, so reading it
   // here describes the state the switch now shows rather than the one it had last frame.
   if let Some(response) = switch_response {
      let checked = ctx.privacy_mode;

      // The drawn word flips between "Privacy mode" and "Public mode", which makes the node's name
      // disagree with its state ("Public mode, switch, on"). The node keeps the stable name and lets
      // `checked` say the rest; the visible text still flips.
      response.widget_info(|| {
         WidgetInfo::selected(
            Role::Switch,
            ui.is_enabled(),
            checked,
            "Privacy mode",
         )
      });

      clicked = response.clicked();
   }

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

/// Put one sentence on the node behind a themed label, whatever the label reported for itself.
///
/// AccessKit names a `Role::Label` node from the node's value rather than its label, so the
/// sentence goes in both: whichever of the two a consumer reads, the label says exactly the one
/// thing it was given. A widget that made no node of its own is given a label node here, so the
/// drawn text is never the only thing a reader gets; one that did keeps the role it has and only
/// takes the words.
pub fn name_text(ui: &Ui, response: &Response, text: impl Into<String>) {
   let text = text.into();
   let egui_ctx = ui.ctx();

   if !egui_ctx.has_accesskit_node(response.id) {
      let name = text.as_str();
      response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), name));
   }

   egui_ctx.accesskit_node_builder(response.id, |node| {
      if node.role() == Role::Label {
         node.set_value(text.clone());
      }

      node.set_label(text.clone());
   });
}

/// A plate that is one clickable row to a reader: the frame, its contents, and the node behind them.
///
/// A row drawn as a frame with a click sense bolted on afterwards is a hit target and nothing more:
/// a reader finds no role, no name, and no way to activate it. The frame's own widget carries all
/// three here — [`WidgetInfo::selected`] gives it the button role, the name the caller writes and,
/// when the caller says so, the selected state that tells a reader which row the surface points at.
///
/// The frame registers itself with an empty sense before its contents, under the id it is about to
/// take, and that withdraws the focus a Tab put on it: a row reached by keyboard would lose focus
/// every frame and Space/Enter would never reach it. `show` is `begin` + the contents + `end`, so
/// writing the three out leaves a moment to put the focus back before the final registration, which
/// is the one that makes the row focusable again.
///
/// `covered` is the ids of the nodes the contents added for words the row's own name already says:
/// they are hidden, so a reader hears the row once instead of once per label inside it.
pub fn clickable_row(
   ui: &mut Ui,
   frame: Frame,
   name: impl Into<String>,
   description: Option<String>,
   selected: bool,
   add_contents: impl FnOnce(&mut Ui) -> Vec<Id>,
) -> Response {
   let focused_before = ui.ctx().memory(|memory| memory.focused());

   let mut row = frame.begin(ui);
   let id = ui.next_auto_id();

   if focused_before == Some(id) {
      ui.ctx().memory_mut(|memory| memory.request_focus(id));
   }

   let covered = add_contents(&mut row.content_ui);

   let response = row.end(ui);
   let name = name.into();

   response.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), selected, &name));

   if let Some(description) = description {
      ui.ctx().accesskit_node_builder(response.id, |node| {
         node.set_description(description)
      });
   }

   for covered in covered {
      ui.ctx().accesskit_node_builder(covered, |node| node.set_hidden());
   }

   response
}

#[cfg(test)]
mod tests {
   use super::*;
   use egui::accesskit::{Action, ActionRequest, NodeId, TreeId};
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

   /// The action a screen reader sends when it moves to a node or activates one.
   fn accesskit_action() -> Event {
      Event::AccessKitActionRequest(ActionRequest {
         action: Action::Focus,
         target_tree: TreeId::ROOT,
         target_node: NodeId(1),
         data: None,
      })
   }

   /// Events that fire on their own must not reset the idle timer.
   #[test]
   fn is_user_input_event_accepts_only_real_input() {
      // Case: user_input_events_are_recognized
      assert!(is_user_input_event(&Event::PointerMoved(
         Pos2::ZERO
      )));
      assert!(is_user_input_event(&Event::Text("a".to_string())));
      assert!(is_user_input_event(&Event::Copy));
      assert!(is_user_input_event(&key(true)));
      // Case: a_screen_reader_driving_the_interface_is_input
      assert!(is_user_input_event(&accesskit_action()));

      // Case: passive_events_are_not_user_input
      assert!(!is_user_input_event(&key(false)));
      assert!(!is_user_input_event(&Event::WindowFocused(true)));
      assert!(!is_user_input_event(&Event::WindowFocused(false)));
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

      for id in [137, u64::MAX] {
         let label = chain_label(id);
         assert!(label.starts_with("Unknown chain ("), "{label}");
         assert!(label.contains(&id.to_string()), "{label}");
      }
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

   /// The rule the Send form follows: a reason that appears or changes is a new sentence, the same
   /// reason a frame later is not, and a block that lifts says nothing.
   #[test]
   fn a_blocked_reason_is_said_when_it_changes() {
      assert_eq!(blocked_announcement(None, None), None);

      assert_eq!(
         blocked_announcement(None, Some("Invalid Amount")).as_deref(),
         Some("Invalid Amount")
      );

      assert_eq!(
         blocked_announcement(Some("Invalid Amount"), Some("Invalid Amount")),
         None
      );

      assert_eq!(
         blocked_announcement(
            Some("Invalid Amount"),
            Some("Insufficient ETH Balance")
         )
         .as_deref(),
         Some("Insufficient ETH Balance")
      );

      assert_eq!(
         blocked_announcement(Some("Invalid Amount"), None),
         None
      );
   }
}
